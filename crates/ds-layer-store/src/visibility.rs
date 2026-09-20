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
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

use crate::scoped::{self, Kind};
pub use crate::scoped::{UpdateError, MAX_STORE_BYTES};

pub const MAX_PROJECT_ENTRIES: usize = 50_000;

/// `<lane>/<uid>/<project>` → runtime layer id → desired visibility.
pub type VisibilityStore = scoped::Store<BTreeMap<String, bool>>;

const KIND: Kind<BTreeMap<String, bool>> = Kind {
    file: "layer-visibility.json",
    lock: "layer-visibility.lock",
    noun: "layer visibility",
    validate: |preferences| {
        if preferences.len() > MAX_PROJECT_ENTRIES
            || preferences
                .keys()
                .any(|id| id.is_empty() || id.chars().count() > 2000)
        {
            return Err("layer visibility preferences exceed their bound".into());
        }
        Ok(())
    },
};

#[cfg(test)]
fn load(path: &Path) -> Result<VisibilityStore, String> {
    scoped::load(&KIND, path)
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
    Ok(scoped::read_at(&KIND, root, lane, uid, project)?.unwrap_or_default())
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
    let (mut receipt, (output, count)) =
        scoped::update_at(&KIND, root, lane, uid, project, |previous| {
            let empty = BTreeMap::new();
            let (next, output) = update(previous.unwrap_or(&empty))?;
            let count = next.len();
            Ok((Some(next), (output, count)))
        })?;
    receipt["layer_count"] = Value::from(count);
    Ok((receipt, output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
