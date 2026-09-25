//! DS-owned local data is private to the account that owns it (owner
//! decision, 2026-09-25): a directory DS creates for its own data is `0700`
//! and a file it writes there is `0600`. This module is the one owner of that
//! rule for every native host — the Server core, the `ds` CLI and the Desktop —
//! and every store in this crate goes through it.
//!
//! It is deliberately not a process umask: a file the user asked for in a
//! place the user chose (`--out`, `--out-dir`, a save dialog) keeps the
//! platform's ordinary permissions, so only DS-owned locations route here.
//!
//! Modes exist on unix only. Elsewhere every function is the plain `std`
//! operation it replaces, so behaviour there is unchanged.
//!
//! A crate that cannot reach this one without a new dependency edge applies
//! the same two modes inline with `std::os::unix::fs` and names this rule.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

/// Mode of a DS-owned directory.
pub const DIR_MODE: u32 = 0o700;
/// Mode of a DS-owned file.
pub const FILE_MODE: u32 = 0o600;

/// `fs::create_dir_all` for a DS-owned directory: every component it creates
/// is `0700`, and the directory itself is tightened to `0700` when it already
/// existed (an install from before this rule). Existing ancestors are left as
/// they are — they may be the user's. A symbolic link is never followed for a
/// mode change.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(DIR_MODE)
            .create(path)?;
        tighten_dir(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}

/// `fs::create_dir` for a DS-owned directory: created `0700`.
pub fn create_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(DIR_MODE).create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

/// `OpenOptions::new()` whose created file is `0600`. An existing file keeps
/// its mode; [`write`] and [`tighten_tree`] tighten one from before this rule.
pub fn open_options() -> OpenOptions {
    #[allow(unused_mut)]
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(FILE_MODE);
    }
    options
}

/// `File::create` for a DS-owned file: created, or truncated and tightened, `0600`.
pub fn create_file(path: &Path) -> io::Result<File> {
    let file = open_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    set_file_mode(&file)?;
    Ok(file)
}

/// `fs::write` for a DS-owned file, `0600` whether it is new or replaced in place.
pub fn write(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    create_file(path)?.write_all(bytes.as_ref())
}

/// The whole file is written beside its destination and renamed over it, so a
/// reader sees the old bytes or the new ones, never a torn file. The staged
/// file is `0600` from its creation, and so is the file after the rename.
pub fn write_atomic(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    // `tempfile` creates the staged file `0600` on unix already; the explicit
    // mode keeps that true whatever its default becomes.
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    set_file_mode(staged.as_file())?;
    staged.write_all(bytes.as_ref())?;
    staged.flush()?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// A DS-owned file a library opens by name (SQLite): created empty `0600`
/// when absent, tightened when present, before the library opens it. SQLite
/// creates its `-wal`, `-shm` and `-journal` files with the mode of the
/// database file, so they follow.
pub fn prepare_file(path: &Path) -> io::Result<()> {
    let file = open_options().write(true).create(true).open(path)?;
    set_file_mode(&file)
}

/// Strip group and other access from one DS-owned entry, never following a
/// link: a directory or file keeps only its owner bits (`0775` becomes
/// `0700`, `0644` becomes `0600`, an owner-executable file stays executable).
/// Absent is not an error.
pub fn tighten(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    tighten_entry(path, &metadata)
}

/// [`tighten`] for everything under a DS-owned root: the root, every
/// directory and every regular file below it. Symbolic links are neither
/// followed nor changed, other entries (sockets, pipes) are left alone, and
/// the walk never leaves the root. Best effort: an entry that cannot be read
/// or changed is skipped, never fatal. Returns how many entries it changed.
pub fn tighten_tree(root: &Path) -> usize {
    let mut changed = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if needs_tightening(&metadata) && tighten_entry(&path, &metadata).is_ok() {
            changed += 1;
        }
        if metadata.is_dir() {
            if let Ok(entries) = fs::read_dir(&path) {
                pending.extend(entries.flatten().map(|entry| entry.path()));
            }
        }
    }
    changed
}

/// [`tighten_tree`] once per process for one DS-owned root that exists: the
/// migration of an install from before this rule, run where the root is
/// opened. New data is created private, so later calls in the same process
/// do nothing. A root that does not exist yet is created private by whoever
/// creates it, and is not remembered.
pub fn tighten_root_once(root: &Path) {
    static DONE: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
    if !cfg!(unix) || !root.exists() {
        return;
    }
    let done = DONE.get_or_init(Default::default);
    {
        let Ok(mut done) = done.lock() else {
            return;
        };
        if done.iter().any(|seen| seen == root) {
            return;
        }
        done.push(root.to_path_buf());
    }
    tighten_tree(root);
}

#[cfg(unix)]
fn tighten_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && metadata.permissions().mode() & 0o777 != DIR_MODE {
        fs::set_permissions(path, fs::Permissions::from_mode(DIR_MODE))?;
    }
    Ok(())
}

fn set_file_mode(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if file.metadata()?.permissions().mode() & 0o777 != FILE_MODE {
            file.set_permissions(fs::Permissions::from_mode(FILE_MODE))?;
        }
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

fn needs_tightening(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        (metadata.is_dir() || metadata.is_file()) && metadata.permissions().mode() & 0o077 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn tighten_entry(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if needs_tightening(metadata) {
            let mode = metadata.permissions().mode() & 0o700;
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        }
    }
    #[cfg(not(unix))]
    let _ = (path, metadata);
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn created_directories_and_files_are_private() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a/b/c");
        create_dir_all(&dir).unwrap();
        for path in [tmp.path().join("a"), tmp.path().join("a/b"), dir.clone()] {
            assert_eq!(mode(&path), DIR_MODE, "{}", path.display());
        }
        create_dir(&dir.join("d")).unwrap();
        assert_eq!(mode(&dir.join("d")), DIR_MODE);

        write(&dir.join("w"), b"w").unwrap();
        write_atomic(&dir.join("r"), b"r").unwrap();
        create_file(&dir.join("c")).unwrap();
        prepare_file(&dir.join("s.sqlite")).unwrap();
        open_options()
            .write(true)
            .create_new(true)
            .open(dir.join("o"))
            .unwrap();
        for name in ["w", "r", "c", "s.sqlite", "o"] {
            assert_eq!(mode(&dir.join(name)), FILE_MODE, "{name}");
        }
        assert_eq!(fs::read(dir.join("r")).unwrap(), b"r");
    }

    #[test]
    fn existing_entries_are_tightened_never_widened() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o775)).unwrap();
        fs::set_permissions(root.join("nested"), fs::Permissions::from_mode(0o755)).unwrap();
        for (name, before) in [("data", 0o644), ("tool", 0o755), ("sealed", 0o444)] {
            fs::write(root.join("nested").join(name), b"x").unwrap();
            fs::set_permissions(
                root.join("nested").join(name),
                fs::Permissions::from_mode(before),
            )
            .unwrap();
        }
        // A link out of the root is neither followed nor changed.
        let outside = tmp.path().join("outside");
        fs::write(&outside, b"user").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o644)).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

        assert_eq!(tighten_tree(&root), 5);
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(&root.join("nested")), 0o700);
        assert_eq!(mode(&root.join("nested/data")), 0o600);
        assert_eq!(mode(&root.join("nested/tool")), 0o700);
        assert_eq!(mode(&root.join("nested/sealed")), 0o400);
        assert_eq!(mode(&outside), 0o644);
        // Idempotent.
        assert_eq!(tighten_tree(&root), 0);

        // The in-place writers tighten a file from before the rule too.
        fs::write(root.join("old"), b"x").unwrap();
        fs::set_permissions(root.join("old"), fs::Permissions::from_mode(0o644)).unwrap();
        write(&root.join("old"), b"y").unwrap();
        assert_eq!(mode(&root.join("old")), FILE_MODE);
        fs::set_permissions(root.join("old"), fs::Permissions::from_mode(0o644)).unwrap();
        prepare_file(&root.join("old")).unwrap();
        assert_eq!(mode(&root.join("old")), FILE_MODE);
        assert_eq!(fs::read(root.join("old")).unwrap(), b"y");

        // An existing directory is tightened by the directory creator.
        fs::set_permissions(root.join("nested"), fs::Permissions::from_mode(0o775)).unwrap();
        create_dir_all(&root.join("nested")).unwrap();
        assert_eq!(mode(&root.join("nested")), DIR_MODE);
    }

    #[test]
    fn a_root_is_migrated_once_per_process() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("once");
        tighten_root_once(&root); // absent: nothing, and not remembered
        fs::create_dir(&root).unwrap();
        fs::write(root.join("f"), b"x").unwrap();
        fs::set_permissions(root.join("f"), fs::Permissions::from_mode(0o644)).unwrap();
        tighten_root_once(&root);
        assert_eq!(mode(&root.join("f")), 0o600);
        fs::set_permissions(root.join("f"), fs::Permissions::from_mode(0o644)).unwrap();
        tighten_root_once(&root);
        assert_eq!(
            mode(&root.join("f")),
            0o644,
            "second call in a process is a no-op"
        );
    }
}

/// Every directory under `root` is `0700` and every regular file `0600`: what
/// a store's own tests assert after they have written through it.
#[cfg(all(test, unix))]
pub(crate) fn assert_private_tree(root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut pending = vec![root.to_path_buf()];
    let mut files = 0;
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        if metadata.is_dir() {
            assert_eq!(mode, DIR_MODE, "{} is a DS-owned directory", path.display());
            pending.extend(fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
        } else if metadata.is_file() {
            assert_eq!(mode, FILE_MODE, "{} is a DS-owned file", path.display());
            files += 1;
        }
    }
    assert!(files > 0, "{} holds no file to check", root.display());
}
