//! Deliberate, digest-bound retirement of one stopped Core's old account cache.
//! Ordinary startup never adopts another principal's rows or connection record.
use crate::auth::{NativeCredential, OwnerAnswer, OwnerCredential};
use crate::cache_recovery::{self as files, HeldFile};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const PENDING: &str = "cache-retirement.pending.json";
const MEMBERS: [&str; 5] = [
    "store.sqlite-wal",
    "store.sqlite-shm",
    "store.sqlite-journal",
    "store.sqlite",
    "connection.json",
];
const MAX_CACHE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

pub static COMMAND: Command = Command {
    id: "core.cache.retire",
    path: &["core", "cache", "retire"],
    contract: 1,
    summary: "Retain an old account's stopped Core cache before starting fresh.",
    purpose: "After an intentional native account change, inspect with --dry-run, then retire that exact plan with --digest and --yes. Under Core's single-owner transport lock and a recheck of the current credential, connection.json, store.sqlite and its fixed sidecars are retained as one digest-bound epoch; an interrupted plan continues explicitly. No rows are imported; the state path, installation identity, credentials, components and authored files are kept. No Core start or stop, account switch, install or network.",
    chapter: Chapter::Operations,
    effect: Effect::MachineWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        crate::STATE,
        crate::LANE,
        Arg::switch(
            "dry-run",
            "Inspect the exact retirement plan without moving cache members.",
        ),
        Arg::value(
            "digest",
            "<sha256>",
            "Exact plan digest returned by --dry-run; required for retirement or interrupted continuation.",
        ),
    ],
    output: "ds.core-cache-retirement/v1: exact plan digest, fixed member digests and lengths, canonical state path, archive prefix and completion status. No credentials or database rows.",
    examples: &[],
    refusals: &[
        crate::PLATFORM,
        Refusal {
            code: "headless_signed_out",
            when: "no protected native credential is held for the requested lane",
            remedy: "connect the intended account through ds account connect for the same lane first",
        },
        Refusal {
            code: "core_cache_retire_refused",
            when: "Core is active, the lane/account did not change as declared, protected files or plan digest changed, or retention cannot finish safely",
            remedy: "stop Core deliberately, inspect ds core cache retire --dry-run under the intended native account and lane, then use that exact digest; retain pending evidence and follow its named reason",
        },
    ],
    reference: Some("docs/reference/core.md"),
    search: &["history", "switch account"],
    requires: Requires::Server,
    availability: crate::native_host_availability,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: String,
    state_directory: String,
    lane: String,
    previous_owner: String,
    current_owner: String,
    files: Vec<HeldFile>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: String,
    digest: String,
    plan: Plan,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionRecord {
    schema: String,
    owner: String,
    lane: String,
}

fn refusal(error: impl ToString) -> Failure {
    Failure::conflict("core_cache_retire_refused", error.to_string())
        .remedy(COMMAND.refusals[2].remedy)
}
fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn digest(plan: &Plan) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(plan).map_err(|e| e.to_string())?)
    ))
}
fn prefix(plan: &Plan) -> Result<String, String> {
    Ok(format!("cache-retired-{}", digest(plan)?))
}
fn archive(directory: &Path, plan: &Plan, file: &HeldFile) -> Result<PathBuf, String> {
    Ok(directory.join(format!("{}.{}", prefix(plan)?, file.name)))
}
fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}
fn read_small(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut file = files::private_file(path)?;
    if file.metadata().map_err(|e| e.to_string())?.len() > limit {
        return Err("protected record exceeds its bound".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("protected record changed beyond its bound".into());
    }
    Ok(bytes)
}
fn held(path: &Path, connection: bool) -> Result<HeldFile, String> {
    let file = files::private_file(path)?;
    let bound = if connection { 4096 } else { MAX_CACHE_BYTES };
    if file.metadata().map_err(|e| e.to_string())?.len() > bound {
        return Err("Core cache member exceeds retirement bound".into());
    }
    drop(file);
    files::held_with_limit(path, bound)
}
fn validate(plan: &Plan, lane: &str, owner: &str) -> Result<(), String> {
    if plan.schema != "ds.core-cache-retirement-plan/v1"
        || plan.lane != lane
        || plan.current_owner != owner
        || !sha256(owner)
        || !sha256(&plan.previous_owner)
        || plan.previous_owner == owner
    {
        return Err("retirement requires an intentional change to a different protected account on this exact lane".into());
    }
    let names: Vec<_> = plan.files.iter().map(|f| f.name.as_str()).collect();
    let expected: Vec<_> = MEMBERS.into_iter().filter(|n| names.contains(n)).collect();
    if names != expected
        || names.last() != Some(&"connection.json")
        || (names.len() > 1 && !names.contains(&"store.sqlite"))
        || plan.files.iter().any(|f| {
            !sha256(&f.sha256)
                || f.size_bytes
                    > if f.name == "connection.json" {
                        4096
                    } else {
                        MAX_CACHE_BYTES
                    }
        })
    {
        return Err(
            "retirement plan contains invalid, duplicate, orphaned or out-of-order cache members"
                .into(),
        );
    }
    Ok(())
}
fn inspect(directory: &Path, lane: &str, owner: &str) -> Result<Plan, String> {
    if exists(&directory.join(PENDING))? {
        let journal: Journal =
            serde_json::from_slice(&read_small(&directory.join(PENDING), 16384)?)
                .map_err(|e| e.to_string())?;
        validate(&journal.plan, lane, owner)?;
        if journal.schema != "ds.core-cache-retirement/v1"
            || journal.digest != digest(&journal.plan)?
        {
            return Err("pending retirement journal digest is invalid".into());
        }
        verify_members(directory, &journal.plan, true)?;
        return Ok(journal.plan);
    }
    let connection = directory.join("connection.json");
    let connection_bytes = read_small(&connection, 4096)?;
    let record: ConnectionRecord = serde_json::from_slice(&connection_bytes).map_err(|_| "retirement requires the current protected connection schema; legacy or invalid records are not adopted")?;
    if record.schema != crate::host::CONNECTION_SCHEMA || record.lane != lane {
        return Err("protected Core connection has a different schema or lane".into());
    }
    let mut members = Vec::new();
    for name in MEMBERS {
        let path = directory.join(name);
        if exists(&path)? {
            members.push(held(&path, name == "connection.json")?);
        }
    }
    let plan = Plan {
        schema: "ds.core-cache-retirement-plan/v1".into(),
        state_directory: location(directory)?,
        lane: lane.into(),
        previous_owner: record.owner,
        current_owner: owner.into(),
        files: members,
    };
    validate(&plan, lane, owner)?;
    // Bind the parsed owner/lane to the exact fingerprint, preserving formatting.
    let connection_sha = format!("{:x}", Sha256::digest(&connection_bytes));
    if plan.files.last().is_none_or(|f| f.sha256 != connection_sha) {
        return Err("Core connection changed during inspection".into());
    }
    verify_members(directory, &plan, false)?;
    Ok(plan)
}
fn matches(path: &Path, file: &HeldFile) -> Result<(), String> {
    let now = held(path, file.name == "connection.json")?;
    if now.size_bytes != file.size_bytes || now.sha256 != file.sha256 {
        return Err("Core cache member changed after plan inspection".into());
    }
    Ok(())
}
fn location(directory: &Path) -> Result<String, String> {
    crate::host::validate_directory(directory)?;
    directory
        .canonicalize()
        .map_err(|e| e.to_string())?
        .into_os_string()
        .into_string()
        .map_err(|_| "Core state path must be valid UTF-8".into())
}
fn verify_members(directory: &Path, plan: &Plan, pending: bool) -> Result<(), String> {
    if location(directory)? != plan.state_directory {
        return Err("retirement plan belongs to a different canonical state directory".into());
    }
    // No newly appeared sidecar may be attached to a subsequent fresh database.
    for name in MEMBERS {
        if !plan.files.iter().any(|f| f.name == name) && exists(&directory.join(name))? {
            return Err("Core cache member set changed after inspection".into());
        }
    }
    for file in &plan.files {
        let source = directory.join(&file.name);
        let destination = archive(directory, plan, file)?;
        match (exists(&source)?, exists(&destination)?) {
            (true, false) => matches(&source, file)?,
            (false, true) if pending => matches(&destination, file)?,
            _ => {
                return Err(
                    "Core cache member is absent, duplicated or archive destination is occupied"
                        .into(),
                );
            }
        }
    }
    Ok(())
}
pub(crate) fn require_complete(directory: &Path) -> Result<(), String> {
    if exists(&directory.join(PENDING))? {
        return Err("Core cache retirement is incomplete; inspect ds core cache retire --dry-run on this lane and explicitly continue its exact digest before starting Core".into());
    }
    Ok(())
}
fn rollback(directory: &Path, plan: &Plan) -> Result<(), String> {
    for file in plan.files.iter().rev() {
        let source = directory.join(&file.name);
        let destination = archive(directory, plan, file)?;
        if exists(&destination)? {
            matches(&destination, file)?;
            files::move_private_no_replace(&destination, &source)?;
        }
        matches(&source, file)?;
    }
    fs::remove_file(directory.join(PENDING)).map_err(|e| e.to_string())?;
    files::sync_directory(directory)
}
fn create_or_verify(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if exists(path)? {
        if read_small(path, 16384)? != bytes {
            return Err("retirement evidence destination is occupied by different bytes".into());
        }
        Ok(())
    } else {
        // Publish only a complete, synced record. A torn staging write is never
        // mistaken for the canonical pending journal or completion evidence.
        let mut nonce = [0u8; 16];
        getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
        let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let staging = path
            .parent()
            .ok_or("retirement evidence has no parent")?
            .join(format!("cache-retirement-evidence-{nonce}.new"));
        files::create_private(&staging, bytes)?;
        files::move_private_no_replace(&staging, path)
    }
}
fn retire(
    directory: &Path,
    plan: &Plan,
    expected: &str,
    authorized: &dyn Fn() -> Result<(), String>,
    rename: &dyn Fn(&Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    if expected != digest(plan)? {
        return Err(
            "plan digest changed; inspect a fresh dry run before authorizing retirement".into(),
        );
    }
    authorized()?;
    let pending = exists(&directory.join(PENDING))?;
    verify_members(directory, plan, pending)?;
    let bytes = serde_json::to_vec(&Journal {
        schema: "ds.core-cache-retirement/v1".into(),
        digest: digest(plan)?,
        plan: plan.clone(),
    })
    .map_err(|e| e.to_string())?;
    create_or_verify(&directory.join(PENDING), &bytes)?;
    files::sync_directory(directory)?;
    let moved = (|| {
        for file in &plan.files {
            authorized()?;
            verify_members(directory, plan, true)?;
            let source = directory.join(&file.name);
            if exists(&source)? {
                rename(&source, &archive(directory, plan, file)?)?;
            }
        }
        authorized()?;
        verify_members(directory, plan, true)?;
        Ok::<(), String>(())
    })();
    if let Err(error) = moved {
        let rollback_error = rollback(directory, plan).err();
        return Err(format!(
            "retirement stopped: {error}; rollback_error={rollback_error:?}; original bytes remain under the canonical or exact archive names"
        ));
    }
    // From this point an I/O interruption leaves a resumable journal and every
    // exact old member in its archive. Startup cannot adopt a partial epoch.
    let prefix = prefix(plan)?;
    create_or_verify(&directory.join(format!("{prefix}.receipt.json")), &bytes)?;
    create_or_verify(
        &directory.join(format!("{prefix}.complete")),
        b"ds.core-cache-retirement/v1\n",
    )?;
    files::sync_directory(directory)?;
    fs::remove_file(directory.join(PENDING)).map_err(|e| e.to_string())?;
    files::sync_directory(directory)
}

pub fn command(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    crate::require_native_host()?;
    let lane = inputs.require("lane")?;
    let source = NativeCredential::new(lane.into());
    let captured = source.held().map_err(refusal)?;
    let owner = match &captured {
        OwnerAnswer::Held { owner, .. } => owner,
        OwnerAnswer::SignedOut => {
            return Err(Failure::unauthorized(
                "headless_signed_out",
                "no native credential is held for this lane",
            )
            .remedy(COMMAND.refusals[1].remedy));
        }
    };
    let directory = crate::state(inputs)?;
    crate::host::validate_directory(&directory).map_err(refusal)?;
    // This is Core's existing OS lock and private socket, never a second server.
    // No request is served. Dropping Listening releases the lock/socket.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(refusal)?;
    let _entered = runtime.enter();
    let _listening = crate::transport::listen(&directory).map_err(refusal)?;
    let authorized = || match source.held()? == captured {
        true => Ok(()),
        false => Err("protected native credential changed during retirement".into()),
    };
    authorized().map_err(refusal)?;
    let plan = inspect(&directory, lane, owner).map_err(refusal)?;
    let plan_digest = digest(&plan).map_err(refusal)?;
    if let Some(expected) = inputs.value("digest") {
        if expected != plan_digest {
            return Err(refusal("plan digest changed; inspect a fresh dry run"));
        }
    }
    let dry_run = inputs.switch("dry-run");
    if !dry_run {
        let expected = inputs
            .value("digest")
            .ok_or_else(|| refusal("retirement requires the exact --digest from --dry-run"))?;
        retire(
            &directory,
            &plan,
            expected,
            &authorized,
            &files::move_private_no_replace,
        )
        .map_err(refusal)?;
    }
    authorized().map_err(refusal)?;
    Ok(
        json!({"schema":"ds.core-cache-retirement/v1","dry_run":dry_run,"complete":!dry_run,"plan_digest":plan_digest,"state_directory":directory,"archive_prefix":prefix(&plan).map_err(refusal)?,"plan":plan,"old_history_reused":false}),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn fixture() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let connection=serde_json::to_vec(&json!({"schema":crate::host::CONNECTION_SCHEMA,"owner":"a".repeat(64),"lane":"canary"})).unwrap();
        files::create_private(&d.path().join("connection.json"), &connection).unwrap();
        drop(ds_sync_store::Store::open(&d.path().join("store.sqlite")).unwrap());
        fs::set_permissions(
            d.path().join("store.sqlite"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        d
    }
    fn plan(d: &Path) -> Plan {
        inspect(d, "canary", &"b".repeat(64)).unwrap()
    }
    #[test]
    fn healthy_old_epoch_is_retained_and_fresh_owner_uses_same_path() {
        let d = fixture();
        for name in [
            "identity.fixture",
            "credentials.fixture",
            "components.fixture",
            "authored.fixture",
        ] {
            files::create_private(&d.path().join(name), name.as_bytes()).unwrap();
        }
        for name in &MEMBERS[..3] {
            files::create_private(&d.path().join(name), name.as_bytes()).unwrap();
        }
        let p = plan(d.path());
        assert_eq!(p.files.len(), 5);
        retire(
            d.path(),
            &p,
            &digest(&p).unwrap(),
            &|| Ok(()),
            &files::move_private_no_replace,
        )
        .unwrap();
        require_complete(d.path()).unwrap();
        for f in &p.files {
            assert!(!d.path().join(&f.name).exists());
            matches(&archive(d.path(), &p, f).unwrap(), f).unwrap();
        }
        for name in [
            "identity.fixture",
            "credentials.fixture",
            "components.fixture",
            "authored.fixture",
        ] {
            assert_eq!(fs::read(d.path().join(name)).unwrap(), name.as_bytes());
        }
        crate::host::connection(d.path(), "b".repeat(64), "canary".into()).unwrap();
        drop(ds_sync_store::Store::open(&d.path().join("store.sqlite")).unwrap());
        assert_eq!(
            crate::host::load_connection(d.path()).unwrap().owner,
            "b".repeat(64)
        );
    }
    #[test]
    fn same_owner_other_lane_and_orphan_sidecar_refuse() {
        let d = fixture();
        assert!(inspect(d.path(), "canary", &"a".repeat(64)).is_err());
        assert!(inspect(d.path(), "stable", &"b".repeat(64)).is_err());
        fs::remove_file(d.path().join("store.sqlite")).unwrap();
        files::create_private(&d.path().join("store.sqlite-wal"), b"orphan").unwrap();
        assert!(inspect(d.path(), "canary", &"b".repeat(64)).is_err());
    }
    #[test]
    fn changed_digest_file_and_new_sidecar_never_move_members() {
        for case in 0..3 {
            let d = fixture();
            let p = plan(d.path());
            let expected = if case == 0 {
                "0".repeat(64)
            } else {
                digest(&p).unwrap()
            };
            if case == 1 {
                fs::write(d.path().join("store.sqlite"), b"changed").unwrap();
            }
            if case == 2 {
                files::create_private(&d.path().join("store.sqlite-wal"), b"new").unwrap();
            }
            assert!(
                retire(
                    d.path(),
                    &p,
                    &expected,
                    &|| Ok(()),
                    &files::move_private_no_replace
                )
                .is_err()
            );
            assert!(d.path().join("connection.json").exists());
            assert!(!d.path().join(PENDING).exists());
        }
    }
    #[test]
    fn rename_failure_after_effect_rolls_back_exact_epoch() {
        let d = fixture();
        let p = plan(d.path());
        let calls = std::cell::Cell::new(0);
        let rename = |from: &Path, to: &Path| {
            files::move_private_no_replace(from, to)?;
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err("injected sync failure after rename".into())
            } else {
                Ok(())
            }
        };
        assert!(retire(d.path(), &p, &digest(&p).unwrap(), &|| Ok(()), &rename).is_err());
        for f in &p.files {
            matches(&d.path().join(&f.name), f).unwrap();
            assert!(!archive(d.path(), &p, f).unwrap().exists());
        }
        require_complete(d.path()).unwrap();
    }
    #[test]
    fn interrupted_epoch_blocks_startup_and_resumes_only_exact_plan() {
        let d = fixture();
        let p = plan(d.path());
        let bytes = serde_json::to_vec(&Journal {
            schema: "ds.core-cache-retirement/v1".into(),
            digest: digest(&p).unwrap(),
            plan: p.clone(),
        })
        .unwrap();
        files::create_private(&d.path().join(PENDING), &bytes).unwrap();
        files::move_private_no_replace(
            &d.path().join(&p.files[0].name),
            &archive(d.path(), &p, &p.files[0]).unwrap(),
        )
        .unwrap();
        assert!(require_complete(d.path()).is_err());
        assert_eq!(inspect(d.path(), "canary", &"b".repeat(64)).unwrap(), p);
        assert!(inspect(d.path(), "canary", &"c".repeat(64)).is_err());
        retire(
            d.path(),
            &p,
            &digest(&p).unwrap(),
            &|| Ok(()),
            &files::move_private_no_replace,
        )
        .unwrap();
        require_complete(d.path()).unwrap();
    }
    #[test]
    fn credential_change_during_moves_restores_old_epoch() {
        let d = fixture();
        let p = plan(d.path());
        let calls = std::cell::Cell::new(0);
        let auth = || {
            calls.set(calls.get() + 1);
            if calls.get() > 2 {
                Err("account changed".into())
            } else {
                Ok(())
            }
        };
        assert!(
            retire(
                d.path(),
                &p,
                &digest(&p).unwrap(),
                &auth,
                &files::move_private_no_replace
            )
            .is_err()
        );
        for f in &p.files {
            matches(&d.path().join(&f.name), f).unwrap();
        }
        require_complete(d.path()).unwrap();
    }
    #[test]
    fn unsafe_files_are_refused_without_retirement() {
        for case in 0..3 {
            let d = fixture();
            let path = d.path().join("store.sqlite");
            let other = d.path().join("other");
            if case < 2 {
                fs::rename(&path, &other).unwrap();
                if case == 0 {
                    symlink(&other, &path).unwrap();
                } else {
                    fs::hard_link(&other, &path).unwrap();
                }
            } else {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            }
            assert!(inspect(d.path(), "canary", &"b".repeat(64)).is_err());
            assert!(d.path().join("connection.json").exists());
            assert!(!d.path().join(PENDING).exists());
        }
    }
    #[test]
    fn native_retention_never_replaces_occupied_or_dangling_archive_names() {
        for link in [false, true] {
            let d = fixture();
            let source = d.path().join("source.fixture");
            let target = d.path().join("target.fixture");
            files::create_private(&source, b"source").unwrap();
            if link {
                symlink(d.path().join("absent"), &target).unwrap();
            } else {
                files::create_private(&target, b"target").unwrap();
            }
            assert!(files::move_private_no_replace(&source, &target).is_err());
            assert_eq!(fs::read(&source).unwrap(), b"source");
            if link {
                assert!(
                    fs::symlink_metadata(&target)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
            } else {
                assert_eq!(fs::read(&target).unwrap(), b"target");
            }
        }
    }
    #[test]
    fn malformed_pending_evidence_fences_startup_without_guessing_repair() {
        let d = fixture();
        files::create_private(&d.path().join(PENDING), b"{").unwrap();
        assert!(require_complete(d.path()).is_err());
        assert!(inspect(d.path(), "canary", &"b".repeat(64)).is_err());
        assert!(d.path().join("connection.json").exists());
        assert!(d.path().join("store.sqlite").exists());
        assert_eq!(fs::read(d.path().join(PENDING)).unwrap(), b"{");
    }
    #[test]
    fn active_native_owner_refuses_maintenance_lock() {
        let d = fixture();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _entered = rt.enter();
        let first = crate::transport::listen(d.path()).unwrap();
        assert!(crate::transport::listen(d.path()).is_err());
        drop(first);
        assert!(crate::transport::listen(d.path()).is_ok());
    }
    #[test]
    fn torn_staging_is_ignored_but_occupied_final_evidence_is_never_overwritten() {
        let d = fixture();
        files::create_private(&d.path().join("cache-retirement-evidence-torn.new"), b"{").unwrap();
        let path = d.path().join("evidence.fixture");
        create_or_verify(&path, b"complete").unwrap();
        create_or_verify(&path, b"complete").unwrap();
        assert!(create_or_verify(&path, b"different").is_err());
        assert_eq!(fs::read(path).unwrap(), b"complete");
        assert_eq!(
            fs::read(d.path().join("cache-retirement-evidence-torn.new")).unwrap(),
            b"{"
        );
        require_complete(d.path()).unwrap();
    }
    #[test]
    fn plan_cannot_retire_identical_bytes_in_another_state_directory() {
        let a = fixture();
        let b = fixture();
        let p = plan(a.path());
        for f in &p.files {
            fs::copy(a.path().join(&f.name), b.path().join(&f.name)).unwrap();
        }
        assert!(
            retire(
                b.path(),
                &p,
                &digest(&p).unwrap(),
                &|| Ok(()),
                &files::move_private_no_replace
            )
            .is_err()
        );
        for f in &p.files {
            matches(&b.path().join(&f.name), f).unwrap();
        }
        require_complete(b.path()).unwrap();
    }
}
