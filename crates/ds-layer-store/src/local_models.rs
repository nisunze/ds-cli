//! The machine's DS Grid working copies: the catalogue file, the package
//! directory, and nothing else.
//!
//! A local model is a `.dsgrid` a machine holds before anything is published —
//! created empty, imported from a file, or taken from a project's governed
//! head. The browser keeps that catalogue in IndexedDB; a native host keeps
//! its own in a file here, one per lane and DS account, under the same root as
//! the overlay registry, the visibility store and the prepared local layers.
//!
//! This crate is a HOST adapter, not a caller. The CLI and the UI both ask the
//! kernel; the kernel decides; whichever host is running it — server, desktop
//! or cloud — persists the answer through this module. Nothing here is a
//! second opinion about a working copy.
//!
//! **Every decision is the kernel's.** `ds_command_kernel::local_models` mints
//! nothing on its own but decides what a duplicate is, which copy is open after
//! one is forgotten, and what each refusal is called. This module reads the
//! catalogue, hands it to the kernel with the operation, and persists exactly
//! the catalogue that comes back.
//!
//! **The package bytes are the caller's.** Whether bytes are a readable
//! `.dsgrid` is `ds-grid-exchange`'s question, and this crate does not link it.
//! A caller validates the package, hands over the identity it read, and this
//! store copies the bytes into the payload directory under the id the caller
//! minted. What the store guarantees is the ORDER: the package is written
//! before the catalogue row that names it, and a row is removed before its
//! package, so a crash leaves a file with no row — recoverable — rather than a
//! row pointing at nothing. A revision in place follows the same order under
//! the same lock: the new package bytes replace the old atomically, then the
//! row moves to name them; a crash between the two leaves a row naming the
//! previous digest beside newer bytes, which the next `Revise` reports as a
//! conflict rather than overwriting silently.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use ds_command_kernel::local_models::{Catalogue, Op, Outcome, Scope};
use fs2::FileExt;

/// The largest catalogue this store will write or reopen.
pub const MAX_CATALOGUE_BYTES: usize = 1024 * 1024;

/// One working copy. Large enough for a real MV network, small enough that a
/// mistyped path cannot fill a laptop.
pub const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug)]
pub enum StoreError {
    /// The kernel refused. The code is its own closed vocabulary and callers
    /// re-raise it literally rather than renaming it.
    Refused { code: String, message: String },
    /// This host could not do what the kernel admitted.
    Store(String),
}

impl StoreError {
    pub fn code(&self) -> &str {
        match self {
            Self::Refused { code, .. } => code,
            Self::Store(_) => "local_model_store_unavailable",
        }
    }
    pub fn message(&self) -> &str {
        match self {
            Self::Refused { message, .. } | Self::Store(message) => message,
        }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

fn io_error(_: impl std::fmt::Display) -> StoreError {
    StoreError::Store("the local model store could not be read or persisted".into())
}

fn safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains(['/', '\\'])
        && matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        )
}

/// `<root>/models/<lane>/<uid>`.
pub fn scope_dir(root: &Path, scope: &Scope) -> Result<PathBuf, StoreError> {
    if !safe_segment(&scope.lane) || !safe_segment(&scope.uid) {
        return Err(StoreError::Store(
            "a local model lane and account uid must each be one bounded path segment".into(),
        ));
    }
    Ok(root.join("models").join(&scope.lane).join(&scope.uid))
}

fn packages_dir(dir: &Path) -> PathBuf {
    dir.join("packages")
}

/// The one file a model owns. The id is held to a single safe segment here as
/// well: a catalogue is a file an operator can edit, and an id that walked out
/// of the package directory would be the one way this store could touch
/// something that is not its own.
pub fn package_path(dir: &Path, id: &str) -> Result<PathBuf, StoreError> {
    if !ds_command_kernel::local_models::id_is_safe(id) {
        return Err(StoreError::Store(format!(
            "local model id {id} is not one bounded path segment, so it names no package here"
        )));
    }
    Ok(packages_dir(dir).join(format!("{id}.dsgrid")))
}

fn load(path: &Path, scope: &Scope) -> Result<Catalogue, StoreError> {
    match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_CATALOGUE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            if bytes.len() > MAX_CATALOGUE_BYTES {
                return Err(StoreError::Store(
                    "this machine's local model catalogue is larger than 1 MiB and was not read"
                        .into(),
                ));
            }
            serde_json::from_slice(&bytes).map_err(|error| {
                StoreError::Store(format!(
                    "this machine's local model catalogue is unreadable: {error}"
                ))
            })
        }
        // An absent catalogue is a machine that has held no working copy yet,
        // which is an ordinary state and not a fault.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(Catalogue::empty(scope.clone()))
        }
        Err(error) => Err(io_error(error)),
    }
}

fn persist(path: &Path, dir: &Path, catalogue: &Catalogue) -> Result<(), StoreError> {
    let bytes = serde_json::to_vec(catalogue)
        .map_err(|error| StoreError::Store(format!("the catalogue is unwritable: {error}")))?;
    // Bound the write, not only the read: a store that could be written past
    // what it can be reopened at would brick itself one model at a time.
    if bytes.len() > MAX_CATALOGUE_BYTES {
        return Err(StoreError::Store(
            "this catalogue would exceed 1 MiB; forget a working copy before adding another".into(),
        ));
    }
    write_atomic(path, dir, &bytes)
}

fn write_atomic(path: &Path, dir: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut staged = tempfile::NamedTempFile::new_in(dir).map_err(io_error)?;
    staged.write_all(bytes).map_err(io_error)?;
    staged.flush().map_err(io_error)?;
    staged.as_file().sync_all().map_err(io_error)?;
    staged.persist(path).map_err(io_error)?;
    #[cfg(unix)]
    File::open(dir)
        .and_then(|handle| handle.sync_all())
        .map_err(io_error)?;
    Ok(())
}

/// This host's clock, in the one shape the catalogue keeps.
fn now_rfc3339() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default();
    // A plain civil-time conversion: the catalogue only needs an instant an
    // operator can read and sort, and a date library would be a dependency for
    // one line.
    let days = (seconds / 86_400) as i64;
    let time = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60
    )
}

/// Howard Hinnant's `civil_from_days`, transcribed. Proleptic Gregorian, which
/// is what an RFC3339 instant after 1970 needs and all this store ever writes.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn refused(refusal: ds_command_kernel::local_models::Refusal) -> StoreError {
    StoreError::Refused {
        code: refusal.code.to_owned(),
        message: refusal.message,
    }
}

/// Read this machine's catalogue without changing anything.
pub fn read_at(root: &Path, scope: &Scope) -> Result<Catalogue, StoreError> {
    load(&scope_dir(root, scope)?.join("catalogue.json"), scope)
}

/// Apply one operation, with `package` holding the bytes a `Register` brings.
///
/// The bytes are the caller's, already validated by the engine that
/// understands them; this store only bounds them and writes them where the
/// catalogue will say they are.
pub fn execute_at(
    root: &Path,
    scope: &Scope,
    op: Op,
    package: Option<&[u8]>,
) -> Result<Outcome, StoreError> {
    let dir = scope_dir(root, scope)?;
    let catalogue_path = dir.join("catalogue.json");

    if let Some(bytes) = package {
        if bytes.is_empty() || bytes.len() as u64 > MAX_PACKAGE_BYTES {
            return Err(StoreError::Store(format!(
                "a local model package must be between 1 byte and {} MiB",
                MAX_PACKAGE_BYTES / (1024 * 1024)
            )));
        }
    }
    // The clock is read once, here, before anything is asked — so the
    // provisional answer and the authoritative one below are the same question
    // about the same instant, and the kernel stays clockless.
    let op = match op {
        Op::Register {
            created_at,
            id,
            display_name,
            origin,
            crs,
            model_revision,
            bytes,
            sha256,
            project,
            head_revision,
            activate,
        } => Op::Register {
            created_at: Some(created_at.unwrap_or_else(now_rfc3339)),
            id,
            display_name,
            origin,
            crs,
            model_revision,
            bytes,
            sha256,
            project,
            head_revision,
            activate,
        },
        Op::Revise {
            id,
            expected_sha256,
            model_revision,
            bytes,
            sha256,
            head_revision,
            revised_at,
        } => Op::Revise {
            id,
            expected_sha256,
            model_revision,
            bytes,
            sha256,
            head_revision,
            revised_at: Some(revised_at.unwrap_or_else(now_rfc3339)),
        },
        // A link records the host's clock the same way, and for the same
        // reason: the kernel refuses a link that carries none.
        Op::Link { id, mut pls_source } => {
            if pls_source.linked_at.is_empty() {
                pls_source.linked_at = now_rfc3339();
            }
            Op::Link { id, pls_source }
        }
        other => other,
    };
    let takes_package = matches!(op, Op::Register { .. } | Op::Revise { .. });
    if takes_package != package.is_some() {
        return Err(StoreError::Store(
            "registering or revising a local model needs its package bytes, and no other operation takes them"
                .into(),
        ));
    }

    // The unlocked read and its provisional answer. A refusal ends here with
    // nothing created and no other writer made to wait.
    let catalogue = load(&catalogue_path, scope)?;
    ds_command_kernel::local_models::evaluate(&catalogue, scope, op.clone()).map_err(refused)?;

    // The kernel admitted a change, so now this host opens its own store: the
    // directory, the lock every writer shares, and a second reading under it.
    // Another writer may have been admitted between the two readings, so the
    // answer over the LOCKED catalogue is the authoritative one.
    fs::create_dir_all(&dir).map_err(io_error)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("catalogue.lock"))
        .map_err(io_error)?;
    lock.lock_exclusive().map_err(io_error)?;

    let catalogue = load(&catalogue_path, scope)?;
    let outcome =
        ds_command_kernel::local_models::evaluate(&catalogue, scope, op).map_err(refused)?;
    let model = outcome
        .model
        .as_ref()
        .ok_or_else(|| StoreError::Store("the admitted operation named no model".into()))?;
    // Every file this answer touches is named BEFORE anything is written, so a
    // refusal never follows a write.
    let file = package_path(&dir, &model.id)?;

    let forgotten = !outcome
        .catalogue
        .models
        .iter()
        .any(|row| row.id == model.id);

    match package {
        // The package is written before the catalogue, so a row never names a
        // file that is not there. For a revision the atomic rename replaces
        // the previous package in one step: there is never a moment with no
        // package under the id.
        Some(bytes) => {
            fs::create_dir_all(packages_dir(&dir)).map_err(io_error)?;
            write_atomic(&file, &packages_dir(&dir), bytes)?;
            persist(&catalogue_path, &dir, &outcome.catalogue)?;
        }
        // A forget: the row goes first, so what a crash leaves behind is a
        // package with no row — recoverable by hand — and never a row that
        // points at nothing.
        None if forgotten => {
            persist(&catalogue_path, &dir, &outcome.catalogue)?;
            match fs::remove_file(&file) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    return Err(StoreError::Store(format!(
                        "the model left the catalogue but its package {} could not be deleted",
                        file.to_string_lossy()
                    )));
                }
            }
        }
        None => persist(&catalogue_path, &dir, &outcome.catalogue)?,
    }

    Ok(outcome)
}

/// Where these rows live, said the same way the overlay registry says it.
pub fn default_root() -> Result<PathBuf, String> {
    crate::default_root()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_command_kernel::local_models::Origin;

    fn scope() -> Scope {
        Scope {
            lane: "stable".into(),
            uid: "uid-1".into(),
        }
    }

    fn register(id: &str, name: &str) -> Op {
        Op::Register {
            id: id.into(),
            display_name: name.into(),
            origin: Origin::Created,
            crs: "EPSG:32635".into(),
            model_revision: 0,
            bytes: 4,
            sha256: "c".repeat(64),
            created_at: None,
            project: None,
            head_revision: None,
            activate: false,
        }
    }

    #[test]
    fn a_revision_replaces_the_package_under_its_id_and_moves_the_row() {
        let root = tempfile::tempdir().expect("temp root");
        execute_at(root.path(), &scope(), register("m-1", "A"), Some(b"pkg1")).unwrap();
        let dir = scope_dir(root.path(), &scope()).unwrap();
        let package = package_path(&dir, "m-1").unwrap();

        let revised = execute_at(
            root.path(),
            &scope(),
            Op::Revise {
                id: "m-1".to_owned(),
                expected_sha256: Some("c".repeat(64)),
                model_revision: 1,
                bytes: 5,
                sha256: "d".repeat(64),
                head_revision: Some("rev:abc".into()),
                revised_at: None,
            },
            Some(b"pkg-2"),
        )
        .expect("revises");
        assert_eq!(std::fs::read(&package).unwrap(), b"pkg-2");
        let row = revised.model.expect("row");
        assert_eq!(row.model_revision, 1);
        assert_eq!(row.sha256, "d".repeat(64));
        assert_eq!(row.head_revision.as_deref(), Some("rev:abc"));
        assert_eq!(
            row.revised_at.as_deref().map(str::len),
            Some(20),
            "the store's clock"
        );
        assert_eq!(read_at(root.path(), &scope()).unwrap(), revised.catalogue);

        // A stale expectation leaves both the row and the bytes alone.
        let stale = execute_at(
            root.path(),
            &scope(),
            Op::Revise {
                id: "m-1".to_owned(),
                expected_sha256: Some("c".repeat(64)),
                model_revision: 2,
                bytes: 5,
                sha256: "e".repeat(64),
                head_revision: None,
                revised_at: None,
            },
            Some(b"pkg-3"),
        )
        .unwrap_err();
        assert_eq!(
            stale.code(),
            ds_command_kernel::local_models::REVISION_CONFLICT
        );
        assert_eq!(std::fs::read(&package).unwrap(), b"pkg-2");
        assert_eq!(read_at(root.path(), &scope()).unwrap(), revised.catalogue);

        // A revision without bytes is not a revision.
        assert!(execute_at(
            root.path(),
            &scope(),
            Op::Revise {
                id: "m-1".to_owned(),
                expected_sha256: None,
                model_revision: 2,
                bytes: 5,
                sha256: "e".repeat(64),
                head_revision: None,
                revised_at: None,
            },
            None
        )
        .is_err());
    }

    #[test]
    fn a_registered_package_is_on_disk_before_the_row_that_names_it() {
        let root = tempfile::tempdir().expect("temp root");
        let outcome = execute_at(
            root.path(),
            &scope(),
            register("m-1", "Feeder A"),
            Some(b"pkg"),
        )
        .expect("registers");
        let dir = scope_dir(root.path(), &scope()).unwrap();
        let package = package_path(&dir, "m-1").unwrap();
        assert!(package.is_file(), "the package was not written");
        assert_eq!(std::fs::read(&package).unwrap(), b"pkg");
        assert_eq!(outcome.catalogue.models.len(), 1);
        assert_eq!(outcome.catalogue.active.as_deref(), Some("m-1"));

        // Reopening reads exactly what was written.
        let reopened = read_at(root.path(), &scope()).expect("reads");
        assert_eq!(reopened, outcome.catalogue);
    }

    #[test]
    fn forgetting_a_copy_removes_its_package_too() {
        let root = tempfile::tempdir().expect("temp root");
        execute_at(root.path(), &scope(), register("m-1", "A"), Some(b"pkg")).unwrap();
        let dir = scope_dir(root.path(), &scope()).unwrap();
        let package = package_path(&dir, "m-1").unwrap();
        assert!(package.is_file());

        execute_at(
            root.path(),
            &scope(),
            Op::Forget {
                id: "m-1".to_owned(),
            },
            None,
        )
        .expect("forgets");
        assert!(!package.exists(), "the package outlived its row");
        assert!(read_at(root.path(), &scope()).unwrap().models.is_empty());
    }

    #[test]
    fn a_refusal_leaves_the_store_exactly_as_it_was() {
        let root = tempfile::tempdir().expect("temp root");
        execute_at(
            root.path(),
            &scope(),
            register("m-1", "Feeder A"),
            Some(b"pkg"),
        )
        .unwrap();
        let before = read_at(root.path(), &scope()).unwrap();

        let refused = execute_at(
            root.path(),
            &scope(),
            register("m-2", "feeder a"),
            Some(b"pkg"),
        )
        .unwrap_err();
        assert_eq!(refused.code(), "local_model_name_taken");
        assert_eq!(read_at(root.path(), &scope()).unwrap(), before);
        let dir = scope_dir(root.path(), &scope()).unwrap();
        assert!(
            !package_path(&dir, "m-2").unwrap().exists(),
            "a refused registration wrote a package anyway"
        );
    }

    #[test]
    fn an_operation_and_its_bytes_must_agree() {
        let root = tempfile::tempdir().expect("temp root");
        assert!(
            execute_at(root.path(), &scope(), register("m-1", "A"), None).is_err(),
            "a registration without bytes was accepted"
        );
        assert!(
            execute_at(
                root.path(),
                &scope(),
                Op::SetActive {
                    id: "m-1".to_owned()
                },
                Some(b"pkg")
            )
            .is_err(),
            "an operation that owns no package took bytes"
        );
    }

    #[test]
    fn the_store_reads_the_clock_the_kernel_does_not_have() {
        let root = tempfile::tempdir().expect("temp root");
        let outcome = execute_at(root.path(), &scope(), register("m-1", "A"), Some(b"pkg"))
            .expect("registers");
        let stamp = &outcome.model.expect("model").created_at;
        // RFC3339 to the second, which is what the catalogue keeps.
        assert_eq!(stamp.len(), 20, "{stamp} is not one RFC3339 second");
        assert!(stamp.ends_with('Z') && stamp.contains('T'));
        assert!(
            stamp.starts_with("20"),
            "{stamp} is not a plausible instant"
        );
    }

    #[test]
    fn a_link_is_persisted_on_the_row_and_stamped_by_the_hosts_clock() {
        use ds_command_kernel::local_models::PlsSourceLink;
        let root = tempfile::tempdir().expect("temp root");
        execute_at(root.path(), &scope(), register("m-1", "A"), Some(b"pkg")).unwrap();
        let outcome = execute_at(
            root.path(),
            &scope(),
            Op::Link {
                id: "m-1".into(),
                pls_source: PlsSourceLink {
                    path: "/srv/pls/Nyamagabe".into(),
                    digest: format!("sha256:{}", "7".repeat(64)),
                    pls_version: "16.81".into(),
                    member_versions: Default::default(),
                    member_count: 42,
                    linked_at: String::new(),
                },
            },
            None,
        )
        .expect("links");
        let link = outcome.model.unwrap().pls_source.expect("link");
        assert_eq!(
            link.linked_at.len(),
            20,
            "{} is not one RFC3339 second",
            link.linked_at
        );
        let reopened = read_at(root.path(), &scope()).unwrap();
        assert_eq!(
            reopened.models[0].pls_source.as_ref().unwrap().digest,
            link.digest
        );
        // The package is untouched by a link.
        let dir = scope_dir(root.path(), &scope()).unwrap();
        assert_eq!(
            std::fs::read(package_path(&dir, "m-1").unwrap()).unwrap(),
            b"pkg"
        );
    }

    #[test]
    fn one_machine_keeps_two_accounts_apart() {
        let root = tempfile::tempdir().expect("temp root");
        let theirs = Scope {
            lane: "stable".into(),
            uid: "uid-2".into(),
        };
        execute_at(root.path(), &scope(), register("m-1", "Mine"), Some(b"pkg")).unwrap();
        execute_at(
            root.path(),
            &theirs,
            register("m-1", "Theirs"),
            Some(b"pkg"),
        )
        .unwrap();

        assert_eq!(
            read_at(root.path(), &scope()).unwrap().models[0].display_name,
            "Mine"
        );
        assert_eq!(
            read_at(root.path(), &theirs).unwrap().models[0].display_name,
            "Theirs"
        );
    }
}
