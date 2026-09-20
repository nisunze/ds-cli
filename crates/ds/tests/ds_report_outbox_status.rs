//! `ds report outbox status` — the answer to "is anything stuck?".
//!
//! These run the real binary against a temporary Server state root, because
//! the property under test is precisely that this reading needs nothing else:
//! no session, no native identity, no project selection, no running Server.
//! The machine that quietly accumulated 593 unpublished artifacts had none of
//! those, and the operator had no command to ask. The queue is the sync
//! store's rows; the artifact directory's lock is reported as a fact about
//! the bytes.

use std::fs;
use std::path::{Path, PathBuf};

mod common;

use common::json;

/// A protected Server state root with an empty report queue below it.
fn state_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ds-report-outbox-{label}-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
    ));
    fs::create_dir_all(root.join("report-artifacts")).expect("create the queue root");
    private(&root);
    root
}

#[cfg(unix)]
fn private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("the Server state root is private");
}

#[cfg(not(unix))]
fn private(_path: &Path) {}

#[test]
fn an_empty_queue_answers_instead_of_refusing() {
    // WHY: a status command that only works when something is wrong is not a
    // status command. "Nothing is queued" has to be an answer an operator can
    // trust, or the first reading of a real backlog will not be believed.
    let root = state_root("empty");
    let (value, code) = json(&[
        "report",
        "outbox",
        "status",
        "--server-state-dir",
        &root.to_string_lossy(),
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "an empty queue is a normal answer: {value}");
    assert_eq!(value["data"]["queued_batches"], 0);
    assert_eq!(value["data"]["stuck"], false);
    assert_eq!(value["data"]["held_batches"], 0);
    assert_eq!(value["data"]["reclaimable_batches"], 0);
    assert_eq!(value["data"]["bytes_lock"]["held"], false);
    assert!(
        value["data"]["next"]
            .as_str()
            .is_some_and(|next| !next.is_empty()),
        "every reading names what to do next"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_dead_holder_is_named_and_reported_releasable_without_any_credential() {
    // WHY: acceptance C and D together. The wedge an operator lost a day to
    // was a lock held by a pid that no longer existed, with nothing to read
    // and nothing to run. This is that exact marker — the bare pid an earlier
    // release wrote — and the command must name the holder, say the holder is
    // gone, and say the next seal or discard releases it, all with no
    // session at all. It is the bytes directory's lock; the queue's own
    // liveness is the store's lease.
    let root = state_root("dead-holder");
    // A pid far above this system's range: certainly not a running process.
    fs::write(root.join("report-artifacts/.publication.lock"), "4000000\n")
        .expect("write an abandoned marker");

    let (value, code) = json(&[
        "report",
        "outbox",
        "status",
        "--server-state-dir",
        &root.to_string_lossy(),
        "--output",
        "json",
    ]);
    assert_eq!(
        code, 0,
        "diagnosing a wedge must not itself refuse: {value}"
    );
    let lock = &value["data"]["bytes_lock"];
    assert_eq!(lock["present"], true);
    assert_eq!(lock["holder"], "gone");
    assert_eq!(lock["owner"]["pid"], 4_000_000);
    assert_eq!(lock["reclaimable"], true);
    assert_eq!(
        value["data"]["stuck"], false,
        "a lock that releases itself on the next run is not a human's problem"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_queue_reading_names_the_project_it_was_narrowed_to() {
    // WHY: one machine authors for several projects, and the Combined Report
    // question is always asked about one of them. A reading that cannot be
    // narrowed would send an operator back to guessing which rooms the
    // backlog belongs to.
    let root = state_root("narrowed");
    let (value, code) = json(&[
        "report",
        "outbox",
        "status",
        "--project",
        "czgmdwth_example",
        "--server-state-dir",
        &root.to_string_lossy(),
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["data"]["projects"].as_array().map(Vec::len), Some(0));
    assert_eq!(value["data"]["queued_batches"], 0);
    let _ = fs::remove_dir_all(&root);
}
