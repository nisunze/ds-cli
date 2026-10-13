//! Fixed retained PM rooms through one existing native persistence owner.
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
const MAX_NOTE_BYTES: usize = 2 * 1024 * 1024;
const MAX_TASK_BYTES: usize = 8 * 1024 * 1024;
const TASK_SCHEMA: &str = "ds.pm.native-task-read/v1";
const NOTE_SCHEMA: &str = "ds.pm.native-note-read/v1";
use ds_command_kernel::project_management::project_notes::{
    Command as NoteCommand, Read as NoteRead,
};

/// One exact task consumer; a changed timeline flag is a different selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRead {
    pub task_id: String,
    pub timeline: bool,
}

#[derive(Clone)]
enum Room {
    Graph,
    NoteRead(NoteRead),
    TaskRead(TaskRead),
}
impl Room {
    fn file(&self) -> &str {
        match self {
            Self::Graph => "graph.json",
            Self::NoteRead(_) => "note-read.json",
            Self::TaskRead(_) => "task-read.json",
        }
    }
    fn schema(&self) -> &str {
        match self {
            Self::Graph => SCHEMA,
            Self::NoteRead(_) => NOTE_SCHEMA,
            Self::TaskRead(_) => TASK_SCHEMA,
        }
    }
    fn bound(&self) -> usize {
        match self {
            Self::Graph => MAX_BYTES,
            Self::NoteRead(_) => MAX_NOTE_BYTES,
            Self::TaskRead(_) => MAX_TASK_BYTES,
        }
    }
    fn serial(&self, state: &State) -> u64 {
        match self {
            Self::Graph => state.serial,
            Self::NoteRead(_) => state.note_serial,
            Self::TaskRead(_) => state.task_serial,
        }
    }
    fn advance(&self, state: &mut State) -> Result<(), String> {
        let serial = match self {
            Self::Graph => &mut state.serial,
            Self::NoteRead(_) => &mut state.note_serial,
            Self::TaskRead(_) => &mut state.task_serial,
        };
        *serial = serial
            .checked_add(1)
            .ok_or("pm_retained_generation_exhausted")?;
        Ok(())
    }
    fn selection(&self) -> Result<Value, String> {
        match self {
            Self::Graph => Ok(Value::Null),
            Self::TaskRead(read) => {
                serde_json::to_value(read).map_err(|_| "pm_retained_selection_invalid".into())
            }
            Self::NoteRead(read) => {
                serde_json::to_value(read).map_err(|_| "pm_retained_selection_invalid".into())
            }
        }
    }
    fn selection_sha(&self) -> Result<Option<String>, String> {
        match self {
            Self::Graph => Ok(None),
            Self::NoteRead(_) | Self::TaskRead(_) => digest(&self.selection()?).map(Some),
        }
    }
}
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
    #[serde(default, skip_serializing_if = "is_zero")]
    note_serial: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    task_serial: u64,
    denied: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    revoked_before_epoch: u64,
}
fn is_zero(value: &u64) -> bool {
    *value == 0
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
    // Preserve the established graph wire field for existing Plan captures.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    graph: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    note_read: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    task_read: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    selection: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selection_sha256: Option<String>,
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
#[cfg(test)]
fn read<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<Option<T>, String> {
    read_bounded(path, MAX_BYTES + 32768)
}
fn read_bounded<T: for<'a> Deserialize<'a>>(path: &Path, cap: usize) -> Result<Option<T>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("pm_retained_unreadable".into()),
    };
    let mut bytes = Vec::new();
    file.take((cap + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "pm_retained_unreadable")?;
    if bytes.len() > cap {
        return Err("pm_retained_unreadable".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "pm_retained_unreadable".into())
}
fn write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    write_bounded(path, value, MAX_BYTES + 32768)
}
fn write_bounded(path: &Path, value: &impl Serialize, cap: usize) -> Result<(), String> {
    let bytes = bounded_bytes(value, cap)?;
    private::write_atomic(path, bytes).map_err(|_| "pm_retained_unavailable".into())
}
fn state(dir: &Path) -> Result<State, String> {
    let mut current: State = read_bounded(&dir.join("state.json"), 32768)?.unwrap_or_default();
    // Legacy denied state already carries the denial epoch, even without a separate floor.
    if current.denied {
        current.revoked_before_epoch = current.revoked_before_epoch.max(current.epoch);
    }
    Ok(current)
}
impl Held {
    fn body(&self, room: &Room) -> &Value {
        match room {
            Room::Graph => &self.graph,
            Room::NoteRead(_) => &self.note_read,
            Room::TaskRead(_) => &self.task_read,
        }
    }
    fn admitted(&self, binding: &Binding, room: &Room, current: &State) -> Result<bool, String> {
        let other_body_absent = match room {
            Room::Graph => self.note_read.is_null() && self.task_read.is_null(),
            Room::NoteRead(_) => self.graph.is_null() && self.task_read.is_null(),
            Room::TaskRead(_) => self.graph.is_null() && self.note_read.is_null(),
        };
        Ok(self.schema == room.schema()
            && self.binding == *binding
            && self.captured_at_ms >= 0
            && self.serial == room.serial(current)
            && self.epoch <= current.epoch
            && self.epoch >= current.revoked_before_epoch
            && other_body_absent
            && self.selection == room.selection()?
            && self.selection_sha256 == room.selection_sha()?
            && self.sha256 == digest(self.body(room))?
            && bounded_bytes(self.body(room), room.bound()).is_ok())
    }
}
fn metadata(held: Held, room: &Room, current: &State, acquired: bool) -> Value {
    let stale = !ds_command_kernel::freshness::is_fresh(Some(&held.epoch), Some(&current.epoch));
    let mut result = match room {
        Room::Graph => held.graph,
        Room::NoteRead(_) => held.note_read,
        Room::TaskRead(_) => held.task_read,
    };
    let mut retained = json!({"schema":if matches!(room, Room::Graph) {"ds.pm.retained-graph/v1"} else if matches!(room, Room::NoteRead(_)) {"ds.pm.retained-note-read/v1"} else {"ds.pm.retained-task-read/v1"},
        "mode":if acquired {"acquired"} else {"offline_capture"}, "captured_at_ms":held.captured_at_ms,
        "known_invalidated":stale,"current_source_verified":acquired,
        "source_freshness":if acquired {"verified_at_capture"} else {"unknown"},
        "current_authorization_verified":acquired,"permission_freshness":if acquired {"verified_at_capture"} else {"captured_only"}});
    match room {
        Room::Graph => {
            retained["graph_sha256"] = json!(held.sha256);
            retained["graph_revision"] = result["revision"].clone();
            retained["schedule_revision"] = result["schedule_revision"].clone();
        }
        Room::TaskRead(_) => {
            retained["body_sha256"] = json!(held.sha256);
            retained["selection"] = held.selection;
            retained["selection_sha256"] = json!(held.selection_sha256);
            retained["source_revisions"] = json!({
                "graph": result["graph"]["revision"],
                "records_context": result["records_context"]["revision"],
                "timeline": result["timeline_receipt"]["revision"],
            });
            retained["snapshot_consistency"] = json!("independent_reads");
        }
        Room::NoteRead(_) => {
            retained["body_sha256"] = json!(held.sha256);
            retained["selection"] = held.selection;
            retained["selection_sha256"] = json!(held.selection_sha256);
            retained["note_id"] = result["note"]["id"].clone();
            retained["note_version"] = result["note"]["version"].clone();
        }
    }
    result["retained"] = retained;
    result
}
fn verify_body<S: Source>(
    binding: &Binding,
    room: &Room,
    value: &Value,
    source: &S,
) -> Result<(), Error<S::Error>> {
    if let Room::NoteRead(selection) = room {
        NoteCommand::Read(selection.clone())
            .verify_response(&binding.project, value)
            .map_err(Error::Local)?;
    }
    if let Room::TaskRead(selection) = room {
        if value["project_id"] != binding.project
            || value["selection"] != room.selection().map_err(Error::Local)?
            || value["records_context"]["task_id"] != selection.task_id
            || value["graph"]["tasks"].as_array().is_none_or(|tasks| {
                !tasks
                    .iter()
                    .any(|task| task["id"] == selection.task_id && task["is_deleted"] != true)
            })
            || selection.timeline != value.get("timeline_receipt").is_some()
        {
            return Err(Error::Local("pm_retained_task_selection_invalid".into()));
        }
    }
    source.verify(value).map_err(Error::Source)
}
/// Normal reads still acquire one canonical graph. Offline never acquires or creates holdings.
pub fn consume<S: Source>(
    root: &Path,
    binding: &Binding,
    offline: bool,
    now_ms: i64,
    source: &mut S,
) -> Result<Value, Error<S::Error>> {
    consume_selected(root, binding, &Room::Graph, offline, now_ms, source)
}
/// One fixed selected-note room. Replacing its selection never grows a per-note cache.
pub fn consume_note<S: Source>(
    root: &Path,
    binding: &Binding,
    selection: &NoteRead,
    offline: bool,
    now_ms: i64,
    source: &mut S,
) -> Result<Value, Error<S::Error>> {
    NoteCommand::Read(selection.clone())
        .payload(&binding.project)
        .map_err(Error::Local)?;
    consume_selected(
        root,
        binding,
        &Room::NoteRead(selection.clone()),
        offline,
        now_ms,
        source,
    )
}
/// One fixed selected-task room; bodies are retained only for an admitted actual consumer.
pub fn consume_task<S: Source>(
    root: &Path,
    binding: &Binding,
    selection: &TaskRead,
    offline: bool,
    now_ms: i64,
    source: &mut S,
) -> Result<Value, Error<S::Error>> {
    if selection.task_id.is_empty()
        || selection.task_id.len() > 2048
        || selection.task_id.trim() != selection.task_id
        || selection.task_id.chars().any(char::is_control)
    {
        return Err(Error::Local("pm_retained_selection_invalid".into()));
    }
    consume_selected(
        root,
        binding,
        &Room::TaskRead(selection.clone()),
        offline,
        now_ms,
        source,
    )
}
fn consume_selected<S: Source>(
    root: &Path,
    binding: &Binding,
    room: &Room,
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
            .join(room.file())
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
        let held: Held = read_bounded(&dir.join(room.file()), room.bound() + 32768)
            .map_err(local)?
            .ok_or(Error::AcquisitionRequired)?;
        // Replacement and known revocation require new acquisition, never old-room resurrection.
        if held.epoch < state.revoked_before_epoch
            || held.selection != room.selection().map_err(local)?
        {
            return Err(Error::AcquisitionRequired);
        }
        if !held.admitted(binding, room, &state).map_err(local)? {
            return Err(local("pm_retained_unreadable".into()));
        }
        verify_body(binding, room, held.body(room), source)?;
        source.check_binding().map_err(Error::Source)?;
        return Ok(metadata(held, room, &state, false));
    }
    // Observe generation before waiting, so overlapping intentional acquisitions coalesce.
    let observed = room.serial(&state(&dir).map_err(local)?);
    let _capture_lock = exclusive_lock(&dir.join("capture.lock")).map_err(local)?;
    let before = {
        let _lock = exclusive_lock(&dir.join("state.lock")).map_err(local)?;
        let before = state(&dir).map_err(local)?;
        if !dir.join("state.json").exists() {
            write(&dir.join("state.json"), &before).map_err(local)?;
        }
        if room.serial(&before) != observed && !before.denied {
            if let Some(held) =
                read_bounded::<Held>(&dir.join(room.file()), room.bound() + 32768).map_err(local)?
            {
                if held.admitted(binding, room, &before).map_err(local)?
                    && held.epoch == before.epoch
                {
                    verify_body(binding, room, held.body(room), source)?;
                    source.check_binding().map_err(Error::Source)?;
                    return Ok(metadata(held, room, &before, true));
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
    verify_body(binding, room, &graph, source)?;
    bounded_bytes(&graph, room.bound()).map_err(local)?;
    let _lock = exclusive_lock(&dir.join("state.lock")).map_err(local)?;
    let mut current = state(&dir).map_err(local)?;
    if current.epoch != before.epoch || room.serial(&current) != room.serial(&before) {
        return Err(Error::Superseded);
    }
    source.check_binding().map_err(Error::Source)?;
    room.advance(&mut current).map_err(local)?;
    current.denied = false;
    let body_sha256 = digest(&graph).map_err(local)?;
    let (graph, note_read, task_read) = match room {
        Room::Graph => (graph, Value::Null, Value::Null),
        Room::NoteRead(_) => (Value::Null, graph, Value::Null),
        Room::TaskRead(_) => (Value::Null, Value::Null, graph),
    };
    let held = Held {
        schema: room.schema().into(),
        binding: binding.clone(),
        epoch: current.epoch,
        serial: room.serial(&current),
        captured_at_ms: now_ms,
        sha256: body_sha256,
        graph,
        note_read,
        task_read,
        selection: room.selection().map_err(local)?,
        selection_sha256: room.selection_sha().map_err(local)?,
    };
    write_bounded(&dir.join(room.file()), &held, room.bound() + 32768).map_err(local)?;
    write(&dir.join("state.json"), &current).map_err(local)?;
    Ok(metadata(held, room, &current, true))
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
    if denied {
        current.revoked_before_epoch = current.epoch;
    }
    write(&dir.join("state.json"), &current)?;
    // Denial is durable even if deletion is impossible. Offline observes the marker first.
    if denied {
        for file in ["graph.json", "note-read.json", "task-read.json"] {
            let _ = std::fs::remove_file(dir.join(file));
        }
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
    fn note_selection(id: &str, version: Option<u64>, claim: Option<&str>) -> NoteRead {
        NoteRead {
            id: id.into(),
            version,
            claim_id: claim.map(str::to_owned),
        }
    }
    struct NoteSource {
        calls: Arc<AtomicUsize>,
        selection: NoteRead,
        fail: Option<&'static str>,
        mutate: Option<PathBuf>,
        changed: bool,
    }
    impl NoteSource {
        fn new(calls: Arc<AtomicUsize>, selection: NoteRead) -> Self {
            Self {
                calls,
                selection,
                fail: None,
                mutate: None,
                changed: false,
            }
        }
        fn response(&self) -> Value {
            let mut result = json!({"project_id":"p", "note":{"project_id":"p", "id":self.selection.id,
                "version":self.selection.version.unwrap_or(3),"document":{"category":"project_note","body":"retained"},
                "memory":{},"legacy":true}});
            if let Some(id) = &self.selection.claim_id {
                result["claim"] = json!({"id":id});
            }
            result
        }
    }
    impl Source for NoteSource {
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
            if let Some(root) = &self.mutate {
                mark(root, &binding(), false).unwrap();
            }
            if let Some(error) = self.fail {
                return Err(error);
            }
            Ok(self.response())
        }
        fn verify(&self, value: &Value) -> Result<(), Self::Error> {
            NoteCommand::Read(self.selection.clone())
                .verify_response("p", value)
                .map_err(|_| "wrong_note")
        }
        fn access_refused(&self, error: &Self::Error) -> bool {
            *error == "denied"
        }
    }
    #[test]
    fn note_retained_selection_is_exact_and_has_zero_offline_source_calls() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let selection = note_selection("n", Some(2), Some("claim"));
        let mut source = NoteSource::new(calls.clone(), selection.clone());
        let captured =
            consume_note(temp.path(), &binding(), &selection, false, 1, &mut source).unwrap();
        assert_eq!(
            captured["retained"]["selection"],
            json!({"id":"n","version":2,"claim_id":"claim"})
        );
        assert_eq!(captured["retained"]["note_version"], 2);
        assert_eq!(captured["retained"]["current_source_verified"], true);
        for _ in 0..8 {
            let held =
                consume_note(temp.path(), &binding(), &selection, true, 2, &mut source).unwrap();
            assert_eq!(held["note"]["document"]["body"], "retained");
            assert_eq!(held["claim"]["id"], "claim");
            assert_eq!(held["retained"]["source_freshness"], "unknown");
            assert_eq!(held["retained"]["permission_freshness"], "captured_only");
            assert_eq!(held["retained"]["current_authorization_verified"], false);
        }
        for other in [
            note_selection("other", Some(2), Some("claim")),
            note_selection("n", Some(3), Some("claim")),
            note_selection("n", None, Some("claim")),
            note_selection("n", Some(2), Some("other")),
            note_selection("n", Some(2), None),
        ] {
            let mut other_source = NoteSource::new(calls.clone(), other.clone());
            assert!(matches!(
                consume_note(temp.path(), &binding(), &other, true, 2, &mut other_source),
                Err(Error::AcquisitionRequired)
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        #[cfg(unix)]
        crate::private::assert_private_tree(&temp.path().join("pm-graph"));
    }
    #[test]
    fn graph_and_last_note_rooms_have_independent_generations_and_fixed_disk_budget() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut graph_source = Fake::new(calls.clone());
        consume(temp.path(), &binding(), false, 1, &mut graph_source).unwrap();
        for n in 0..20 {
            let selection = note_selection(&format!("n{n}"), None, None);
            let mut source = NoteSource::new(calls.clone(), selection.clone());
            consume_note(temp.path(), &binding(), &selection, false, 2, &mut source).unwrap();
            assert_eq!(
                consume(temp.path(), &binding(), true, 3, &mut graph_source).unwrap()["revision"],
                3
            );
        }
        let last = note_selection("n19", None, None);
        let mut source = NoteSource::new(calls.clone(), last.clone());
        consume(temp.path(), &binding(), false, 4, &mut graph_source).unwrap();
        assert_eq!(
            consume_note(temp.path(), &binding(), &last, true, 5, &mut source).unwrap()["note"]
                ["id"],
            "n19"
        );
        let old = note_selection("n0", None, None);
        assert!(matches!(
            consume_note(temp.path(), &binding(), &old, true, 5, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        let dir = binding().directory(temp.path()).unwrap();
        let mut payloads: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".json") && n != "state.json")
            .collect();
        payloads.sort();
        assert_eq!(
            payloads,
            vec!["graph.json", "note-read.json"]
        );
        assert!(
            std::fs::metadata(dir.join("graph.json")).unwrap().len() <= (MAX_BYTES + 32768) as u64
        );
        assert!(
            std::fs::metadata(dir.join("note-read.json")).unwrap().len()
                <= (MAX_NOTE_BYTES + 32768) as u64
        );
    }
    #[test]
    fn both_rooms_share_invalidation_and_durable_denial_without_unused_creation() {
        let temp = tempfile::tempdir().unwrap();
        let absent = temp.path().join("unused");
        let calls = Arc::new(AtomicUsize::new(0));
        let selection = note_selection("n", None, None);
        let mut source = NoteSource::new(calls.clone(), selection.clone());
        assert!(matches!(
            consume_note(&absent, &binding(), &selection, true, 1, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert!(!absent.exists());
        let mut graph_source = Fake::new(calls.clone());
        consume(temp.path(), &binding(), false, 1, &mut graph_source).unwrap();
        consume_note(temp.path(), &binding(), &selection, false, 1, &mut source).unwrap();
        mark(temp.path(), &binding(), false).unwrap();
        assert_eq!(
            consume(temp.path(), &binding(), true, 2, &mut graph_source).unwrap()["retained"]
                ["known_invalidated"],
            true
        );
        assert_eq!(
            consume_note(temp.path(), &binding(), &selection, true, 2, &mut source).unwrap()
                ["retained"]["known_invalidated"],
            true
        );
        let note_path = binding()
            .directory(temp.path())
            .unwrap()
            .join("note-read.json");
        std::fs::remove_file(&note_path).unwrap();
        std::fs::create_dir(&note_path).unwrap();
        mark(temp.path(), &binding(), true).unwrap();
        assert!(matches!(
            consume(temp.path(), &binding(), true, 3, &mut graph_source),
            Err(Error::AcquisitionRequired)
        ));
        assert!(matches!(
            consume_note(temp.path(), &binding(), &selection, true, 3, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert!(note_path.is_dir());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn note_absence_preserves_graph_access_but_authoritative_denial_revokes_it() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut graph_source = Fake::new(calls.clone());
        consume(temp.path(), &binding(), false, 1, &mut graph_source).unwrap();
        let selection = note_selection("n", None, None);
        let mut source = NoteSource::new(calls.clone(), selection.clone());
        source.fail = Some("missing");
        assert!(matches!(
            consume_note(temp.path(), &binding(), &selection, false, 2, &mut source),
            Err(Error::Source("missing"))
        ));
        assert!(consume(temp.path(), &binding(), true, 3, &mut graph_source).is_ok());
        source.fail = Some("denied");
        assert!(matches!(
            consume_note(temp.path(), &binding(), &selection, false, 4, &mut source),
            Err(Error::Source("denied"))
        ));
        assert!(matches!(
            consume(temp.path(), &binding(), true, 5, &mut graph_source),
            Err(Error::AcquisitionRequired)
        ));
    }
    #[test]
    fn note_scope_corruption_bounds_and_capture_mutation_are_fenced() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let selection = note_selection("n", Some(2), None);
        let mut source = NoteSource::new(calls.clone(), selection.clone());
        consume_note(temp.path(), &binding(), &selection, false, 1, &mut source).unwrap();
        for field in 0..5 {
            let mut other = binding();
            match field {
                0 => other.uid = "other".into(),
                1 => other.lane = "stable".into(),
                2 => other.audience = "other".into(),
                3 => other.credential_instance = "other".into(),
                _ => other.project = "other".into(),
            }
            assert!(matches!(
                consume_note(temp.path(), &other, &selection, true, 2, &mut source),
                Err(Error::AcquisitionRequired)
            ));
        }
        let path = binding()
            .directory(temp.path())
            .unwrap()
            .join("note-read.json");
        let mut held: Value = read(&path).unwrap().unwrap();
        held["note_read"]["note"]["version"] = json!(9);
        write(&path, &held).unwrap();
        assert!(matches!(
            consume_note(temp.path(), &binding(), &selection, true, 2, &mut source),
            Err(Error::Local(_))
        ));
        source.changed = true;
        assert!(matches!(
            consume_note(temp.path(), &binding(), &selection, true, 2, &mut source),
            Err(Error::Source("changed"))
        ));
        source.changed = false;
        source.mutate = Some(temp.path().into());
        assert!(matches!(
            consume_note(temp.path(), &binding(), &selection, false, 3, &mut source),
            Err(Error::Superseded)
        ));
        assert!(bounded_bytes(&json!("x".repeat(MAX_NOTE_BYTES + 1)), MAX_NOTE_BYTES).is_err());
        let invalid = note_selection("../escape", None, None);
        assert!(matches!(
            consume_note(temp.path(), &binding(), &invalid, true, 3, &mut source),
            Err(Error::Local(_))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn legacy_graph_state_and_capture_still_admit_after_note_extension() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(calls.clone());
        consume(temp.path(), &binding(), false, 1, &mut source).unwrap();
        let dir = binding().directory(temp.path()).unwrap();
        let mut held: Value = read(&dir.join("graph.json")).unwrap().unwrap();
        for key in ["note_read", "selection", "selection_sha256"] {
            held.as_object_mut().unwrap().remove(key);
        }
        write(&dir.join("graph.json"), &held).unwrap();
        write(
            &dir.join("state.json"),
            &json!({"epoch":0,"serial":1,"denied":false}),
        )
        .unwrap();
        assert_eq!(
            consume(temp.path(), &binding(), true, 2, &mut source).unwrap()["revision"],
            3
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn concurrent_selected_note_captures_coalesce_then_share_offline_bytes() {
        struct Concurrent {
            inner: NoteSource,
            start: Arc<std::sync::Barrier>,
            initial: bool,
        }
        impl Source for Concurrent {
            type Error = &'static str;
            fn check_binding(&mut self) -> Result<(), Self::Error> {
                if self.initial {
                    self.initial = false;
                    self.start.wait();
                }
                self.inner.check_binding()
            }
            fn capture(&mut self) -> Result<Value, Self::Error> {
                // Hold the source open while all intentional readers observe the old room serial.
                std::thread::sleep(std::time::Duration::from_millis(150));
                self.inner.capture()
            }
            fn verify(&self, value: &Value) -> Result<(), Self::Error> {
                self.inner.verify(value)
            }
            fn access_refused(&self, error: &Self::Error) -> bool {
                self.inner.access_refused(error)
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let selection = note_selection("n", Some(2), Some("claim"));
        std::thread::scope(|threads| {
            for _ in 0..8 {
                let calls = calls.clone();
                let barrier = barrier.clone();
                let selection = selection.clone();
                let root = temp.path();
                threads.spawn(move || {
                    let mut source = Concurrent {
                        inner: NoteSource::new(calls, selection.clone()),
                        start: barrier,
                        initial: true,
                    };
                    assert_eq!(
                        consume_note(root, &binding(), &selection, false, 1, &mut source).unwrap()
                            ["note"]["version"],
                        2
                    );
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        std::thread::scope(|threads| {
            for _ in 0..8 {
                let calls = calls.clone();
                let selection = selection.clone();
                let root = temp.path();
                threads.spawn(move || {
                    let mut source = NoteSource::new(calls, selection.clone());
                    assert_eq!(
                        consume_note(root, &binding(), &selection, true, 2, &mut source).unwrap()
                            ["claim"]["id"],
                        "claim"
                    );
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn one_room_reacquisition_cannot_resurrect_other_revoked_capture() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut graph_source = Fake::new(calls.clone());
        consume(temp.path(), &binding(), false, 1, &mut graph_source).unwrap();
        let dir = binding().directory(temp.path()).unwrap();
        let old_graph = std::fs::read(dir.join("graph.json")).unwrap();
        mark(temp.path(), &binding(), true).unwrap();
        // Recreate the exact bytes that a failed best-effort cleanup would have left.
        private::write_atomic(&dir.join("graph.json"), old_graph).unwrap();
        let selection = note_selection("n", None, None);
        let mut source = NoteSource::new(calls.clone(), selection.clone());
        consume_note(temp.path(), &binding(), &selection, false, 2, &mut source).unwrap();
        assert!(consume_note(temp.path(), &binding(), &selection, true, 3, &mut source).is_ok());
        assert!(matches!(
            consume(temp.path(), &binding(), true, 3, &mut graph_source),
            Err(Error::AcquisitionRequired)
        ));
        consume(temp.path(), &binding(), false, 4, &mut graph_source).unwrap();
        assert!(consume(temp.path(), &binding(), true, 5, &mut graph_source).is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn selected_note_receipt_is_checked_by_owner_even_if_source_accepts_another_selection() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let selected = note_selection("wanted", Some(2), Some("claim"));
        let mut source = NoteSource::new(
            calls.clone(),
            note_selection("other", Some(3), Some("other-claim")),
        );
        assert!(matches!(
            consume_note(temp.path(), &binding(), &selected, false, 1, &mut source),
            Err(Error::Local(_))
        ));
        assert!(!binding()
            .directory(temp.path())
            .unwrap()
            .join("note-read.json")
            .exists());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    fn task_selection(id: &str, timeline: bool) -> TaskRead {
        TaskRead {
            task_id: id.into(),
            timeline,
        }
    }
    fn task_capture(selection: &TaskRead) -> Value {
        let mut capture = json!({"project_id":"p","selection":selection,
            "graph":{"tasks":[{"id":selection.task_id}],"revision":3},
            "records_context":{"project_id":"p","task_id":selection.task_id,"records":[],
                "truncated_by_collection":{"pm_records":true},"next_cursors":{"pm_records":"scan-boundary"}}});
        if selection.timeline {
            capture["timeline_receipt"] = json!({"revision":4,"timeline":{"task_id":selection.task_id,"total":0,"entries":[],"truncated":true}});
        }
        capture
    }
    struct TaskSource {
        selection: TaskRead,
        calls: Arc<AtomicUsize>,
        failure: Option<&'static str>,
        mutation: Option<PathBuf>,
        barrier: Option<Arc<std::sync::Barrier>>,
    }
    impl TaskSource {
        fn new(calls: Arc<AtomicUsize>, selection: TaskRead) -> Self {
            Self {
                selection,
                calls,
                failure: None,
                mutation: None,
                barrier: None,
            }
        }
    }
    impl Source for TaskSource {
        type Error = &'static str;
        fn check_binding(&mut self) -> Result<(), Self::Error> {
            if let Some(barrier) = self.barrier.take() {
                barrier.wait();
            }
            Ok(())
        }
        fn capture(&mut self) -> Result<Value, Self::Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(failure) = self.failure {
                return Err(failure);
            }
            if let Some(root) = &self.mutation {
                mark(root, &binding(), false).unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(80));
            Ok(task_capture(&self.selection))
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
    fn task_offline_preserves_partial_and_distinct_revisions_without_acquisition() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let selection = task_selection("t", true);
        let mut source = TaskSource::new(calls.clone(), selection.clone());
        let first =
            consume_task(temp.path(), &binding(), &selection, false, 1, &mut source).unwrap();
        assert_eq!(
            first["retained"]["source_revisions"],
            json!({"graph":3,"records_context":null,"timeline":4})
        );
        assert_eq!(
            first["retained"]["snapshot_consistency"],
            "independent_reads"
        );
        let held = consume_task(temp.path(), &binding(), &selection, true, 2, &mut source).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(held["retained"]["permission_freshness"], "captured_only");
        assert_eq!(
            held["records_context"]["next_cursors"]["pm_records"],
            "scan-boundary"
        );
        assert_eq!(held["timeline_receipt"]["timeline"]["truncated"], true);
        consume_task(temp.path(), &binding(), &selection, false, 3, &mut source).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        #[cfg(unix)]
        crate::private::assert_private_tree(&temp.path().join("pm-graph"));
    }
    #[test]
    fn task_selection_and_authority_cannot_reuse_another_capture_or_create_unused_rooms() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let selection = task_selection("t", false);
        let mut source = TaskSource::new(calls.clone(), selection.clone());
        assert!(matches!(
            consume_task(temp.path(), &binding(), &selection, true, 1, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        consume_task(temp.path(), &binding(), &selection, false, 1, &mut source).unwrap();
        for other in [task_selection("other", false), task_selection("t", true)] {
            assert!(matches!(
                consume_task(temp.path(), &binding(), &other, true, 2, &mut source),
                Err(Error::AcquisitionRequired)
            ));
        }
        for field in 0..5 {
            let mut other = binding();
            match field {
                0 => other.uid = "other".into(),
                1 => other.lane = "stable".into(),
                2 => other.audience = "other".into(),
                3 => other.credential_instance = "other".into(),
                _ => other.project = "other".into(),
            };
            assert!(matches!(
                consume_task(temp.path(), &other, &selection, true, 2, &mut source),
                Err(Error::AcquisitionRequired)
            ));
        }
        let other = task_selection("second", false);
        source.selection = other.clone();
        consume_task(temp.path(), &binding(), &other, false, 3, &mut source).unwrap();
        assert!(matches!(
            consume_task(temp.path(), &binding(), &selection, true, 4, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert_eq!(
            std::fs::read_dir(binding().directory(temp.path()).unwrap())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().ends_with("-read.json"))
                .count(),
            1
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn task_denial_is_durable_before_cleanup_and_other_room_reacquisition() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let selection = task_selection("t", false);
        let mut source = TaskSource::new(calls.clone(), selection.clone());
        consume_task(temp.path(), &binding(), &selection, false, 1, &mut source).unwrap();
        let dir = binding().directory(temp.path()).unwrap();
        let bytes = std::fs::read(dir.join("task-read.json")).unwrap();
        std::fs::remove_file(dir.join("task-read.json")).unwrap();
        std::fs::create_dir(dir.join("task-read.json")).unwrap();
        source.failure = Some("denied");
        assert!(matches!(
            consume_task(temp.path(), &binding(), &selection, false, 2, &mut source),
            Err(Error::Source("denied"))
        ));
        assert!(dir.join("task-read.json").is_dir());
        assert!(matches!(
            consume_task(temp.path(), &binding(), &selection, true, 3, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        std::fs::remove_dir(dir.join("task-read.json")).unwrap();
        private::write_atomic(&dir.join("task-read.json"), bytes).unwrap();
        let mut graph_source = Fake::new(calls.clone());
        consume(temp.path(), &binding(), false, 4, &mut graph_source).unwrap();
        assert!(matches!(
            consume_task(temp.path(), &binding(), &selection, true, 5, &mut source),
            Err(Error::AcquisitionRequired)
        ));
    }
    #[test]
    fn task_absence_does_not_revoke_other_room_and_local_write_is_truthful() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let selection = task_selection("t", false);
        let mut source = TaskSource::new(calls.clone(), selection.clone());
        let mut graph_source = Fake::new(calls.clone());
        consume(temp.path(), &binding(), false, 1, &mut graph_source).unwrap();
        source.failure = Some("missing");
        assert!(matches!(
            consume_task(temp.path(), &binding(), &selection, false, 2, &mut source),
            Err(Error::Source("missing"))
        ));
        assert!(consume(temp.path(), &binding(), true, 3, &mut graph_source).is_ok());
        source.failure = None;
        consume_task(temp.path(), &binding(), &selection, false, 4, &mut source).unwrap();
        mark(temp.path(), &binding(), false).unwrap();
        assert_eq!(
            consume_task(temp.path(), &binding(), &selection, true, 5, &mut source).unwrap()
                ["retained"]["known_invalidated"],
            true
        );
        source.mutation = Some(temp.path().into());
        assert!(matches!(
            consume_task(temp.path(), &binding(), &selection, false, 6, &mut source),
            Err(Error::Superseded)
        ));
    }
    #[test]
    fn concurrent_task_consumers_share_one_acquisition_then_zero_offline_calls() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let selection = task_selection("t", true);
        std::thread::scope(|threads| {
            for _ in 0..8 {
                let calls = calls.clone();
                let barrier = barrier.clone();
                let selection = selection.clone();
                let root = temp.path();
                threads.spawn(move || {
                    let mut source = TaskSource::new(calls, selection.clone());
                    source.barrier = Some(barrier);
                    consume_task(root, &binding(), &selection, false, 1, &mut source).unwrap();
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        for _ in 0..8 {
            let mut source = TaskSource::new(calls.clone(), selection.clone());
            consume_task(temp.path(), &binding(), &selection, true, 2, &mut source).unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn task_owner_refuses_wrong_selection_corruption_and_oversized_body() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let selection = task_selection("t", false);
        let mut source = TaskSource::new(calls.clone(), task_selection("other", false));
        assert!(matches!(
            consume_task(temp.path(), &binding(), &selection, false, 1, &mut source),
            Err(Error::Local(_))
        ));
        source.selection = selection.clone();
        consume_task(temp.path(), &binding(), &selection, false, 2, &mut source).unwrap();
        let path = binding()
            .directory(temp.path())
            .unwrap()
            .join("task-read.json");
        let mut held: Value = read(&path).unwrap().unwrap();
        held["task_read"]["records_context"]["task_id"] = json!("other");
        write(&path, &held).unwrap();
        assert!(matches!(
            consume_task(temp.path(), &binding(), &selection, true, 3, &mut source),
            Err(Error::Local(_))
        ));
        assert!(bounded_bytes(&json!("x".repeat(MAX_TASK_BYTES + 1)), MAX_TASK_BYTES).is_err());
    }
}
