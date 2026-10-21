//! Machine-local working-area form selection for native hosts: which survey
//! forms the working area loads for one project, remembered per lane, DS
//! account and project exactly as the drawer's visibility toggles are.
//!
//! What the selection IS is the kernel's decision
//! (`ds-command-kernel::survey_working_area_forms`); this module persists the
//! `next_selection` it returns and reads the selection back. An absent entry
//! means the operator never chose on this host — the kernel reads that as
//! "load nothing", never as "all".
use serde_json::Value;
use std::path::Path;

pub use crate::scoped::UpdateError;
use crate::scoped::{self, Kind};

pub const MAX_FORMS: usize = 5_000;

const KIND: Kind<Vec<String>> = Kind {
    file: "working-area-forms.json",
    lock: "working-area-forms.lock",
    noun: "working-area forms",
    validate: |forms| {
        if forms.len() > MAX_FORMS
            || forms
                .iter()
                .any(|slug| slug.is_empty() || slug.chars().count() > 200)
        {
            return Err("working-area form selection exceeds its bound".into());
        }
        Ok(())
    },
};

/// The remembered selection for one lane, account and project; `None` when
/// the operator never chose on this host.
pub fn read(lane: &str, uid: &str, project: &str) -> Result<Option<Vec<String>>, String> {
    read_at(&super::default_root()?, lane, uid, project)
}

pub fn read_at(
    root: &Path,
    lane: &str,
    uid: &str,
    project: &str,
) -> Result<Option<Vec<String>>, String> {
    scoped::read_at(&KIND, root, lane, uid, project)
}

/// Read, decide and persist under one lock. The transition sees the entry as
/// stored (`None` = never chosen) and returns what the kernel said to persist
/// (`None` = remove the entry) with whatever the caller wants back.
pub fn update_at<T, E>(
    root: &Path,
    lane: &str,
    uid: &str,
    project: &str,
    update: impl FnOnce(Option<&Vec<String>>) -> Result<(Option<Vec<String>>, T), E>,
) -> Result<(Value, T), UpdateError<E>> {
    scoped::update_at(&KIND, root, lane, uid, project, update)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms(slugs: &[&str]) -> Vec<String> {
        slugs.iter().map(|slug| (*slug).to_owned()).collect()
    }

    #[test]
    fn never_chosen_reads_as_none_and_creates_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(read_at(tmp.path(), "canary", "u1", "p1").unwrap(), None);
        assert!(!tmp.path().join("working-area-forms.json").exists());
    }

    #[test]
    fn a_choice_survives_restart_and_is_fenced_by_lane_account_and_project() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let (receipt, ()) = update_at(root, "canary", "u1", "p1", |previous| {
            assert_eq!(previous, None);
            Ok::<_, String>((Some(forms(&["tr", "lv"])), ()))
        })
        .unwrap();
        assert_eq!(receipt["changed"], true);
        assert_eq!(receipt["revision"], 1);
        assert_eq!(
            read_at(root, "canary", "u1", "p1").unwrap(),
            Some(forms(&["tr", "lv"]))
        );
        assert_eq!(read_at(root, "stable", "u1", "p1").unwrap(), None);
        assert_eq!(read_at(root, "canary", "u2", "p1").unwrap(), None);
        assert_eq!(read_at(root, "canary", "u1", "p2").unwrap(), None);
        // An explicit empty choice is a choice, distinct from never chosen.
        let (receipt, ()) = update_at(root, "canary", "u1", "p1", |_| {
            Ok::<_, String>((Some(Vec::new()), ()))
        })
        .unwrap();
        assert_eq!(receipt["changed"], true);
        assert_eq!(
            read_at(root, "canary", "u1", "p1").unwrap(),
            Some(Vec::new())
        );
        // Clearing removes the entry; clearing again changes nothing.
        let (receipt, ()) =
            update_at(root, "canary", "u1", "p1", |_| Ok::<_, String>((None, ()))).unwrap();
        assert_eq!(receipt["changed"], true);
        assert_eq!(read_at(root, "canary", "u1", "p1").unwrap(), None);
        let (receipt, ()) =
            update_at(root, "canary", "u1", "p1", |_| Ok::<_, String>((None, ()))).unwrap();
        assert_eq!(receipt["changed"], false);
        assert_eq!(receipt["revision"], 3);
    }

    #[test]
    fn the_visibility_store_is_a_sibling_file_not_the_same_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        update_at(root, "canary", "u1", "p1", |_| {
            Ok::<_, String>((Some(forms(&["tr"])), ()))
        })
        .unwrap();
        assert!(crate::visibility::read_at(root, "canary", "u1", "p1")
            .unwrap()
            .is_empty());
        assert!(root.join("working-area-forms.json").exists());
        assert!(!root.join("layer-visibility.json").exists());
    }

    #[test]
    fn a_refused_transition_and_a_malformed_store_leave_the_bytes_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        update_at(root, "canary", "u1", "p1", |_| {
            Ok::<_, String>((Some(forms(&["tr"])), ()))
        })
        .unwrap();
        let path = root.join("working-area-forms.json");
        let before = std::fs::read(&path).unwrap();
        let refused = update_at::<(), _>(root, "canary", "u1", "p1", |_| Err("refused"));
        assert!(matches!(refused, Err(UpdateError::Transition("refused"))));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::write(&path, b"{not json").unwrap();
        assert!(read_at(root, "canary", "u1", "p1")
            .unwrap_err()
            .contains("malformed"));
        assert!(read_at(root, "prod", "u1", "p1").is_err());
    }
}
