//! The process boundary, pinned.
//!
//! `docs/contracts/process-boundary-contract.md` and `CLAUDE.md` both promise
//! that no caller-supplied argv reaches any owner. Until this suite existed
//! that promise was enforced by reading: `ds-cli-exec` honours it exactly —
//! `External::call` takes a `&'static str` subcommand — but four other files
//! construct a process directly, one of them behind a
//! `run_quiet(executable, args, timeout)` helper that is literally the shape
//! the contract says does not exist. The security property held at every one
//! of those sites, because each passes a statically-known argument list. The
//! structural claim did not, and nothing failed when it stopped being true.
//!
//! So this suite pins the inventory instead of the prose. A new file that
//! spawns a process fails here and has to be classified: either route it
//! through `ds-cli-exec`, or add it below with the owner class it serves and
//! the reason its argument list is static.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Every non-test file permitted to construct a process, and why.
///
/// Paths are workspace-relative. Adding a row is a deliberate act: it means
/// somebody read the call site and confirmed the argument list is built here,
/// from constants and typed inputs, never from a caller's string.
const SPAWN_OWNERS: &[(&str, &str)] = &[
    (
        "crates/ds-cli-exec/src/lib.rs",
        "the audited engine boundary: one named `&'static str` subcommand per call, \
         for the owners that chose process separation (ds-report, ds-solar)",
    ),
    (
        "crates/ds-cli-mcp/src/tools.rs",
        "re-invokes this same executable to read its own `ds capabilities`; the argv \
         is a fixed literal and the path is `current_exe`",
    ),
    (
        "crates/ds-cli-workstation/src/detect.rs",
        "asks a detected local tool for its version; the argv is a fixed literal",
    ),
    (
        "crates/ds-cli-workstation/src/install.rs",
        "drives the platform package manager; the argv is a fixed array with a `const` \
         package id, never an operator string",
    ),
    (
        "crates/ds-cli-workstation/src/verify.rs",
        "runs a component's own verification probe; the argv is a fixed literal",
    ),
];

/// Textual constructors for a child process. `ProcessCommand` is the alias
/// `ds-cli-workstation` imports `std::process::Command` under.
const CONSTRUCTORS: &[&str] = &["Command::new(", "ProcessCommand::new("];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root resolves")
}

fn rust_sources(root: &Path, into: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(root).unwrap_or_else(|error| {
        panic!("could not read {}: {error}", root.display());
    });
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            rust_sources(&path, into);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            into.push(path);
        }
    }
}

/// Files that actually construct a process, ignoring the crate's own unit
/// tests: a `#[cfg(test)]` block cannot widen the shipped boundary.
fn spawning_files() -> BTreeSet<String> {
    let root = workspace_root();
    let mut sources = Vec::new();
    for crate_dir in std::fs::read_dir(root.join("crates")).expect("crates/ exists") {
        let source = crate_dir.expect("a crate directory").path().join("src");
        if source.is_dir() {
            rust_sources(&source, &mut sources);
        }
    }
    assert!(
        sources.len() > 50,
        "only {} source files found; the walk is not reaching the tree",
        sources.len()
    );

    let mut found = BTreeSet::new();
    for path in sources {
        let text = std::fs::read_to_string(&path).expect("a UTF-8 source file");
        let shipped = text.split("#[cfg(test)]").next().unwrap_or_default();
        if CONSTRUCTORS
            .iter()
            .any(|constructor| shipped.contains(constructor))
        {
            let relative = path
                .strip_prefix(&root)
                .expect("inside the workspace")
                .to_string_lossy()
                .replace('\\', "/");
            found.insert(relative);
        }
    }
    found
}

#[test]
fn only_the_pinned_files_may_start_a_process() {
    let declared: BTreeSet<String> = SPAWN_OWNERS
        .iter()
        .map(|(path, _)| (*path).to_string())
        .collect();
    let found = spawning_files();

    let undeclared: Vec<&String> = found.difference(&declared).collect();
    assert!(
        undeclared.is_empty(),
        "these files start a process but are not in the pinned inventory: {undeclared:?}\n\
         Route the call through `ds-cli-exec`, or add it to `SPAWN_OWNERS` in this file \
         with the owner class it serves and why its argument list is static. See \
         docs/contracts/process-boundary-contract.md."
    );

    let stale: Vec<&String> = declared.difference(&found).collect();
    assert!(
        stale.is_empty(),
        "these pinned entries no longer start a process; remove them: {stale:?}"
    );
}

#[test]
fn every_pinned_owner_states_why_its_argv_is_static() {
    for (path, reason) in SPAWN_OWNERS {
        assert!(
            reason.len() > 40,
            "`{path}` needs a real justification, not a placeholder"
        );
        assert!(
            workspace_root().join(path).is_file(),
            "`{path}` is pinned but does not exist"
        );
    }
    let paths: BTreeSet<&str> = SPAWN_OWNERS.iter().map(|(path, _)| *path).collect();
    assert_eq!(
        paths.len(),
        SPAWN_OWNERS.len(),
        "a file is pinned twice, so one of the two reasons is unread"
    );
}

#[test]
fn the_engine_boundary_still_takes_a_static_subcommand() {
    // The one property the whole contract rests on. `&'static str` is what
    // makes "a subcommand no command registers stays unreachable" true: a
    // value derived from model output cannot be one.
    let source = std::fs::read_to_string(workspace_root().join("crates/ds-cli-exec/src/lib.rs"))
        .expect("ds-cli-exec source");
    let shipped = source.split("#[cfg(test)]").next().unwrap_or_default();

    let signatures = shipped.matches("subcommand: &'static str").count();
    assert!(
        signatures >= 2,
        "`call` and `call_json` must both take a `&'static str` subcommand; found {signatures}"
    );

    // Any function that reaches the constructor must have named its
    // subcommand statically. A `&str` on a function that only *formats* a
    // refusal is fine — it cannot start anything.
    let mut checked = 0usize;
    for block in shipped.split("\n    pub fn ").skip(1) {
        if !CONSTRUCTORS
            .iter()
            .any(|constructor| block.contains(constructor))
        {
            continue;
        }
        checked += 1;
        let name = block.split('(').next().unwrap_or("?");
        assert!(
            block.contains("subcommand: &'static str"),
            "`{name}` starts a process without a `&'static str` subcommand; a value \
             derived from model output could then name one no command registers"
        );
    }
    assert!(
        checked > 0,
        "no function here starts a process; this suite is asserting nothing"
    );

    assert!(
        !shipped.contains("pub fn run("),
        "`run(binary, argv)` is the shape this crate exists to not have"
    );
}
