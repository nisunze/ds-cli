//! Portable native compute host shared by headless Server and Desktop.
//! Domain lifecycle/admission belongs to the kernel; Fast LV belongs to ds-network.
use ds_command_kernel::compute_jobs::{self, Event, Job, Phase};
use ds_network::network::native_fast_lv::{
    decode_native_fast_lv_request, encode_native_fast_lv_result, process_native_fast_lv,
};
use ds_sync_store::Store;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn open(path: &Path) -> Result<Store, String> {
    let store = Store::open(path).map_err(|e| e.to_string())?;
    store.initialize_jobs().map_err(|e| e.to_string())?;
    Ok(store)
}

pub fn submit(
    path: &Path,
    owner: &str,
    lane: &str,
    key: &str,
    input: &[u8],
) -> Result<Job, String> {
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err("job_key_invalid: use 1..128 letters, digits, _ or -".into());
    }
    decode_native_fast_lv_request(input).map_err(|e| e.to_string())?;
    let id = digest(&serde_json::to_vec(&(owner, lane, key)).map_err(|e| e.to_string())?);
    let now = now_ms();
    let job = Job {
        id,
        owner: owner.into(),
        lane: lane.into(),
        input_sha256: digest(input),
        phase: Phase::Queued,
        attempts: 0,
        created_at_ms: now,
        updated_at_ms: now,
        worker: None,
        lease_until_ms: 0,
        result_sha256: None,
        error: None,
    };
    open(path)?
        .submit_job(&job, input)
        .map_err(|e| e.to_string())
}

/// Performs live native credential validation. Implemented by the native auth
/// host; no credential, browser token or backend route enters the compute host.
pub trait Authorizer: Send + Sync + 'static {
    fn authorize(&self, expected_owner: &str) -> Result<(), String>;
}

pub struct Workers {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}
impl Workers {
    pub fn start(
        path: PathBuf,
        owner: String,
        lane: String,
        count: usize,
        auth: Arc<dyn Authorizer>,
    ) -> Result<Self, String> {
        if count == 0 || count > 256 {
            return Err("worker count must be 1..256".into());
        }
        open(&path)?;
        // All batches share this bounded pool, including the engine's nested
        // parallel transformer work. A batch cannot escape memory admission by
        // starting another machine-wide pool for its individual transformers.
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(count)
                .thread_name(|i| format!("ds-compute-{i}"))
                .build()
                .map_err(|e| e.to_string())?,
        );
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::with_capacity(count);
        for index in 0..count {
            let (path, owner, lane, auth, stop) = (
                path.clone(),
                owner.clone(),
                lane.clone(),
                auth.clone(),
                stop.clone(),
            );
            let pool = pool.clone();
            let worker = format!("{}-{}-{index}", std::process::id(), now_ms());
            threads.push(thread::spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    if auth.authorize(&owner).is_err() {
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    match pool.install(|| run_one(&path, &owner, &lane, &worker, auth.clone())) {
                        Ok(true) => {}
                        Ok(false) => thread::sleep(Duration::from_millis(200)),
                        Err(error) => {
                            eprintln!("compute worker: {error}");
                            thread::sleep(Duration::from_secs(1));
                        }
                    }
                }
            }));
        }
        Ok(Self { stop, threads })
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Drop for Workers {
    fn drop(&mut self) {
        self.stop();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

pub fn run_one(
    path: &Path,
    owner: &str,
    lane: &str,
    worker: &str,
    auth: Arc<dyn Authorizer>,
) -> Result<bool, String> {
    auth.authorize(owner)?;
    let Some((job, input)) = open(path)?
        .claim_job(owner, lane, worker, now_ms(), 30_000)
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    let done = Arc::new(AtomicBool::new(false));
    let renewed = Arc::new(AtomicBool::new(true));
    let renew_thread = {
        let (path, owner, lane, worker, id, done, renewed, auth) = (
            path.to_owned(),
            owner.to_owned(),
            lane.to_owned(),
            worker.to_owned(),
            job.id.clone(),
            done.clone(),
            renewed.clone(),
            auth.clone(),
        );
        thread::spawn(move || {
            let mut ticks = 0;
            while !done.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(100));
                ticks += 1;
                if ticks < 50 {
                    continue;
                }
                ticks = 0;
                let result = auth.authorize(&owner).and_then(|_| {
                    open(&path)?
                        .update_job(
                            &owner,
                            &lane,
                            &id,
                            Event::Renew {
                                worker: &worker,
                                lease_ms: 30_000,
                            },
                            now_ms(),
                            None,
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                });
                if result.is_err() {
                    renewed.store(false, Ordering::Release);
                    break;
                }
            }
        })
    };
    let output = std::panic::catch_unwind(|| {
        if digest(&input) != job.input_sha256 {
            return Err("durable input digest mismatch".to_string());
        }
        let request = decode_native_fast_lv_request(&input).map_err(|e| e.to_string())?;
        let result = process_native_fast_lv(request).map_err(|e| e.to_string())?;
        encode_native_fast_lv_result(&result).map_err(|e| e.to_string())
    })
    .unwrap_or_else(|_| Err("native compute panicked; input retained".into()));
    done.store(true, Ordering::Release);
    let _ = renew_thread.join();
    if !renewed.load(Ordering::Acquire) {
        return Err("job lease or authorization lost; input retained for recovery".into());
    }
    auth.authorize(owner)?;
    match output {
        Ok(bytes) => {
            let sha = digest(&bytes);
            open(path)?
                .update_job(
                    owner,
                    lane,
                    &job.id,
                    Event::Complete {
                        worker,
                        result_sha256: &sha,
                    },
                    now_ms(),
                    Some(&bytes),
                )
                .map_err(|e| e.to_string())?;
        }
        Err(error) => {
            open(path)?
                .update_job(
                    owner,
                    lane,
                    &job.id,
                    Event::Fail {
                        worker,
                        error: &error,
                    },
                    now_ms(),
                    None,
                )
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(true)
}

pub fn capacity() -> usize {
    let cpus = thread::available_parallelism().map_or(1, usize::from);
    #[cfg(target_os = "linux")]
    let memory = std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| {
        s.lines().find_map(|l| {
            l.strip_prefix("MemAvailable:")
                .and_then(|r| r.split_whitespace().next())
                .and_then(|n| n.parse::<u64>().ok())
                .and_then(|n| n.checked_mul(1024))
        })
    });
    #[cfg(not(target_os = "linux"))]
    let memory = None;
    compute_jobs::concurrent_jobs(cpus, memory).min(256)
}
