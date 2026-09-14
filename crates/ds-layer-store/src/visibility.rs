//! Machine-local project-layer visibility preferences for native hosts.
//!
//! The browser remembers a user's layer toggles per project in IndexedDB;
//! `ds` and the desktop shell remember them here, under the same root as the
//! overlay registry. What a toggle WRITES is the kernel's decision
//! (`ds-command-kernel::layer_state`); this module only persists the
//! preferences it returns and reads them back, keyed by lane, DS account and
//! project: a canary project never inherits a stable project's toggles, and
//! two accounts signed in on one host (or one Server) never share theirs.
//!
//! One file, one lock, atomic replacement, an absent file reads as empty and
//! creates nothing, a malformed file is refused rather than repaired.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub const MAX_STORE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PROJECT_ENTRIES: usize = 50_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VisibilityStore {
    pub schema: u32,
    pub revision: u64,
    /// `<lane>/<uid>/<project>` → runtime layer id → desired visibility.
    pub projects: BTreeMap<String, BTreeMap<String, bool>>,
}
impl Default for VisibilityStore {
    fn default() -> Self {
        Self {
            schema: 1,
            revision: 0,
            projects: BTreeMap::new(),
        }
    }
}

fn scope(lane: &str, uid: &str, project: &str) -> Result<String, String> {
    let ok = |value: &str| {
        !value.is_empty()
            && value.trim() == value
            && value.chars().count() <= 500
            && !value.chars().any(|c| c.is_control() || c == '/')
    };
    if !matches!(lane, "stable" | "canary") || !ok(uid) || !ok(project) {
        return Err(
            "layer visibility needs a lane, a bounded account uid and a bounded project id".into(),
        );
    }
    Ok(format!("{lane}/{uid}/{project}"))
}

fn io_error(_: impl std::fmt::Display) -> String {
    "local layer visibility store could not be read or persisted".into()
}

fn load(path: &Path) -> Result<VisibilityStore, String> {
    match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_STORE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            if bytes.len() > MAX_STORE_BYTES {
                return Err("local layer visibility store exceeds 4 MiB".into());
            }
            let store: VisibilityStore = serde_json::from_slice(&bytes).map_err(|_| {
                "local layer visibility store is malformed; repair it before editing"
            })?;
            if store.schema != 1 {
                return Err("local layer visibility store has an unsupported schema".into());
            }
            Ok(store)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(VisibilityStore::default()),
        Err(e) => Err(io_error(e)),
    }
}

/// The remembered toggles for one lane, account and project; empty when none were recorded.
pub fn read(lane: &str, uid: &str, project: &str) -> Result<BTreeMap<String, bool>, String> {
    read_at(&super::default_root()?, lane, uid, project)
}

pub fn read_at(
    root: &Path,
    lane: &str,
    uid: &str,
    project: &str,
) -> Result<BTreeMap<String, bool>, String> {
    let key = scope(lane, uid, project)?;
    let path = root.join("layer-visibility.json");
    if !path.try_exists().map_err(io_error)? {
        return Ok(BTreeMap::new());
    }
    let lock = OpenOptions::new()
        .read(true)
        .open(root.join("layer-visibility.lock"))
        .ok();
    if let Some(lock) = &lock {
        lock.lock_shared().map_err(io_error)?;
    }
    Ok(load(&path)?.projects.remove(&key).unwrap_or_default())
}

/// Replace the remembered toggles for one lane, account and project with what
/// the kernel returned. Returns the store revision after the write.
pub fn replace(
    lane: &str,
    uid: &str,
    project: &str,
    preferences: &BTreeMap<String, bool>,
) -> Result<Value, String> {
    replace_at(&super::default_root()?, lane, uid, project, preferences)
}

pub fn replace_at(
    root: &Path,
    lane: &str,
    uid: &str,
    project: &str,
    preferences: &BTreeMap<String, bool>,
) -> Result<Value, String> {
    update_at(root, lane, uid, project, |_| {
        Ok::<_, String>((preferences.clone(), ()))
    })
    .map(|(receipt, ())| receipt)
    .map_err(|error| match error {
        UpdateError::Store(message) | UpdateError::Transition(message) => message,
    })
}

#[derive(Debug)]
pub enum UpdateError<E> {
    Store(String),
    Transition(E),
}

/// Read, decide and persist under one lock. The caller supplies the shared
/// kernel transition; concurrent requests cannot replace one another's edits
/// with preferences computed from an old snapshot.
pub fn update_at<T, E>(
    root: &Path,
    lane: &str,
    uid: &str,
    project: &str,
    update: impl FnOnce(&BTreeMap<String, bool>) -> Result<(BTreeMap<String, bool>, T), E>,
) -> Result<(Value, T), UpdateError<E>> {
    let store_error = |error| UpdateError::Store(io_error(error));
    let key = scope(lane, uid, project).map_err(UpdateError::Store)?;
    fs::create_dir_all(root).map_err(&store_error)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("layer-visibility.lock"))
        .map_err(&store_error)?;
    lock.lock_exclusive().map_err(&store_error)?;
    let path = root.join("layer-visibility.json");
    let mut store = load(&path).map_err(UpdateError::Store)?;
    let previous = store.projects.get(&key).cloned().unwrap_or_default();
    let (preferences, output) = update(&previous).map_err(UpdateError::Transition)?;
    if preferences.len() > MAX_PROJECT_ENTRIES
        || preferences
            .keys()
            .any(|id| id.is_empty() || id.chars().count() > 2000)
    {
        return Err(UpdateError::Store(
            "layer visibility preferences exceed their bound".into(),
        ));
    }
    let changed = store.projects.get(&key) != Some(&preferences);
    if changed {
        store.projects.insert(key.clone(), preferences.clone());
        store.revision = store
            .revision
            .checked_add(1)
            .ok_or_else(|| UpdateError::Store("layer visibility revision exhausted".into()))?;
        let bytes =
            serde_json::to_vec(&store).map_err(|error| UpdateError::Store(io_error(error)))?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(UpdateError::Store(
                "local layer visibility store exceeds 4 MiB".into(),
            ));
        }
        let mut staged = tempfile::NamedTempFile::new_in(root).map_err(&store_error)?;
        staged.write_all(&bytes).map_err(&store_error)?;
        staged.flush().map_err(&store_error)?;
        staged.as_file().sync_all().map_err(&store_error)?;
        staged
            .persist(&path)
            .map_err(|error| UpdateError::Store(io_error(error)))?;
        #[cfg(unix)]
        File::open(root)
            .and_then(|f| f.sync_all())
            .map_err(&store_error)?;
    }
    Ok((
        json!({
            "scope": key,
            "changed": changed,
            "revision": store.revision,
            "persisted": "native_local",
            "layer_count": preferences.len(),
        }),
        output,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(pairs: &[(&str, bool)]) -> BTreeMap<String, bool> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    #[test]
    fn legacy_unowned_preferences_are_preserved_but_never_assigned_to_an_account() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("layer-visibility.json");
        fs::write(
            &path,
            br#"{"schema":1,"revision":7,"projects":{"canary/p1":{"roads":false}}}"#,
        )
        .unwrap();
        assert!(read_at(tmp.path(), "canary", "u1", "p1")
            .unwrap()
            .is_empty());
        replace_at(
            tmp.path(),
            "canary",
            "u1",
            "p1",
            &prefs(&[("poles", false)]),
        )
        .unwrap();
        let stored = load(&path).unwrap();
        assert_eq!(stored.projects["canary/p1"], prefs(&[("roads", false)]));
        assert!(read_at(tmp.path(), "canary", "u2", "p1")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn same_scope_atomic_updates_preserve_all_concurrent_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(12));
        std::thread::scope(|threads| {
            for n in 0..12 {
                let barrier = barrier.clone();
                let root = tmp.path();
                threads.spawn(move || {
                    barrier.wait();
                    update_at(root, "canary", "u1", "p1", |previous| {
                        let mut next = previous.clone();
                        next.insert(format!("layer-{n}"), false);
                        Ok::<_, String>((next, ()))
                    })
                    .unwrap();
                });
            }
        });
        let saved = read_at(tmp.path(), "canary", "u1", "p1").unwrap();
        assert_eq!(saved.len(), 12);
        assert!(saved.values().all(|visible| !visible));
    }

    #[test]
    fn refused_transition_preserves_the_previous_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        replace_at(
            tmp.path(),
            "canary",
            "u1",
            "p1",
            &prefs(&[("roads", false)]),
        )
        .unwrap();
        let path = tmp.path().join("layer-visibility.json");
        let before = fs::read(&path).unwrap();
        let rejected = update_at::<(), _>(tmp.path(), "canary", "u1", "p1", |_| Err("refused"));
        assert!(matches!(rejected, Err(UpdateError::Transition("refused"))));
        assert_eq!(fs::read(path).unwrap(), before);
    }

    #[test]
    fn preferences_survive_restart_and_are_fenced_by_lane_and_project() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert_eq!(
            read_at(root, "canary", "u1", "p1").unwrap(),
            BTreeMap::new()
        );
        assert!(
            !root.join("layer-visibility.json").exists(),
            "a read creates nothing"
        );
        let receipt = replace_at(
            root,
            "canary",
            "u1",
            "p1",
            &prefs(&[("roads", false), ("poles", true)]),
        )
        .unwrap();
        assert_eq!(receipt["changed"], true);
        assert_eq!(receipt["revision"], 1);
        assert_eq!(
            read_at(root, "canary", "u1", "p1").unwrap(),
            prefs(&[("roads", false), ("poles", true)])
        );
        assert_eq!(
            read_at(root, "stable", "u1", "p1").unwrap(),
            BTreeMap::new(),
            "another lane sees nothing"
        );
        assert_eq!(
            read_at(root, "canary", "u1", "p2").unwrap(),
            BTreeMap::new(),
            "another project sees nothing"
        );
        assert_eq!(
            read_at(root, "canary", "u2", "p1").unwrap(),
            BTreeMap::new(),
            "another account on the same host sees nothing"
        );
        let again = replace_at(
            root,
            "canary",
            "u1",
            "p1",
            &prefs(&[("roads", false), ("poles", true)]),
        )
        .unwrap();
        assert_eq!(again["changed"], false);
        assert_eq!(again["revision"], 1, "an identical write is not a revision");
    }

    #[test]
    fn concurrent_writers_keep_every_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            for n in 0..12 {
                let root = tmp.path();
                scope.spawn(move || {
                    replace_at(
                        root,
                        "canary",
                        "u1",
                        &format!("p{n}"),
                        &prefs(&[("roads", n % 2 == 0)]),
                    )
                    .unwrap();
                });
            }
        });
        for n in 0..12 {
            assert_eq!(
                read_at(tmp.path(), "canary", "u1", &format!("p{n}")).unwrap(),
                prefs(&[("roads", n % 2 == 0)])
            );
        }
    }

    #[test]
    fn malformed_store_and_bad_scopes_are_refused_not_repaired() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("layer-visibility.json"), b"{not json").unwrap();
        assert!(read_at(tmp.path(), "canary", "u1", "p1")
            .unwrap_err()
            .contains("malformed"));
        assert!(
            replace_at(tmp.path(), "canary", "u1", "p1", &prefs(&[("a", true)]))
                .unwrap_err()
                .contains("malformed")
        );
        assert_eq!(
            fs::read(tmp.path().join("layer-visibility.json")).unwrap(),
            b"{not json".to_vec()
        );
        assert!(
            read_at(tmp.path(), "prod", "u1", "p1").is_err(),
            "an unknown lane is refused"
        );
        assert!(
            read_at(tmp.path(), "canary", "u1", "a/b").is_err(),
            "a project id with a separator is refused"
        );
        assert!(read_at(tmp.path(), "canary", "u1", " p1").is_err());
    }
}
