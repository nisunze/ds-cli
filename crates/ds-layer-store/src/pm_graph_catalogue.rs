//! One private retained canonical PM graph in the existing native persistence owner.
//! Offline holdings attest capture-time access only; no permission or source probe runs.
use crate::{exclusive_lock, private};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const SCHEMA: &str = "ds.pm.native-graph/v1";
const MAX_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub uid: String,
    pub lane: String,
    pub audience: String,
    pub credential_instance: String,
    pub project: String,
}
impl Binding {
    fn directory(&self, root: &Path) -> Result<PathBuf, String> {
        if !["stable", "canary"].contains(&self.lane.as_str())
            || [
                &self.uid,
                &self.audience,
                &self.credential_instance,
                &self.project,
            ]
            .into_iter()
            .any(|s| {
                s.is_empty() || s.len() > 2048 || s.trim() != s || s.chars().any(char::is_control)
            })
        {
            return Err("pm_retained_scope_invalid".into());
        }
        Ok(root.join("pm-graph/v1").join(digest(self)?))
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    epoch: u64,
    serial: u64,
    denied: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Held {
    schema: String,
    binding: Binding,
    epoch: u64,
    serial: u64,
    captured_at_ms: i64,
    sha256: String,
    graph: Value,
}
pub trait Source {
    type Error;
    fn check_binding(&mut self) -> Result<(), Self::Error>;
    fn capture(&mut self) -> Result<Value, Self::Error>;
    fn verify(&self, graph: &Value) -> Result<(), Self::Error>;
    fn access_refused(&self, error: &Self::Error) -> bool;
}
#[derive(Debug)]
pub enum Error<E> {
    Source(E),
    Local(String),
    AcquisitionRequired,
    Superseded,
}
fn bounded_bytes(value: &impl Serialize, cap: usize) -> Result<Vec<u8>, String> {
    struct Bounded {
        bytes: Vec<u8>,
        cap: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.cap.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("PM retained bound exceeded"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Bounded {
        bytes: Vec::new(),
        cap,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| "pm_retained_bound_exceeded")?;
    Ok(writer.bytes)
}
fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(bounded_bytes(value, MAX_BYTES + 32768)?)
    ))
}
fn read<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<Option<T>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("pm_retained_unreadable".into()),
    };
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 32769) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "pm_retained_unreadable")?;
    if bytes.len() > MAX_BYTES + 32768 {
        return Err("pm_retained_unreadable".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "pm_retained_unreadable".into())
}
fn write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = bounded_bytes(value, MAX_BYTES + 32768)?;
    private::write_atomic(path, bytes).map_err(|_| "pm_retained_unavailable".into())
}
fn state(dir: &Path) -> Result<State, String> {
    Ok(read(&dir.join("state.json"))?.unwrap_or_default())
}
fn metadata(mut held: Held, state: &State, acquired: bool) -> Value {
    let stale = !ds_command_kernel::freshness::is_fresh(Some(&held.epoch), Some(&state.epoch));
    held.graph["retained"] = json!({"schema":"ds.pm.retained-graph/v1", "mode":if acquired {"acquired"} else {"offline_capture"},
        "captured_at_ms":held.captured_at_ms,"graph_sha256":held.sha256,"known_invalidated":stale,
        "graph_revision":held.graph["revision"],"schedule_revision":held.graph["schedule_revision"],
        "current_source_verified":acquired,"source_freshness":if acquired {"verified_at_capture"} else {"unknown"},
        "current_authorization_verified":acquired,"permission_freshness":if acquired {"verified_at_capture"} else {"captured_only"}});
    held.graph
}
/// Normal reads still acquire one canonical graph. Offline never acquires or creates holdings.
pub fn consume<S: Source>(
    root: &Path,
    binding: &Binding,
    offline: bool,
    now_ms: i64,
    source: &mut S,
) -> Result<Value, Error<S::Error>> {
    let local = Error::Local;
    if now_ms < 0 {
        return Err(local("pm_retained_time_invalid".into()));
    }
    let dir = binding.directory(root).map_err(local)?;
    source.check_binding().map_err(Error::Source)?;
    if offline {
        if !dir
            .join("graph.json")
            .try_exists()
            .map_err(|e| local(e.to_string()))?
        {
            return Err(Error::AcquisitionRequired);
        }
        let lock = private::open_options()
            .read(true)
            .write(true)
            .open(dir.join("state.lock"))
            .map_err(|_| local("pm_retained_unavailable".into()))?;
        fs2::FileExt::lock_exclusive(&lock).map_err(|_| local("pm_retained_unavailable".into()))?;
        let state = state(&dir).map_err(local)?;
        if state.denied {
            return Err(Error::AcquisitionRequired);
        }
        let held: Held = read(&dir.join("graph.json"))
            .map_err(local)?
            .ok_or(Error::AcquisitionRequired)?;
        if held.schema != SCHEMA
            || held.binding != *binding
            || held.sha256 != digest(&held.graph).map_err(local)?
            || held.captured_at_ms < 0
            || held.serial != state.serial
            || held.epoch > state.epoch
        {
            return Err(local("pm_retained_unreadable".into()));
        }
        source.verify(&held.graph).map_err(Error::Source)?;
        source.check_binding().map_err(Error::Source)?;
        return Ok(metadata(held, &state, false));
    }
    // Observe generation before waiting, so overlapping intentional acquisitions coalesce.
    let observed = read::<State>(&dir.join("state.json"))
        .map_err(local)?
        .unwrap_or_default()
        .serial;
    let _capture_lock = exclusive_lock(&dir.join("capture.lock")).map_err(local)?;
    let before = {
        let _lock = exclusive_lock(&dir.join("state.lock")).map_err(local)?;
        let before = state(&dir).map_err(local)?;
        if !dir.join("state.json").exists() {
            write(&dir.join("state.json"), &before).map_err(local)?;
        }
        if before.serial != observed && !before.denied {
            if let Some(held) = read::<Held>(&dir.join("graph.json")).map_err(local)? {
                if held.schema == SCHEMA
                    && held.binding == *binding
                    && held.serial == before.serial
                    && held.epoch == before.epoch
                    && held.sha256 == digest(&held.graph).map_err(local)?
                {
                    source.verify(&held.graph).map_err(Error::Source)?;
                    source.check_binding().map_err(Error::Source)?;
                    return Ok(metadata(held, &before, true));
                }
            }
        }
        before
    };
    let graph = match source.capture() {
        Ok(graph) => graph,
        Err(error) => {
            if source.access_refused(&error) {
                mark(root, binding, true).map_err(local)?;
            }
            return Err(Error::Source(error));
        }
    };
    source.check_binding().map_err(Error::Source)?;
    source.verify(&graph).map_err(Error::Source)?;
    bounded_bytes(&graph, MAX_BYTES).map_err(local)?;
    let _lock = exclusive_lock(&dir.join("state.lock")).map_err(local)?;
    let mut current = state(&dir).map_err(local)?;
    if current.epoch != before.epoch || current.serial != before.serial {
        return Err(Error::Superseded);
    }
    source.check_binding().map_err(Error::Source)?;
    current.serial = current
        .serial
        .checked_add(1)
        .ok_or_else(|| local("pm_retained_generation_exhausted".into()))?;
    current.denied = false;
    let held = Held {
        schema: SCHEMA.into(),
        binding: binding.clone(),
        epoch: current.epoch,
        serial: current.serial,
        captured_at_ms: now_ms,
        sha256: digest(&graph).map_err(local)?,
        graph,
    };
    write(&dir.join("graph.json"), &held).map_err(local)?;
    write(&dir.join("state.json"), &current).map_err(local)?;
    Ok(metadata(held, &current, true))
}
/// Persist invalidation/denial only for an existing consumer; never create unused holdings.
pub fn mark(root: &Path, binding: &Binding, denied: bool) -> Result<bool, String> {
    let dir = binding.directory(root)?;
    if !dir
        .join("state.json")
        .try_exists()
        .map_err(|_| "pm_retained_unavailable")?
    {
        return Ok(false);
    }
    let _lock = exclusive_lock(&dir.join("state.lock"))?;
    let mut current = state(&dir)?;
    current.epoch = current
        .epoch
        .checked_add(1)
        .ok_or("pm_retained_generation_exhausted")?;
    current.denied |= denied;
    write(&dir.join("state.json"), &current)?;
    // Denial is durable even if deletion is impossible. Offline observes the marker first.
    if denied {
        let _ = std::fs::remove_file(dir.join("graph.json"));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    fn binding() -> Binding {
        Binding {
            uid: "u".into(),
            lane: "canary".into(),
            audience: "a".into(),
            credential_instance: "login1".into(),
            project: "p".into(),
        }
    }
    fn graph() -> Value {
        json!({"project_id":"p","revision":3,"schedule_revision":2,"tasks":[],"dependencies":[],"residual_items":[],"calendar":{},"permissions":{"view":true,"comment":false,"contribute":false,"edit_schedule":false,"admin":false},"correspondence":{"records":[],"truncated":true,"records_total":1}})
    }
    struct Fake {
        calls: Arc<AtomicUsize>,
        fail: bool,
        changed: bool,
        root: Option<PathBuf>,
    }
    impl Fake {
        fn new(calls: Arc<AtomicUsize>) -> Self {
            Self {
                calls,
                fail: false,
                changed: false,
                root: None,
            }
        }
    }
    impl Source for Fake {
        type Error = &'static str;
        fn check_binding(&mut self) -> Result<(), Self::Error> {
            if self.changed {
                Err("changed")
            } else {
                Ok(())
            }
        }
        fn capture(&mut self) -> Result<Value, Self::Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(root) = &self.root {
                mark(root, &binding(), false).unwrap();
            }
            if self.fail {
                Err("denied")
            } else {
                Ok(graph())
            }
        }
        fn verify(&self, value: &Value) -> Result<(), Self::Error> {
            if value["project_id"] == "p" {
                Ok(())
            } else {
                Err("wrong_project")
            }
        }
        fn access_refused(&self, error: &Self::Error) -> bool {
            *error == "denied"
        }
    }
    #[test]
    fn retained_offline_has_zero_source_and_preserves_partial_projection() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(calls.clone());
        let captured = consume(temp.path(), &binding(), false, 1, &mut source).unwrap();
        assert_eq!(captured["retained"]["current_authorization_verified"], true);
        let offline = consume(temp.path(), &binding(), true, 2, &mut source).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(offline["correspondence"]["truncated"], true);
        assert_eq!(offline["retained"]["source_freshness"], "unknown");
        assert_eq!(offline["retained"]["permission_freshness"], "captured_only");
        consume(temp.path(), &binding(), false, 3, &mut source).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        #[cfg(unix)]
        crate::private::assert_private_tree(&temp.path().join("pm-graph"));
    }
    #[test]
    fn missing_or_other_scope_offline_and_unused_invalidation_create_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("absent");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(calls.clone());
        assert!(!mark(&root, &binding(), false).unwrap());
        assert!(matches!(
            consume(&root, &binding(), true, 1, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert!(!root.exists());
        consume(temp.path(), &binding(), false, 1, &mut source).unwrap();
        for field in 0..5 {
            let mut other = binding();
            match field {
                0 => other.uid = "other".into(),
                1 => other.lane = "stable".into(),
                2 => other.audience = "other".into(),
                3 => other.credential_instance = "login2".into(),
                _ => other.project = "other".into(),
            };
            assert!(matches!(
                consume(temp.path(), &other, true, 2, &mut source),
                Err(Error::AcquisitionRequired)
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn mutation_marks_retained_stale_denial_stays_durable_when_cleanup_fails() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(calls.clone());
        consume(temp.path(), &binding(), false, 1, &mut source).unwrap();
        mark(temp.path(), &binding(), false).unwrap();
        assert_eq!(
            consume(temp.path(), &binding(), true, 2, &mut source).unwrap()["retained"]
                ["known_invalidated"],
            true
        );
        let path = binding().directory(temp.path()).unwrap().join("graph.json");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        mark(temp.path(), &binding(), true).unwrap();
        assert!(matches!(
            consume(temp.path(), &binding(), true, 3, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert!(path.is_dir());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn denial_from_source_revokes_and_capture_cannot_publish_after_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(calls);
        consume(temp.path(), &binding(), false, 1, &mut source).unwrap();
        source.fail = true;
        assert!(matches!(
            consume(temp.path(), &binding(), false, 2, &mut source),
            Err(Error::Source("denied"))
        ));
        assert!(matches!(
            consume(temp.path(), &binding(), true, 3, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        source.fail = false;
        source.root = Some(temp.path().into());
        assert!(matches!(
            consume(temp.path(), &binding(), false, 4, &mut source),
            Err(Error::Superseded)
        ));
    }
    #[test]
    fn corrupt_bytes_and_changed_local_authority_never_acquire_offline() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(calls.clone());
        consume(temp.path(), &binding(), false, 1, &mut source).unwrap();
        let path = binding().directory(temp.path()).unwrap().join("graph.json");
        let mut held: Value = read(&path).unwrap().unwrap();
        held["graph"]["revision"] = json!(999);
        write(&path, &held).unwrap();
        assert!(matches!(
            consume(temp.path(), &binding(), true, 2, &mut source),
            Err(Error::Local(_))
        ));
        source.changed = true;
        assert!(matches!(
            consume(temp.path(), &binding(), true, 3, &mut source),
            Err(Error::Source("changed"))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn concurrent_offline_consumers_share_bytes_without_source_calls() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(calls.clone());
        consume(temp.path(), &binding(), false, 1, &mut source).unwrap();
        std::thread::scope(|threads| {
            for _ in 0..8 {
                let calls = calls.clone();
                let root = temp.path();
                threads.spawn(move || {
                    let mut source = Fake::new(calls);
                    assert_eq!(
                        consume(root, &binding(), true, 2, &mut source).unwrap()["revision"],
                        3
                    );
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn body_admission_stops_at_bound() {
        assert!(bounded_bytes(&json!("x".repeat(1024)), 10).is_err());
    }
}
