//! DS Grid project models: a project's model heads, their immutable
//! versions and deliberate exports, the content-addressed upload of a
//! model's bytes, a new version at the head the caller saw, and the
//! retirement of a head. Brain answers lists page by page; a listing follows
//! the pages to the end.

use ds_gateway_client::Operation;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Map, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

const PAGE_SIZE: u64 = 500;
/// A cold Desktop session prepares its route and credentials before a read.
const READ_TIMEOUT_MS: u64 = 120_000;
/// Pages one listing follows; a project holding more is refused, not cut.
const MAX_PAGES: usize = 200;
pub const INVALID: &str = "grid_models_answer_invalid";
pub const INPUT_INVALID: &str = "grid_models_input_invalid";

/// Go writes an empty list as `null`.
fn list<'de, D: Deserializer<'de>, T: Deserialize<'de>>(input: D) -> Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(input)?.unwrap_or_default())
}

/// Go writes an unset time as its zero value; that reads as absent.
fn stamp<'de, D: Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(input)?
        .filter(|stamp| !stamp.is_empty() && !stamp.starts_with("0001-01-01")))
}

/// A digest as Brain keys it: bare lowercase hex.
fn bare(digest: &str) -> String {
    digest
        .trim()
        .trim_start_matches("sha256:")
        .to_ascii_lowercase()
}

/// One content-addressed object.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridObjectRef {
    /// Bare 64-character lowercase hex sha256 of the exact bytes.
    pub digest: String,
    pub object: String,
    pub byte_length: u64,
}

/// One project model head.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridModelSummary {
    pub model_id: String,
    pub display_name: String,
    pub head_revision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_model_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_version_revision_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'general' | 'lv_network' | 'mv_line'")
    )]
    pub model_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_model_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_stage_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_level_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'temporary' | 'promoted'")
    )]
    pub lifecycle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'active' | 'deleted'")
    )]
    pub state: Option<String>,
    #[serde(
        default,
        deserialize_with = "stamp",
        skip_serializing_if = "Option::is_none"
    )]
    pub updated_at: Option<String>,
    #[serde(
        default,
        deserialize_with = "stamp",
        skip_serializing_if = "Option::is_none"
    )]
    pub deleted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_reason: Option<String>,
}

/// The approval a revision carries.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridApprovalSnapshot {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_by: Option<String>,
    #[serde(
        default,
        deserialize_with = "stamp",
        skip_serializing_if = "Option::is_none"
    )]
    pub decision_at: Option<String>,
}

/// The validation a version carries.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridValidationSummary {
    pub state: String,
    pub issue_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_digest: Option<String>,
}

/// One model a composed version was made from.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridCompositionSource {
    pub sequence: i64,
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'temporary_model' | 'project_model'")
    )]
    pub source_kind: String,
    pub model_id: String,
    pub model_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<String>,
    /// A label the page shows; never sent to Brain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// One immutable model version.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridModelVersion {
    pub model_id: String,
    pub revision_id: String,
    pub version: i64,
    pub model: GridObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_ordinal_within_version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_model_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'general' | 'lv_network' | 'mv_line'")
    )]
    pub model_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'temporary' | 'promoted'")
    )]
    pub lifecycle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_revision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_stage_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_level_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<GridApprovalSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<GridValidationSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_sources: Option<Vec<GridCompositionSource>>,
    /// Who created the version.
    #[serde(default, alias = "created_by", skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(
        default,
        deserialize_with = "stamp",
        skip_serializing_if = "Option::is_none"
    )]
    pub created_at: Option<String>,
}

/// One version resolved for opening: a short-lived signed read of its bytes.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ResolvedGridVersion {
    pub version: GridModelVersion,
    pub download_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// One output of a deliberate export.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridExportItem {
    pub output_id: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_id: Option<String>,
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<GridObjectRef>,
}

/// One immutable deliberate export manifest.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridExportRecord {
    pub export_id: String,
    pub revision_id: String,
    pub model_digest: String,
    #[serde(deserialize_with = "list")]
    pub items: Vec<GridExportItem>,
    #[serde(
        default,
        deserialize_with = "stamp",
        skip_serializing_if = "Option::is_none"
    )]
    pub created_at: Option<String>,
}

/// The session for one content-addressed object, or none when it is stored.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridUploadStarted {
    /// The resumable session to send the bytes to; empty when stored.
    pub upload_url: String,
    pub object: String,
    pub digest: String,
    pub byte_length: u64,
    /// The object already holds these exact bytes; nothing is sent.
    pub already_stored: bool,
}

/// A new version's declaration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridVersionInput {
    pub manifest_model_id: String,
    pub manifest_model_revision: i64,
    pub ds_project: String,
    pub model_id: String,
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'general' | 'lv_network' | 'mv_line'")
    )]
    pub model_kind: String,
    /// Client-minted immutable revision id; an identical retry is idempotent.
    pub revision_id: String,
    /// The head the caller saw; empty for a first version.
    pub expected_head_revision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bump_version: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub model: GridObjectRef,
    pub model_schema_version: String,
    pub engine_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_stage_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_level_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<GridApprovalInput>,
    pub validation: GridValidationSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_source: Option<GridMigrationSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_sources: Option<Vec<GridCompositionSource>>,
}

/// A review classification a new version is created with.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridApprovalInput {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
}

/// One-time migration evidence.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridMigrationSource {
    pub kind: String,
    pub reference: String,
    pub source_digest: String,
    pub manifest_id: String,
}

/// What retiring a head did; revisions and bytes are retained.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GridModelDeletion {
    pub project_id: String,
    pub model_id: String,
    pub head_revision_id: String,
    pub head_model_digest: String,
    pub deleted_at: String,
    pub revisions_retained: bool,
    pub model_object_retained: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>[]")
    )]
    pub tile_invalidation: Option<Vec<Map<String, Value>>>,
}

fn invalid(what: &str) -> Refusal {
    Refusal::new(
        INVALID,
        format!("The DS Grid models service answered an invalid {what}."),
    )
}

fn post(action: &str, project: &str, mut body: Value) -> Result<Request, Refusal> {
    body["action"] = action.into();
    body["project_id"] = project.into();
    Request::generated(Operation::ProjectGridModelsAction, Some(body))
}

fn read<T: for<'de> Deserialize<'de>>(value: Option<&Value>, what: &str) -> Result<T, Refusal> {
    value
        .and_then(|value| T::deserialize(value).ok())
        .ok_or_else(|| invalid(what))
}

/// A project's model heads, every page; newest change first.
#[derive(Clone, Debug, PartialEq)]
pub struct GridModelsList {
    pub project: String,
    pub include_deleted: bool,
    cursor: Option<String>,
    pages: usize,
    listed: Vec<GridModelSummary>,
}

impl GridModelsList {
    pub fn new(project: String, include_deleted: bool) -> Self {
        Self {
            project,
            include_deleted,
            cursor: None,
            pages: 0,
            listed: Vec::new(),
        }
    }
}

/// Where one page leaves a listing: the next cursor, or the end.
fn next_page(
    page: &Value,
    cursor: &Option<String>,
    pages: usize,
) -> Result<Option<String>, Refusal> {
    if page.get("has_more").and_then(Value::as_bool) != Some(true) {
        return Ok(None);
    }
    let next = page
        .get("next_cursor")
        .and_then(Value::as_str)
        .filter(|next| !next.is_empty() && Some(*next) != cursor.as_deref())
        .ok_or_else(|| invalid("page cursor"))?;
    if pages >= MAX_PAGES {
        return Err(Refusal::new(
            INVALID,
            "The DS Grid listing is larger than this client follows.",
        ));
    }
    Ok(Some(next.to_owned()))
}

impl Exchange for GridModelsList {
    type Output = Vec<GridModelSummary>;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = json!({"page_size": PAGE_SIZE});
        if self.include_deleted {
            body["include_deleted"] = true.into();
        }
        if let Some(cursor) = &self.cursor {
            body["cursor"] = cursor.clone().into();
        }
        Ok(post("list_models", &self.project, body)?
            .timeout_ms(READ_TIMEOUT_MS)
            .into())
    }

    fn admit(mut self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let page = data(&body);
        let models: Vec<GridModelSummary> = read(page.get("models"), "model list")?;
        self.listed.extend(models);
        self.pages += 1;
        match next_page(page, &self.cursor, self.pages)? {
            Some(cursor) => {
                self.cursor = Some(cursor);
                Ok(Admitted::Next(self))
            }
            None => {
                let mut models = self.listed;
                models.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
                Ok(Admitted::Done(models))
            }
        }
    }
}

/// One model head.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridModelGet {
    pub project: String,
    pub model: String,
}

impl Exchange for GridModelGet {
    type Output = GridModelSummary;

    fn plan(&self) -> Result<Plan, Refusal> {
        let request = post("get_model", &self.project, json!({"model_id": self.model}))?;
        Ok(request.timeout_ms(READ_TIMEOUT_MS).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        read(data(&body).get("model"), "model").map(Admitted::Done)
    }
}

/// A model's immutable versions, every page; highest version first.
#[derive(Clone, Debug, PartialEq)]
pub struct GridVersionsList {
    pub project: String,
    pub model: String,
    cursor: Option<String>,
    pages: usize,
    listed: Vec<GridModelVersion>,
}

impl GridVersionsList {
    pub fn new(project: String, model: String) -> Self {
        Self {
            project,
            model,
            cursor: None,
            pages: 0,
            listed: Vec::new(),
        }
    }
}

impl Exchange for GridVersionsList {
    type Output = Vec<GridModelVersion>;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = json!({"model_id": self.model, "page_size": PAGE_SIZE});
        if let Some(cursor) = &self.cursor {
            body["cursor"] = cursor.clone().into();
        }
        Ok(post("list_versions", &self.project, body)?
            .timeout_ms(READ_TIMEOUT_MS)
            .into())
    }

    fn admit(mut self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let page = data(&body);
        let versions: Vec<GridModelVersion> = read(page.get("versions"), "version list")?;
        self.listed.extend(versions);
        self.pages += 1;
        match next_page(page, &self.cursor, self.pages)? {
            Some(cursor) => {
                self.cursor = Some(cursor);
                Ok(Admitted::Next(self))
            }
            None => {
                let mut versions = self.listed;
                versions.sort_by(|left, right| right.version.cmp(&left.version));
                Ok(Admitted::Done(versions))
            }
        }
    }
}

/// One exact version with a short-lived signed read of its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridVersionResolve {
    pub project: String,
    pub model: String,
    pub revision: String,
}

impl Exchange for GridVersionResolve {
    type Output = ResolvedGridVersion;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"model_id": self.model, "revision_id": self.revision});
        Ok(post("get_version", &self.project, body)?
            .timeout_ms(READ_TIMEOUT_MS)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let resolved = data(&body);
        let version: GridModelVersion = read(resolved.get("revision"), "version")?;
        let download_url = resolved
            .get("download_url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .ok_or_else(|| invalid("version download"))?;
        if version.revision_id != self.revision || version.model_id != self.model {
            return Err(invalid("version identity"));
        }
        Ok(Admitted::Done(ResolvedGridVersion {
            version,
            download_url: download_url.into(),
            expires_at: resolved
                .get("expires_at")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }))
    }
}

/// A revision's deliberate export manifests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridExportsList {
    pub project: String,
    pub model: String,
    pub revision: String,
}

impl Exchange for GridExportsList {
    type Output = Vec<GridExportRecord>;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"model_id": self.model, "revision_id": self.revision});
        Ok(post("list_exports", &self.project, body)?
            .timeout_ms(READ_TIMEOUT_MS)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        match data(&body).get("exports") {
            None | Some(Value::Null) => Ok(Admitted::Done(Vec::new())),
            exports => read(exports, "export list").map(Admitted::Done),
        }
    }
}

/// Open a session for one content-addressed object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridUploadStart {
    pub project: String,
    pub model: String,
    pub digest: String,
    pub byte_length: u64,
    /// `export` for a selected GIS or model output; none for the model.
    pub purpose: Option<String>,
    pub content_type: Option<String>,
}

impl Exchange for GridUploadStart {
    type Output = GridUploadStarted;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut upload = json!({"model_id": self.model, "digest": bare(&self.digest),
            "byte_length": self.byte_length});
        if let Some(purpose) = &self.purpose {
            upload["purpose"] = purpose.clone().into();
        }
        if let Some(content_type) = &self.content_type {
            upload["content_type"] = content_type.clone().into();
        }
        Ok(post("start_upload", &self.project, json!({"upload": upload}))?.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let started = data(&body);
        let artifact: GridObjectRef = read(started.get("artifact"), "upload artifact")?;
        let already_stored = started.get("already_exists").and_then(Value::as_bool) == Some(true);
        let upload_url = started
            .get("session_uri")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if upload_url.is_empty() && !already_stored {
            return Err(invalid("upload session"));
        }
        if artifact.digest != bare(&self.digest) {
            return Err(invalid("upload digest"));
        }
        Ok(Admitted::Done(GridUploadStarted {
            upload_url,
            object: artifact.object,
            digest: artifact.digest,
            byte_length: artifact.byte_length,
            already_stored,
        }))
    }
}

/// Create one immutable version at the head the caller saw; a moved head
/// refuses with a conflict the caller surfaces, never retries past.
#[derive(Clone, Debug, PartialEq)]
pub struct GridVersionCreate(pub GridVersionInput);

impl Exchange for GridVersionCreate {
    type Output = GridModelVersion;

    fn plan(&self) -> Result<Plan, Refusal> {
        let input = &self.0;
        if !["general", "lv_network", "mv_line"].contains(&input.model_kind.as_str()) {
            return Err(Refusal::new(
                INPUT_INVALID,
                "A canonical model kind is required.",
            ));
        }
        if input.model_schema_version.trim().is_empty() || input.engine_version.trim().is_empty() {
            return Err(Refusal::new(
                INPUT_INVALID,
                "Model schema and engine versions are required to create a version.",
            ));
        }
        if input.validation.state != "valid" {
            return Err(Refusal::new(
                INPUT_INVALID,
                "Only a valid model can be versioned.",
            ));
        }
        let trimmed = |text: &Option<String>| {
            text.as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        let mut version = json!({
            "manifest_model_id": input.manifest_model_id,
            "manifest_model_revision": input.manifest_model_revision,
            "model_id": input.model_id,
            "model_kind": input.model_kind,
            "revision_id": input.revision_id,
            "expected_head_revision_id": input.expected_head_revision_id,
            "model": {"digest": bare(&input.model.digest), "object": input.model.object,
                "byte_length": input.model.byte_length},
            "bump_version": input.bump_version.unwrap_or(false),
            "model_schema_version": input.model_schema_version,
            "engine_version": input.engine_version,
            "validation": input.validation,
        });
        if let Some(name) = input.display_name.as_ref().filter(|name| !name.is_empty()) {
            version["display_name"] = name.clone().into();
        }
        for (field, text) in [
            ("reason", trimmed(&input.reason)),
            ("design_stage_id", trimmed(&input.design_stage_id)),
            ("detail_level_id", trimmed(&input.detail_level_id)),
        ] {
            if let Some(text) = text {
                version[field] = text.into();
            }
        }
        if let Some(approval) = input
            .approval
            .as_ref()
            .filter(|approval| !approval.status.trim().is_empty())
        {
            version["approval"] = json!(approval);
        }
        if let Some(source) = &input.migration_source {
            let mut wire = json!(source);
            wire["source_digest"] = bare(&source.source_digest).into();
            version["migration_source"] = wire;
        }
        if let Some(sources) = &input.composition_sources {
            version["composition_sources"] = sources
                .iter()
                .map(|source| {
                    let mut wire = json!({"sequence": source.sequence,
                        "source_kind": source.source_kind, "model_id": source.model_id,
                        "model_digest": bare(&source.model_digest)});
                    if let Some(revision) = &source.revision_id {
                        wire["revision_id"] = revision.clone().into();
                    }
                    wire
                })
                .collect();
        }
        let body = json!({"version": version});
        Ok(post("create_version", &input.ds_project, body)?.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        read(data(&body).get("version"), "created version").map(Admitted::Done)
    }
}

/// Retire one active head, fenced by the head and bytes the caller saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridModelDelete {
    pub project: String,
    pub model: String,
    pub expected_head_revision_id: String,
    pub expected_model_digest: String,
    pub reason: String,
}

impl Exchange for GridModelDelete {
    type Output = GridModelDeletion;

    fn plan(&self) -> Result<Plan, Refusal> {
        let reason = self.reason.trim();
        if reason.is_empty() {
            return Err(Refusal::new(
                INPUT_INVALID,
                "A deletion reason is required.",
            ));
        }
        let body = json!({"delete": {"model_id": self.model,
            "expected_head_revision_id": self.expected_head_revision_id,
            "expected_model_digest": bare(&self.expected_model_digest), "reason": reason}});
        Ok(post("delete_model", &self.project, body)?.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let deletion: GridModelDeletion = read(data(&body).get("deletion"), "deletion")?;
        if deletion.model_id != self.model || deletion.deleted_at.is_empty() {
            return Err(invalid("deletion"));
        }
        Ok(Admitted::Done(deletion))
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use wasm_bindgen::prelude::*;

    use super::{
        GridExportsList, GridModelDelete, GridModelGet, GridModelsList, GridUploadStart,
        GridVersionCreate, GridVersionInput, GridVersionResolve, GridVersionsList,
    };
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};

    /// A project's model heads, newest change first.
    #[wasm_bindgen(js_name = gridModelsList, unchecked_return_type = "GridModelSummary[]")]
    pub async fn grid_models_list(
        host: GatewayHost,
        project: String,
        include_deleted: bool,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, GridModelsList::new(project, include_deleted)).await?)
    }

    /// One model head.
    #[wasm_bindgen(js_name = gridModelGet, unchecked_return_type = "GridModelSummary")]
    pub async fn grid_model_get(
        host: GatewayHost,
        project: String,
        model: String,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, GridModelGet { project, model }).await?)
    }

    /// A model's immutable versions, highest first.
    #[wasm_bindgen(js_name = gridVersionsList, unchecked_return_type = "GridModelVersion[]")]
    pub async fn grid_versions_list(
        host: GatewayHost,
        project: String,
        model: String,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, GridVersionsList::new(project, model)).await?)
    }

    /// One exact version with a short-lived signed read of its bytes.
    #[wasm_bindgen(js_name = gridVersionResolve, unchecked_return_type = "ResolvedGridVersion")]
    pub async fn grid_version_resolve(
        host: GatewayHost,
        project: String,
        model: String,
        revision: String,
    ) -> Result<JsValue, JsValue> {
        let call = GridVersionResolve {
            project,
            model,
            revision,
        };
        typed(&run(&host, call).await?)
    }

    /// A revision's deliberate export manifests.
    #[wasm_bindgen(js_name = gridExportsList, unchecked_return_type = "GridExportRecord[]")]
    pub async fn grid_exports_list(
        host: GatewayHost,
        project: String,
        model: String,
        revision: String,
    ) -> Result<JsValue, JsValue> {
        let call = GridExportsList {
            project,
            model,
            revision,
        };
        typed(&run(&host, call).await?)
    }

    /// Open a session for one content-addressed object.
    #[wasm_bindgen(js_name = gridUploadStart, unchecked_return_type = "GridUploadStarted")]
    pub async fn grid_upload_start(
        host: GatewayHost,
        project: String,
        model: String,
        digest: String,
        byte_length: f64,
        purpose: Option<String>,
        content_type: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let call = GridUploadStart {
            project,
            model,
            digest,
            byte_length: byte_length.max(0.0) as u64,
            purpose,
            content_type,
        };
        typed(&run(&host, call).await?)
    }

    /// Create one immutable version at the head the caller saw.
    #[wasm_bindgen(js_name = gridVersionCreate, unchecked_return_type = "GridModelVersion")]
    pub async fn grid_version_create(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "GridVersionInput")] input: JsValue,
    ) -> Result<JsValue, JsValue> {
        let input: GridVersionInput = tsify::serde_wasm_bindgen::from_value(input)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        typed(&run(&host, GridVersionCreate(input)).await?)
    }

    /// Retire one active head.
    #[wasm_bindgen(js_name = gridModelDelete, unchecked_return_type = "GridModelDeletion")]
    pub async fn grid_model_delete(
        host: GatewayHost,
        project: String,
        model: String,
        expected_head_revision_id: String,
        expected_model_digest: String,
        reason: String,
    ) -> Result<JsValue, JsValue> {
        let call = GridModelDelete {
            project,
            model,
            expected_head_revision_id,
            expected_model_digest,
            reason,
        };
        typed(&run(&host, call).await?)
    }
}

#[cfg(test)]
#[path = "grid_models_tests.rs"]
mod tests;
