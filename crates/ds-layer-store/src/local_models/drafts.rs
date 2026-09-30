//! Durable content first, independently pinned catalogue second. All writers
//! share the existing catalogue lock; orphan content is never discovered.

use super::*;
use ds_command_kernel::local_models::{
    draft_content_digest, draft_digest_is_safe, DraftCheckpointPin, LocalDraft, PackageIdentity,
    DRAFT_CONFLICT, DRAFT_CORRUPT, MAX_DRAFT_BYTES,
};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredDraft {
    pub base: PackageIdentity,
    pub draft: LocalDraft,
    pub bytes: Vec<u8>,
}

fn fail(code: &str, message: &str) -> StoreError {
    StoreError::Refused {
        code: code.into(),
        message: message.into(),
    }
}

fn drafts_dir(dir: &Path) -> PathBuf {
    dir.join("drafts")
}

/// Only a lowercase engine digest can name local content. No caller path or
/// model name enters this filename, and no directory scan resolves a draft.
pub fn draft_path(dir: &Path, content_digest: &str) -> Result<PathBuf, StoreError> {
    if !draft_digest_is_safe(content_digest) {
        return Err(fail(
            DRAFT_CORRUPT,
            "the draft digest names no safe local content path",
        ));
    }
    Ok(drafts_dir(dir).join(format!("{content_digest}.draft")))
}

fn regular_file(path: &Path) -> Result<File, StoreError> {
    if !fs::symlink_metadata(path).map_err(io_error)?.is_file() {
        return Err(fail(
            DRAFT_CORRUPT,
            "local recovery content must be a regular file",
        ));
    }
    File::open(path).map_err(io_error)
}

fn check_directory(path: &Path) -> Result<(), StoreError> {
    if !fs::symlink_metadata(path).map_err(io_error)?.is_dir() {
        return Err(fail(
            DRAFT_CORRUPT,
            "local recovery content must be in its own directory",
        ));
    }
    Ok(())
}

fn verify_base(dir: &Path, id: &str, base: &PackageIdentity) -> Result<(), StoreError> {
    if base.bytes == 0 || base.bytes > MAX_PACKAGE_BYTES {
        return Err(fail(
            DRAFT_CONFLICT,
            "the retained base package exceeds its byte bound",
        ));
    }
    let file = regular_file(&package_path(dir, id)?)?;
    if file.metadata().map_err(io_error)?.len() != base.bytes {
        return Err(fail(
            DRAFT_CONFLICT,
            "the retained base package size changed",
        ));
    }
    let mut hash = Sha256::new();
    let mut read = file.take(base.bytes + 1);
    let mut count = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = read.read(&mut buffer).map_err(io_error)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        hash.update(&buffer[..n]);
    }
    if count != base.bytes || !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&base.sha256) {
        return Err(fail(
            DRAFT_CONFLICT,
            "the retained base package digest changed",
        ));
    }
    Ok(())
}

fn read_content(dir: &Path, draft: &LocalDraft) -> Result<Vec<u8>, StoreError> {
    check_directory(&drafts_dir(dir))?;
    let file = regular_file(&draft_path(dir, &draft.pin.content_digest)?)?;
    if file.metadata().map_err(io_error)?.len() != draft.bytes {
        return Err(fail(DRAFT_CORRUPT, "the pinned local draft size changed"));
    }
    let mut bytes = Vec::new();
    file.take(draft.bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 != draft.bytes || draft_content_digest(&bytes) != draft.pin.content_digest
    {
        return Err(fail(DRAFT_CORRUPT, "the pinned local draft digest changed"));
    }
    Ok(bytes)
}

/// Stage exact engine checkpoint bytes, then CAS the independently retained
/// pin and base identity under the catalogue's existing exclusive lock.
/// The caller supplies an engine-produced pin; journal replay remains the
/// engine's responsibility. This does not authorize package exchange.
pub fn stage_draft_at(
    root: &Path,
    scope: &Scope,
    id: &str,
    expected_base: &PackageIdentity,
    expected_draft: Option<&DraftCheckpointPin>,
    pin: &DraftCheckpointPin,
    bytes: &[u8],
) -> Result<Outcome, StoreError> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_DRAFT_BYTES {
        return Err(fail(DRAFT_CORRUPT, "a local draft exceeds its byte bound"));
    }
    if draft_content_digest(bytes) != pin.content_digest {
        return Err(fail(
            DRAFT_CORRUPT,
            "draft bytes do not match the supplied checkpoint pin",
        ));
    }
    let draft = LocalDraft {
        pin: pin.clone(),
        bytes: bytes.len() as u64,
    };
    let op = Op::StageDraft {
        id: id.into(),
        expected_base: expected_base.clone(),
        expected_draft: expected_draft.cloned(),
        draft: draft.clone(),
    };
    let dir = scope_dir(root, scope)?;
    let catalogue_path = dir.join("catalogue.json");
    ds_command_kernel::local_models::evaluate(&load(&catalogue_path, scope)?, scope, op.clone())
        .map_err(refused)?;

    // Registration already created this lock; never invent a row or package.
    check_directory(&dir)?;
    let lock = private::open_options()
        .read(true)
        .write(true)
        .open(dir.join("catalogue.lock"))
        .map_err(io_error)?;
    lock.lock_exclusive().map_err(io_error)?;
    let outcome =
        ds_command_kernel::local_models::evaluate(&load(&catalogue_path, scope)?, scope, op)
            .map_err(refused)?;
    verify_base(&dir, id, expected_base)?;

    let content_dir = drafts_dir(&dir);
    match private::create_dir(&content_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error(error)),
    }
    check_directory(&content_dir)?;
    // Persist the new directory entry before a catalogue can name its files.
    #[cfg(unix)]
    File::open(&dir)
        .and_then(|file| file.sync_all())
        .map_err(io_error)?;
    let path = draft_path(&dir, &pin.content_digest)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            // Existing content is immutable, including orphan content. Refuse
            // corruption rather than repairing bytes another pin may name.
            read_content(&dir, &draft)?;
            regular_file(&path)?.sync_all().map_err(io_error)?;
            #[cfg(unix)]
            File::open(&content_dir)
                .and_then(|file| file.sync_all())
                .map_err(io_error)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_atomic(&path, &content_dir, bytes)?;
        }
        Err(error) => return Err(io_error(error)),
    }
    // A crash/failure here leaves only harmless, unpinned content. Forget
    // intentionally leaves drafts behind too: cleanup has no recovery role.
    persist(&catalogue_path, &dir, &outcome.catalogue)?;
    Ok(outcome)
}

/// Read only the independently pinned draft while holding the shared lock.
/// Verify retained base bytes and the draft's exact size/digest before return.
/// `None` means no pin, even if complete or partial orphan files exist.
pub fn read_draft_at(
    root: &Path,
    scope: &Scope,
    id: &str,
) -> Result<Option<StoredDraft>, StoreError> {
    let dir = scope_dir(root, scope)?;
    let catalogue_path = dir.join("catalogue.json");
    // An absent model is an ordinary kernel refusal, without creating files.
    load(&catalogue_path, scope)?
        .model(scope, id)
        .map_err(refused)?;
    check_directory(&dir)?;
    let lock = private::open_options()
        .read(true)
        .open(dir.join("catalogue.lock"))
        .map_err(io_error)?;
    FileExt::lock_shared(&lock).map_err(io_error)?;
    let catalogue = load(&catalogue_path, scope)?;
    let model = catalogue.model(scope, id).map_err(refused)?;
    let Some(draft) = model.draft.as_ref() else {
        return Ok(None);
    };
    draft.validate_for(model).map_err(refused)?;
    let base = model.package_identity();
    verify_base(&dir, id, &base)?;
    let bytes = read_content(&dir, draft)?;
    Ok(Some(StoredDraft {
        base,
        draft: draft.clone(),
        bytes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_command_kernel::local_models::{DraftBinding, Origin, DRAFT_PENDING};

    fn fixture() -> (tempfile::TempDir, Scope, PackageIdentity) {
        let root = tempfile::tempdir().unwrap();
        let scope = Scope {
            lane: "stable".into(),
            uid: "u-1".into(),
        };
        let row = execute_at(
            root.path(),
            &scope,
            Op::Register {
                id: "m-1".into(),
                display_name: "A".into(),
                origin: Origin::Created,
                crs: "EPSG:32635".into(),
                model_revision: 1,
                bytes: 4,
                sha256: format!("{:x}", Sha256::digest(b"base")),
                created_at: None,
                project: None,
                head_revision: None,
                activate: false,
            },
            Some(b"base"),
        )
        .unwrap()
        .model
        .unwrap();
        (root, scope, row.package_identity())
    }

    fn pin(base: &PackageIdentity, sequence: u64, bytes: &[u8]) -> DraftCheckpointPin {
        DraftCheckpointPin {
            binding: DraftBinding {
                local_model_id: "m-1".into(),
                base_artifact_sha256: base.sha256.clone(),
            },
            head_revision: format!("rev:{}", "b".repeat(64)),
            sequence,
            content_digest: draft_content_digest(bytes),
        }
    }

    #[test]
    fn stage_reopen_and_advance_never_overwrite_the_package_or_its_identity() {
        let (root, scope, base) = fixture();
        let dir = scope_dir(root.path(), &scope).unwrap();
        let first = pin(&base, 1, b"draft-1");
        let staged =
            stage_draft_at(root.path(), &scope, "m-1", &base, None, &first, b"draft-1").unwrap();
        assert_eq!(read_at(root.path(), &scope).unwrap(), staged.catalogue);
        let loaded = read_draft_at(root.path(), &scope, "m-1").unwrap().unwrap();
        assert_eq!(loaded.bytes, b"draft-1");
        assert_eq!(loaded.draft.pin, first);
        assert_eq!(loaded.base, base);
        let second = pin(&base, 2, b"draft-2");
        stage_draft_at(
            root.path(),
            &scope,
            "m-1",
            &base,
            Some(&first),
            &second,
            b"draft-2",
        )
        .unwrap();
        assert_eq!(
            read_draft_at(root.path(), &scope, "m-1")
                .unwrap()
                .unwrap()
                .draft
                .pin,
            second
        );
        let catalogue = read_at(root.path(), &scope).unwrap();
        assert_eq!(catalogue.models[0].package_identity(), base);
        for expected_sha256 in [None, Some(base.sha256.clone())] {
            let err = execute_at(
                root.path(),
                &scope,
                Op::Revise {
                    id: "m-1".into(),
                    expected_sha256,
                    model_revision: 2,
                    bytes: 7,
                    sha256: format!("{:x}", Sha256::digest(b"newbase")),
                    head_revision: None,
                    revised_at: None,
                },
                Some(b"newbase"),
            )
            .unwrap_err();
            assert_eq!(err.code(), DRAFT_PENDING);
        }
        assert_eq!(
            fs::read(package_path(&dir, "m-1").unwrap()).unwrap(),
            b"base"
        );
        assert_eq!(read_at(root.path(), &scope).unwrap(), catalogue);
        #[cfg(unix)]
        private::assert_private_tree(&dir);
    }

    #[test]
    fn two_concurrent_writers_have_only_one_cas_winner() {
        let (root, scope, base) = fixture();
        let barrier = std::sync::Barrier::new(2);
        let results = std::thread::scope(|threads| {
            let handles: Vec<_> = [b"writer-a".as_slice(), b"writer-b".as_slice()]
                .into_iter()
                .map(|bytes| {
                    let barrier = &barrier;
                    let root = root.path();
                    let scope = &scope;
                    let base = &base;
                    threads.spawn(move || {
                        let next = pin(base, 1, bytes);
                        barrier.wait();
                        stage_draft_at(root, scope, "m-1", base, None, &next, bytes)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results.into_iter().find_map(Result::err).unwrap().code(),
            DRAFT_CONFLICT
        );
        let read = read_draft_at(root.path(), &scope, "m-1").unwrap().unwrap();
        assert!(read.bytes == b"writer-a" || read.bytes == b"writer-b");
    }

    #[test]
    fn stale_pin_or_base_refuses_without_touching_catalogue_or_package() {
        let (root, scope, base) = fixture();
        let first = pin(&base, 1, b"first");
        stage_draft_at(root.path(), &scope, "m-1", &base, None, &first, b"first").unwrap();
        let before = read_at(root.path(), &scope).unwrap();
        let next = pin(&base, 2, b"next");
        assert_eq!(
            stage_draft_at(root.path(), &scope, "m-1", &base, None, &next, b"next")
                .unwrap_err()
                .code(),
            DRAFT_CONFLICT
        );
        let mut stale_base = base.clone();
        stale_base.bytes += 1;
        assert_eq!(
            stage_draft_at(
                root.path(),
                &scope,
                "m-1",
                &stale_base,
                Some(&first),
                &next,
                b"next"
            )
            .unwrap_err()
            .code(),
            DRAFT_CONFLICT
        );
        assert_eq!(read_at(root.path(), &scope).unwrap(), before);
        let dir = scope_dir(root.path(), &scope).unwrap();
        assert!(!draft_path(&dir, &next.content_digest).unwrap().exists());
        assert_eq!(
            fs::read(package_path(&dir, "m-1").unwrap()).unwrap(),
            b"base"
        );
    }

    #[test]
    fn corrupt_truncated_oversized_or_missing_bytes_never_fall_back_to_orphans() {
        let (root, scope, base) = fixture();
        let first = pin(&base, 1, b"first");
        stage_draft_at(root.path(), &scope, "m-1", &base, None, &first, b"first").unwrap();
        let dir = scope_dir(root.path(), &scope).unwrap();
        let path = draft_path(&dir, &first.content_digest).unwrap();
        let orphan = pin(&base, 2, b"orphan");
        fs::write(draft_path(&dir, &orphan.content_digest).unwrap(), b"orphan").unwrap();
        for bytes in [
            b"FIRST".as_slice(),
            b"fir".as_slice(),
            b"first-extra".as_slice(),
        ] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(
                read_draft_at(root.path(), &scope, "m-1")
                    .unwrap_err()
                    .code(),
                DRAFT_CORRUPT
            );
        }
        File::create(&path)
            .unwrap()
            .set_len(MAX_DRAFT_BYTES + 1)
            .unwrap();
        assert_eq!(
            read_draft_at(root.path(), &scope, "m-1")
                .unwrap_err()
                .code(),
            DRAFT_CORRUPT
        );
        fs::remove_file(&path).unwrap();
        assert!(read_draft_at(root.path(), &scope, "m-1").is_err());
        assert_eq!(
            read_at(root.path(), &scope).unwrap().models[0]
                .draft
                .as_ref()
                .unwrap()
                .pin,
            first
        );
    }

    #[test]
    fn old_catalogue_and_interrupted_staging_ignore_all_unpinned_content() {
        let (root, scope, base) = fixture();
        let dir = scope_dir(root.path(), &scope).unwrap();
        let original = fs::read(dir.join("catalogue.json")).unwrap();
        assert!(!String::from_utf8_lossy(&original).contains("draft"));
        private::create_dir(&drafts_dir(&dir)).unwrap();
        let orphan = pin(&base, 1, b"orphan");
        write_atomic(
            &draft_path(&dir, &orphan.content_digest).unwrap(),
            &drafts_dir(&dir),
            b"orphan",
        )
        .unwrap();
        // Both interruption points: incomplete temporary bytes and durable
        // content left before the independent catalogue pin was committed.
        fs::write(drafts_dir(&dir).join("partial-tmp"), b"partial").unwrap();
        assert!(read_draft_at(root.path(), &scope, "m-1").unwrap().is_none());
        assert_eq!(fs::read(dir.join("catalogue.json")).unwrap(), original);
        // A retry can safely use complete orphan bytes of the exact digest.
        stage_draft_at(root.path(), &scope, "m-1", &base, None, &orphan, b"orphan").unwrap();
        assert_eq!(
            read_draft_at(root.path(), &scope, "m-1")
                .unwrap()
                .unwrap()
                .bytes,
            b"orphan"
        );
        execute_at(root.path(), &scope, Op::Forget { id: "m-1".into() }, None).unwrap();
        assert!(draft_path(&dir, &orphan.content_digest).unwrap().exists());
        assert_eq!(
            read_draft_at(root.path(), &scope, "m-1")
                .unwrap_err()
                .code(),
            ds_command_kernel::local_models::UNKNOWN_MODEL
        );
    }

    #[test]
    fn failed_catalogue_commit_leaves_unpinned_durable_content_and_old_catalogue() {
        let (root, scope, base) = fixture();
        let dir = scope_dir(root.path(), &scope).unwrap();
        let mut catalogue = read_at(root.path(), &scope).unwrap();
        // A readable legacy catalogue with only 100 bytes of headroom. The
        // admitted pin exceeds the host's catalogue write limit, after its
        // content has been durably written: a real interrupted-commit path.
        let initial_size = serde_json::to_vec(&catalogue).unwrap().len();
        catalogue.models[0].display_name = "x".repeat(MAX_CATALOGUE_BYTES - 100 - initial_size + 1);
        let original = serde_json::to_vec(&catalogue).unwrap();
        assert_eq!(original.len(), MAX_CATALOGUE_BYTES - 100);
        write_atomic(&dir.join("catalogue.json"), &dir, &original).unwrap();
        let first = pin(&base, 1, b"first");
        assert!(stage_draft_at(root.path(), &scope, "m-1", &base, None, &first, b"first").is_err());
        assert_eq!(fs::read(dir.join("catalogue.json")).unwrap(), original);
        assert_eq!(
            fs::read(draft_path(&dir, &first.content_digest).unwrap()).unwrap(),
            b"first"
        );
        assert!(read_draft_at(root.path(), &scope, "m-1").unwrap().is_none());
        assert_eq!(
            fs::read(package_path(&dir, "m-1").unwrap()).unwrap(),
            b"base"
        );
    }

    #[test]
    fn stage_cannot_bypass_bytes_and_corrupt_orphans_are_not_overwritten() {
        let (root, scope, base) = fixture();
        let first = pin(&base, 1, b"first");
        let before = read_at(root.path(), &scope).unwrap();
        assert_eq!(
            stage_draft_at(root.path(), &scope, "m-1", &base, None, &first, b"wrong")
                .unwrap_err()
                .code(),
            DRAFT_CORRUPT
        );
        assert!(execute_at(
            root.path(),
            &scope,
            Op::StageDraft {
                id: "m-1".into(),
                expected_base: base.clone(),
                expected_draft: None,
                draft: LocalDraft {
                    pin: first.clone(),
                    bytes: 5
                },
            },
            None
        )
        .is_err());
        let dir = scope_dir(root.path(), &scope).unwrap();
        private::create_dir(&drafts_dir(&dir)).unwrap();
        let path = draft_path(&dir, &first.content_digest).unwrap();
        fs::write(&path, b"wrong").unwrap();
        assert_eq!(
            stage_draft_at(root.path(), &scope, "m-1", &base, None, &first, b"first")
                .unwrap_err()
                .code(),
            DRAFT_CORRUPT
        );
        assert_eq!(fs::read(path).unwrap(), b"wrong");
        assert_eq!(read_at(root.path(), &scope).unwrap(), before);
        assert!(draft_path(&dir, "../escape").is_err());
    }

    #[test]
    fn changed_base_bytes_refuse_both_stage_and_recovery() {
        let (root, scope, base) = fixture();
        let first = pin(&base, 1, b"first");
        stage_draft_at(root.path(), &scope, "m-1", &base, None, &first, b"first").unwrap();
        let dir = scope_dir(root.path(), &scope).unwrap();
        let next = pin(&base, 2, b"next");
        for bytes in [b"BASE".as_slice(), b"longer-base".as_slice()] {
            fs::write(package_path(&dir, "m-1").unwrap(), bytes).unwrap();
            assert_eq!(
                read_draft_at(root.path(), &scope, "m-1")
                    .unwrap_err()
                    .code(),
                DRAFT_CONFLICT
            );
            assert_eq!(
                stage_draft_at(
                    root.path(),
                    &scope,
                    "m-1",
                    &base,
                    Some(&first),
                    &next,
                    b"next"
                )
                .unwrap_err()
                .code(),
                DRAFT_CONFLICT
            );
            assert!(!draft_path(&dir, &next.content_digest).unwrap().exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn content_symlinks_refuse_without_touching_their_target() {
        let (root, scope, base) = fixture();
        let dir = scope_dir(root.path(), &scope).unwrap();
        let outside = root.path().join("outside");
        fs::write(&outside, b"first").unwrap();
        let first = pin(&base, 1, b"first");
        private::create_dir(&drafts_dir(&dir)).unwrap();
        std::os::unix::fs::symlink(&outside, draft_path(&dir, &first.content_digest).unwrap())
            .unwrap();
        assert_eq!(
            stage_draft_at(root.path(), &scope, "m-1", &base, None, &first, b"first")
                .unwrap_err()
                .code(),
            DRAFT_CORRUPT
        );
        assert_eq!(fs::read(&outside).unwrap(), b"first");
    }
}
