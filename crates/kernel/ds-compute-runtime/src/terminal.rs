//! Process-local terminal invalidations. Durable rows remain the authority.
use ds_command_kernel::compute_jobs::{Job, Phase};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::sync::watch;

type Key = (PathBuf, String);
type Listeners = HashMap<Key, Weak<watch::Sender<bool>>>;
fn listeners() -> &'static Mutex<Listeners> {
    static LISTENERS: OnceLock<Mutex<Listeners>> = OnceLock::new();
    LISTENERS.get_or_init(Mutex::default)
}

/// Register before reading the durable row. A notification between that read
/// and `changed` is retained, including when several clients observe one job.
/// Hosts bound concurrent observers separately from their ordinary requests.
pub struct TerminalListener {
    _sender: Arc<watch::Sender<bool>>,
    receiver: watch::Receiver<bool>,
}
pub fn observe_terminal(path: &Path, id: &str) -> TerminalListener {
    let mut held = listeners().lock().unwrap_or_else(|e| e.into_inner());
    held.retain(|_, sender| sender.strong_count() > 0);
    let key = (path.to_owned(), id.to_owned());
    let sender = held.get(&key).and_then(Weak::upgrade).unwrap_or_else(|| {
        let sender = Arc::new(watch::channel(false).0);
        held.insert(key, Arc::downgrade(&sender));
        sender
    });
    let receiver = sender.subscribe();
    TerminalListener {
        _sender: sender,
        receiver,
    }
}
impl TerminalListener {
    pub async fn changed(&mut self) {
        let _ = self.receiver.changed().await;
    }
}

/// Call only AFTER the store's completed/failed/cancelled transaction returns.
/// This carries no result or authority and never changes or cancels the job.
pub fn notify_terminal(path: &Path, job: &Job) {
    if !matches!(
        job.phase,
        Phase::Completed | Phase::Failed | Phase::Cancelled
    ) {
        return;
    }
    let held = listeners().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(sender) = held
        .get(&(path.to_owned(), job.id.clone()))
        .and_then(Weak::upgrade)
    {
        sender.send_replace(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn job(phase: Phase) -> Job {
        serde_json::from_value(serde_json::json!({
            "id":"1".repeat(64),"owner":"owner","lane":"stable",
            "input_sha256":"2".repeat(64),"engine":"lv_processing",
            "input_tag":"ds.fast-lv.request/v1","phase":phase,
            "attempts":0,"created_at_ms":1,"updated_at_ms":1,"worker":null,
            "lease_until_ms":0,"result_sha256":null,"error":null,
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn terminal_notification_before_await_is_retained_for_all_registered_observers() {
        let directory = tempfile::tempdir().unwrap();
        let job = job(Phase::Completed);
        let mut first = observe_terminal(directory.path(), &job.id);
        let mut second = observe_terminal(directory.path(), &job.id);
        notify_terminal(directory.path(), &job);
        for listener in [&mut first, &mut second] {
            tokio::time::timeout(Duration::from_secs(1), listener.changed())
                .await
                .unwrap();
        }
        drop(first);
        drop(second);
        // Dropping observers keeps no strong sender; a later observer starts
        // clean and recovers terminal state from the durable row itself.
        let key = (directory.path().to_owned(), job.id.clone());
        assert_eq!(
            listeners()
                .lock()
                .unwrap()
                .get(&key)
                .unwrap()
                .strong_count(),
            0
        );
    }

    #[tokio::test]
    async fn queued_and_unrelated_notifications_do_not_wake_an_observer() {
        let directory = tempfile::tempdir().unwrap();
        let queued = job(Phase::Queued);
        let mut observer = observe_terminal(directory.path(), &queued.id);
        notify_terminal(directory.path(), &queued);
        let mut unrelated = job(Phase::Failed);
        unrelated.id = "3".repeat(64);
        notify_terminal(directory.path(), &unrelated);
        notify_terminal(
            &directory.path().join("another-store"),
            &job(Phase::Cancelled),
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(1), observer.changed())
                .await
                .is_err()
        );
        notify_terminal(directory.path(), &job(Phase::Failed));
        tokio::time::timeout(Duration::from_secs(1), observer.changed())
            .await
            .unwrap();
    }
}
