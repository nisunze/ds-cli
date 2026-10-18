//! Offline, protected-file adapter to the native Survey queue owner.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_command_kernel::survey_entry_outbox::Fence;
use ds_sync_store::survey_entries::{self, SurveyEntryDiscardExpected, create};
use ds_sync_store::{Store, SurveyEntryScope};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const INVALID: Refusal = Refusal {
    code: "survey_queue_invalid_input",
    when: "the closed request, exact project, or absolute private input file is invalid",
    remedy: "use headers {}, row {id}, admit {rows,now}, transition {event,now}, prepare_create {fence}, verify_create_receipt {expected,status,response}, prepare_discard {ids,now}, or commit_discard {expected,now} with action as the discriminator; preserve the source file",
};
const DISCARD: Refusal = Refusal {
    code: "survey_queue_discard_requires_audit",
    when: "a generic transition attempts to discard retained Survey rows",
    remedy: "use prepare_discard, submit the exact prepared rows to the existing audit endpoint, then commit_discard with their fences and row_sha256 values",
};
const STORE: Refusal = Refusal {
    code: "survey_queue_store_refused",
    when: "the existing private native store is unsafe, corrupt, incompatible, or rejects the requested queue operation",
    remedy: "retain the existing store and source, inspect the exact owned project queue and resolve the native refusal; no repair or alternate identity is attempted",
};
const OUTPUT: Refusal = Refusal {
    code: "survey_queue_output_invalid",
    when: "the absolute new output cannot be created privately or the result exceeds 64 MiB",
    remedy: "use an unused absolute output file in an existing protected directory; if detail says mutation_completed, inspect the retained queue before retrying",
};
const CHANGED: Refusal = Refusal {
    code: "survey_queue_identity_changed",
    when: "the protected account, deployment or installation changes before an effect or its acknowledgment",
    remedy: "restore the intended protected identity and inspect the retained queue before retrying any completed mutation",
};

pub static COMMAND: Command = Command {
    id: "core.survey.queue", path: &["core", "survey", "queue"], contract: 2,
    summary: "Read, admit or transition the retained native Survey entry queue offline.",
    purpose: "Use the installed Core SQLite owner under the existing lane's protected account, deployment and installation and one exact project. Closed request actions are headers, row, admit, transition, prepare_create, verify_create_receipt, prepare_discard and commit_discard. Create preparation returns the shared Rust direct Firestore commit effect from the exact retained row and protected principal/public Firebase profile. Verification reconstructs that effect and checks the actual commit receipt, fenced by account, project, lifecycle, full-row and direct-plan digests; neither action mutates or dispatches. Prepare is read-only; callers audit its exact rows before commit, which atomically rechecks every row digest, lifecycle fence and Kernel discard decision. Generic discard transitions refuse. The Kernel decides replay; persist/remove actions commit atomically before transport effects are returned. This command runs no transport, Core daemon, network, credential refresh, project selection or store repair. Reads never create a missing store or table. Admissions refuse foreign or ownerless rows. Output is preadmitted before mutations, bounded to 64 MiB and acknowledged only after a protected identity recheck.",
    chapter: Chapter::Survey, effect: Effect::LocalAuthState, authority: Authority::HeadlessUser, execution: Execution::Sync,
    args: &[Arg::value("lane", "<stable|canary>", "Existing protected native identity lane.").choices(&["stable", "canary"]).required(), super::PROJECT.required(),
        Arg::value("request", "<absolute-existing-json-file>", "Owner-only ordinary single-link file, at most 64 MiB. Closed JSON discriminator action: headers; row with id; admit with rows and now; transition with event and now; prepare_create with fence; verify_create_receipt with expected {fence,row_sha256,direct_sha256}, status and response; prepare_discard with ids and now; commit_discard with expected [{fence,row_sha256}] and now. Discard batches contain at most 500 unique IDs. No extra request fields.").required(),
        Arg::value("out", "<absolute-new-json-file>", "New owner-only result file in an existing protected directory; never replaced, at most 64 MiB.").required()],
    output: "Bounded ds.survey-queue-file/v1 receipt with path, sha256, size_bytes, action, project, lane and safe owner context (account, deployment_sha256, install_id). The bound file is ds.survey-queue/v1 with the same context and result. Transition results retain Kernel effects after atomic persistence. Create preparation returns {expected:{fence,row_sha256},plan}; receipt verification returns {expected,receipt}. Discard prepare returns {rows:[{fence,row_sha256,row}]}; commit returns {removed:[id]}.",
    examples: &[], refusals: &[super::PLATFORM, INVALID, DISCARD, STORE, OUTPUT, CHANGED, super::CONTEXT_CORRUPT, super::PROJECT_REQUIRED],
    reference: Some("docs/reference/core.md"), search: &["survey", "offline", "queue", "sqlite"], requires: Requires::Server, availability: super::native_host_availability,
};
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Headers {},
    PrepareCreate {
        fence: Fence,
    },
    VerifyCreateReceipt {
        expected: create::Expected,
        status: u16,
        response: Value,
    },
    Row {
        id: String,
    },
    Admit {
        rows: Vec<Value>,
        now: u64,
    },
    Transition {
        event: Value,
        now: u64,
    },
    PrepareDiscard {
        ids: Vec<String>,
        now: u64,
    },
    CommitDiscard {
        expected: Vec<SurveyEntryDiscardExpected>,
        now: u64,
    },
}
impl Request {
    fn action(&self) -> &'static str {
        match self {
            Self::Headers {} => "headers",
            Self::PrepareCreate { .. } => "prepare_create",
            Self::VerifyCreateReceipt { .. } => "verify_create_receipt",
            Self::Row { .. } => "row",
            Self::Admit { .. } => "admit",
            Self::Transition { .. } => "transition",
            Self::PrepareDiscard { .. } => "prepare_discard",
            Self::CommitDiscard { .. } => "commit_discard",
        }
    }
    fn mutates(&self) -> bool {
        !matches!(
            self,
            Self::Headers {}
                | Self::Row { .. }
                | Self::PrepareDiscard { .. }
                | Self::PrepareCreate { .. }
                | Self::VerifyCreateReceipt { .. }
        )
    }
    fn validate(&self, scope: &SurveyEntryScope, project: &str) -> Result<(), Failure> {
        match self {
            Self::Admit { rows, .. } => {
                if rows
                    .iter()
                    .any(|row| row["project_id"] != project || row["owner_uid"] != scope.account)
                {
                    return Err(failure(
                        INVALID,
                        "every incoming row must name the captured account and exact requested project; source preserved",
                    ));
                }
            }
            Self::PrepareDiscard { ids, .. } => {
                survey_entries::validate_discard_ids(ids).map_err(|e| failure(INVALID, e))?
            }
            Self::CommitDiscard { expected, .. } => {
                survey_entries::validate_discard_expected(expected)
                    .map_err(|e| failure(INVALID, e))?
            }
            Self::Transition { event, .. } => {
                if event.get("kind").and_then(Value::as_str) == Some("discard") {
                    return Err(failure(
                        DISCARD,
                        "Audit the prepared Survey rows before discarding them.",
                    ));
                }
                if !event.is_object()
                    || event
                        .get("current_uid")
                        .is_some_and(|uid| uid != &Value::String(scope.account.clone()))
                {
                    return Err(failure(
                        INVALID,
                        "transition must be an object under the captured current_uid",
                    ));
                }
            }
            Self::Row { id }
                if id.is_empty()
                    || id.len() > 512
                    || id.trim() != id
                    || id.chars().any(char::is_control) =>
            {
                return Err(failure(INVALID, "row id is invalid"));
            }
            _ => {}
        }
        Ok(())
    }
}
fn failure(refusal: Refusal, detail: impl ToString) -> Failure {
    Failure::failed(refusal.code, detail.to_string()).remedy(refusal.remedy)
}
fn identity(lane: &str) -> Result<SurveyEntryScope, Failure> {
    let principal = ds_cli_auth::headless_principal_read_only(lane)?;
    Ok(SurveyEntryScope {
        account: principal.account_uid().into(),
        deployment: principal.deployment().into(),
        install_id: principal.install_id().into(),
    })
}
fn owner(scope: &SurveyEntryScope) -> Value {
    json!({"account":scope.account,"deployment_sha256":format!("{:x}", Sha256::digest(scope.deployment.as_bytes())),"install_id":scope.install_id})
}
fn absolute(path: &Path) -> Result<(), Failure> {
    if !path.is_absolute() || path.to_str().is_none() {
        return Err(failure(
            INVALID,
            "request and output must be absolute Unicode paths",
        ));
    }
    Ok(())
}
fn request(path: &Path) -> Result<Request, Failure> {
    absolute(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|e| failure(INVALID, e))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(failure(
            INVALID,
            "request must be an ordinary file, never a symlink",
        ));
    }
    let mut file = super::cache_recovery::private_file(path).map_err(|e| failure(INVALID, e))?;
    if file.metadata().map_err(|e| failure(INVALID, e))?.len() > MAX_BYTES as u64 {
        return Err(failure(INVALID, "request exceeds 64 MiB"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| failure(INVALID, e))?;
    if bytes.len() > MAX_BYTES {
        return Err(failure(INVALID, "request exceeds 64 MiB"));
    }
    serde_json::from_slice(&bytes).map_err(|e| failure(INVALID, e))
}
fn create_output(path: &Path) -> Result<fs::File, Failure> {
    absolute(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| failure(OUTPUT, "output has no parent"))?;
    super::host::validate_directory(parent).map_err(|e| failure(OUTPUT, e))?;
    #[cfg(windows)]
    {
        ds_cli_auth::windows_host::create_private_output_file(path).map_err(|e| failure(OUTPUT, e))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| failure(OUTPUT, e))
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(failure(OUTPUT, "private native files unavailable"))
    }
}
fn validate_database(database: &Path) -> Result<(), Failure> {
    match fs::symlink_metadata(database) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(failure(
                    STORE,
                    "native store must be an ordinary file, never a symlink",
                ));
            }
            #[cfg(windows)]
            let file = ds_cli_auth::windows_host::read_live_store_file(database);
            #[cfg(not(windows))]
            let file = super::cache_recovery::private_file(database);
            drop(file.map_err(|e| failure(STORE, e))?);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(failure(STORE, e)),
    }
    Ok(())
}
fn read(
    store: Option<&Store>,
    scope: &SurveyEntryScope,
    project: &str,
    request: &Request,
    direct: Option<&ds_command_kernel::survey_entry_create::direct::Authority>,
) -> Result<Value, Failure> {
    match request {
        Request::PrepareCreate { fence } => {
            create::prepare_direct(store, scope, project, fence, direct.ok_or_else(|| failure(STORE, "Firebase create authority unavailable"))?).map_err(|e| failure(STORE, e))
        }
        Request::VerifyCreateReceipt {
            expected,
            status,
            response,
        } => create::verify_direct(store, scope, project, expected, direct.ok_or_else(|| failure(STORE, "Firebase create authority unavailable"))?, *status, response)
            .map_err(|e| failure(STORE, e)),
        Request::Headers {} => match store {
            Some(s) => s
                .survey_entry_headers(scope, Some(project))
                .and_then(|h| Ok(serde_json::to_value(h)?))
                .map_err(|e| failure(STORE, e)),
            None => Ok(json!([])),
        },
        Request::Row { id } => {
            let row = match store {
                Some(s) => s
                    .survey_entry_row(scope, id)
                    .map_err(|e| failure(STORE, e))?,
                None => None,
            };
            if row.as_ref().is_some_and(|row| row["project_id"] != project) {
                return Err(failure(
                    STORE,
                    "requested row is unavailable in the exact project",
                ));
            }
            Ok(row.unwrap_or(Value::Null))
        }
        Request::PrepareDiscard { ids, now } => {
            survey_entries::prepare_discard(store, scope, project, ids, *now)
                .map(|rows| json!({"rows":rows}))
                .map_err(|e| failure(STORE, e))
        }
        _ => unreachable!("closed read dispatch"),
    }
}
fn perform(
    database: &Path,
    direct: Option<&ds_command_kernel::survey_entry_create::direct::Authority>,
    scope: &SurveyEntryScope,
    project: &str,
    request: Request,
) -> Result<Value, Failure> {
    validate_database(database)?;
    if !request.mutates() {
        let store = Store::open_read_only(database).map_err(|e| failure(STORE, e))?;
        if matches!(
            &request,
            Request::PrepareDiscard { .. }
                | Request::PrepareCreate { .. }
                | Request::VerifyCreateReceipt { .. }
        ) {
            // The owning preparation API holds its one read snapshot itself.
            return read(store.as_ref(), scope, project, &request, direct);
        }
        return match store.as_ref() {
            Some(store) => store
                .with_read_snapshot(|s| read(Some(s), scope, project, &request, direct))
                .map_err(|e| failure(STORE, e))?,
            None => read(None, scope, project, &request, direct),
        };
    }
    if let Request::CommitDiscard { expected, now } = &request {
        if expected.is_empty() {
            survey_entries::prepare_discard(None, scope, project, &[], *now)
                .map_err(|e| failure(STORE, e))?;
            return Ok(json!({"removed":[]}));
        }
    }
    let parent = database
        .parent()
        .ok_or_else(|| failure(STORE, "store has no parent"))?;
    super::host::prepare_directory(parent).map_err(|e| failure(STORE, e))?;
    let mut store = Store::open(database).map_err(|e| failure(STORE, e))?;
    match request {
        Request::Admit { rows, now } => {
            store
                .survey_entry_admit_batch(scope, rows, now)
                .map_err(|e| failure(STORE, e))?;
            Ok(json!({"admitted":true}))
        }
        Request::CommitDiscard { expected, now } => store
            .survey_entry_commit_discard(scope, project, &expected, now)
            .map(|removed| json!({"removed":removed}))
            .map_err(|e| failure(STORE, e)),
        Request::Transition { event, now } => store
            .survey_entry_transition(scope, project, event, now)
            .map_err(|e| failure(STORE, e)),
        _ => unreachable!("closed mutation dispatch"),
    }
}
struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_BYTES {
            return Err(std::io::Error::other("Survey result exceeds 64 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn capture(
    database: &Path,
    lane: &str,
    direct: Option<&ds_command_kernel::survey_entry_create::direct::Authority>,
    project: &str,
    request: Request,
    out: &Path,
    observe: impl Fn() -> Result<SurveyEntryScope, Failure>,
) -> Result<Value, Failure> {
    let project = super::bounded_project(project)?;
    let scope = observe()?;
    scope.validate().map_err(|e| failure(INVALID, e))?;
    request.validate(&scope, &project)?;
    let action = request.action();
    let mutates = request.mutates();
    // Reserve the actual owner-only file before any queue effect.
    let mut file = create_output(out)?;
    let mut completed = false;
    let result = (|| {
        if observe()? != scope {
            return Err(failure(
                CHANGED,
                "protected identity changed before queue effect",
            ));
        }
        let result = perform(database, direct, &scope, &project, request)?;
        completed = mutates;
        let payload = json!({"schema":"ds.survey-queue/v1","action":action,"project":project,"lane":lane,"owner":owner(&scope),"result":result});
        let mut bytes = BoundedBytes(Vec::new());
        serde_json::to_writer(&mut bytes, &payload).map_err(|e| failure(OUTPUT, e))?;
        file.write_all(&bytes.0)
            .and_then(|_| file.sync_all())
            .map_err(|e| failure(OUTPUT, e))?;
        if observe()? != scope {
            return Err(failure(
                CHANGED,
                "protected identity changed before queue acknowledgment",
            ));
        }
        Ok(
            json!({"schema":"ds.survey-queue-file/v1","path":out,"sha256":format!("{:x}",Sha256::digest(&bytes.0)),"size_bytes":bytes.0.len(),"action":action,"project":project,"lane":lane,"owner":owner(&scope)}),
        )
    })();
    drop(file);
    match result {
        Ok(receipt) => Ok(receipt),
        Err(error) => {
            let cleanup = fs::remove_file(out);
            let cleanup_error = cleanup.err().map(|e| e.to_string());
            if completed || cleanup_error.is_some() {
                return Err(failure(OUTPUT, if completed { "Survey mutation committed; queued state is retained; output acknowledgment failed" } else { "Survey operation failed and unacknowledged output cleanup failed" }).detail(json!({"mutation_completed":completed,"action":action,"project":project,"lane":lane,"owner":owner(&scope),"cause":error.code(),"cause_message":error.message(),"cleanup_error":cleanup_error})));
            }
            Err(error)
        }
    }
}
pub fn command(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    super::require_native_host()?;
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let request = request(&PathBuf::from(inputs.require("request")?))?;
    let database = ds_compute_runtime::server_state_directory(lane, None)
        .map_err(|e| failure(INVALID, e))?
        .join("store.sqlite");
    let direct = if matches!(&request, Request::PrepareCreate { .. } | Request::VerifyCreateReceipt { .. }) {
        Some(ds_cli_auth::survey_direct_authority_read_only(lane)?)
    } else { None };
    capture(
        &database,
        lane,
        direct.as_ref(),
        project,
        request,
        &PathBuf::from(inputs.require("out")?),
        || identity(lane),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[cfg(windows)]
    struct Directory(PathBuf);
    #[cfg(windows)]
    impl Directory {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    #[cfg(windows)]
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[cfg(windows)]
    fn directory() -> Directory {
        let mut nonce = [0u8; 16];
        getrandom::getrandom(&mut nonce).unwrap();
        let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let path = ds_cli_auth::windows_host::local_application_data()
            .unwrap()
            .join(format!(
                "ds-survey-queue-test-{}-{nonce}",
                std::process::id()
            ));
        super::super::host::prepare_directory(&path).unwrap();
        Directory(path)
    }
    #[cfg(not(windows))]
    fn directory() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        super::super::host::prepare_directory(dir.path()).unwrap();
        dir
    }
    fn direct_authority() -> ds_command_kernel::survey_entry_create::direct::Authority {
        ds_command_kernel::survey_entry_create::direct::Authority { firebase_project:"test-project".into(), uid:"owner-a".into(), email:"test@example.invalid".into(), lane:"stable".into() }
    }
    fn scope() -> SurveyEntryScope {
        SurveyEntryScope {
            account: "owner-a".into(),
            deployment: "https://gateway.example".into(),
            install_id: "device-a".into(),
        }
    }
    fn row(id: &str, project: &str) -> Value {
        json!({"kind":"survey","id":id,"owner_uid":"owner-a","generation":"11111111-2222-3333-4444-555555555555","project_id":project,"form_id":"form-a","doc_id":format!("doc-{id}"),"operation":"create","payload":{"answers":{"opaque":[1,2,3]},"expected_version":3},"overlay":{"type":"Feature","properties":{"label":"offline"}},"status":"pending","attempts":0,"created_at":17,"synced_at":null,"error":null,"idempotency_key":format!("key-{id}")})
    }
    fn run(db: &Path, out: &Path, project: &str, request: Request) -> Result<Value, Failure> {
        capture(db, "stable", Some(&direct_authority()), project, request, out, || Ok(scope()))
    }
    fn payload(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }
    fn private_write(path: &Path, bytes: &[u8]) {
        let mut f = create_output(path).unwrap();
        f.write_all(bytes).unwrap();
    }
    #[test]
    fn closed_requests_refuse_malformed_unknown_and_arbitrary_fields() {
        let dir = directory();
        for (i, value) in [
            json!({"action":"sql","sql":"select 1"}),
            json!({"action":"headers","uid":"foreign"}),
            json!({"action":"headers","project":"other"}),
            json!({"action":"admit","rows":[],"now":20,"transport":"http"}),
            json!({"action":"transition","event":{},"now":-1}),
        ]
        .into_iter()
        .enumerate()
        {
            let file = dir.path().join(format!("{i}.json"));
            private_write(&file, &serde_json::to_vec(&value).unwrap());
            assert!(request(&file).is_err());
        }
        let oversized = dir.path().join("oversized.json");
        create_output(&oversized)
            .unwrap()
            .set_len(MAX_BYTES as u64 + 1)
            .unwrap();
        assert!(request(&oversized).is_err());
        let file = dir.path().join("broken.json");
        private_write(&file, b"{");
        assert!(request(&file).is_err());
        assert!(request(Path::new("relative.json")).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(request(&file).is_err());
            let link = dir.path().join("linked.json");
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert!(request(&link).is_err());
        }
    }
    #[test]
    fn native_create_policy_reads_private_retained_bytes_without_queue_effects() {
        let dir = directory();
        let db = dir.path().join("state/store.sqlite");
        let mut captured = row("one", "project-a");
        captured["form_id"] = json!("field_notes");
        captured["payload"] = json!({"data":{"note":"retained capture"}});
        run(
            &db,
            &dir.path().join("admit.json"),
            "project-a",
            Request::Admit {
                rows: vec![captured],
                now: 20,
            },
        )
        .unwrap();
        let store = Store::open_read_only(&db).unwrap().unwrap();
        let fence = Fence::from(
            &store
                .survey_entry_headers(&scope(), Some("project-a"))
                .unwrap()[0],
        );
        let before = store.survey_entry_row(&scope(), "one").unwrap();
        drop(store);
        let out = dir.path().join("prepare.json");
        let receipt = run(
            &db,
            &out,
            "project-a",
            Request::PrepareCreate {
                fence: fence.clone(),
            },
        )
        .unwrap();
        assert_eq!(receipt["action"], "prepare_create");
        let prepared = payload(&out)["result"].clone();
        assert_eq!(
            prepared["plan"]["payload"]["data"],
            json!({"note":"retained capture"})
        );
        assert_eq!(prepared["plan"]["identity"]["doc_id"], "doc-one");
        let expected: create::Expected =
            serde_json::from_value(prepared["expected"].clone()).unwrap();
        let response = json!({"commitTime":"2026-10-18T09:02:47Z","writeResults":[{"updateTime":"2026-10-18T09:02:47Z","transformResults":[{"timestampValue":"2026-10-18T09:02:47Z"}]}]});
        let verified = dir.path().join("verified.json");
        run(
            &db,
            &verified,
            "project-a",
            Request::VerifyCreateReceipt {
                expected,
                status: 200,
                response,
            },
        )
        .unwrap();
        assert_eq!(payload(&verified)["result"]["receipt"]["doc_id"], "doc-one");
        assert!(
            run(
                &db,
                &dir.path().join("crossed.json"),
                "project-b",
                Request::PrepareCreate { fence }
            )
            .is_err()
        );
        let store = Store::open_read_only(&db).unwrap().unwrap();
        assert_eq!(store.survey_entry_row(&scope(), "one").unwrap(), before);
        assert!(!serde_json::from_value::<Request>(json!({"action":"prepare_create","fence":prepared["expected"]["fence"],"payload":{}})).is_ok());
    }

    #[test]
    fn absent_store_and_absent_table_are_empty_without_creation_or_changes() {
        let dir = directory();
        let db = dir.path().join("absent/store.sqlite");
        let out = dir.path().join("headers.json");
        let receipt = run(&db, &out, "p", Request::Headers {}).unwrap();
        assert_eq!(payload(&out)["result"], json!([]));
        assert!(!db.parent().unwrap().exists());
        assert_eq!(receipt["schema"], "ds.survey-queue-file/v1");
        assert_eq!(
            receipt["sha256"],
            format!("{:x}", Sha256::digest(fs::read(&out).unwrap()))
        );
        assert_eq!(receipt["owner"]["account"], "owner-a");
        assert!(receipt["owner"].get("deployment").is_none());
        let out = dir.path().join("row.json");
        run(
            &db,
            &out,
            "p",
            Request::Row {
                id: "missing".into(),
            },
        )
        .unwrap();
        assert!(payload(&out)["result"].is_null());
        let db = dir.path().join("store.sqlite");
        drop(Store::open(&db).unwrap());
        let before = fs::read(&db).unwrap();
        let out = dir.path().join("old.json");
        run(&db, &out, "p", Request::Headers {}).unwrap();
        assert_eq!(fs::read(&db).unwrap(), before);
        let connection =
            rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let count: u64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='survey_entry_outbox'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
    #[test]
    fn admission_readback_restart_and_transition_use_the_owner() {
        let dir = directory();
        let db = dir.path().join("state/store.sqlite");
        let captured = row("one", "p");
        let out = dir.path().join("admitted.json");
        run(
            &db,
            &out,
            "p",
            Request::Admit {
                rows: vec![captured.clone()],
                now: 20,
            },
        )
        .unwrap();
        assert_eq!(payload(&out)["result"], json!({"admitted":true}));
        run(
            &db,
            &dir.path().join("second.json"),
            "p",
            Request::Admit {
                rows: vec![row("two", "p")],
                now: 20,
            },
        )
        .unwrap();
        let out = dir.path().join("row.json");
        run(&db, &out, "p", Request::Row { id: "one".into() }).unwrap();
        assert_eq!(payload(&out)["result"]["payload"], captured["payload"]);
        assert_eq!(payload(&out)["result"]["overlay"], captured["overlay"]);
        assert_eq!(payload(&out)["result"]["owner_uid"], "owner-a");
        let out = dir.path().join("headers.json");
        run(&db, &out, "p", Request::Headers {}).unwrap();
        assert_eq!(payload(&out)["result"].as_array().unwrap().len(), 2);
        let out = dir.path().join("scheduled.json");
        run(
            &db,
            &out,
            "p",
            Request::Transition {
                event: json!({"kind":"schedule","current_uid":"owner-a","online":true}),
                now: 100,
            },
        )
        .unwrap();
        assert!(
            payload(&out)["result"]["actions"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
        );
        let reopened = Store::open_read_only(&db).unwrap().unwrap();
        assert!(
            reopened
                .survey_entry_headers(&scope(), Some("p"))
                .unwrap()
                .iter()
                .any(|h| h.lease > 0)
        );
    }

    #[test]
    fn project_account_and_output_refusals_preserve_queue_and_request_source() {
        let dir = directory();
        let db = dir.path().join("store.sqlite");
        for mut bad in [
            row("foreign", "p"),
            row("ownerless", "p"),
            row("crossed", "other"),
        ] {
            if bad["id"] == "foreign" {
                bad["owner_uid"] = json!("other");
            }
            if bad["id"] == "ownerless" {
                bad.as_object_mut().unwrap().remove("owner_uid");
            }
            assert!(
                run(
                    &db,
                    &dir.path().join("refused.json"),
                    "p",
                    Request::Admit {
                        rows: vec![bad],
                        now: 20
                    }
                )
                .is_err()
            );
            assert!(!db.exists());
        }
        let source = dir.path().join("source.json");
        private_write(
            &source,
            &serde_json::to_vec(&json!({"action":"admit","rows":[row("one","p")],"now":20}))
                .unwrap(),
        );
        let original = fs::read(&source).unwrap();
        let occupied = dir.path().join("occupied.json");
        private_write(&occupied, b"occupied");
        assert!(run(&db, &occupied, "p", request(&source).unwrap()).is_err());
        assert!(!db.exists());
        assert_eq!(fs::read(&source).unwrap(), original);
        assert_eq!(fs::read(&occupied).unwrap(), b"occupied");
        run(
            &db,
            &dir.path().join("admit.json"),
            "p",
            Request::Admit {
                rows: vec![row("one", "p")],
                now: 20,
            },
        )
        .unwrap();
        assert!(
            run(
                &db,
                &dir.path().join("cross.json"),
                "other",
                Request::Row { id: "one".into() }
            )
            .is_err()
        );
        assert!(
            run(
                &db,
                &dir.path().join("event.json"),
                "p",
                Request::Transition {
                    event: json!({"kind":"schedule","current_uid":"other","online":true}),
                    now: 100
                }
            )
            .is_err()
        );
        let out = dir.path().join("foreign-read.json");
        let mut foreign = scope();
        foreign.account = "other".into();
        capture(&db, "stable", None, "p", Request::Headers {}, &out, || {
            Ok(foreign.clone())
        })
        .unwrap();
        assert_eq!(payload(&out)["result"], json!([]));
        let mut duplicate = row("two", "p");
        duplicate["doc_id"] = json!("doc-one");
        assert!(
            run(
                &db,
                &dir.path().join("atomic.json"),
                "p",
                Request::Admit {
                    rows: vec![row("three", "p"), duplicate],
                    now: 20
                }
            )
            .is_err()
        );
        assert!(
            Store::open_read_only(&db)
                .unwrap()
                .unwrap()
                .survey_entry_row(&scope(), "three")
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn identity_rechecked_before_effect_and_failed_ack_retains_committed_rows() {
        for field in ["account", "deployment", "install_id"] {
            for change_at in [2, 3] {
                let dir = directory();
                let db = dir.path().join("store.sqlite");
                let out = dir.path().join("out.json");
                let calls = Cell::new(0);
                let error = capture(
                    &db,
                    "stable",
                    None,
                    "p",
                    Request::Admit {
                        rows: vec![row("one", "p")],
                        now: 20,
                    },
                    &out,
                    || {
                        calls.set(calls.get() + 1);
                        let mut s = scope();
                        if calls.get() >= change_at {
                            match field {
                                "account" => s.account = "changed".into(),
                                "deployment" => s.deployment = "changed".into(),
                                _ => s.install_id = "changed".into(),
                            }
                        }
                        Ok(s)
                    },
                )
                .unwrap_err();
                assert!(!out.exists());
                if change_at == 2 {
                    assert_eq!(error.code(), CHANGED.code);
                    assert!(!db.exists());
                } else {
                    assert_eq!(error.detail_value().unwrap()["mutation_completed"], true);
                    assert!(
                        Store::open_read_only(&db)
                            .unwrap()
                            .unwrap()
                            .survey_entry_row(&scope(), "one")
                            .unwrap()
                            .is_some()
                    );
                }
            }
        }
    }
    fn terminal_rows(database: &Path) {
        let mut store = Store::open(database).unwrap();
        for header in store.survey_entry_headers(&scope(), Some("p")).unwrap() {
            let mut terminal = header.clone();
            terminal.status = ds_command_kernel::survey_entry_outbox::Status::Failed;
            assert!(
                store
                    .survey_entry_cas_apply(
                        &scope(),
                        &ds_command_kernel::survey_entry_outbox::Fence::from(&header),
                        &terminal,
                        None
                    )
                    .unwrap()
            );
        }
    }
    fn prepared_expected(payload: &Value) -> Vec<SurveyEntryDiscardExpected> {
        payload["result"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                serde_json::from_value(json!({"fence":r["fence"],"row_sha256":r["row_sha256"]}))
                    .unwrap()
            })
            .collect()
    }
    #[test]
    fn discard_prepare_is_readonly_on_missing_store_and_commit_empty_is_a_noop() {
        let dir = directory();
        let db = dir.path().join("missing/store.sqlite");
        let out = dir.path().join("prepared.json");
        let prepare = Request::PrepareDiscard {
            ids: vec!["one".into()],
            now: 100,
        };
        assert!(!prepare.mutates());
        let receipt = run(&db, &out, "p", prepare).unwrap();
        assert_eq!(receipt["action"], "prepare_discard");
        assert_eq!(payload(&out)["result"], json!({"rows":[]}));
        run(
            &db,
            &dir.path().join("empty-commit.json"),
            "p",
            Request::CommitDiscard {
                expected: vec![],
                now: 100,
            },
        )
        .unwrap();
        assert!(!db.parent().unwrap().exists());
        let out = dir.path().join("discard.json");
        let error = run(
            &db,
            &out,
            "p",
            Request::Transition {
                event: json!({"kind":"discard","ids":["one"]}),
                now: 100,
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), DISCARD.code);
        assert!(error.remedy_text().unwrap().contains("prepare_discard"));
        assert!(!out.exists());
    }
    #[test]
    fn discard_prepare_retains_auditable_rows_and_exact_commit_removes_them() {
        let dir = directory();
        let db = dir.path().join("store.sqlite");
        run(
            &db,
            &dir.path().join("admitted.json"),
            "p",
            Request::Admit {
                rows: vec![row("one", "p"), row("two", "p")],
                now: 20,
            },
        )
        .unwrap();
        terminal_rows(&db);
        let out = dir.path().join("prepared.json");
        run(
            &db,
            &out,
            "p",
            Request::PrepareDiscard {
                ids: vec!["one".into(), "two".into()],
                now: 100,
            },
        )
        .unwrap();
        let prepared = payload(&out);
        let expected = prepared_expected(&prepared);
        assert_eq!(expected.len(), 2);
        let store = Store::open_read_only(&db).unwrap().unwrap();
        for entry in prepared["result"]["rows"].as_array().unwrap() {
            assert_eq!(
                entry["row"],
                store
                    .survey_entry_row(&scope(), entry["fence"]["id"].as_str().unwrap())
                    .unwrap()
                    .unwrap()
            );
            assert_eq!(
                entry["row_sha256"],
                format!(
                    "{:x}",
                    Sha256::digest(serde_json::to_vec(&entry["row"]).unwrap())
                )
            );
        }
        drop(store);
        let committed = dir.path().join("committed.json");
        run(
            &db,
            &committed,
            "p",
            Request::CommitDiscard { expected, now: 101 },
        )
        .unwrap();
        assert_eq!(
            payload(&committed)["result"],
            json!({"removed":["one","two"]})
        );
        assert!(
            Store::open_read_only(&db)
                .unwrap()
                .unwrap()
                .survey_entry_headers(&scope(), Some("p"))
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn discard_closed_batches_refuse_malformed_fields_and_bounds_before_effects() {
        let dir = directory();
        let db = dir.path().join("missing/store.sqlite");
        for ids in [
            vec!["one".into(), "one".into()],
            vec!["".into()],
            (0..501).map(|n| format!("{n}")).collect(),
        ] {
            assert!(
                run(
                    &db,
                    &dir.path().join("invalid.json"),
                    "p",
                    Request::PrepareDiscard { ids, now: 100 }
                )
                .is_err()
            );
        }
        for value in [
            json!({"action":"prepare_discard","ids":[],"now":100,"account":"other"}),
            json!({"action":"commit_discard","expected":[],"now":100,"audit_succeeded":true}),
            json!({"action":"commit_discard","expected":[{"fence":{},"row_sha256":"bad","row":{}}],"now":100}),
            json!({"action":"prepare_discard","ids":[1],"now":100}),
        ] {
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
        assert!(!db.parent().unwrap().exists());
    }
    #[test]
    fn discard_identity_change_before_commit_retains_rows_and_late_ack_reports_completed_mutation()
    {
        for change_at in [2, 3] {
            let dir = directory();
            let db = dir.path().join("store.sqlite");
            run(
                &db,
                &dir.path().join("admit.json"),
                "p",
                Request::Admit {
                    rows: vec![row("one", "p")],
                    now: 20,
                },
            )
            .unwrap();
            terminal_rows(&db);
            let prepared = dir.path().join("prepared.json");
            run(
                &db,
                &prepared,
                "p",
                Request::PrepareDiscard {
                    ids: vec!["one".into()],
                    now: 100,
                },
            )
            .unwrap();
            let expected = prepared_expected(&payload(&prepared));
            let calls = Cell::new(0);
            let error = capture(
                &db,
                "stable",
                None,
                "p",
                Request::CommitDiscard { expected, now: 101 },
                &dir.path().join("committed.json"),
                || {
                    calls.set(calls.get() + 1);
                    let mut s = scope();
                    if calls.get() >= change_at {
                        s.install_id = "changed".into();
                    }
                    Ok(s)
                },
            )
            .unwrap_err();
            let held = Store::open_read_only(&db)
                .unwrap()
                .unwrap()
                .survey_entry_row(&scope(), "one")
                .unwrap();
            if change_at == 2 {
                assert_eq!(error.code(), CHANGED.code);
                assert!(held.is_some());
            } else {
                assert_eq!(error.detail_value().unwrap()["mutation_completed"], true);
                assert!(held.is_none());
            }
        }
    }
    #[test]
    fn corrupt_store_is_not_repaired_and_command_effect_includes_writes() {
        let dir = directory();
        let db = dir.path().join("store.sqlite");
        private_write(&db, b"not SQLite");
        let out = dir.path().join("out.json");
        assert!(run(&db, &out, "p", Request::Headers {}).is_err());
        assert!(!out.exists());
        assert_eq!(fs::read(&db).unwrap(), b"not SQLite");
        assert_eq!(COMMAND.effect, Effect::LocalAuthState);
        assert_eq!(COMMAND.authority, Authority::HeadlessUser);
        assert!(COMMAND.arg("project").unwrap().required);
        assert!(COMMAND.arg("state-dir").is_none());
    }
}
