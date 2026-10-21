//! A native keepalive for the kernel's project lease. No scheduling policy:
//! it only keeps an already admitted pass fenced while blocking IO runs.
use crate::{
    rows::{now_ms, store_scope},
    store, SharedStore,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;

pub(crate) struct LeaseKeeper {
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
    valid: Arc<AtomicBool>,
}
impl LeaseKeeper {
    pub(crate) fn start(
        shared: SharedStore,
        fence: store::Fence,
        project: String,
        worker: String,
        ttl_ms: u64,
    ) -> Result<Self, String> {
        let (stop, recv) = mpsc::channel();
        let valid = Arc::new(AtomicBool::new(true));
        let live = valid.clone();
        let thread = std::thread::Builder::new()
            .name("ds-sync-lease".into())
            .spawn(move || {
                while matches!(
                    recv.recv_timeout(Duration::from_millis((ttl_ms / 3).max(1))),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    let renewed = shared
                        .lock()
                        .ok()
                        .and_then(|mut database| {
                            database
                                .apply(
                                    &fence,
                                    now_ms(),
                                    store::Event::LeaseRenew {
                                        scope: store_scope(&project),
                                        worker_id: worker.clone(),
                                        ttl_ms,
                                    },
                                )
                                .ok()
                        })
                        .is_some_and(|transition| transition.refusals.is_empty());
                    if !renewed {
                        live.store(false, Ordering::Release);
                        break;
                    }
                }
            })
            .map_err(|error| format!("could not keep the sync lease alive: {error}"))?;
        Ok(Self {
            stop,
            thread: Some(thread),
            valid,
        })
    }
    pub(crate) fn valid(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }
}
impl Drop for LeaseKeeper {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keepalive_renews_during_io_and_fences_after_ownership_loss() {
        let dir = tempfile::tempdir().unwrap();
        let shared = crate::open_store(&dir.path().join("sync.sqlite")).unwrap();
        let fence = store::Fence {
            account: "a".into(),
            deployment: "d".into(),
            install_id: "i".into(),
        };
        let scope = store_scope("p");
        let start = now_ms();
        shared
            .lock()
            .unwrap()
            .apply(
                &fence,
                start,
                store::Event::LeaseTake {
                    scope: scope.clone(),
                    worker_id: "first".into(),
                    ttl_ms: 600,
                },
            )
            .unwrap();
        let keeper = LeaseKeeper::start(
            shared.clone(),
            fence.clone(),
            "p".into(),
            "first".into(),
            600,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(900));
        let other = shared
            .lock()
            .unwrap()
            .apply(
                &fence,
                now_ms(),
                store::Event::LeaseTake {
                    scope: scope.clone(),
                    worker_id: "second".into(),
                    ttl_ms: 600,
                },
            )
            .unwrap();
        assert!(
            !other.refusals.is_empty(),
            "IO lasting beyond the original lease must stay exclusive"
        );
        shared
            .lock()
            .unwrap()
            .apply(
                &fence,
                now_ms(),
                store::Event::LeaseRelease {
                    scope: scope.clone(),
                    worker_id: "first".into(),
                },
            )
            .unwrap();
        shared
            .lock()
            .unwrap()
            .apply(
                &fence,
                now_ms(),
                store::Event::LeaseTake {
                    scope,
                    worker_id: "second".into(),
                    ttl_ms: 600,
                },
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(!keeper.valid(), "a lost lease cannot be silently retaken");
    }
}
