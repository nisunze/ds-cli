//! Demand-driven retained printing reads. Hosts persist the admitted answers;
//! this owner coalesces acquisitions and keeps invalidation free of transport.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
pub struct Demand {
    pub online: bool,
    pub acquire: bool,
    pub refresh: bool,
}

#[derive(Debug, PartialEq)]
pub enum Failure<E> {
    AcquisitionRequired,
    Unavailable(E),
    GateUnavailable,
}

/// Storage and transport stay in the host; retained values include revisions.
pub trait Host<T> {
    type Error;
    fn retained(&mut self, key: &str) -> Option<(T, bool)>;
    fn invalidate(&mut self) -> Result<(), Self::Error>;
    fn acquire(&mut self, key: &str) -> Result<T, Self::Error>;
    fn retain(&mut self, key: &str, value: &T) -> Result<(), Self::Error>;
}

#[derive(Default)]
struct Room {
    completed: AtomicU64,
    gate: Mutex<BTreeMap<String, u64>>,
}

/// Independent account/deployment/project rooms never block one another.
#[derive(Default)]
pub struct Reads(Mutex<BTreeMap<String, Arc<Room>>>);

impl Reads {
    fn room(&self, scope: &str) -> Result<Arc<Room>, ()> {
        let mut rooms = self.0.lock().map_err(|_| ())?;
        Ok(rooms.entry(scope.to_owned()).or_default().clone())
    }

    /// A canonical mutation/push marks state stale. It performs no acquisition.
    pub fn invalidate<T, H: Host<T>>(
        &self,
        scope: &str,
        host: &mut H,
    ) -> Result<(), Failure<H::Error>> {
        let room = self.room(scope).map_err(|_| Failure::GateUnavailable)?;
        let _held = room.gate.lock().map_err(|_| Failure::GateUnavailable)?;
        host.invalidate().map_err(Failure::Unavailable)
    }

    pub fn read<T, H: Host<T>>(
        &self,
        scope: &str,
        key: &str,
        demand: Demand,
        host: &mut H,
    ) -> Result<T, Failure<H::Error>> {
        let room = self.room(scope).map_err(|_| Failure::GateUnavailable)?;
        let observed = room.completed.load(Ordering::Acquire);
        let mut completed = room.gate.lock().map_err(|_| Failure::GateUnavailable)?;
        // Overlapping explicit refreshes share the acquisition already completed
        // for this key. A later intentional refresh gets a new answer.
        let refreshed = completed.get(key).is_some_and(|epoch| *epoch > observed);
        if demand.refresh && demand.online && !refreshed {
            host.invalidate().map_err(Failure::Unavailable)?;
        }
        if let Some((value, invalidated)) = host.retained(key) {
            if !demand.online || !invalidated {
                return Ok(value);
            }
        }
        if !demand.online || !demand.acquire {
            return Err(Failure::AcquisitionRequired);
        }
        let value = host.acquire(key).map_err(Failure::Unavailable)?;
        host.retain(key, &value).map_err(Failure::Unavailable)?;
        let epoch = room.completed.fetch_add(1, Ordering::AcqRel) + 1;
        completed.insert(key.to_owned(), epoch);
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::thread;
    use std::time::Duration;

    #[derive(Default)]
    struct Store {
        values: Mutex<BTreeMap<String, (String, bool)>>,
        requests: AtomicUsize,
    }
    struct Memory(Arc<Store>);
    impl Host<String> for Memory {
        type Error = &'static str;
        fn retained(&mut self, key: &str) -> Option<(String, bool)> {
            self.0.values.lock().unwrap().get(key).cloned()
        }
        fn invalidate(&mut self) -> Result<(), Self::Error> {
            for entry in self.0.values.lock().unwrap().values_mut() {
                entry.1 = true;
            }
            Ok(())
        }
        fn acquire(&mut self, _key: &str) -> Result<String, Self::Error> {
            self.0.requests.fetch_add(1, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(20));
            Ok("revision-7".into())
        }
        fn retain(&mut self, key: &str, value: &String) -> Result<(), Self::Error> {
            self.0
                .values
                .lock()
                .unwrap()
                .insert(key.into(), (value.clone(), false));
            Ok(())
        }
    }
    fn demand() -> Demand {
        Demand {
            online: true,
            acquire: true,
            refresh: false,
        }
    }

    #[test]
    fn warm_navigation_and_offline_reuse_do_no_remote_read() {
        let reads = Reads::default();
        let store = Arc::new(Store::default());
        let mut host = Memory(store.clone());
        assert_eq!(
            reads
                .read("u1/p1", "catalogue", demand(), &mut host)
                .unwrap(),
            "revision-7"
        );
        for _ in 0..5 {
            reads
                .read("u1/p1", "catalogue", demand(), &mut host)
                .unwrap();
        }
        // Persisted holdings survive the coordinator's replacement/restart.
        let restarted = Reads::default();
        restarted
            .read(
                "u1/p1",
                "catalogue",
                Demand {
                    online: false,
                    acquire: false,
                    refresh: true,
                },
                &mut host,
            )
            .unwrap();
        assert_eq!(store.requests.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn invalidation_of_unused_data_does_no_read_and_next_consumer_resolves_it() {
        let reads = Reads::default();
        let store = Arc::new(Store::default());
        let mut host = Memory(store.clone());
        reads
            .read("u1/p1", "catalogue", demand(), &mut host)
            .unwrap();
        reads.invalidate("u1/p1", &mut host).unwrap();
        assert_eq!(store.requests.load(Ordering::Relaxed), 1);
        reads
            .read(
                "u1/p1",
                "catalogue",
                Demand {
                    online: false,
                    ..demand()
                },
                &mut host,
            )
            .unwrap();
        assert_eq!(store.requests.load(Ordering::Relaxed), 1);
        reads
            .read("u1/p1", "catalogue", demand(), &mut host)
            .unwrap();
        assert_eq!(store.requests.load(Ordering::Relaxed), 2);
    }
    #[test]
    fn concurrent_consumers_share_one_acquisition_and_refresh_is_explicit() {
        let reads = Arc::new(Reads::default());
        let store = Arc::new(Store::default());
        let jobs: Vec<_> = (0..6)
            .map(|_| {
                let reads = reads.clone();
                let store = store.clone();
                thread::spawn(move || {
                    reads
                        .read("u1/p1", "catalogue", demand(), &mut Memory(store))
                        .unwrap()
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
        assert_eq!(store.requests.load(Ordering::Relaxed), 1);
        reads
            .read(
                "u1/p1",
                "catalogue",
                Demand {
                    refresh: true,
                    ..demand()
                },
                &mut Memory(store.clone()),
            )
            .unwrap();
        assert_eq!(store.requests.load(Ordering::Relaxed), 2);
    }
    #[test]
    fn overlapping_explicit_refreshes_coalesce_but_a_later_refresh_reads_again() {
        let reads = Arc::new(Reads::default());
        let store = Arc::new(Store::default());
        reads
            .read("u1/p1", "catalogue", demand(), &mut Memory(store.clone()))
            .unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(6));
        let jobs: Vec<_> = (0..6)
            .map(|_| {
                let reads = reads.clone();
                let store = store.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    reads
                        .read(
                            "u1/p1",
                            "catalogue",
                            Demand {
                                refresh: true,
                                ..demand()
                            },
                            &mut Memory(store),
                        )
                        .unwrap()
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
        assert_eq!(store.requests.load(Ordering::Relaxed), 2);
        reads
            .read(
                "u1/p1",
                "catalogue",
                Demand {
                    refresh: true,
                    ..demand()
                },
                &mut Memory(store.clone()),
            )
            .unwrap();
        assert_eq!(store.requests.load(Ordering::Relaxed), 3);
    }
    #[test]
    fn absent_inputs_require_acquisition_and_never_launch_offline_work() {
        let reads = Reads::default();
        let store = Arc::new(Store::default());
        for online in [false, true] {
            assert_eq!(
                reads.read(
                    "u1/p1",
                    "catalogue",
                    Demand {
                        online,
                        acquire: false,
                        refresh: false
                    },
                    &mut Memory(store.clone())
                ),
                Err(Failure::AcquisitionRequired)
            );
        }
        assert_eq!(store.requests.load(Ordering::Relaxed), 0);
    }
}
