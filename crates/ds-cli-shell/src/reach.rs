//! Where `ds` resolves from, on the PATH a shell actually has — and the
//! PATH-list edits the registration makes.
//!
//! Everything in this module is pure over its inputs so it can be tested
//! without touching a registry or a home directory. The one exception is
//! [`probe`], which reads this process's own executable and PATH and hands
//! them to [`probe_with`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;

/// The file names a shell would resolve `ds` to on this platform, in the
/// order a shell tries them. On Windows a `ds.cmd` shim earlier on PATH
/// would shadow our executable, so shims are looked for too.
#[cfg(windows)]
pub const EXECUTABLE_NAMES: &[&str] = &["ds.com", "ds.exe", "ds.bat", "ds.cmd"];
#[cfg(not(windows))]
pub const EXECUTABLE_NAMES: &[&str] = &["ds"];

/// The separator of a PATH-style list on this platform.
pub const LIST_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

/// What `ds` resolves to on one PATH, relative to one executable.
pub struct Reach {
    /// The executable that answered — this process.
    pub executable: PathBuf,
    /// Its directory: the one entry the registration ever writes.
    pub directory: PathBuf,
    /// The first `ds` on the probed PATH, if any.
    pub resolves_to: Option<PathBuf>,
    /// Whether that first `ds` is this executable.
    pub reachable: bool,
    /// Every other `ds` on the probed PATH, in PATH order. Not an error —
    /// but a second build on the machine is worth knowing about.
    pub others: Vec<PathBuf>,
}

/// Probe this process's own executable against its own PATH.
pub fn probe() -> Result<Reach, Failure> {
    let executable = std::env::current_exe().map_err(|error| {
        Failure::failed(
            "executable_unresolved",
            format!(
                "the running ds executable cannot be located: {}",
                error.kind()
            ),
        )
        .remedy(crate::EXECUTABLE_UNRESOLVED.remedy)
    })?;
    if !executable.is_file() {
        return Err(Failure::failed(
            "executable_unresolved",
            "the running ds executable is no longer on disk",
        )
        .remedy(crate::EXECUTABLE_UNRESOLVED.remedy));
    }
    Ok(probe_with(executable, std::env::var_os("PATH")))
}

/// Resolve `ds` on `path`, exactly as a shell would, and relate the answer
/// to `executable`.
pub fn probe_with(executable: PathBuf, path: Option<OsString>) -> Reach {
    let directory = executable
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let mut found: Vec<PathBuf> = Vec::new();
    if let Some(path) = path {
        for dir in std::env::split_paths(&path) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            for name in EXECUTABLE_NAMES {
                let candidate = dir.join(name);
                if candidate.is_file() && !found.iter().any(|seen| same_file(seen, &candidate)) {
                    found.push(candidate);
                }
            }
        }
    }
    let resolves_to = found.first().cloned();
    let reachable = resolves_to
        .as_deref()
        .is_some_and(|first| same_file(first, &executable));
    let others = found
        .into_iter()
        .filter(|candidate| !same_file(candidate, &executable))
        .collect();
    Reach {
        executable,
        directory,
        resolves_to,
        reachable,
        others,
    }
}

/// Whether two paths name the same file, following links and the case rules
/// the filesystem applies. Falls back to textual equality when a path cannot
/// be canonicalized — a missing file is never "the same" as a present one.
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// A path as a person reads it. Windows canonical paths carry a `\\?\`
/// prefix that no shell needs to see.
pub fn display(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if cfg!(windows) => rest.to_string(),
        _ => text.into_owned(),
    }
}

// ---------------------------------------------------------------------------
// PATH-list edits
// ---------------------------------------------------------------------------

/// Whether two PATH entries name the same directory. Quotes, a trailing
/// separator, and — on Windows — letter case and slash direction are all
/// spellings, not differences.
pub fn same_entry(a: &str, b: &str) -> bool {
    normalize_entry(a) == normalize_entry(b)
}

fn normalize_entry(entry: &str) -> String {
    let trimmed = entry.trim().trim_matches('"');
    let trimmed = trimmed.trim_end_matches(['\\', '/']);
    if cfg!(windows) {
        trimmed.replace('/', "\\").to_lowercase()
    } else {
        trimmed.to_string()
    }
}

/// Whether `list` already carries `entry`.
pub fn list_contains(list: &str, entry: &str) -> bool {
    list.split(LIST_SEPARATOR)
        .any(|candidate| !candidate.trim().is_empty() && same_entry(candidate, entry))
}

/// `list` with `entry` appended, or `list` untouched when it is already
/// there. Appending — not prepending — is deliberate: a user who put a
/// different `ds` first on purpose keeps it, and `ds shell status` says so.
pub fn list_with(list: &str, entry: &str) -> String {
    if list_contains(list, entry) {
        return list.to_string();
    }
    let trimmed = list.trim_end_matches(LIST_SEPARATOR);
    if trimmed.is_empty() {
        entry.to_string()
    } else {
        format!("{trimmed}{LIST_SEPARATOR}{entry}")
    }
}

/// `list` with every spelling of `entry` removed and everything else — the
/// user's own entries, in their own order and spelling — left exactly as it
/// was.
pub fn list_without(list: &str, entry: &str) -> String {
    list.split(LIST_SEPARATOR)
        .filter(|candidate| candidate.trim().is_empty() || !same_entry(candidate, entry))
        .collect::<Vec<_>>()
        .join(&LIST_SEPARATOR.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sep() -> String {
        LIST_SEPARATOR.to_string()
    }

    #[test]
    fn an_absent_entry_is_appended_after_the_users_own() {
        let list = ["/usr/bin", "/home/me/bin"].join(&sep());
        let next = list_with(&list, "/opt/ds");
        assert_eq!(next, ["/usr/bin", "/home/me/bin", "/opt/ds"].join(&sep()));
        assert!(list_contains(&next, "/opt/ds"));
    }

    #[test]
    fn a_present_entry_is_left_exactly_as_it_was() {
        let list = ["/usr/bin", "/opt/ds/", "/home/me/bin"].join(&sep());
        assert_eq!(list_with(&list, "/opt/ds"), list);
        assert!(list_contains(&list, "/opt/ds"));
    }

    #[test]
    fn an_empty_or_separator_only_list_becomes_the_entry() {
        assert_eq!(list_with("", "/opt/ds"), "/opt/ds");
        assert_eq!(list_with(&sep(), "/opt/ds"), "/opt/ds");
    }

    #[test]
    fn removal_keeps_everything_else_verbatim() {
        let list = ["/usr/bin", "\"/opt/ds\"", "", "/home/me/bin", "/opt/ds/"].join(&sep());
        assert_eq!(
            list_without(&list, "/opt/ds"),
            ["/usr/bin", "", "/home/me/bin"].join(&sep())
        );
        assert!(!list_contains(&list_without(&list, "/opt/ds"), "/opt/ds"));
    }

    #[test]
    fn removal_of_an_absent_entry_changes_nothing() {
        let list = ["/usr/bin", "/home/me/bin"].join(&sep());
        assert_eq!(list_without(&list, "/opt/ds"), list);
    }

    #[cfg(windows)]
    #[test]
    fn windows_entries_compare_by_case_and_slash_direction() {
        assert!(same_entry(
            r"C:\Users\Me\AppData\Local\DS GridDesign",
            "c:/users/me/appdata/local/ds griddesign/"
        ));
        assert!(list_contains(
            r"C:\Windows;C:\USERS\ME\APPDATA\LOCAL\DS GRIDDESIGN\",
            r"C:\Users\Me\AppData\Local\DS GridDesign"
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_entries_compare_exactly_apart_from_a_trailing_slash() {
        assert!(same_entry("/opt/ds", "/opt/ds/"));
        assert!(!same_entry("/opt/ds", "/opt/DS"));
    }

    #[test]
    fn probing_finds_this_executable_only_when_its_directory_leads_the_path() {
        // Two staged copies of one executable, each named `ds`, in two
        // directories of our own: a Cargo target directory is not a fixture
        // (it holds a `deps/ds` of its own), so nothing here depends on it.
        let executable = std::env::current_exe().expect("test executable");
        let root = std::env::temp_dir().join(format!("ds-cli-shell-reach-{}", std::process::id()));
        let name = if cfg!(windows) { "ds.exe" } else { "ds" };
        let stage = |leaf: &str| {
            let dir = root.join(leaf);
            std::fs::create_dir_all(&dir).expect("stage dir");
            let copy = dir.join(name);
            std::fs::copy(&executable, &copy).expect("stage a ds");
            (dir, copy)
        };
        let (first_dir, first) = stage("first");
        let (second_dir, second) = stage("second");
        let path = std::env::join_paths([first_dir.clone(), second_dir]).expect("path");

        let reach = probe_with(first.clone(), Some(path.clone()));
        assert!(reach.reachable, "the first ds on the PATH is reachable");
        assert_eq!(reach.directory, first_dir);
        assert_eq!(
            reach.others,
            vec![second.clone()],
            "the second ds is reported, in PATH order"
        );

        let shadowed = probe_with(second.clone(), Some(path));
        assert!(
            !shadowed.reachable,
            "a ds that is not first on the PATH is not reachable"
        );
        assert_eq!(shadowed.resolves_to, Some(first.clone()));
        assert_eq!(shadowed.others, vec![first]);

        let nowhere = probe_with(second, None);
        assert!(!nowhere.reachable);
        assert!(nowhere.resolves_to.is_none());
        assert!(nowhere.others.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
