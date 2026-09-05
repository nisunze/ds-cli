//! Thin filesystem and native-auth host for the offline Survey owner.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_survey_store::Workspace;
use serde_json::{Value, json};
use std::{io::Read, path::Path};
const WORKSPACE: Arg = Arg::value(
    "workspace",
    "<directory>",
    "Native SQLite Survey workspace.",
)
.required();
const STORE: Refusal = Refusal {
    code: "survey_workspace_refused",
    when: "the offline workspace, schema, data, lock or identity binding is invalid",
    remedy: "inspect the exact workspace and cached form; keep pending entries intact",
};
const LOCAL_REFUSALS: &[Refusal] = &[
    STORE,
    crate::INVALID_DOCUMENT,
    crate::INVALID_TEXT,
    ds_cli_desktop::ops::INVALID_NUMBER,
];
const NATIVE_REFUSALS: &[Refusal] = &{
    const BASE: &[Refusal] = crate::create::REFUSALS;
    let mut list = [STORE; BASE.len() + 4];
    let mut i = 0;
    while i < BASE.len() {
        list[i] = BASE[i];
        i += 1;
    }
    list[BASE.len() + 1] = crate::INVALID_DOCUMENT;
    list[BASE.len() + 2] = crate::INVALID_TEXT;
    list[BASE.len() + 3] = ds_cli_desktop::ops::INVALID_NUMBER;
    list
};
macro_rules! command {
 ($name:ident,$id:literal,$leaf:literal,$summary:literal,$purpose:literal,$effect:expr,$auth:expr,$args:expr,$refusals:expr,$availability:expr)=>{
 pub static $name:Command=Command{id:$id,path:&["survey","workspace",$leaf],contract:1,summary:$summary,purpose:$purpose,chapter:Chapter::Survey,effect:$effect,authority:$auth,execution:Execution::Sync,args:$args,output:"Bounded workspace and entry receipts. Pending means durably local; committed means Firestore acknowledged, with mirror visibility unconfirmed.",examples:&[],refusals:$refusals,reference:Some("docs/reference/survey.md"),availability:$availability};
 };
}
fn available() -> ds_cli_contract::spec::Availability {
    ds_cli_contract::spec::Availability::Available
}
command!(
    INIT,
    "survey.workspace.init",
    "init",
    "Initialize offline Survey capture from a resolved snapshot.",
    "Creates a new workspace from a local resolved project/forms JSON snapshot. The snapshot guides offline validation and does not prove permission to publish. No sign-in, desktop or network is required.",
    Effect::LocalFileWrite,
    Authority::None,
    &[
        WORKSPACE,
        Arg::value(
            "snapshot",
            "<json-file>",
            "Resolved project_id and full forms, including entry schemas."
        )
        .required()
    ],
    LOCAL_REFUSALS,
    available
);
command!(
    PREPARE,
    "survey.workspace.prepare",
    "prepare",
    "Cache selected project forms for offline Survey capture.",
    "Fetches one complete selected-project form resolution through the native client, then initializes a new durable local workspace. Subsequent collect and list operations require no network.",
    Effect::LocalFileWrite,
    Authority::HeadlessProject,
    &[WORKSPACE, crate::LANE],
    NATIVE_REFUSALS,
    ds_cli_auth::native_availability
);
command!(
    COLLECT,
    "survey.workspace.collect",
    "collect",
    "Collect one validated Survey entry entirely offline.",
    "Validates data using the cached form and shared Rust kernel, then commits one entry and stable replay identity in a SQLite transaction. Unknown or hidden fields are refused instead of discarded. No network is attempted; sync is a separate confirmed action.",
    Effect::LocalFileWrite,
    Authority::None,
    &[
        WORKSPACE,
        Arg::value("form", "<slug>", "Enabled form from the cached snapshot.").required(),
        Arg::value(
            "document",
            "<json-file>",
            "Canonical data and optional geometry/connectivity/location buckets."
        )
        .required(),
        Arg::value(
            "created-at",
            "<rfc3339>",
            "Device capture time, retained through migration."
        )
        .required(),
        Arg::value(
            "doc-id",
            "<id>",
            "Optional stable source document id; a UUID is generated otherwise."
        )
    ],
    LOCAL_REFUSALS,
    available
);
command!(
    LIST,
    "survey.workspace.list",
    "list",
    "Inspect durable offline Survey entries and pending counts.",
    "Opens one local workspace and returns a bounded ordered entry inventory without field values, coordinates or replay keys. It never contacts the backend.",
    Effect::LocalFileWrite,
    Authority::None,
    &[
        WORKSPACE,
        Arg::value("limit", "<n>", "Return 1..500 entry summaries.").default("50")
    ],
    LOCAL_REFUSALS,
    available
);
command!(
    SYNC,
    "survey.workspace.sync",
    "sync",
    "Publish a bounded offline Survey batch with stable replay keys.",
    "Requires --yes. Binds the workspace to the restored principal, audience, lane and selected project before sending any row. Sends sequentially through the governed create API, persists each verified acknowledgement, and stops on any refusal. An ambiguous outcome stays pending for an exact idempotent retry; no automatic retry or data deletion occurs.",
    Effect::GlobalWrite,
    Authority::HeadlessProject,
    &[
        WORKSPACE,
        crate::LANE,
        Arg::value("limit", "<n>", "Publish at most 1..100 pending entries.").default("10")
    ],
    NATIVE_REFUSALS,
    ds_cli_auth::native_availability
);
fn refusal(error: impl std::fmt::Display) -> Failure {
    Failure::invalid("survey_workspace_refused", error.to_string()).remedy(STORE.remedy)
}
fn read_json(path: &str, max: u64) -> Result<Value, Failure> {
    let path = Path::new(path);
    let meta = std::fs::symlink_metadata(path).map_err(refusal)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > max {
        return Err(refusal("Input must be an ordinary bounded JSON file"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(refusal)?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(refusal)?;
    if bytes.len() as u64 > max {
        return Err(refusal("Input grew beyond its bound"));
    }
    serde_json::from_slice(&bytes).map_err(refusal)
}
pub fn init(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let snapshot = read_json(inputs.require("snapshot")?, 8 * 1024 * 1024)?;
    Workspace::initialize(Path::new(inputs.require("workspace")?), &snapshot).map_err(refusal)
}
pub fn prepare(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = Path::new(inputs.require("workspace")?);
    if path.exists() {
        return Err(refusal("Workspace already exists"));
    }
    let snapshot = ds_cli_auth::survey_workspace_snapshot(inputs.require("lane")?)?;
    Workspace::initialize(path, &snapshot).map_err(refusal)
}
pub fn collect(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let document = read_json(inputs.require("document")?, 900 * 1024)?;
    let ws = Workspace::open(Path::new(inputs.require("workspace")?)).map_err(refusal)?;
    ws.collect(
        inputs.require("form")?,
        &document,
        inputs.require("created-at")?,
        inputs.value("doc-id"),
    )
    .map_err(refusal)
}
pub fn list(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = ds_cli_desktop::ops::integer(inputs.require("limit")?, "limit", 1, 500)? as usize;
    Workspace::open(Path::new(inputs.require("workspace")?))
        .map_err(refusal)?
        .summary(limit)
        .map_err(refusal)
}
pub fn sync(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = ds_cli_desktop::ops::integer(inputs.require("limit")?, "limit", 1, 100)? as usize;
    let ws = Workspace::open(Path::new(inputs.require("workspace")?)).map_err(refusal)?;
    let pending = ws.pending(limit).map_err(refusal)?;
    if pending.is_empty() {
        return ws.summary(20).map_err(refusal);
    }
    let mut session = ds_cli_auth::survey_import_session(inputs.require("lane")?)?;
    ws.bind(
        session.project_id(),
        session.lane(),
        session.principal_sha256(),
        session.credential_audience_sha256(),
    )
    .map_err(refusal)?;
    let mut committed = 0;
    for item in pending {
        let receipt = session.create(&item.request)?;
        ws.acknowledge(&item, &receipt).map_err(refusal)?;
        committed += 1;
    }
    let mut result = ws.summary(20).map_err(refusal)?;
    result["committed_this_run"] = json!(committed);
    result["bigquery_mirror"] = json!("unconfirmed");
    Ok(result)
}
pub fn render(data: &Value) -> String {
    if data["doc_id"].is_string() {
        format!("{} · pending locally\n", data["doc_id"].as_str().unwrap())
    } else {
        format!(
            "{} · {} entries · {} pending\n",
            data["project"].as_str().unwrap_or("workspace"),
            data["total"],
            data["pending"]
        )
    }
}
