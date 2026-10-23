//! The Core-to-Reporter transformer export contract: the server-minted input
//! receipt, the identities and bounds every request carries, the print-context
//! document and the print warnings a result may carry. The Kernel plans and
//! verifies runs over these documents (`ds-command-kernel` `report_export`);
//! the Reporter reads the same documents. Pure: no IO, store or transport.

pub mod jcs;
pub mod preview;
pub mod proof;
pub mod survey;

pub use survey::{survey_layer_name, SurveyLayerResult, SURVEY_LAYERS_SCHEMA};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::printing::table_rows::PrintWarning;
use crate::report_formats::{self, DesignOutputSelection};

/// The wire schema of the receipt ds-brain mints beside a project's
/// configuration (`network_reporter_input_receipt.go`).
pub const INPUT_RECEIPT_SCHEMA: u32 = 1;
/// The member of the `/config/{project}` document that carries it.
pub const INPUT_RECEIPT_MEMBER: &str = "network_reporter_input_receipt";
/// The sibling member ds-brain writes instead when the receipt could not be
/// minted: why, and what resolves it (`NetworkReporterInputReceiptRefusal`
/// in `config_service.go`). A document with an effective configuration
/// carries exactly one of the two.
pub const INPUT_RECEIPT_REFUSAL_MEMBER: &str = "network_reporter_input_receipt_refusal";
/// The refusal code every host raises for a missing or refused receipt; the
/// message key names the reason ds-brain gave, or this code when it gave none.
pub const INPUT_RECEIPT_UNAVAILABLE: &str = "report_input_receipt_unavailable";
/// The engine task this module plans and verifies.
pub const REPORT_TASK: &str = "export_transformer_report";
/// The engine subcommand a host names for that task.
pub const REPORT_SUBCOMMAND: &str = "export-transformer-report";
/// The publication engine identity every receipt carries.
pub const ENGINE: &str = "network_reporter";
/// The publication variant of an individual transformer report.
pub const VARIANT: &str = "default";
/// The input shape a native host stages: plain GeoJSON layers and the cached
/// `{groups, sheets, models}` configuration.
pub const INPUT_SHAPE: &str = "plain_local";
/// The publication batch schema of a release engine's run.
pub const PUBLICATION_SCHEMA: u32 = 3;
/// The most semantic request bytes (room plus configuration) one run stages.
pub const MAX_SEMANTIC_REQUEST_BYTES: u64 = 64 * 1024 * 1024;
/// The most bytes a result document may occupy.
pub const MAX_RESULT_BYTES: u64 = 1024 * 1024;
/// The largest artifact a receipt may describe; a streamed publication never
/// materializes it, so this is the transfer bound, not a preview bound.
pub const MAX_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// The most print warnings one result may carry.
pub const MAX_PRINT_WARNINGS: usize = 2048;
/// The most transformers one batch may run.
pub const MAX_BATCH_TRANSFORMERS: usize = 2000;
/// The most engine processes a batch may keep resident at once.
pub const MAX_RESIDENT_LIMIT: usize = 64;
/// The resident bound a host uses when the operator names none — the same
/// four the browser batch runner defaulted to.
pub const DEFAULT_RESIDENT_LIMIT: usize = 4;
pub const RUN_RECEIPT_SCHEMA: &str = "ds.report-run/v1";
pub const BATCH_RECEIPT_SCHEMA: &str = "ds.report-batch/v1";
/// The receipt file a host writes beside a run's artifacts.
pub const RUN_RECEIPT_FILE: &str = "report-run.json";
/// The receipt file a host writes at the root of a batch's output directory.
pub const BATCH_RECEIPT_FILE: &str = "report-batch.json";
/// The wire schema of the print-context document a host stages beside a
/// transformer's room and the engine reads digest-bound
/// (`ds-network-reporter` `load_print_context`).
pub const PRINT_CONTEXT_SCHEMA: &str = "ds.print-context/v1";
/// The most context layers one print-context document may carry — the same
/// bound a print layout may request.
pub const MAX_PRINT_CONTEXT_LAYERS: usize = 64;
/// The most features one print-context document may carry, over every layer.
pub const MAX_PRINT_CONTEXT_FEATURES: usize = 100_000;
/// The most bytes one print-context document may occupy: the engine's bound.
pub const MAX_PRINT_CONTEXT_BYTES: u64 = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Identities and bounds
// ---------------------------------------------------------------------------

pub fn exact_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A publishable project identity: what ds-brain's compute registry accepts.
pub fn valid_project_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// A publishable transformer identity: ds-brain's canonical reference name,
/// already canonical — the same predicate the desktop shell applies before it
/// stages a room.
pub fn valid_transformer_identity(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 121
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return false;
    }
    let core = value.trim_matches('_');
    if core.is_empty() {
        return false;
    }
    let canonical = match core {
        "all_transformers" | "combined_transformers" | "combined_transformer" => {
            "combined_transformer".to_string()
        }
        _ if core.as_bytes()[0].is_ascii_digit() => format!("_{core}"),
        _ => core.to_string(),
    };
    value == canonical
}

pub fn admitted_printing_label(project: &str, label: Option<&str>) -> Result<String, String> {
    let label = label
        .filter(|text| valid_semantic_text(text, 200) && *text != project)
        .ok_or("The captured printing context has no bounded canonical project label")?;
    Ok(label.to_owned())
}

/// Consume a label captured from the sealed settings receipt and carried in
/// the same digest-pinned native request. There is no caller-label fallback.
pub fn captured_printing_project_label(
    project: &str,
    source: &Value,
    network_config: &Value,
    receipt: &InputReceipt,
) -> Result<String, String> {
    if source["project"]["ds_project"] != project
        || network_config["printing_context"]["project_id"] != project
    {
        return Err("The captured label context belongs to another project".into());
    }
    let config: BTreeMap<String, Value> = serde_json::from_value(network_config.clone())
        .map_err(|_| "The captured raw process configuration is invalid")?;
    let config_sha = sha256_hex(&serde_json::to_vec(&config).map_err(|e| e.to_string())?);
    if source["project_config_sha256"] != config_sha
        || source["project"]["printing_receipt"]["sheets_sha256"] != receipt.sheets_sha256
    {
        return Err(
            "The captured printing receipt differs from the raw configuration capture".into(),
        );
    }
    let context = receipt.printing_context_for_project(project)?;
    let label = admitted_printing_label(project, context["project"].as_str())?;
    if source["project"]["printing_label"] != label {
        return Err("The captured caption differs from its validated server receipt".into());
    }
    Ok(label)
}

pub fn valid_semantic_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

// ---------------------------------------------------------------------------
// The project's reporter input receipt
// ---------------------------------------------------------------------------

/// The server-minted project-wide portion of a report's input base: the
/// reporting country, the exact settings sheets the report policy reads (as
/// the bytes ds-brain serialized, never re-encoded) and the semantic identity
/// of the Rwanda administrative reference the artifacts are stamped with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputReceipt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_print_recipe: Option<proof::Provenance>,
    pub schema: u32,
    pub country: String,
    pub sheets_json: String,
    pub sheets_sha256: String,
    pub reference_semantic_sha256: String,
}

/// Why ds-brain could not mint the project's receipt, as the configuration
/// document carries it. `reason_key` is one closed key the hosts already
/// render (`printing_style_unavailable`, `printing_inputs_incomplete`,
/// `project_country_missing`, `report_input_receipt_unavailable`); `detail`
/// and `remedy` are the server's own sentences, carried unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InputReceiptRefusal {
    pub reason_key: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub remedy: String,
    /// The governed print styles a selected setup binds that the catalogue
    /// no longer holds, and the setups binding them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_print_styles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub printing_setups: Vec<String>,
}

impl InputReceiptRefusal {
    /// The refusal a host raises: the server's reason, detail and remedy in
    /// one sentence an operator can act on.
    pub fn sentence(&self) -> String {
        let mut sentence = format!(
            "the project configuration carries no `{INPUT_RECEIPT_MEMBER}`: {}",
            self.reason_key
        );
        if !self.detail.trim().is_empty() {
            sentence.push_str(": ");
            sentence.push_str(self.detail.trim());
        }
        if !self.remedy.trim().is_empty() {
            sentence.push_str("; ");
            sentence.push_str(self.remedy.trim());
        }
        sentence
    }
}

/// Whether a project can run a local report at all: the receipt is present
/// and proven, or the reason it is not. `ds report project settings` and the
/// Desktop's export panels read this so `ready` never says yes to an export
/// the kernel would refuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReceiptReadiness {
    pub ready: bool,
    /// One sentence naming the problem, absent when ready.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    /// The refusal a host renders: `code` is `report_input_receipt_unavailable`,
    /// `message_key` the server's reason key (or the code), plus the server's
    /// detail and remedy when it gave them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Value>,
}

impl InputReceipt {
    /// The server's refusal beside a missing receipt, when the document
    /// carries one. An unreadable refusal member reads as none: the receipt's
    /// absence is still reported, with the generic reason.
    pub fn refusal_from_config(document: &Value) -> Option<InputReceiptRefusal> {
        let value = document.get(INPUT_RECEIPT_REFUSAL_MEMBER)?;
        let refusal: InputReceiptRefusal = serde_json::from_value(value.clone()).ok()?;
        if !valid_semantic_text(&refusal.reason_key, 128) {
            return None;
        }
        Some(refusal)
    }

    /// Ready, or why not: the one answer both `ds` and the Desktop give
    /// before an export is attempted.
    pub fn readiness(document: &Value) -> ReceiptReadiness {
        match Self::from_config(document) {
            Ok(_) => ReceiptReadiness {
                ready: true,
                issue: None,
                refusal: None,
            },
            Err(issue) => {
                let refusal = Self::refusal_from_config(document);
                let mut rendered = json!({
                    "code": INPUT_RECEIPT_UNAVAILABLE,
                    "message_key": refusal
                        .as_ref()
                        .map_or(INPUT_RECEIPT_UNAVAILABLE, |r| r.reason_key.as_str()),
                    "mode": "refreshed",
                });
                if let Some(refusal) = refusal {
                    rendered["reason_key"] = json!(refusal.reason_key);
                    rendered["detail"] = json!(refusal.detail);
                    rendered["remedy"] = json!(refusal.remedy);
                    if !refusal.missing_print_styles.is_empty() {
                        rendered["missing_print_styles"] = json!(refusal.missing_print_styles);
                    }
                    if !refusal.printing_setups.is_empty() {
                        rendered["printing_setups"] = json!(refusal.printing_setups);
                    }
                }
                ReceiptReadiness {
                    ready: false,
                    issue: Some(issue),
                    refusal: Some(rendered),
                }
            }
        }
    }

    /// Read the receipt out of one `/config/{project}` document — the same
    /// member the browser reads from its cached copy — and prove it. When the
    /// member is absent and the server said why, the error is the server's
    /// reason, detail and remedy, not an invitation to refresh.
    pub fn from_config(document: &Value) -> Result<Self, String> {
        let value = document.get(INPUT_RECEIPT_MEMBER).ok_or_else(|| {
            match Self::refusal_from_config(document) {
                Some(refusal) => refusal.sentence(),
                None => format!(
                    "the project configuration carries no `{INPUT_RECEIPT_MEMBER}` and no `{INPUT_RECEIPT_REFUSAL_MEMBER}`; re-read the project configuration (`ds report project settings`) and retry"
                ),
            }
        })?;
        let receipt: Self = serde_json::from_value(value.clone()).map_err(|error| {
            format!("the Network Reporter input receipt is unreadable: {error}")
        })?;
        receipt.validate()?;
        Ok(receipt)
    }

    /// The receipt is well-formed and its sheets bytes are the ones it names.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INPUT_RECEIPT_SCHEMA
            || !valid_semantic_text(&self.country, 80)
            || self.sheets_json.is_empty()
            || self.sheets_json.len() as u64 > MAX_SEMANTIC_REQUEST_BYTES
            || !exact_lower_sha256(&self.sheets_sha256)
            || !exact_lower_sha256(&self.reference_semantic_sha256)
        {
            return Err("Network Reporter input receipt is invalid".to_string());
        }
        if format!("{:x}", Sha256::digest(self.sheets_json.as_bytes())) != self.sheets_sha256 {
            return Err(
                "Network Reporter sheets_json does not match its server receipt".to_string(),
            );
        }
        let sheets: Value = serde_json::from_str(&self.sheets_json)
            .map_err(|error| format!("Network Reporter sheets_json is invalid: {error}"))?;
        if !sheets.as_object().is_some_and(|object| !object.is_empty()) {
            return Err("Network Reporter sheets_json must be a non-empty JSON object".to_string());
        }
        Ok(())
    }

    /// The canonical project caption already sealed by the authorized config
    /// producer. Raw sheet captions and saved selection are not substitutes.
    pub fn printing_context_for_project(&self, project: &str) -> Result<Value, String> {
        self.validate()?;
        let context = self.sheets()?["printing_context"].clone();
        if context["project_id"] != project {
            return Err("The sealed printing context belongs to another project".into());
        }
        admitted_printing_label(project, context["project"].as_str())?;
        Ok(context)
    }

    /// A Rwanda project is stamped with the administrative reference; every
    /// other country scrubs those columns and must not stage the asset.
    pub fn requires_admin_bounds(&self) -> bool {
        self.country.trim().eq_ignore_ascii_case("Rwanda")
    }

    pub fn sheets(&self) -> Result<Value, String> {
        serde_json::from_str(&self.sheets_json)
            .map_err(|error| format!("Network Reporter sheets_json is invalid: {error}"))
    }

    /// The exact `{groups, sheets, models}` configuration document the engine
    /// reads under `plain_local`, spliced around the server's own sheets bytes
    /// so a Go spelling (an int64 above 2^53, `-0`, an escaped separator)
    /// survives unchanged.
    pub fn network_config_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        const PREFIX: &[u8] = b"{\"groups\":{},\"sheets\":";
        const SUFFIX: &[u8] = b",\"models\":{}}";
        let capacity = PREFIX
            .len()
            .checked_add(self.sheets_json.len())
            .and_then(|size| size.checked_add(SUFFIX.len()))
            .ok_or_else(|| "Network Reporter config size overflowed".to_string())?;
        let mut config = Vec::with_capacity(capacity);
        config.extend_from_slice(PREFIX);
        config.extend_from_slice(self.sheets_json.as_bytes());
        config.extend_from_slice(SUFFIX);
        Ok(config)
    }
}

// The survey layers a local run is produced without
// ---------------------------------------------------------------------------

/// The settings row that switches survey appending on, under every spelling
/// the reporter reads (`INCLUDE_SURVEY_EXPORT_SETTING_ALIASES`).
pub const INCLUDE_SURVEY_ALIASES: &[&str] = &[
    "include_survey_data_in_transformer_exports",
    "include-survey-data-in-transformer-exports",
];
/// The settings row naming the appended forms, under every spelling the
/// reporter reads (`SURVEY_EXPORT_FORM_SETTING_ALIASES`).
pub const SURVEY_FORMS_ALIASES: &[&str] = &[
    "survey_forms_in_transformer_exports",
    "survey-forms-in-transformer-exports",
    "transformer_export_survey_forms",
    "transformer-export-survey-forms",
];

pub fn setting_truthy(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => matches!(
            text.trim().to_ascii_lowercase().as_str(),
            "true" | "yes" | "1" | "on"
        ),
        _ => false,
    }
}

pub fn parameter_matches(row: &Value, aliases: &[&str]) -> bool {
    row["parameter"]
        .as_str()
        .map(|name| name.trim().to_ascii_lowercase())
        .is_some_and(|name| aliases.contains(&name.as_str()))
}

pub fn include_survey_row(row: &Value) -> bool {
    parameter_matches(row, INCLUDE_SURVEY_ALIASES)
}

/// The survey forms a project's settings append to a delivered transformer
/// report, read from the `project_settings` rows exactly as the reporter
/// reads them from the same bytes. Empty when the policy is off or names no
/// form — then nothing is appended and nothing is omitted.
pub fn survey_append_forms(rows: &[Value]) -> Vec<String> {
    let enabled = rows
        .iter()
        .any(|row| include_survey_row(row) && setting_truthy(&row["value"]));
    if !enabled {
        return Vec::new();
    }
    rows.iter()
        .filter(|row| parameter_matches(row, SURVEY_FORMS_ALIASES))
        .flat_map(|row| crate::report_formats::string_list(&row["value"]))
        .map(|form| form.trim().to_string())
        .filter(|form| !form.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Two spellings of the same form set. The kernel reads the settings rows and
/// the engine reads the same bytes through its own policy decoder, which
/// dedupes case-insensitively and keeps the document's order; neither order
/// nor case is the fact, so they are compared as sets.
pub fn same_survey_forms(left: &[String], right: &[String]) -> bool {
    let normalize = |forms: &[String]| {
        forms
            .iter()
            .map(|form| form.trim().to_ascii_lowercase())
            .filter(|form| !form.is_empty())
            .collect::<std::collections::BTreeSet<_>>()
    };
    normalize(left) == normalize(right)
}

// ---------------------------------------------------------------------------
// The print context
// ---------------------------------------------------------------------------

/// Lowercase hex SHA-256 of `bytes` — the digest form every receipt and
/// engine request in this module carries.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The exact `ds.print-context/v1` document a host stages beside a
/// transformer's room, and that the engine reads digest-bound.
///
/// One recipe for every host — the desktop shell assembled this document
/// itself until now — so a desktop and a server given the same coverage,
/// sources and layers stage byte-identical assets under one digest:
/// `{"schema","coverage","sources","layers"}` in that order, layers in the
/// map's own order, compact serialization.
///
/// Validation mirrors what the engine would refuse, so a bad document is
/// refused where its layers were gathered rather than after an engine
/// started: `coverage` is the area the layers were read for (a Polygon or
/// MultiPolygon); every layer id is an identifier and its collection exactly
/// `{type: FeatureCollection, features}` of Features with object properties
/// and an object or null geometry; no layer is empty — the engine drops an
/// empty collection silently, and a host names what it could not supply in
/// the run receipt instead; every `sources` entry is an object naming a
/// `layer` of the document; and the whole stays inside the feature and byte
/// bounds the engine enforces.
pub fn print_context_document(
    coverage: &Value,
    sources: &Value,
    layers: &BTreeMap<String, Value>,
) -> Result<Vec<u8>, String> {
    crate::area::validate_area(coverage)
        .map_err(|error| format!("print context coverage: {error}"))?;
    if layers.is_empty() {
        return Err("print context has no layers".to_string());
    }
    if layers.len() > MAX_PRINT_CONTEXT_LAYERS {
        return Err(format!(
            "print context carries {} layers, over the {MAX_PRINT_CONTEXT_LAYERS}-layer bound",
            layers.len()
        ));
    }
    let mut features = 0_usize;
    for (id, collection) in layers {
        if !crate::printing::identifier(id) {
            return Err(format!(
                "print context layer id {id:?} is not an identifier"
            ));
        }
        let object = collection.as_object().ok_or_else(|| {
            format!("print context layer {id:?} must be a GeoJSON FeatureCollection")
        })?;
        if object.len() != 2
            || object.get("type").and_then(Value::as_str) != Some("FeatureCollection")
        {
            return Err(format!(
                "print context layer {id:?} must contain only type=FeatureCollection and features"
            ));
        }
        let rows = object
            .get("features")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("print context layer {id:?} features must be an array"))?;
        if rows.is_empty() {
            return Err(format!("print context layer {id:?} is empty"));
        }
        for (index, feature) in rows.iter().enumerate() {
            let shaped = feature.get("type").and_then(Value::as_str) == Some("Feature")
                && feature.get("properties").is_some_and(Value::is_object)
                && feature
                    .get("geometry")
                    .is_some_and(|geometry| geometry.is_object() || geometry.is_null());
            if !shaped {
                return Err(format!(
                    "print context layer {id:?} feature[{index}] is not a GeoJSON Feature with object properties"
                ));
            }
        }
        features = features
            .checked_add(rows.len())
            .filter(|total| *total <= MAX_PRINT_CONTEXT_FEATURES)
            .ok_or_else(|| {
                format!("print context carries more than {MAX_PRINT_CONTEXT_FEATURES} features")
            })?;
    }
    let entries = sources
        .as_array()
        .ok_or("print context sources must be an array")?;
    if entries.len() > MAX_PRINT_CONTEXT_LAYERS {
        return Err(format!(
            "print context names {} sources, over the {MAX_PRINT_CONTEXT_LAYERS}-source bound",
            entries.len()
        ));
    }
    for (index, entry) in entries.iter().enumerate() {
        let named = entry
            .get("layer")
            .and_then(Value::as_str)
            .is_some_and(|layer| layers.contains_key(layer));
        if !entry.is_object() || !named {
            return Err(format!(
                "print context source[{index}] must be an object naming a layer of the document"
            ));
        }
    }
    let bytes = serde_json::to_vec(&json!({
        "schema": PRINT_CONTEXT_SCHEMA,
        "coverage": coverage,
        "sources": sources,
        "layers": layers,
    }))
    .map_err(|error| format!("could not encode print context: {error}"))?;
    if bytes.len() as u64 > MAX_PRINT_CONTEXT_BYTES {
        return Err(format!(
            "print context is {} bytes, over the 64 MiB bound",
            bytes.len()
        ));
    }
    Ok(bytes)
}

/// The warnings that are engineering findings. Each is a row on the workbook's
/// Engineering Review sheet, and a report that carries one was generated but is
/// not ready for approval. Every other print warning is cartographic and never
/// blocks a submission.
pub const ENGINEERING_REVIEW_CODES: [&str; 4] = [
    "lv_line_cable_recommendation",
    // What reporters released before 2026-10-20 still send.
    "lv_pole_line_cable_conflict",
    "transformer_protection_mismatch",
    "lv_span_exceeds_max_length",
];

/// The longest warning code carried from a Reporter this Kernel does not know.
pub const MAX_PRINT_WARNING_CODE_BYTES: usize = 64;

/// Is this warning admissible on this run? Every host that reads an engine
/// result asks here, so the desktop shell and `ds` accept the same warnings.
///
/// A code this Kernel knows must have that code's exact shape. A code it does
/// not know — a Reporter released after this Kernel (owner 22 Oct) — is never
/// a refusal: it is carried as the typed, displayable [`PrintWarning`] it is,
/// code and message unchanged, within the bounds every warning shares
/// ([`carried_print_warning`]). It is cartographic: only
/// [`ENGINEERING_REVIEW_CODES`] hold approval.
pub fn valid_print_warning(warning: &PrintWarning, transformer: &str, formats: &[String]) -> bool {
    match known_print_warning(warning, transformer, formats) {
        Some(valid) => valid,
        None => carried_print_warning(warning, transformer, formats),
    }
}

/// The bounds an unknown code is carried within: a snake_case identifier code,
/// this run's transformer (any case) or none, one of this run's outputs or
/// none, bounded identifiers and message, and honest row counts.
pub fn carried_print_warning(
    warning: &PrintWarning,
    transformer: &str,
    formats: &[String],
) -> bool {
    let identifier = |value: &str| value.len() <= 240 && !value.chars().any(char::is_control);
    !warning.code.is_empty()
        && warning.code.len() <= MAX_PRINT_WARNING_CODE_BYTES
        && warning.code.as_bytes()[0].is_ascii_lowercase()
        && warning
            .code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && (warning.transformer.is_empty() || warning.transformer.eq_ignore_ascii_case(transformer))
        && (warning.output_id.is_empty() || formats.contains(&warning.output_id))
        && identifier(&warning.layout_id)
        && identifier(&warning.element_id)
        && warning.omitted_rows <= warning.total_rows
        && warning.total_rows <= 1_000_000
        && warning.message.len() <= 2048
}

/// `Some(valid)` for a code this Kernel's tables, diagnostics or engineering
/// audits raise, checked against that code's exact shape; `None` for any other
/// code.
pub fn known_print_warning(
    warning: &PrintWarning,
    transformer: &str,
    formats: &[String],
) -> Option<bool> {
    let plan_transformer = transformer;
    let placed = warning.transformer == plan_transformer
        && formats.contains(&warning.output_id)
        && report_formats::named_print_output(&warning.output_id).map(|(_, id)| id)
            == Some(warning.layout_id.as_str())
        && warning.message.len() <= 2048;
    let valid = match warning.code.as_str() {
        // A table that could not show every row names the panel and
        // the honest counts.
        "table_truncated" => {
            placed
                && crate::printing::identifier(&warning.element_id)
                && warning.omitted_rows > 0
                && warning.omitted_rows < warning.total_rows
                && warning.total_rows <= 1_000_000
        }
        // Measured furniture diagnostics (printing::diagnostics) and
        // their bounded summary: mechanical findings in millimetres,
        // no table rows behind them. The summary names no element.
        "print_element_clipped"
        | "print_furniture_overlap"
        | "print_design_overlap"
        | "print_layout_warning" => {
            placed
                && crate::printing::identifier(&warning.element_id)
                && warning.omitted_rows == 0
                && warning.total_rows == 0
        }
        "print_diagnostics_summary" | "print_label_omissions" => {
            placed
                && warning.element_id.is_empty()
                && warning.omitted_rows == 0
                && warning.total_rows == 0
        }
        // A text cell whose `{key}` had no value in the title block or
        // the project printed empty. Same shape as the summary: no
        // element, no rows.
        "print_text_unbound" => {
            placed
                && warning.element_id.is_empty()
                && warning.omitted_rows == 0
                && warning.total_rows == 0
        }
        // A saved style choice one map layer could not print with, skipped
        // so the rest of the sheet still renders: an override part the
        // governed style cannot take (printing::style_overrides::usable),
        // a point symbol style meeting line or polygon features, or a
        // governed style the renderer cannot draw, whole or one part. The
        // element is the map layer; no table rows stand behind it.
        "print_style_override_skipped" | "print_symbol_skipped" | "print_style_skipped" => {
            placed
                && crate::printing::identifier(&warning.element_id)
                && warning.omitted_rows == 0
                && warning.total_rows == 0
        }
        // A drawn design pole lacks its authored type. The renderer keeps
        // its exact geometry, flags the visible pole and names the layer;
        // no omitted table row or inferred category stands behind it.
        "print_pole_category_missing" => {
            placed
                && crate::printing::identifier(&warning.element_id)
                && crate::printing::pole_categories::design_pole_layer(&warning.element_id)
                && warning.omitted_rows == 0
                && warning.total_rows == 0
        }
        // Cable findings (ds-network `lv_cable`): one row per LV line
        // whose computed size differs from its approved saved one, which
        // every print states; a reporter released before 2026-10-20 sends
        // one row per pole whose saved label disagrees with its line. They
        // ride the xlsx output and name no paper; the transformer is the
        // saved data label, which differs from the run's name only by
        // case; the element is the saved line number. No table rows stand
        // behind them.
        "lv_line_cable_recommendation" | "lv_pole_line_cable_conflict" => {
            warning.transformer.eq_ignore_ascii_case(plan_transformer)
                && warning.output_id == "xlsx"
                && formats.contains(&warning.output_id)
                && warning.layout_id.is_empty()
                && !warning.element_id.is_empty()
                && warning.element_id.len() <= 240
                && warning.omitted_rows == 0
                && warning.total_rows == 0
                && warning.message.len() <= 2048
        }
        // ds-network's LV engineering audits, re-run by the reporter over
        // the saved layers (network::audit_transformer_protection and
        // audit_lv_span_lengths), one Engineering Review row each. They
        // ride the xlsx output like the cable conflicts; the element is the
        // audited layer. No table rows stand behind them.
        "transformer_protection_mismatch" | "lv_span_exceeds_max_length" => {
            let layer = if warning.code == "transformer_protection_mismatch" {
                "tr"
            } else {
                "spans"
            };
            warning.transformer.eq_ignore_ascii_case(plan_transformer)
                && warning.output_id == "xlsx"
                && formats.contains(&warning.output_id)
                && warning.layout_id.is_empty()
                && warning.element_id == layer
                && warning.omitted_rows == 0
                && warning.total_rows == 0
                && warning.message.len() <= 2048
        }
        _ => return None,
    };
    Some(valid)
}
