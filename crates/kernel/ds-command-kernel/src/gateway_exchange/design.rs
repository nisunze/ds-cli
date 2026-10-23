//! The design domain's backend actions, as the editor, Transformer Status and
//! Project Control use them: a project's configuration, transformer documents
//! (read, save, working drafts, digests, copy), the edit lease, rename and
//! delete, collision cleanup, and the project's combined and compounded
//! report files. Brain authorizes and stores; each [`DesignCall`] names its
//! one request, bounds its input and admits its answer.
//!
//! A [`DesignCall`] is the typed JSON a screen, a JSON window or a script
//! sends; the answer is the admitted JSON document. Report generation,
//! combined inclusion and collision detection are edge compute and have no
//! call here.

use ds_gateway_client::Operation;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[cfg(test)]
use super::Method;
use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};
use crate::transformer_status_store;

pub const INPUT_INVALID: &str = "design_input_invalid";
pub const ANSWER_INVALID: &str = "design_answer_invalid";
pub const SAVE_TOO_LARGE: &str = "design_save_too_large";

/// The complete project configuration shared by native callers and the page
/// exchange. Sheet names and their values belong to their domain owners;
/// additive configuration metadata survives independently built consumers.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ProjectConfigurationDocument {
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub sheets: Map<String, Value>,
    #[serde(flatten)]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub additional: Map<String, Value>,
}

/// A save acknowledges transport success; it does not return a configuration.
/// Brain currently returns message/records metadata; older producers returned
/// success:true. Neither metadata shape replaces the required fresh readback.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, schemars::JsonSchema)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(untagged)]
pub enum ProjectConfigurationSaveAcknowledgement {
    Updated { message: String, records: u64 },
    Legacy { success: bool },
}

pub fn admit_project_configuration_save(
    body: &Value,
) -> Result<ProjectConfigurationSaveAcknowledgement, Refusal> {
    if body
        .get("success")
        .is_some_and(|value| value.as_bool() != Some(true))
    {
        return Err(answer_invalid("configuration save acknowledgement"));
    }
    let object = data(body)
        .as_object()
        .ok_or_else(|| answer_invalid("configuration save acknowledgement"))?;
    if object
        .get("success")
        .is_some_and(|value| value.as_bool() != Some(true))
    {
        return Err(answer_invalid("configuration save acknowledgement"));
    }
    serde_json::from_value(Value::Object(object.clone()))
        .map_err(|_| answer_invalid("configuration save acknowledgement"))
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum ProjectConfigurationRead {
    Held,
    Refresh,
    /// Held-first: the held document answers at once and offline; only a
    /// project with nothing held performs the one explicit configuration read.
    Acquire,
}

impl ProjectConfigurationRead {
    pub const NAMES: &'static [&'static str] = &["held", "refresh", "acquire"];

    /// Whether `source` may describe a document this read mode returned. An
    /// observation names what produced the document (held SQLite or one
    /// fresh read), never the request mode `acquire`.
    pub fn admits_source(self, source: ProjectConfigurationRead) -> bool {
        match (self, source) {
            (_, Self::Acquire) => false,
            (Self::Acquire, _) => true,
            (read, source) => read == source,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ProjectConfigurationObservation {
    pub source: ProjectConfigurationRead,
    pub saved_at_ms: u64,
}

/// The same complete document answers native command callers and the UI.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ProjectConfigurationReply {
    pub project: String,
    pub document: ProjectConfigurationDocument,
    pub observation: ProjectConfigurationObservation,
}

/// Complete-document output travels in the declared `out` file. This receipt
/// accompanies it in command data; the path is diagnostic metadata only.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq)]
pub struct ProjectConfigurationFileReceipt {
    pub project: String,
    pub observation: ProjectConfigurationObservation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub const PROJECT_CONFIG_RECEIPT_INVALID: &str = "project_config_receipt_invalid";
const MAX_SAFE_OBSERVATION_MS: u64 = 9_007_199_254_740_991;

fn config_receipt_invalid() -> Refusal {
    Refusal::new(
        PROJECT_CONFIG_RECEIPT_INVALID,
        "The configuration owner answered another command, project or read mode, or an invalid observation.",
    )
}

fn admit_config_context(
    actual: &str,
    observation: &ProjectConfigurationObservation,
    project: &str,
    read: ProjectConfigurationRead,
) -> Result<(), Refusal> {
    project_reference(project)?;
    if actual != project
        || !read.admits_source(observation.source)
        || observation.saved_at_ms > MAX_SAFE_OBSERVATION_MS
    {
        return Err(config_receipt_invalid());
    }
    Ok(())
}

impl ProjectConfigurationReply {
    /// Shared native/browser admission. An observation must survive a JS
    /// number crossing exactly, even when its producer is a native caller.
    pub fn admit_context(
        &self,
        project: &str,
        read: ProjectConfigurationRead,
    ) -> Result<(), Refusal> {
        admit_config_context(&self.project, &self.observation, project, read)
    }

    pub fn file_receipt(
        &self,
        project: &str,
        read: ProjectConfigurationRead,
        path: String,
    ) -> Result<ProjectConfigurationFileReceipt, Refusal> {
        self.admit_context(project, read)?;
        Ok(ProjectConfigurationFileReceipt {
            project: self.project.clone(),
            observation: self.observation.clone(),
            path: Some(path),
        })
    }
}

pub fn admit_project_configuration_document(
    value: Value,
) -> Result<ProjectConfigurationDocument, Refusal> {
    serde_json::from_value(value).map_err(|_| answer_invalid("project configuration"))
}

pub fn admit_project_configuration_reply(
    project: &str,
    read: ProjectConfigurationRead,
    value: Value,
) -> Result<ProjectConfigurationReply, Refusal> {
    // Decode observation and context before the document, retaining the
    // existing distinct receipt/document refusal codes.
    let Value::Object(mut value) = value else {
        return Err(config_receipt_invalid());
    };
    let document = value
        .remove("document")
        .ok_or_else(config_receipt_invalid)?;
    let receipt: ProjectConfigurationFileReceipt =
        serde_json::from_value(Value::Object(value)).map_err(|_| config_receipt_invalid())?;
    admit_config_context(&receipt.project, &receipt.observation, project, read)?;
    let answer = ProjectConfigurationReply {
        project: receipt.project,
        observation: receipt.observation,
        document: admit_project_configuration_document(document)?,
    };
    Ok(answer)
}

#[derive(Debug, PartialEq)]
pub enum ProjectConfigurationFileFailure {
    Receipt(Refusal),
    /// Preserve a real command refusal and compatible additions verbatim.
    Command(Value),
}
impl From<Refusal> for ProjectConfigurationFileFailure {
    fn from(value: Refusal) -> Self {
        Self::Receipt(value)
    }
}

/// Decode the actual Core file reply, independently of numeric producer
/// versions. Needed fields determine compatibility; additions are harmless.
pub fn admit_project_configuration_file_reply(
    project: &str,
    read: ProjectConfigurationRead,
    value: Value,
) -> Result<Option<ProjectConfigurationReply>, ProjectConfigurationFileFailure> {
    #[derive(Deserialize)]
    struct Envelope {
        #[serde(rename = "v")]
        _version: serde_json::Number,
        command: String,
        #[serde(rename = "contract")]
        _contract: serde_json::Number,
        #[serde(flatten)]
        outcome: Outcome,
    }
    #[derive(Deserialize)]
    #[serde(tag = "status", rename_all = "snake_case")]
    enum Outcome {
        Ok {
            data: ProjectConfigurationFileReceipt,
            files: Map<String, Value>,
        },
        Error {
            error: Map<String, Value>,
        },
    }
    project_reference(project)?;
    let envelope: Envelope = serde_json::from_value(value).map_err(|_| config_receipt_invalid())?;
    if envelope.command != "design.config.read" {
        return Err(config_receipt_invalid().into());
    }
    match envelope.outcome {
        Outcome::Ok { data, mut files } => {
            admit_config_context(&data.project, &data.observation, project, read)?;
            let document = files.remove("out").ok_or_else(config_receipt_invalid)?;
            let answer = ProjectConfigurationReply {
                project: data.project,
                observation: data.observation,
                document: admit_project_configuration_document(document)?,
            };
            Ok(Some(answer))
        }
        Outcome::Error { error } => {
            if !error.get("class").is_some_and(Value::is_string)
                || !error
                    .get("code")
                    .and_then(Value::as_str)
                    .is_some_and(|v| !v.is_empty())
                || !error.get("message").is_some_and(Value::is_string)
                || !error.get("retryable").is_some_and(Value::is_boolean)
            {
                return Err(config_receipt_invalid().into());
            }
            if read == ProjectConfigurationRead::Held && error["code"] == "project_config_not_held"
            {
                return Ok(None);
            }
            Err(ProjectConfigurationFileFailure::Command(Value::Object(
                error,
            )))
        }
    }
}

/// The most transformers one read, save or digest check names.
pub const MAX_BATCH: usize = 100;
/// The most transformers one delete names.
pub const MAX_DELETE: usize = 500;
/// Brain refuses a request body above 32 MiB.
pub const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
const NAME_MAX_CHARS: usize = 200;
const TEXT_MAX_CHARS: usize = 500;
/// A batch of saved transformer documents.
const DOCUMENTS_CAP: u64 = 256 * 1024 * 1024;
const CONFIG_CAP: u64 = 32 * 1024 * 1024;
const RECEIPT_CAP: u64 = 4 * 1024 * 1024;
const READ_MS: u64 = 120_000;
const CONFIG_MS: u64 = 180_000;
const SAVE_MS: u64 = 180_000;
const BATCH_MS: u64 = 300_000;
/// A read is sent again after a transport failure the response policy calls
/// safe; a write never is.
const READ_RETRIES: u32 = 2;

/// Which projection of a saved document a read asks for: `context` is
/// geometry with scalar properties, `saved` the exact persisted layers, and
/// `process` those layers with the governed transformer point.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum ReadFields {
    Context,
    Saved,
    Process,
}

/// One edited transformer to save.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct TransformerSave {
    pub transformer: String,
    /// Layer name to GeoJSON FeatureCollection.
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub layers: Map<String, Value>,
    /// The saved version this edit started from; a moved head refuses the save.
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub base_version: Option<i64>,
    /// The cloud version the operator compared before choosing to overwrite.
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub overwrite_reviewed_version: Option<i64>,
    #[serde(default)]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(optional, type = "Record<string, unknown>")
    )]
    pub process_metadata: Option<Map<String, Value>>,
    #[serde(default)]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(optional, type = "unknown")
    )]
    pub voltage_drop: Option<Value>,
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub content_digest: Option<String>,
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub operation_id: Option<String>,
}

/// What this device holds for one transformer, compared with what is saved.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct HeldDigest {
    pub transformer: String,
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub content_digest: Option<String>,
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub base_version: Option<i64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum LeaseOp {
    Acquire,
    Heartbeat,
    /// End this session's lease; unsaved work keeps its hold.
    Release,
    /// End the lease and the hold: the unsaved work is gone.
    ReleaseHold,
}

impl LeaseOp {
    const fn wire(self) -> &'static str {
        match self {
            Self::Acquire => "acquire",
            Self::Heartbeat => "heartbeat",
            Self::Release => "release",
            Self::ReleaseHold => "release_hold",
        }
    }
}

/// Evidence that this session holds unsaved work: when it began, its content
/// digest and feature counts per layer, never features.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct UnsavedWork {
    pub since: String,
    pub local_digest: String,
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "Record<string, number>")
    )]
    pub feature_counts: Map<String, Value>,
}

/// One design action of one project.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesignCall {
    /// The project's design configuration document.
    ProjectConfig {
        project: String,
    },
    /// Replace one configuration sheet; `expected` is the sheet as it was
    /// read, and a sheet changed since refuses the save.
    SaveConfigSheet {
        project: String,
        sheet: String,
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(type = "unknown"))]
        rows: Value,
        #[serde(default)]
        #[cfg_attr(
            all(feature = "wasm", target_arch = "wasm32"),
            tsify(optional, type = "unknown")
        )]
        expected: Option<Value>,
    },
    /// The project's transformer status document.
    TransformerStatus {
        project: String,
    },
    /// Saved transformer documents, by name.
    ReadTransformers {
        project: String,
        transformers: Vec<String>,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        fields: Option<ReadFields>,
    },
    /// Save edited transformer documents.
    SaveTransformers {
        project: String,
        saves: Vec<TransformerSave>,
    },
    /// Whether held documents still match what is saved.
    DigestStatus {
        project: String,
        digests: Vec<HeldDigest>,
    },
    /// Keep the signed-in user's working draft of one transformer.
    PutDraft {
        project: String,
        transformer: String,
        #[cfg_attr(
            all(feature = "wasm", target_arch = "wasm32"),
            tsify(type = "Record<string, unknown>")
        )]
        layers: Map<String, Value>,
        local_digest: String,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        base_version: Option<i64>,
    },
    /// One user's working draft; the signed-in user's when `user` is absent.
    GetDraft {
        project: String,
        transformer: String,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        user: Option<String>,
    },
    DeleteDraft {
        project: String,
        transformer: String,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        user: Option<String>,
    },
    /// Every user's working draft of one transformer, without layers.
    ListDrafts {
        project: String,
        transformer: String,
    },
    /// The edit lease of one transformer for this session.
    Lease {
        project: String,
        transformer: String,
        op: LeaseOp,
        session: String,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        mode: Option<String>,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        unsaved: Option<UnsavedWork>,
        /// Take or end another user's lease; Brain applies it only for a
        /// caller holding the override capability.
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        take_over: Option<bool>,
    },
    Rename {
        project: String,
        transformer: String,
        to: String,
    },
    DeleteTransformers {
        project: String,
        transformers: Vec<String>,
    },
    /// Copy transformers into another project.
    CopyTransformers {
        project: String,
        target_project: String,
        transformers: Vec<String>,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        overwrite: Option<bool>,
    },
    /// Delete the duplicated sources of one collision, keeping `target`.
    CleanupCollision {
        project: String,
        collision: String,
        sources: Vec<String>,
        delete: Vec<String>,
        target: String,
        overlap: String,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        coverage_pct: Option<f64>,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        name_signal: Option<String>,
        confirm_phrase: String,
    },
    DeleteCombinedReport {
        project: String,
        prefix: String,
    },
    /// The project's compounded report archives.
    CompoundedReports {
        project: String,
        #[serde(default)]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        artifacts: Option<bool>,
    },
    DeleteCompoundedReport {
        project: String,
        stem: String,
    },
}

fn input_invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(INPUT_INVALID, message)
}

fn answer_invalid(what: &str) -> Refusal {
    Refusal::new(
        ANSWER_INVALID,
        format!("The design service answered an invalid {what}."),
    )
}

fn bounded(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && !text.chars().any(char::is_control)
}

fn name<'a>(text: &'a str, what: &str) -> Result<&'a str, Refusal> {
    let trimmed = text.trim();
    if bounded(trimmed, NAME_MAX_CHARS) {
        Ok(trimmed)
    } else {
        Err(input_invalid(format!(
            "A {what} is one name of at most {NAME_MAX_CHARS} characters."
        )))
    }
}

fn names(list: &[String], max: usize) -> Result<Vec<&str>, Refusal> {
    if list.is_empty() || list.len() > max {
        return Err(input_invalid(format!(
            "Name between 1 and {max} transformers."
        )));
    }
    list.iter().map(|item| name(item, "transformer")).collect()
}

fn text<'a>(value: &'a str, what: &str) -> Result<&'a str, Refusal> {
    let trimmed = value.trim();
    if bounded(trimmed, TEXT_MAX_CHARS) {
        Ok(trimmed)
    } else {
        Err(input_invalid(format!(
            "The {what} is required and bounded."
        )))
    }
}

/// The project id as one percent-encoded path segment.

fn post(operation: Operation, body: Value, timeout_ms: u64, cap: u64) -> Result<Request, Refusal> {
    Ok(Request::generated(operation, Some(body))?
        .timeout_ms(timeout_ms)
        .response_cap(cap))
}

fn read(request: Request) -> Plan {
    Plan {
        request,
        retries: READ_RETRIES,
    }
}

fn layers(value: &Map<String, Value>) -> Result<(), Refusal> {
    if value.is_empty() || value.values().any(|layer| !layer.is_object()) {
        return Err(input_invalid(
            "A transformer document is one or more layers, each a FeatureCollection.",
        ));
    }
    Ok(())
}

fn save_item(save: &TransformerSave) -> Result<Value, Refusal> {
    let mut item = json!({"transformer_name": name(&save.transformer, "transformer")?});
    layers(&save.layers)?;
    item["gdfs"] = Value::Object(save.layers.clone());
    if let Some(version) = save.base_version {
        item["base_version"] = json!(version);
    }
    if let Some(reviewed) = save.overwrite_reviewed_version {
        if reviewed < 1 {
            return Err(input_invalid(
                "An overwrite names the positive version it reviewed.",
            ));
        }
        item["force_version_conflict"] = json!(true);
        item["reviewed_current_version"] = json!(reviewed);
    }
    if let Some(metadata) = &save.process_metadata {
        item["process_metadata"] = Value::Object(metadata.clone());
    }
    if let Some(drop) = save.voltage_drop.as_ref().filter(|drop| !drop.is_null()) {
        item["voltage_drop"] = drop.clone();
    }
    if let Some(digest) = save
        .content_digest
        .as_deref()
        .filter(|d| !d.trim().is_empty())
    {
        item["content_digest"] = json!(text(digest, "content digest")?);
    }
    if let Some(operation) = save
        .operation_id
        .as_deref()
        .filter(|o| !o.trim().is_empty())
    {
        item["operation_id"] = json!(text(operation, "operation id")?);
    }
    Ok(item)
}

fn within_body(body: &Value) -> Result<(), Refusal> {
    let size = serde_json::to_vec(body)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    if size > MAX_BODY_BYTES {
        return Err(Refusal::new(
            SAVE_TOO_LARGE,
            "This is too large to save in one request. Save fewer transformers at once, or reduce the geometry.",
        ));
    }
    Ok(())
}

impl DesignCall {
    fn project_id(&self) -> &str {
        match self {
            Self::ProjectConfig { project }
            | Self::SaveConfigSheet { project, .. }
            | Self::TransformerStatus { project }
            | Self::ReadTransformers { project, .. }
            | Self::SaveTransformers { project, .. }
            | Self::DigestStatus { project, .. }
            | Self::PutDraft { project, .. }
            | Self::GetDraft { project, .. }
            | Self::DeleteDraft { project, .. }
            | Self::ListDrafts { project, .. }
            | Self::Lease { project, .. }
            | Self::Rename { project, .. }
            | Self::DeleteTransformers { project, .. }
            | Self::CopyTransformers { project, .. }
            | Self::CleanupCollision { project, .. }
            | Self::DeleteCombinedReport { project, .. }
            | Self::CompoundedReports { project, .. }
            | Self::DeleteCompoundedReport { project, .. } => project,
        }
    }

    fn request(&self) -> Result<Plan, Refusal> {
        let project = project_reference(self.project_id())?;
        let act = |action: &str, mut body: Value| {
            body["action"] = json!(action);
            body["eds_project_id"] = json!(project);
            body
        };
        Ok(match self {
            Self::ProjectConfig { .. } => read(
                Request::generated_parameters(
                    Operation::DsSystemGetConfig,
                    &[("eds_project_id", project)],
                    None,
                )?
                .timeout_ms(CONFIG_MS)
                .response_cap(CONFIG_CAP),
            ),
            Self::SaveConfigSheet {
                sheet,
                rows,
                expected,
                ..
            } => {
                let mut body = json!({"action":"save_config","eds_project_id":project,
                    "sheet_name":name(sheet, "configuration sheet")?,"data":rows});
                if let Some(expected) = expected {
                    body["expected_data"] = expected.clone();
                }
                within_body(&body)?;
                post(Operation::DsSystemReport, body, READ_MS, RECEIPT_CAP)?.into()
            }
            Self::TransformerStatus { .. } => read(post(
                Operation::DsSystemReport,
                transformer_status_store::request_body(project, &[]),
                transformer_status_store::READ_TIMEOUT_SECONDS * 1000,
                transformer_status_store::MAX_RESPONSE_BYTES as u64,
            )?),
            Self::ReadTransformers {
                transformers,
                fields,
                ..
            } => {
                let mut body = act(
                    "get_transformers_data",
                    json!({"transformers": names(transformers, MAX_BATCH)?}),
                );
                if let Some(fields) = fields {
                    body["fields"] = json!(fields);
                }
                read(post(Operation::DataInterop, body, READ_MS, DOCUMENTS_CAP)?)
            }
            Self::SaveTransformers { saves, .. } => {
                if saves.is_empty() || saves.len() > MAX_BATCH {
                    return Err(input_invalid(format!(
                        "Save between 1 and {MAX_BATCH} transformers at once."
                    )));
                }
                let items = saves.iter().map(save_item).collect::<Result<Vec<_>, _>>()?;
                let body = act(
                    "save_transformers_data",
                    json!({"transformer_saves": items}),
                );
                within_body(&body)?;
                post(Operation::DataInterop, body, SAVE_MS, RECEIPT_CAP)?.into()
            }
            Self::DigestStatus { digests, .. } => {
                if digests.is_empty() || digests.len() > MAX_BATCH {
                    return Err(input_invalid(format!(
                        "Compare between 1 and {MAX_BATCH} transformers at once."
                    )));
                }
                let rows = digests
                    .iter()
                    .map(|held| {
                        let mut row =
                            json!({"transformer_name": name(&held.transformer, "transformer")?});
                        if let Some(digest) = held
                            .content_digest
                            .as_deref()
                            .filter(|d| !d.trim().is_empty())
                        {
                            row["content_digest"] = json!(text(digest, "content digest")?);
                        }
                        if let Some(version) = held.base_version {
                            row["base_version"] = json!(version);
                        }
                        Ok(row)
                    })
                    .collect::<Result<Vec<_>, Refusal>>()?;
                read(post(
                    Operation::DataInterop,
                    act(
                        "transformer_digest_status",
                        json!({"transformer_digests": rows}),
                    ),
                    READ_MS,
                    RECEIPT_CAP,
                )?)
            }
            Self::PutDraft {
                transformer,
                layers: draft,
                local_digest,
                base_version,
                ..
            } => {
                layers(draft)?;
                let mut body = act(
                    "put_transformer_draft",
                    json!({"transformer_name": name(transformer, "transformer")?,
                        "gdfs": draft, "local_digest": text(local_digest, "local digest")?}),
                );
                if let Some(version) = base_version {
                    body["base_version"] = json!(version);
                }
                within_body(&body)?;
                post(Operation::DataInterop, body, SAVE_MS, RECEIPT_CAP)?.into()
            }
            Self::GetDraft {
                transformer, user, ..
            }
            | Self::DeleteDraft {
                transformer, user, ..
            } => {
                let action = if matches!(self, Self::GetDraft { .. }) {
                    "get_transformer_draft"
                } else {
                    "delete_transformer_draft"
                };
                let mut body = act(
                    action,
                    json!({"transformer_name": name(transformer, "transformer")?}),
                );
                if let Some(user) = user.as_deref().filter(|u| !u.trim().is_empty()) {
                    body["draft_user"] = json!(text(user, "draft owner")?);
                }
                let request = post(Operation::DataInterop, body, READ_MS, DOCUMENTS_CAP)?;
                if action == "get_transformer_draft" {
                    read(request)
                } else {
                    request.into()
                }
            }
            Self::ListDrafts { transformer, .. } => read(post(
                Operation::DataInterop,
                act(
                    "list_transformer_drafts",
                    json!({"transformer_name": name(transformer, "transformer")?}),
                ),
                READ_MS,
                RECEIPT_CAP,
            )?),
            Self::Lease {
                transformer,
                op,
                session,
                mode,
                unsaved,
                take_over,
                ..
            } => {
                let mut body = act(
                    "lock_transformer",
                    json!({"transfo": name(transformer, "transformer")?, "lock_op": op.wire(),
                        "session_id": text(session, "edit session")?}),
                );
                if let Some(mode) = mode.as_deref().filter(|m| !m.trim().is_empty()) {
                    body["edit_mode"] = json!(text(mode, "edit mode")?);
                }
                if let Some(work) = unsaved {
                    body["lock_hold"] = json!(true);
                    body["unsaved_since"] = json!(text(&work.since, "unsaved-since time")?);
                    body["local_digest"] = json!(text(&work.local_digest, "local digest")?);
                    if work
                        .feature_counts
                        .values()
                        .any(|count| count.as_i64().is_none_or(|n| n < 0))
                    {
                        return Err(input_invalid("Feature counts are whole numbers."));
                    }
                    body["feature_counts"] = Value::Object(work.feature_counts.clone());
                }
                if *take_over == Some(true) {
                    body["lock_override"] = json!(true);
                }
                post(Operation::DsSystemReport, body, READ_MS, RECEIPT_CAP)?.into()
            }
            Self::Rename {
                transformer, to, ..
            } => {
                let (from, to) = (name(transformer, "transformer")?, name(to, "new name")?);
                post(
                    Operation::DsSystemReport,
                    act(
                        "rename_transformer",
                        json!({"transfo": from, "new_transformer_name": to,
                            "governance_reason": format!("Rename transformer {from} to {to}"),
                            "governance_acknowledged": true,
                            "governance_confirm_phrase": "RENAME TRANSFORMER"}),
                    ),
                    BATCH_MS,
                    RECEIPT_CAP,
                )?
                .into()
            }
            Self::DeleteTransformers { transformers, .. } => post(
                Operation::DsSystemReport,
                act(
                    "delete_transformer",
                    json!({"transformers": names(transformers, MAX_DELETE)?}),
                ),
                BATCH_MS,
                RECEIPT_CAP,
            )?
            .into(),
            Self::CopyTransformers {
                target_project,
                transformers,
                overwrite,
                ..
            } => post(
                Operation::DataInterop,
                act(
                    "copy_transformers",
                    json!({"target_project_id": project_reference(target_project)?,
                        "transformers": names(transformers, MAX_DELETE)?,
                        "overwrite_existing": overwrite.unwrap_or(false)}),
                ),
                BATCH_MS,
                RECEIPT_CAP,
            )?
            .into(),
            Self::CleanupCollision {
                collision,
                sources,
                delete,
                target,
                overlap,
                coverage_pct,
                name_signal,
                confirm_phrase,
                ..
            } => {
                let collision = text(collision, "collision id")?;
                let coverage = coverage_pct.unwrap_or(0.0);
                if !(0.0..=100.0).contains(&coverage) {
                    return Err(input_invalid("Coverage is a percentage."));
                }
                post(
                    Operation::DsSystemReport,
                    act(
                        "cleanup_collision_sources",
                        json!({"collision_id": collision,
                            "source_transformers": names(sources, MAX_DELETE)?,
                            "delete_transformers": names(delete, MAX_DELETE)?,
                            "target_transformer": name(target, "transformer")?,
                            "overlap_class": text(overlap, "overlap class")?,
                            "coverage_pct": coverage.round() as i64,
                            "name_signal": name_signal.as_deref().unwrap_or("").trim(),
                            "governance_reason": format!("Collision cleanup {collision}"),
                            "governance_acknowledged": true,
                            "governance_confirm_phrase": confirm_phrase.trim()}),
                    ),
                    BATCH_MS,
                    RECEIPT_CAP,
                )?
                .into()
            }
            Self::DeleteCombinedReport { prefix, .. } => post(
                Operation::DsSystemReport,
                act(
                    "delete_combined_report",
                    json!({"prefix": text(prefix, "combined report")?}),
                ),
                READ_MS,
                RECEIPT_CAP,
            )?
            .into(),
            Self::CompoundedReports { artifacts, .. } => {
                let mut body = act("list_compounded_reports", json!({}));
                if *artifacts == Some(true) {
                    body["include_artifacts"] = json!(true);
                }
                read(post(Operation::DsSystemReport, body, READ_MS, CONFIG_CAP)?)
            }
            Self::DeleteCompoundedReport { stem, .. } => post(
                Operation::DsSystemReport,
                act(
                    "delete_compounded_report",
                    json!({"stem": text(stem, "compounded report")?}),
                ),
                READ_MS,
                RECEIPT_CAP,
            )?
            .into(),
        })
    }

    fn admitted(&self, body: Value) -> Result<Value, Refusal> {
        let object = |value: &Value, what: &str| {
            value
                .as_object()
                .cloned()
                .ok_or_else(|| answer_invalid(what))
        };
        Ok(match self {
            Self::TransformerStatus { .. } => transformer_status_store::admit_response(body)
                .map_err(|_| answer_invalid("transformer status"))?,
            Self::ReadTransformers { .. } => {
                let answer = object(data(&body), "transformer read")?;
                rows(&answer, "results", "transformer read", |row| {
                    let ok = row.get("ok").and_then(Value::as_bool).ok_or(())?;
                    let layers = row.get("data").and_then(|data| data.get("layers"));
                    if ok && !layers.is_some_and(Value::is_object) {
                        return Err(());
                    }
                    Ok(())
                })?;
                Value::Object(answer)
            }
            Self::SaveTransformers { .. } => {
                let answer = object(data(&body), "transformer save")?;
                rows(&answer, "results", "transformer save", |row| {
                    row.get("ok").and_then(Value::as_bool).map(drop).ok_or(())
                })?;
                Value::Object(answer)
            }
            Self::DigestStatus { .. } => {
                let answer = object(data(&body), "digest status")?;
                rows(&answer, "items", "digest status", |row| {
                    row.get("status")
                        .and_then(Value::as_str)
                        .map(drop)
                        .ok_or(())
                })?;
                Value::Object(answer)
            }
            Self::Lease { op, .. } => {
                let answer = object(&body, "edit lease")?;
                let key = if matches!(op, LeaseOp::Acquire | LeaseOp::Heartbeat) {
                    "locked"
                } else {
                    "released"
                };
                if answer.get(key).and_then(Value::as_bool).is_none() {
                    return Err(answer_invalid("edit lease"));
                }
                Value::Object(answer)
            }
            Self::Rename { .. } => {
                let answer = object(&body, "rename")?;
                if !answer
                    .get("new_name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| !n.is_empty())
                {
                    return Err(answer_invalid("rename"));
                }
                Value::Object(answer)
            }
            Self::DeleteTransformers { .. } => {
                let answer = object(&body, "delete")?;
                rows(&answer, "results", "delete", |row| {
                    row.get("name").and_then(Value::as_str).map(drop).ok_or(())
                })?;
                Value::Object(answer)
            }
            Self::CompoundedReports { .. } => {
                let mut answer = object(&body, "compounded report list")?;
                match answer.get("compounded_reports") {
                    None | Some(Value::Null) => {
                        answer.insert("compounded_reports".into(), json!([]));
                    }
                    Some(Value::Array(_)) => {}
                    Some(_) => return Err(answer_invalid("compounded report list")),
                }
                Value::Object(answer)
            }
            Self::ProjectConfig { .. } => {
                serde_json::to_value(admit_project_configuration_document(body)?)
                    .map_err(|_| answer_invalid("project configuration"))?
            }
            Self::SaveConfigSheet { .. } => {
                admit_project_configuration_save(&body)?;
                Value::Object(object(data(&body), "configuration save acknowledgement")?)
            }
            Self::PutDraft { .. }
            | Self::GetDraft { .. }
            | Self::DeleteDraft { .. }
            | Self::ListDrafts { .. }
            | Self::CopyTransformers { .. }
            | Self::CleanupCollision { .. }
            | Self::DeleteCombinedReport { .. }
            | Self::DeleteCompoundedReport { .. } => Value::Object(object(data(&body), "answer")?),
        })
    }
}

/// Every row of `key` is an object naming its transformer and passing `row`.
fn rows(
    answer: &Map<String, Value>,
    key: &str,
    what: &str,
    row: impl Fn(&Map<String, Value>) -> Result<(), ()>,
) -> Result<(), Refusal> {
    let list = answer
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| answer_invalid(what))?;
    for item in list {
        let item = item.as_object().ok_or_else(|| answer_invalid(what))?;
        let named = item
            .get("transformer_name")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .is_some();
        if !named || row(item).is_err() {
            return Err(answer_invalid(what));
        }
    }
    Ok(())
}

impl Exchange for DesignCall {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        self.request()
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        self.admitted(body).map(Admitted::Done)
    }

    fn project(&self) -> Option<&str> {
        Some(self.project_id())
    }
}

/// The typed browser export Svelte and JSON windows call; it runs the shared
/// executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{DesignCall, ProjectConfigurationFileFailure, ProjectConfigurationRead};
    use crate::gateway_exchange::browser::{rejection, run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// Admit complete Core output with the same Rust DTO as the native
    /// command and gateway exchange. Additive configuration fields survive.
    #[wasm_bindgen(js_name = projectConfigDocumentAdmit, unchecked_return_type = "ProjectConfigurationDocument")]
    pub fn project_config_document_admit(value: JsValue) -> Result<JsValue, JsValue> {
        // Preserve JSON arrays before admitting the DTO. Deserializing a JS
        // array directly into a Map accepts [] as an empty object, unlike
        // the native JSON decoder used by the command and gateway exchange.
        let json: serde_json::Value = tsify::serde_wasm_bindgen::from_value(value)
            .map_err(|_| rejection(&super::answer_invalid("project configuration")))?;
        let document =
            super::admit_project_configuration_document(json).map_err(|error| rejection(&error))?;
        typed(&document)
    }

    #[wasm_bindgen(js_name = projectConfigReplyAdmit, unchecked_return_type = "ProjectConfigurationReply")]
    pub fn project_config_reply_admit(
        project: &str,
        #[wasm_bindgen(unchecked_param_type = "ProjectConfigurationRead")] read: JsValue,
        value: JsValue,
    ) -> Result<JsValue, JsValue> {
        let read: ProjectConfigurationRead =
            tsify::serde_wasm_bindgen::from_value(read).map_err(|_| {
                rejection(&super::input_invalid(
                    "Invalid project configuration read mode.",
                ))
            })?;
        let value = tsify::serde_wasm_bindgen::from_value(value)
            .map_err(|_| rejection(&super::config_receipt_invalid()))?;
        typed(
            &super::admit_project_configuration_reply(project, read, value)
                .map_err(|error| rejection(&error))?,
        )
    }

    #[wasm_bindgen(js_name = projectConfigFileReplyAdmit, unchecked_return_type = "ProjectConfigurationReply | null")]
    pub fn project_config_file_reply_admit(
        project: &str,
        #[wasm_bindgen(unchecked_param_type = "ProjectConfigurationRead")] read: JsValue,
        value: JsValue,
    ) -> Result<JsValue, JsValue> {
        let read: ProjectConfigurationRead =
            tsify::serde_wasm_bindgen::from_value(read).map_err(|_| {
                rejection(&super::input_invalid(
                    "Invalid project configuration read mode.",
                ))
            })?;
        let value = tsify::serde_wasm_bindgen::from_value(value)
            .map_err(|_| rejection(&super::config_receipt_invalid()))?;
        match super::admit_project_configuration_file_reply(project, read, value) {
            Ok(Some(answer)) => typed(&answer),
            Ok(None) => Ok(JsValue::NULL),
            Err(ProjectConfigurationFileFailure::Receipt(error)) => Err(rejection(&error)),
            Err(ProjectConfigurationFileFailure::Command(error)) => Err(typed(&error)?),
        }
    }

    /// One design action; the answer is its admitted JSON document.
    #[wasm_bindgen(js_name = designCall, unchecked_return_type = "any")]
    pub async fn design_call(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "DesignCall")] call: JsValue,
    ) -> Result<JsValue, JsValue> {
        let call: DesignCall = tsify::serde_wasm_bindgen::from_value(call)
            .map_err(|error| js_sys::Error::new(&format!("Invalid design call: {error}")))?;
        typed(&run(&host, call).await?)
    }
}

#[cfg(test)]
#[path = "design_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "design_config_document_tests.rs"]
mod config_document_tests;

#[cfg(test)]
#[path = "design_config_save_tests.rs"]
mod config_save_tests;
