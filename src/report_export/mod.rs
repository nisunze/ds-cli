//! One transformer report run, decided once for every native host.
//!
//! The desktop shell (`ds-web/src-tauri/src/network_reporter.rs`), the `ds`
//! executable and a headless server all drive the same engine —
//! `ds-report export-transformer-report` — from the same governed inputs: the
//! project's server-minted Network Reporter input receipt, one exact
//! transformer room and the project's output policy. What those inputs mean
//! is decided here, without IO: which formats to ask for, what the input-base
//! fingerprint and the room digest are, the exact request document the engine
//! reads, whether the engine's result answers the request, and what a run's
//! receipt says. Hosts stage bytes, start the engine, verify files and hold
//! the clock.
//!
//! Every recipe here is byte-identical to the one the desktop shell and
//! ds-brain already apply, so a server-produced receipt carries the same
//! `input_base_fingerprint` and `room_content_sha256` the publication contract
//! admits against.

pub mod jcs;
pub mod preview;
pub mod proof;

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::printing::table_rows::{PrintWarning, ONE_PANEL_PRESENTATION};
use crate::printing::ArtifactPresentation;
use crate::report::{publication_state, PublicationState, LOCAL_ONLY_SCHEMA};
use crate::report_formats::{self, DesignOutputSelection, Output};

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

/// A transformer an individual report may be run for: publishable, and not
/// one of the names a report produces or the project holds as a document.
pub fn reportable_transformer(value: &str) -> Result<(), String> {
    if !valid_transformer_identity(value) {
        return Err(format!(
            "transformer {value:?} is not a publishable canonical identity"
        ));
    }
    if crate::design::is_special_row(value) {
        return Err(format!(
            "transformer {value:?} is a reserved identity, never an individual report"
        ));
    }
    Ok(())
}

fn valid_semantic_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

fn decode_sha256(value: &str) -> Result<[u8; 32], String> {
    if !exact_lower_sha256(value) {
        return Err("Network Reporter input digest is invalid".to_string());
    }
    let mut decoded = [0_u8; 32];
    for (index, output) in decoded.iter_mut().enumerate() {
        let pair = &value.as_bytes()[index * 2..index * 2 + 2];
        let nibble = |byte: u8| -> u8 {
            match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => unreachable!("exact_lower_sha256 already validated the byte"),
            }
        };
        *output = (nibble(pair[0]) << 4) | nibble(pair[1]);
    }
    Ok(decoded)
}

/// `network_reporter/input-base/v2`: the exact server-owned input base of one
/// transformer export, as ds-brain fingerprints it for `work/open` admission
/// and the desktop shell records it. Variable-length strings are uint32
/// big-endian length-prefixed, the version is uint64 big-endian, and both
/// digests contribute their decoded bytes.
pub fn input_base_fingerprint(
    transformer: &str,
    server_version: i64,
    sheets_sha256: &str,
    country: &str,
    reference_semantic_sha256: &str,
) -> Result<String, String> {
    if !valid_transformer_identity(transformer) || server_version < 0 {
        return Err("Network Reporter transformer input version is invalid".to_string());
    }
    let transformer_len = u32::try_from(transformer.len())
        .map_err(|_| "Network Reporter transformer identity is too long".to_string())?;
    let country_len = u32::try_from(country.len())
        .map_err(|_| "Network Reporter country is too long".to_string())?;
    let sheets_digest = decode_sha256(sheets_sha256)?;
    let reference_digest = decode_sha256(reference_semantic_sha256)?;
    let mut preimage =
        Vec::with_capacity(31 + 4 + transformer.len() + 8 + 32 + 4 + country.len() + 32);
    preimage.extend_from_slice(b"network_reporter/input-base/v2\0");
    preimage.extend_from_slice(&transformer_len.to_be_bytes());
    preimage.extend_from_slice(transformer.as_bytes());
    preimage.extend_from_slice(&(server_version as u64).to_be_bytes());
    preimage.extend_from_slice(&sheets_digest);
    preimage.extend_from_slice(&country_len.to_be_bytes());
    preimage.extend_from_slice(country.as_bytes());
    preimage.extend_from_slice(&reference_digest);
    Ok(format!("{:x}", Sha256::digest(preimage)))
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

// ---------------------------------------------------------------------------
// The room
// ---------------------------------------------------------------------------

/// What one room holds, proven before it is staged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RoomEvidence {
    pub layers: usize,
    pub features: usize,
}

fn validate_feature_collection(name: &str, collection: &Value) -> Result<usize, String> {
    if !valid_semantic_text(name, 256) {
        return Err("Network Reporter layer names must be canonical non-empty text".to_string());
    }
    let object = collection
        .as_object()
        .ok_or_else(|| format!("Network Reporter layer {name:?} must be a GeoJSON collection"))?;
    if object.len() != 2
        || object.get("type").and_then(Value::as_str) != Some("FeatureCollection")
        || !object.contains_key("features")
    {
        return Err(format!(
            "Network Reporter layer {name:?} must contain only type=FeatureCollection and features"
        ));
    }
    let features = object
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Network Reporter layer {name:?} features must be an array"))?;
    for (index, feature) in features.iter().enumerate() {
        if feature.get("type").and_then(Value::as_str) != Some("Feature") {
            return Err(format!(
                "Network Reporter layer {name:?} feature[{index}] is not a GeoJSON Feature"
            ));
        }
    }
    Ok(features.len())
}

// ---------------------------------------------------------------------------
// The survey layers a local run is produced without
// ---------------------------------------------------------------------------

/// The settings row that switches survey appending on, under every spelling
/// the reporter reads (`INCLUDE_SURVEY_EXPORT_SETTING_ALIASES`).
const INCLUDE_SURVEY_ALIASES: &[&str] = &[
    "include_survey_data_in_transformer_exports",
    "include-survey-data-in-transformer-exports",
];
/// The settings row naming the appended forms, under every spelling the
/// reporter reads (`SURVEY_EXPORT_FORM_SETTING_ALIASES`).
const SURVEY_FORMS_ALIASES: &[&str] = &[
    "survey_forms_in_transformer_exports",
    "survey-forms-in-transformer-exports",
    "transformer_export_survey_forms",
    "transformer-export-survey-forms",
];

fn setting_truthy(value: &Value) -> bool {
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

pub(crate) fn parameter_matches(row: &Value, aliases: &[&str]) -> bool {
    row["parameter"]
        .as_str()
        .map(|name| name.trim().to_ascii_lowercase())
        .is_some_and(|name| aliases.contains(&name.as_str()))
}

pub(crate) fn include_survey_row(row: &Value) -> bool {
    parameter_matches(row, INCLUDE_SURVEY_ALIASES)
}

/// The survey forms a project's settings append to a delivered transformer
/// report, read from the `project_settings` rows exactly as the reporter
/// reads them from the same bytes. Empty when the policy is off or names no
/// form — then nothing is appended and nothing is omitted.
pub(crate) fn survey_append_forms(rows: &[Value]) -> Vec<String> {
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

/// The diagnostic code every host names a survey-layer omission with: the
/// preview page's diagnostics, the delivered run's receipt and the batch row
/// all carry this one code, so a reader learns the vocabulary once.
pub const SURVEY_LAYERS_OMITTED: &str = "print_survey_layers_omitted";

/// The reason one omitted survey form carries beside the print context ids.
pub const SURVEY_LAYER_OMISSION_REASON: &str =
    "survey layer: appended to delivered reports from the cloud, absent from a local room";

/// One sentence naming what a locally produced sheet was made without.
pub fn survey_layers_omitted_message(forms: &[String]) -> String {
    format!(
        "produced without the survey layers the project appends to delivered reports ({}); survey layers are cloud-resident and a local transformer room holds none",
        forms.join(", ")
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SurveyContextOmission {
    pub layer: String,
    pub reason: String,
}

/// The survey forms a project's policy appends to a delivered report that
/// one run was produced without.
///
/// A local transformer room holds design layers only, so a project whose
/// policy appends cloud-resident survey forms cannot have them on a locally
/// produced sheet. That is an omission to declare, never a reason to refuse
/// the sheet: the operator asked for the drawing, and a named omission tells
/// them exactly what is not on it. Previews have said this since they were
/// built; a delivery says it in the same words, under the same code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurveyLayersOmitted {
    /// Always [`SURVEY_LAYERS_OMITTED`].
    pub code: String,
    /// The form names, as the project's settings spell them.
    pub forms: Vec<String>,
    pub message: String,
    /// One context_omitted row per form, matching a print preview's receipt.
    pub context_omitted: Vec<SurveyContextOmission>,
}

impl SurveyLayersOmitted {
    /// The omission these forms describe, or `None` when nothing is omitted.
    pub fn new(forms: Vec<String>) -> Option<Self> {
        if forms.is_empty() {
            return None;
        }
        Some(Self {
            code: SURVEY_LAYERS_OMITTED.to_string(),
            message: survey_layers_omitted_message(&forms),
            context_omitted: forms
                .iter()
                .map(|form| SurveyContextOmission {
                    layer: form.clone(),
                    reason: SURVEY_LAYER_OMISSION_REASON.to_string(),
                })
                .collect(),
            forms,
        })
    }

    /// The union of several runs' omissions, for a batch's own receipt.
    pub fn union<'a>(omissions: impl IntoIterator<Item = &'a Self>) -> Option<Self> {
        let forms = omissions
            .into_iter()
            .flat_map(|omitted| omitted.forms.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        Self::new(forms)
    }
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

/// The room is the exact `{layers: {name: FeatureCollection}}` projection the
/// engine consumes, with at least one feature somewhere.
pub fn validate_layers(layers: &BTreeMap<String, Value>) -> Result<RoomEvidence, String> {
    if layers.is_empty() {
        return Err("Network Reporter export requires local transformer layers".to_string());
    }
    let features = layers
        .iter()
        .try_fold(0_usize, |total, (name, collection)| {
            let count = validate_feature_collection(name, collection)?;
            total
                .checked_add(count)
                .ok_or_else(|| "Network Reporter feature count overflowed".to_string())
        })?;
    if features == 0 {
        return Err("Network Reporter export has no local features".to_string());
    }
    Ok(RoomEvidence {
        layers: layers.len(),
        features,
    })
}

/// The exact transformer document a host stages: `{"layers": …}`, the
/// desktop room projection, in the layer map's own order.
pub fn transformer_document_bytes(layers: &BTreeMap<String, Value>) -> Result<Vec<u8>, String> {
    #[derive(Serialize)]
    struct PlainTransformerDocument<'a> {
        layers: &'a BTreeMap<String, Value>,
    }
    serde_json::to_vec(&PlainTransformerDocument { layers })
        .map_err(|error| format!("could not encode local transformer layers: {error}"))
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
    crate::reference_cache::validate_area(coverage)
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

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

/// Everything one run is bound to before the engine starts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_print_recipe: Option<proof::Provenance>,
    pub project_id: String,
    pub transformer: String,
    pub server_version: i64,
    pub country: String,
    pub requires_admin_bounds: bool,
    /// The engine formats, in the policy's order; also the output ids.
    pub formats: Vec<String>,
    pub outputs: Vec<Output>,
    pub input_base_fingerprint: String,
    pub room_content_sha256: String,
    /// ds-brain's `metadata.content_digest` recipe over the same layers, so
    /// a host that fetched the room from the service can prove it holds the
    /// bytes the service holds.
    pub content_digest: String,
    pub sheets_sha256: String,
    pub reference_semantic_sha256: String,
    pub room: RoomEvidence,
    /// The survey forms this run is produced without, when the project's
    /// policy appends any. A local room holds no survey layers, so the run
    /// proceeds and names them. Absent for a preview, whose recipe already
    /// switched the append off and names the omission itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub survey_layers_omitted: Option<SurveyLayersOmitted>,
}

/// Decide one run from its governed inputs. Refuses an unpublishable
/// identity, a reserved name, an empty room, a receipt that does not prove
/// its sheets, and a selection the project cannot render.
pub fn plan_transformer_export(
    project_id: &str,
    receipt: &InputReceipt,
    transformer: &str,
    server_version: i64,
    layers: &BTreeMap<String, Value>,
    selection: Option<&DesignOutputSelection>,
) -> Result<ExportPlan, String> {
    if !valid_project_id(project_id) {
        return Err("Network Reporter project identity is not publishable".to_string());
    }
    reportable_transformer(transformer)?;
    if !(0..=9_007_199_254_740_991).contains(&server_version) {
        return Err(
            "Network Reporter local version must be a non-negative exact JavaScript integer"
                .to_string(),
        );
    }
    receipt.validate()?;
    let room = validate_layers(layers)?;
    let sheets = receipt.sheets()?;
    // A project that appends cloud-resident survey forms to its delivered
    // reports still gets its sheet here: the forms are named as omitted and
    // the run proceeds. Refusing would leave an operator with no drawing at
    // all for a policy they cannot satisfy from a local room.
    let survey_layers_omitted = SurveyLayersOmitted::new(
        sheets["project_settings"]
            .as_array()
            .map(|rows| survey_append_forms(rows))
            .unwrap_or_default(),
    );
    let outputs = report_formats::selected_plan(&sheets, false, selection)?;
    if outputs.is_empty() {
        return Err("the project's output policy names no individual report output".to_string());
    }
    let formats = outputs
        .iter()
        .map(|output| output.format.clone())
        .collect::<Vec<_>>();
    let room_content_sha256 = jcs::room_content_sha256(layers).map_err(|error| {
        format!("Network Reporter room is not RFC 8785 canonicalizable: {error}")
    })?;
    let content_digest = jcs::layers_content_digest(layers).map_err(|error| {
        format!("Network Reporter room is not RFC 8785 canonicalizable: {error}")
    })?;
    let input_base_fingerprint = input_base_fingerprint(
        transformer,
        server_version,
        &receipt.sheets_sha256,
        &receipt.country,
        &receipt.reference_semantic_sha256,
    )?;
    Ok(ExportPlan {
        local_print_recipe: receipt.local_print_recipe.clone(),
        project_id: project_id.to_string(),
        transformer: transformer.to_string(),
        server_version,
        country: receipt.country.clone(),
        requires_admin_bounds: receipt.requires_admin_bounds(),
        formats,
        outputs,
        input_base_fingerprint,
        room_content_sha256,
        content_digest,
        sheets_sha256: receipt.sheets_sha256.clone(),
        reference_semantic_sha256: receipt.reference_semantic_sha256.clone(),
        room,
        survey_layers_omitted,
    })
}

// ---------------------------------------------------------------------------
// The engine request
// ---------------------------------------------------------------------------

/// The files a host staged for one run. Paths are the host's; digests are the
/// exact bytes it wrote or verified.
#[derive(Debug, Clone)]
pub struct StagedInputs<'a> {
    pub transformer_document: &'a Path,
    pub network_config: &'a Path,
    pub out_dir: &'a Path,
    /// The verified Rwanda villages asset and the SHA-256 of its exact bytes.
    pub admin_bounds: Option<(&'a Path, &'a str)>,
    pub media_grant: Option<&'a Path>,
    /// A `ds.print-context/v1` document and the SHA-256 of its exact bytes.
    pub print_context: Option<(&'a Path, &'a str)>,
    /// This transformer's one-based position in the drawing set and the set's
    /// size, so a sheet can print "12 of 86"; `None` prints a set of one.
    pub sheet: Option<(u32, u32)>,
}

/// The exact typed request document `ds-report export-transformer-report`
/// reads. The country decides whether the administrative asset may be staged
/// at all: a Rwanda run without it is refused before the engine reports the
/// same, and another country never stages Rwanda bounds.
pub fn engine_request(plan: &ExportPlan, staged: &StagedInputs<'_>) -> Result<Value, String> {
    if plan.requires_admin_bounds != staged.admin_bounds.is_some() {
        return Err(if plan.requires_admin_bounds {
            "a Rwanda report requires the verified administrative reference asset".to_string()
        } else {
            "Network Reporter must not stage Rwanda administrative bounds for another country"
                .to_string()
        });
    }
    if staged
        .admin_bounds
        .is_some_and(|(_, digest)| !exact_lower_sha256(digest))
        || staged
            .print_context
            .is_some_and(|(_, digest)| !exact_lower_sha256(digest))
    {
        return Err("a staged input digest is not a lowercase SHA-256".to_string());
    }
    let mut request = serde_json::Map::new();
    request.insert(
        "transformer_document".into(),
        json!(staged.transformer_document),
    );
    request.insert("network_config".into(), json!(staged.network_config));
    request.insert("input_shape".into(), json!(INPUT_SHAPE));
    request.insert("transformer".into(), json!(plan.transformer));
    request.insert("out_dir".into(), json!(staged.out_dir));
    request.insert("country".into(), json!(plan.country));
    if let Some((path, digest)) = staged.admin_bounds {
        request.insert("admin_bounds_asset".into(), json!(path));
        request.insert("admin_bounds_sha256".into(), json!(digest));
    }
    if let Some(path) = staged.media_grant {
        request.insert("media_grant".into(), json!(path));
    }
    if let Some((index, count)) = staged.sheet {
        if index == 0 || count == 0 || index > count {
            return Err(format!(
                "sheet {index} of {count} is not a position in a drawing set"
            ));
        }
        request.insert("sheet_index".into(), json!(index));
        request.insert("sheet_count".into(), json!(count));
    }
    if let Some((path, digest)) = staged.print_context {
        request.insert("print_context_asset".into(), json!(path));
        request.insert("print_context_sha256".into(), json!(digest));
    }
    // The survey forms the project appends that this room does not hold. The
    // engine reads the same configuration and names the same forms; saying
    // them here makes the request self-describing and lets the engine refuse
    // a caller whose idea of the policy differs from the bytes it staged.
    // Written only when something is omitted, so a request that omits nothing
    // is byte-identical to the one every earlier release wrote.
    if let Some(omitted) = &plan.survey_layers_omitted {
        request.insert("survey_layers_omitted".into(), json!(omitted.forms));
    }
    request.insert("formats".into(), json!(plan.formats));
    Ok(Value::Object(request))
}

// ---------------------------------------------------------------------------
// The engine identity
// ---------------------------------------------------------------------------

/// The release identity of the engine that produced a run, read from
/// `ds-report build-info`. Artifacts are bound to it; a receipt names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineIdentity {
    pub engine_version: String,
    pub build_manifest_sha256: String,
    pub package_version: String,
    pub source_sha: String,
    pub profile: String,
}

impl EngineIdentity {
    /// A development build's artifacts stay local; a release build's may be
    /// published.
    pub fn publication_state(&self) -> Result<PublicationState, String> {
        publication_state(&self.engine_version, true, false)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildInfo {
    schema: String,
    service: String,
    binary: String,
    package_version: String,
    source_sha: String,
    engine_version: String,
    cargo_lock_sha256: String,
    features: Vec<String>,
    rustc: String,
    target: String,
    profile: String,
    schemas: Vec<String>,
    build_manifest_sha256: String,
}

/// Prove one `build-info` document is the reporter's closed build identity.
pub fn engine_identity(document: &Value) -> Result<EngineIdentity, String> {
    let info: BuildInfo = serde_json::from_value(document.clone())
        .map_err(|error| format!("reporter build identity is malformed: {error}"))?;
    if info.schema != "ds.engine-build/v1"
        || info.service != "ds-network-reporter"
        || info.binary != "ds-report"
        || info.engine_version
            != format!(
                "ds-network-reporter@{}+{}",
                info.package_version, info.source_sha
            )
        || !exact_lower_sha256(&info.build_manifest_sha256)
        || !exact_lower_sha256(&info.cargo_lock_sha256)
        || info.rustc.is_empty()
        || info.target.is_empty()
        || info.profile.is_empty()
        || info.schemas.is_empty()
        || info.features.len() > 64
    {
        return Err("reporter build identity is outside the closed contract".to_string());
    }
    // A release identity or the one development spelling; anything else is
    // not an engine this product knows.
    publication_state(&info.engine_version, true, false)?;
    Ok(EngineIdentity {
        engine_version: info.engine_version,
        build_manifest_sha256: info.build_manifest_sha256,
        package_version: info.package_version,
        source_sha: info.source_sha,
        profile: info.profile,
    })
}

// ---------------------------------------------------------------------------
// The result document
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRecord {
    pub format: String,
    pub filename: String,
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<ArtifactPresentation>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Blocker {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AdminBoundsEvidence {
    pub configured: bool,
    pub villages_loaded: usize,
    pub features_matched: usize,
    pub features_requested: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultDocument {
    #[serde(default)]
    warnings: Vec<PrintWarning>,
    /// One measured assessment per named print output (`printing::
    /// diagnostics`): the typed collision findings behind the warnings,
    /// with rectangles and suggested moves. Bounded by the engine.
    #[serde(default)]
    print_diagnostics: Vec<Value>,
    /// The survey forms the engine produced this report without. Defaulted,
    /// because an engine that omits nothing writes nothing here — and an
    /// engine built before the member existed simply never omitted anything
    /// without refusing.
    #[serde(default)]
    survey_layers_omitted: Vec<String>,
    task: String,
    status: String,
    transformer: String,
    country: String,
    layers_decoded: usize,
    features_decoded: usize,
    admin_bounds: AdminBoundsEvidence,
    geometry_changed: bool,
    formats_requested: Vec<String>,
    artifacts: Vec<ArtifactRecord>,
    blockers: Vec<Blocker>,
    elapsed_ms: u64,
}

/// One artifact the engine declared, matched to the output it answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VerifiedArtifact {
    pub output_id: String,
    pub format: String,
    pub filename: String,
    pub content_type: &'static str,
    pub size_bytes: u64,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paper_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<ArtifactPresentation>,
}

/// The layout member an operator turns to make a sheet that did not fit,
/// fit. "The A0 failed" is not actionable; "the A0's `lv_schedule` needs
/// three panels and the layout permits one" is.
pub const KNOB_PANELS: &str = "panels";
pub const KNOB_OVERFLOW: &str = "overflow";
pub const KNOB_ROW_MM: &str = "row_mm";
/// The most panels `printing::table_rows::plan` will ever permit; at that
/// ceiling the knob is no longer `panels`.
const MAX_TABLE_PANELS: usize = 8;
/// The bound on an engine blocker's own words where a failed format repeats
/// them — the same bound [`blocker_summary`] applies.
const MAX_BLOCKER_MESSAGE: usize = 180;

/// Which layout member decides a print that did not fit, and on which
/// element. Read out of the message `printing::table_rows::plan` wrote, so
/// the knob is named by the code that authored the refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayoutKnob {
    /// The layout element the message named, when it named one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>,
    /// `panels`, `overflow` or `row_mm`.
    pub knob: &'static str,
    /// The measured shortfall, in the terms the layout is authored in.
    pub detail: String,
}

/// One format this run asked for and did not get, named precisely enough to
/// act on: the output id an operator selected, the engine's own typed code,
/// its bounded words, the remedy, and — for a print that did not fit — the
/// element and the layout knob that decides it.
///
/// This exists so a partial run can still be an answer. The formats that
/// completed are on disk and publishable; these are what is missing, and
/// nothing is left unaccounted between the two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedFormat {
    pub output_id: String,
    pub format: String,
    pub code: String,
    pub message: String,
    pub remedy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutKnob>,
}

impl FailedFormat {
    fn new(output: &Output, blocker: &Blocker) -> Self {
        let layout = layout_knob(&blocker.message);
        Self {
            remedy: remedy(&blocker.code, layout.as_ref()),
            output_id: output.output_id.clone(),
            format: output.format.clone(),
            code: bounded_words(&blocker.code),
            message: bounded_words(&blocker.message),
            layout,
        }
    }
}

fn bounded_words(message: &str) -> String {
    message
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_BLOCKER_MESSAGE)
        .collect()
}

/// What to do about one unbuilt format. A refusal that names no way forward
/// is a dead end, so every code answers here, layout knob or not.
fn remedy(code: &str, layout: Option<&LayoutKnob>) -> String {
    let element = layout
        .and_then(|knob| knob.element_id.as_deref())
        .map(|id| format!(" on `{id}`"))
        .unwrap_or_default();
    match layout.map(|knob| knob.knob) {
        Some(KNOB_PANELS) => format!(
            "raise `panels`{element} in this printing setup, or set its table `overflow` to `truncate_warn` to print the rows that fit with a truncation warning"
        ),
        Some(KNOB_OVERFLOW) => format!(
            "set the table `overflow`{element} to `truncate_warn`, or lower `row_mm` so more rows fit each panel: `panels` cannot rise for this table"
        ),
        Some(KNOB_ROW_MM) => format!(
            "lower `row_mm`{element}, or clear it for automatic spacing, so the rows fit the frame at the authored type size"
        ),
        _ => match code {
            "format_not_in_policy" => {
                "add the format to the project's export policy, or drop it from the selection"
                    .to_string()
            }
            "artifact_stage_failed" => {
                "export into a fresh output directory: the engine could not write this file where it was asked to"
                    .to_string()
            }
            "printing_execution_disabled" => {
                "render this format on a device with printing enabled; the formats that completed are already publishable"
                    .to_string()
            }
            _ => {
                "re-run this format alone once what the message names is fixed; the formats that completed are already publishable"
                    .to_string()
            }
        },
    }
}

/// The element id a table message named, reading back from `marker` to the
/// `table ` the message opened the name with.
fn table_element(message: &str, marker: &str) -> Option<String> {
    let head = &message[..message.find(marker)?];
    let id = &head[head.rfind("table ")? + "table ".len()..];
    crate::printing::identifier(id).then(|| id.to_string())
}

/// The count immediately before `suffix`.
fn count_before(message: &str, suffix: &str) -> Option<usize> {
    message[..message.find(suffix)?]
        .rsplit(' ')
        .next()?
        .parse()
        .ok()
}

/// The count immediately after `marker`, read digit by digit. The permitted
/// panel count is read this way and not from the first `)` in the message:
/// one bracket anywhere in a wrapping prefix would otherwise be read as the
/// ceiling, and an operator would be told to raise `panels` to a number the
/// layout already permits.
fn count_after(message: &str, marker: &str) -> Option<usize> {
    let tail = &message[message.find(marker)? + marker.len()..];
    let digits = tail
        .find(|character: char| !character.is_ascii_digit())
        .map_or(tail, |end| &tail[..end]);
    digits.parse().ok()
}

/// Read the layout knob out of a table message `printing::table_rows::plan`
/// wrote, wherever the engine wrapped it. `None` when the failure is not one
/// of the layout shortfalls this kernel raises — a caller then still has the
/// code, the message and a remedy, and claims no knob it cannot name.
pub fn layout_knob(message: &str) -> Option<LayoutKnob> {
    const PANELS: &str = " panels at the authored type size";
    const TYPE: &str = " row height cannot contain its readable type";
    const FRAME: &str = " frame ";
    if message.contains(PANELS) {
        let rows = count_before(message, " rows; needs ")?;
        let panels = count_before(message, PANELS)?;
        let permitted = count_after(message, "(maximum ")?;
        // A workbook table prints in one panel whatever the setup authors,
        // so `panels` is not the member that decides it. Naming `panels`
        // there sends an operator to re-run a whole batch against a number
        // that cannot move.
        let forced = message.contains(ONE_PANEL_PRESENTATION);
        let at_ceiling = permitted >= MAX_TABLE_PANELS;
        return Some(LayoutKnob {
            element_id: table_element(message, " has "),
            knob: if forced || at_ceiling {
                KNOB_OVERFLOW
            } else {
                KNOB_PANELS
            },
            detail: if forced {
                format!("{rows} rows need {panels} panels; {ONE_PANEL_PRESENTATION}")
            } else if at_ceiling {
                format!(
                    "{rows} rows need {panels} panels; the layout is already at its \
                     {permitted}-panel ceiling"
                )
            } else {
                format!("{rows} rows need {panels} panels; the layout permits {permitted}")
            },
        });
    }
    if message.contains(TYPE) {
        return Some(LayoutKnob {
            element_id: table_element(message, TYPE),
            knob: KNOB_ROW_MM,
            detail: "the authored row height is below the readable type size".to_string(),
        });
    }
    if message.contains("room for its title, header and one row")
        || message.contains("no room for data and a truncation warning")
    {
        return Some(LayoutKnob {
            // The frame refusals name their table too, so the remedy says
            // which table to lower. A message from an older engine that did
            // not name one still reads, and simply names no element.
            element_id: table_element(message, FRAME),
            knob: KNOB_ROW_MM,
            detail: if message.contains("no room for data and a truncation warning") {
                "the frame has no room for both a row of data and the truncation warning"
                    .to_string()
            } else {
                "the table frame has no room for a title, a header and one row".to_string()
            },
        });
    }
    None
}

/// A result that answers its plan: what completed, and what did not. The host
/// still has to prove the bytes on disk are the declared ones.
///
/// `failed` is empty on a complete run. When it is not, the artifacts are
/// still this run's answer — they are finished, correct and publishable — and
/// every format missing from them is named here with its reason. Computed
/// work that cannot leave the machine is the state the edge must never be in.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VerifiedResult {
    pub artifacts: Vec<VerifiedArtifact>,
    pub failed: Vec<FailedFormat>,
    pub warnings: Vec<PrintWarning>,
    pub print_diagnostics: Vec<Value>,
    pub elapsed_ms: u64,
    pub geometry_changed: bool,
    pub layers_decoded: usize,
    pub features_decoded: usize,
    pub admin_bounds: AdminBoundsEvidence,
}

/// Why a result document is not an answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResultRefusal {
    /// The engine ran and refused, or completed only part of the request;
    /// its own typed blockers say which format and why.
    Blocked {
        status: String,
        blockers: Vec<Blocker>,
        artifacts: usize,
    },
    /// The document is not this request's answer: malformed, mismatched, or
    /// declaring something the contract does not allow.
    Invalid { message: String },
}

impl ResultRefusal {
    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid {
            message: message.into(),
        }
    }
}

/// A bounded, human-readable summary of the first blockers.
pub fn blocker_summary(blockers: &[Blocker]) -> String {
    let bounded = |message: &str| {
        message
            .chars()
            .filter(|character| !character.is_control())
            .take(180)
            .collect::<String>()
    };
    blockers
        .iter()
        .take(3)
        .map(|blocker| {
            let format = blocker
                .format
                .as_deref()
                .map(|format| format!("/{format}"))
                .unwrap_or_default();
            format!(
                "{}{format}: {}",
                bounded(&blocker.code),
                bounded(&blocker.message)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The filename the engine must have given an artifact of this format.
pub fn valid_artifact_filename(filename: &str, format: &str) -> bool {
    if filename.is_empty()
        || filename.len() > 255
        || filename
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | b'\0'))
        || filename.chars().any(char::is_control)
        || filename == "."
        || filename == ".."
    {
        return false;
    }
    let path = Path::new(filename);
    if path.file_name().and_then(|name| name.to_str()) != Some(filename)
        || path.components().count() != 1
    {
        return false;
    }
    match format {
        "shp" => filename.ends_with(".shp.zip"),
        "kmz" => filename.ends_with(".kmz"),
        "xlsx" => filename.ends_with(".xlsx"),
        "network_information" => filename.ends_with(".network-information.json"),
        "geojsonseq" => filename.ends_with("_geojsonl.zip"),
        "gpkg" => filename.ends_with(".gpkg"),
        "pdf_a0" => filename.ends_with(".a0.pdf"),
        "pdf_a3" => filename.ends_with(".a3.pdf"),
        format => match report_formats::output(format) {
            Ok(output) if report_formats::named_print_output(format).is_some() => {
                filename.ends_with(&output.suffix)
            }
            _ => false,
        },
    }
}

fn content_type(format: &str) -> Result<&'static str, ResultRefusal> {
    report_formats::output(format)
        .map(|output| output.content_type)
        .map_err(ResultRefusal::invalid)
}

fn parse_result(document: &[u8]) -> Result<ResultDocument, ResultRefusal> {
    if document.len() as u64 > MAX_RESULT_BYTES {
        return Err(ResultRefusal::invalid(
            "Network Reporter result exceeds its byte bound",
        ));
    }
    serde_json::from_slice(document).map_err(|error| {
        ResultRefusal::invalid(format!("Network Reporter result is malformed: {error}"))
    })
}

/// Read the engine's typed refusal out of a result it wrote before exiting
/// non-zero. `None` when the document is absent, malformed, or not this
/// request's — the host then reports the process failure itself.
pub fn typed_failure(document: &[u8], plan: &ExportPlan) -> Option<ResultRefusal> {
    let result = parse_result(document).ok()?;
    if result.task != REPORT_TASK
        || result.transformer != plan.transformer
        || result.country != plan.country
        || (result.status == "completed" && result.blockers.is_empty())
    {
        return None;
    }
    Some(ResultRefusal::Blocked {
        status: result.status,
        blockers: result.blockers,
        artifacts: result.artifacts.len(),
    })
}

/// Prove one result document answers the plan: the same task, transformer
/// and country; at least one format built; every declared artifact a direct
/// child of the staged output directory with a plausible name, size and
/// digest; every requested format either built or named as failed;
/// administrative evidence consistent with the country; and only the print
/// warnings the kernel's tables can raise.
///
/// A run that built some of its formats is an answer, not a refusal. The
/// artifacts it did build are finished work: they must be able to leave the
/// machine — verified, receipted, publishable — while the formats it did not
/// build are named with the knob that would fix them. Only a run that built
/// nothing is [`ResultRefusal::Blocked`].
pub fn verify_result(
    document: &[u8],
    plan: &ExportPlan,
    out_dir: &Path,
) -> Result<VerifiedResult, ResultRefusal> {
    let result = parse_result(document)?;
    if result.task != REPORT_TASK
        || result.transformer != plan.transformer
        || result.country != plan.country
    {
        return Err(ResultRefusal::invalid(
            "Network Reporter result does not match its semantic request",
        ));
    }
    let partial = match result.status.as_str() {
        "completed" => false,
        "partial" => true,
        // `failed`, and any status this contract does not know, produced
        // nothing to keep.
        _ => {
            return Err(ResultRefusal::Blocked {
                status: result.status,
                blockers: result.blockers,
                artifacts: result.artifacts.len(),
            });
        }
    };
    if result.artifacts.is_empty() {
        return Err(ResultRefusal::Blocked {
            status: result.status,
            blockers: result.blockers,
            artifacts: 0,
        });
    }
    if partial == result.blockers.is_empty() {
        return Err(ResultRefusal::invalid(
            "Network Reporter result status does not agree with its blockers",
        ));
    }
    // What the engine says it was asked for is this plan's formats, in this
    // plan's order — possibly fewer, when the project's policy dropped one,
    // and that drop is a blocker below.
    let mut requested = 0_usize;
    for format in &result.formats_requested {
        match plan.formats[requested..].iter().position(|f| f == format) {
            Some(offset) => requested += offset + 1,
            None => {
                return Err(ResultRefusal::invalid(
                    "Network Reporter did not answer every requested format in the plan's order",
                ));
            }
        }
    }
    if result.layers_decoded == 0 || result.features_decoded == 0 {
        return Err(ResultRefusal::invalid(
            "Network Reporter result has no decoded local input",
        ));
    }
    if result.admin_bounds.features_matched > result.admin_bounds.features_requested {
        return Err(ResultRefusal::invalid(
            "Network Reporter admin-bounds evidence is inconsistent",
        ));
    }
    if plan.requires_admin_bounds {
        if !result.admin_bounds.configured || result.admin_bounds.villages_loaded == 0 {
            return Err(ResultRefusal::invalid(
                "Network Reporter did not use the verified Rwanda reference asset",
            ));
        }
    } else if result.admin_bounds.configured
        || result.admin_bounds.villages_loaded != 0
        || result.admin_bounds.features_requested != 0
        || result.admin_bounds.features_matched != 0
    {
        return Err(ResultRefusal::invalid(
            "Network Reporter performed Rwanda administrative-bound work for another country",
        ));
    }
    let mut filenames = HashSet::new();
    let mut artifacts = Vec::with_capacity(result.artifacts.len());
    // Artifacts are matched to the outputs they answer BY FORMAT, walking the
    // plan forward. A format the engine did not build shifts nothing: the
    // sheets that did build are still matched to their own outputs.
    let mut position = 0_usize;
    for artifact in result.artifacts.into_iter() {
        let Some(offset) = plan.formats[position..]
            .iter()
            .position(|format| *format == artifact.format)
        else {
            return Err(ResultRefusal::invalid(
                "Network Reporter returned an artifact this run did not request, or out of the plan's order",
            ));
        };
        let output = &plan.outputs[position + offset];
        position += offset + 1;
        if !result
            .formats_requested
            .iter()
            .any(|format| *format == artifact.format)
        {
            return Err(ResultRefusal::invalid(
                "Network Reporter built a format it did not declare as requested",
            ));
        }
        if content_type(&artifact.format)? != output.content_type
            || !valid_artifact_filename(&artifact.filename, &artifact.format)
            || !filenames.insert(artifact.filename.clone())
            || artifact.bytes == 0
            || artifact.bytes > MAX_ARTIFACT_BYTES
            || !exact_lower_sha256(&artifact.sha256)
        {
            return Err(ResultRefusal::invalid(
                "Network Reporter returned an invalid artifact declaration",
            ));
        }
        if Path::new(&artifact.path) != out_dir.join(&artifact.filename) {
            return Err(ResultRefusal::invalid(
                "Network Reporter artifact is not the declared direct output child",
            ));
        }
        crate::printing::validate_artifact_presentation(
            &artifact.format,
            artifact.paper_size.as_deref(),
            artifact.presentation.as_ref(),
        )
        .map_err(ResultRefusal::invalid)?;
        artifacts.push(VerifiedArtifact {
            output_id: output.output_id.clone(),
            format: artifact.format,
            filename: artifact.filename,
            content_type: output.content_type,
            size_bytes: artifact.bytes,
            sha256: artifact.sha256,
            paper_size: artifact.paper_size,
            presentation: artifact.presentation,
        });
    }
    // Nothing between "built" and "failed" is left unsaid: every blocker
    // names one distinct format of this run that produced no artifact, and
    // every requested format that produced no artifact has a blocker.
    let built = artifacts
        .iter()
        .map(|artifact| artifact.format.as_str())
        .collect::<HashSet<_>>();
    let mut blocked = BTreeMap::new();
    for blocker in &result.blockers {
        let named = blocker.format.as_deref().filter(|format| {
            plan.formats.iter().any(|planned| planned == format) && !built.contains(format)
        });
        match named {
            Some(format) if blocked.insert(format, blocker).is_none() => {}
            _ => {
                return Err(ResultRefusal::invalid(
                    "Network Reporter blocker does not name one distinct unbuilt format of this run",
                ));
            }
        }
    }
    let failed = plan
        .formats
        .iter()
        .zip(plan.outputs.iter())
        .filter(|(format, _)| !built.contains(format.as_str()))
        .map(|(format, output)| {
            blocked
                .get(format.as_str())
                .map(|blocker| FailedFormat::new(output, blocker))
                .ok_or_else(|| {
                    ResultRefusal::invalid(
                        "Network Reporter did not complete every requested format",
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if result.warnings.len() > MAX_PRINT_WARNINGS
        || result
            .warnings
            .iter()
            .any(|warning| !valid_print_warning(warning, &plan.transformer, &plan.formats))
    {
        return Err(ResultRefusal::invalid(
            "Network Reporter returned invalid print warnings",
        ));
    }
    if result.print_diagnostics.len() > 64
        || result.print_diagnostics.iter().any(|assessment| {
            !assessment.is_object()
                || assessment["output_id"]
                    .as_str()
                    .is_none_or(|id| !plan.formats.iter().any(|f| f == id))
                || assessment["findings"]
                    .as_array()
                    .is_none_or(|f| f.len() > 200)
        })
    {
        return Err(ResultRefusal::invalid(
            "Network Reporter returned invalid print diagnostics",
        ));
    }
    // The host and the engine read the same configuration bytes. If they do
    // not name the same omitted survey forms, one of them is reading another
    // project's policy and the sheet's declared contents cannot be trusted.
    let planned = plan
        .survey_layers_omitted
        .as_ref()
        .map_or(&[][..], |omitted| omitted.forms.as_slice());
    if !same_survey_forms(planned, &result.survey_layers_omitted) {
        return Err(ResultRefusal::invalid(
            "Network Reporter named different omitted survey layers than this run planned",
        ));
    }
    Ok(VerifiedResult {
        artifacts,
        failed,
        warnings: result.warnings,
        print_diagnostics: result.print_diagnostics,
        elapsed_ms: result.elapsed_ms,
        geometry_changed: result.geometry_changed,
        layers_decoded: result.layers_decoded,
        features_decoded: result.features_decoded,
        admin_bounds: result.admin_bounds,
    })
}

/// The warnings that are engineering findings. Each is a row on the workbook's
/// Engineering Review sheet, and a report that carries one was generated but is
/// not ready for approval. Every other print warning is cartographic and never
/// blocks a submission.
pub const ENGINEERING_REVIEW_CODES: [&str; 3] = [
    "lv_pole_line_cable_conflict",
    "transformer_protection_mismatch",
    "lv_span_exceeds_max_length",
];

/// Is this one of the print warnings the kernel's own tables and diagnostics
/// can raise, placed on this run? Every host that reads an engine result
/// asks here, so the desktop shell and `ds` accept the same warnings.
pub fn valid_print_warning(warning: &PrintWarning, transformer: &str, formats: &[String]) -> bool {
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
        // Cable consistency findings (report_consistency::lv_cables):
        // one row per pole whose saved cable label disagrees with its
        // LV line, mirrored on the workbook's Engineering Review
        // sheet. They ride the xlsx output and name no paper; the
        // transformer is the saved data label, which differs from the
        // run's name only by case; the element is the saved line
        // number. No table rows stand behind them.
        "lv_pole_line_cable_conflict" => {
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
        _ => false,
    };
    valid
}

// ---------------------------------------------------------------------------
// Receipts
// ---------------------------------------------------------------------------

/// The facts a run receipt is written from. `artifact_dir` is the path the
/// host placed the artifacts under, relative to the batch root; `now_ms` is
/// the host's clock.
#[derive(Debug, Clone)]
pub struct RunReceiptFacts<'a> {
    pub plan: &'a ExportPlan,
    pub client_run_id: &'a str,
    pub engine: &'a EngineIdentity,
    pub result: &'a VerifiedResult,
    pub artifact_dir: &'a str,
    pub now_ms: u64,
    /// ds-brain's `metadata.content_digest` for the fetched room, when the
    /// host had one to compare against.
    pub expected_content_digest: Option<&'a str>,
    /// The digest of the `ds.print-context/v1` document staged beside the
    /// room (`print_context_document`), or `None` when the run printed
    /// without external context.
    pub print_context_sha256: Option<&'a str>,
    /// The layer ids that document carried, in document order.
    pub print_context_layers: &'a [String],
    /// The selected context ids the host could not supply on this run,
    /// each named so a receipt never reads as complete when it is not.
    pub print_context_omitted: &'a [String],
}

/// The publication identity this run would be enqueued under — the same
/// members the desktop shell's batch receipt carries, minus the local
/// artifact locators a pending-upload store mints.
pub fn publication_facts(plan: &ExportPlan, engine: &EngineIdentity) -> Result<Value, String> {
    let state = if plan.local_print_recipe.is_some() {
        PublicationState::LocalOnly
    } else {
        engine.publication_state()?
    };
    Ok(json!({
        "state": state,
        "schema": if state == PublicationState::LocalOnly { LOCAL_ONLY_SCHEMA } else { PUBLICATION_SCHEMA },
        "engine": ENGINE,
        "operation": format!("export-{}", plan.transformer),
        "variant": VARIANT,
        "root": format!("eds_project/{}/eds_design", plan.project_id),
        "engine_version": engine.engine_version,
        "engine_build_manifest_sha256": engine.build_manifest_sha256,
        "transformer": plan.transformer,
        "transformer_revision": plan.server_version,
        "input_base_fingerprint": plan.input_base_fingerprint,
        "room_content_sha256": plan.room_content_sha256,
    }))
}

/// Refuse a room that does not match the governing service's saved content.
pub fn verify_content_digest(plan: &ExportPlan, expected: Option<&str>) -> Result<(), String> {
    if expected.is_some_and(|digest| digest != plan.content_digest) {
        return Err("saved transformer content digest does not match the fetched layers".into());
    }
    Ok(())
}

/// The receipt one run leaves beside its artifacts.
pub fn run_receipt(facts: &RunReceiptFacts<'_>) -> Result<Value, String> {
    if !valid_run_id(facts.client_run_id) {
        return Err("Network Reporter run identity is invalid".to_string());
    }
    verify_content_digest(facts.plan, facts.expected_content_digest)?;
    match facts.print_context_sha256 {
        Some(digest) if !exact_lower_sha256(digest) => {
            return Err("Network Reporter print context digest is invalid".to_string());
        }
        Some(_) if facts.print_context_layers.is_empty() => {
            return Err("Network Reporter print context digest names no layers".to_string());
        }
        None if !facts.print_context_layers.is_empty() => {
            return Err("Network Reporter print context layers have no digest".to_string());
        }
        _ => {}
    }
    let mut context_ids = HashSet::new();
    for id in facts
        .print_context_layers
        .iter()
        .chain(facts.print_context_omitted)
    {
        if !crate::printing::identifier(id) || !context_ids.insert(id.as_str()) {
            return Err(format!(
                "Network Reporter print context names an invalid or repeated id {id:?}"
            ));
        }
    }
    let content_digest_verified = facts
        .expected_content_digest
        .map(|expected| expected == facts.plan.content_digest);
    let outputs = facts
        .result
        .artifacts
        .iter()
        .map(|artifact| {
            let mut output = json!({
                "output_id": artifact.output_id,
                "format": artifact.format,
                "filename": artifact.filename,
                "path": format!("{}/{}", facts.artifact_dir, artifact.filename),
                "content_type": artifact.content_type,
                "sha256": artifact.sha256,
                "size_bytes": artifact.size_bytes,
            });
            if let Some(paper) = &artifact.paper_size {
                output["paper_size"] = json!(paper);
            }
            if let Some(presentation) = &artifact.presentation {
                output["presentation"] = json!(presentation);
            }
            output
        })
        .collect::<Vec<_>>();
    let mut receipt = json!({
        "schema": RUN_RECEIPT_SCHEMA,
        "task": REPORT_TASK,
        // A run that built some of its formats says so here, beside the
        // formats it did not build. A reader of this receipt can tell what
        // is publishable from what is missing without opening the directory.
        "status": if facts.result.failed.is_empty() { "completed" } else { "partial" },
        "project_id": facts.plan.project_id,
        "transformer": facts.plan.transformer,
        "transformer_revision": facts.plan.server_version,
        "country": facts.plan.country,
        "client_run_id": facts.client_run_id,
        "produced_at_ms": facts.now_ms,
        "engine_version": facts.engine.engine_version,
        "engine_build_manifest_sha256": facts.engine.build_manifest_sha256,
        "input_base_fingerprint": facts.plan.input_base_fingerprint,
        "room_content_sha256": facts.plan.room_content_sha256,
        "content_digest": facts.plan.content_digest,
        "content_digest_verified": content_digest_verified,
        "sheets_sha256": facts.plan.sheets_sha256,
        "local_print_recipe": facts.plan.local_print_recipe,
        "reference_semantic_sha256": facts.plan.reference_semantic_sha256,
        "room": facts.plan.room,
        "print_context_sha256": facts.print_context_sha256,
        "print_context_layers": facts.print_context_layers,
        "print_context_omitted": facts.print_context_omitted,
        "formats_requested": facts.plan.formats,
        "outputs": outputs,
        "failed_formats": facts.result.failed,
        "warnings": facts.result.warnings,
        "print_diagnostics": facts.result.print_diagnostics,
        "admin_bounds": facts.result.admin_bounds,
        "layers_decoded": facts.result.layers_decoded,
        "features_decoded": facts.result.features_decoded,
        "geometry_changed": facts.result.geometry_changed,
        "elapsed_ms": facts.result.elapsed_ms,
        "publication": publication_facts(facts.plan, facts.engine)?,
    });
    // A no-omission receipt keeps the exact preexisting shape.
    if let Some(omitted) = &facts.plan.survey_layers_omitted {
        receipt["survey_layers_omitted"] = json!(omitted);
    }
    Ok(receipt)
}

/// A run identity a host minted: `report-` plus 32 lowercase hex characters.
pub fn valid_run_id(value: &str) -> bool {
    value.strip_prefix("report-").is_some_and(|hex| {
        hex.len() == 32
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

/// One transformer's outcome inside a batch.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BatchResult {
    pub transformer: String,
    /// `ok` or `error` — the batch-outcome vocabulary every host folds.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<usize>,
    /// The formats this run asked for and did not get. Absent when it got
    /// them all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_formats: Option<Vec<FailedFormat>>,
    /// The survey forms this run's sheet was produced without. Absent when
    /// the project appends none, so a row that omits nothing reads exactly as
    /// it always did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub survey_layers_omitted: Option<SurveyLayersOmitted>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

impl BatchResult {
    pub fn ok(transformer: &str, receipt: String, artifacts: usize) -> Self {
        Self {
            transformer: transformer.to_string(),
            status: "ok",
            receipt: Some(receipt),
            artifacts: Some(artifacts),
            failed_formats: None,
            survey_layers_omitted: None,
            error: None,
        }
    }

    /// Name the survey forms this otherwise-complete run's sheet does not
    /// carry. The row stays `ok` — the artifacts are on disk and
    /// publishable — and the batch never reads as carrying survey layers it
    /// never had.
    pub fn with_survey_layers_omitted(mut self, omitted: Option<SurveyLayersOmitted>) -> Self {
        self.survey_layers_omitted = omitted;
        self
    }

    /// Name what one otherwise-successful run did not build. The row stays
    /// `ok` — its artifacts are on disk, receipted and publishable — and a
    /// batch never reads as complete when part of a sheet set is missing.
    pub fn with_failed_formats(mut self, failed: Vec<FailedFormat>) -> Self {
        self.failed_formats = (!failed.is_empty()).then_some(failed);
        self
    }

    pub fn error(transformer: &str, code: &str, message: &str, detail: Option<Value>) -> Self {
        let mut error = json!({"code": code, "message": message});
        if let Some(detail) = detail {
            error["detail"] = detail;
        }
        Self {
            transformer: transformer.to_string(),
            status: "error",
            receipt: None,
            artifacts: None,
            failed_formats: None,
            survey_layers_omitted: None,
            error: Some(error),
        }
    }
}

/// The batch's own status: every run completed, some did, or none did.
pub fn batch_status(results: &[BatchResult]) -> &'static str {
    let failed = results.iter().filter(|r| r.status == "error").count();
    if results.is_empty() || failed == results.len() {
        "failed"
    } else if failed == 0 {
        "completed"
    } else {
        "partial"
    }
}

/// The receipt a batch leaves at the root of its output directory, results
/// in the kernel's stable order.
pub fn batch_receipt(
    project_id: &str,
    lane: &str,
    engine: Option<&EngineIdentity>,
    concurrency: usize,
    requested: &[String],
    results: &[BatchResult],
    now_ms: u64,
) -> Result<Value, String> {
    let rows = results
        .iter()
        .map(|result| serde_json::to_value(result).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let request = json!({"command": "batch_outcome", "results": rows});
    let reply = crate::report::evaluate(&serde_json::to_vec(&request).map_err(|e| e.to_string())?)?;
    let reply: Value = serde_json::from_str(&reply).map_err(|error| error.to_string())?;
    let ordered = reply["result"]["results"].clone();
    let failed = reply["result"]["failed"].as_u64().unwrap_or(0);
    let mut receipt = json!({
        "schema": BATCH_RECEIPT_SCHEMA,
        "task": REPORT_TASK,
        "status": batch_status(results),
        "project_id": project_id,
        "lane": lane,
        "produced_at_ms": now_ms,
        "engine_version": engine.map(|engine| engine.engine_version.clone()),
        "engine_build_manifest_sha256": engine.map(|engine| engine.build_manifest_sha256.clone()),
        "publication_state": engine.map(|engine| engine.publication_state()).transpose()?,
        "concurrency": concurrency,
        "requested": requested,
        "requested_count": requested.len(),
        "completed": results.len() as u64 - failed,
        "failed": failed,
        // Runs that delivered artifacts and still lost a format. They count
        // as completed — their work is publishable — and the batch says how
        // many of them a reader must look into.
        "partial_formats": results
            .iter()
            .filter(|result| result.failed_formats.is_some())
            .count(),
        "results": ordered,
    });
    // Union the named omissions once for the batch reader.
    if let Some(omitted) = SurveyLayersOmitted::union(
        results
            .iter()
            .filter_map(|result| result.survey_layers_omitted.as_ref()),
    ) {
        receipt["survey_layers_omitted"] = json!(omitted);
    }
    Ok(receipt)
}

/// The names a batch runs, in the kernel's order, and how many engines it may
/// keep resident: `min(scope, processors, resident_limit)` — the same
/// decision the browser batch runner asked `report::plan` for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BatchPlan {
    pub names: Vec<String>,
    pub concurrency: usize,
}

pub fn batch_plan(
    requested: &[String],
    processors: usize,
    resident_limit: usize,
) -> Result<BatchPlan, String> {
    if requested.is_empty() {
        return Err("a report batch names at least one transformer".to_string());
    }
    if requested.len() > MAX_BATCH_TRANSFORMERS {
        return Err(format!(
            "a report batch names at most {MAX_BATCH_TRANSFORMERS} transformers"
        ));
    }
    for name in requested {
        reportable_transformer(name)?;
    }
    let rooms = requested
        .iter()
        .map(|name| json!({"name": name, "available": true, "dirty": false}))
        .collect::<Vec<_>>();
    let request = json!({
        "command": "plan",
        "requested": requested,
        "inventory": Value::Null,
        "rooms": rooms,
        "processors": processors,
        "resident_limit": resident_limit,
    });
    let reply = crate::report::evaluate(&serde_json::to_vec(&request).map_err(|e| e.to_string())?)?;
    let reply: Value = serde_json::from_str(&reply).map_err(|error| error.to_string())?;
    let names = reply["result"]["names"]
        .as_array()
        .ok_or("report plan returned no names")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let concurrency = reply["result"]["concurrency"]
        .as_u64()
        .ok_or("report plan returned no concurrency")? as usize;
    Ok(BatchPlan { names, concurrency })
}

/// Stable one-based sheet positions across a complete active project inventory.
/// A selected export looks up its members here; it never renumbers its subset.
pub fn drawing_set_positions(active: &[String]) -> Result<BTreeMap<String, (u32, u32)>, String> {
    let mut names = batch_plan(active, 1, 1)?.names;
    names.sort();
    let count = u32::try_from(names.len()).map_err(|_| "drawing set is too large")?;
    Ok(names
        .into_iter()
        .enumerate()
        .map(|(index, name)| (name, (index as u32 + 1, count)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn selected_sheets_retain_positions_in_the_complete_drawing_set() {
        let active = ["zihare", "agahehe", "kibungo"].map(str::to_string);
        let positions = drawing_set_positions(&active).unwrap();
        assert_eq!(positions["agahehe"], (1, 3));
        assert_eq!(positions["kibungo"], (2, 3));
        assert_eq!(positions["zihare"], (3, 3));
        assert!(!positions.contains_key("retired"));
        assert_eq!(
            drawing_set_positions(&active.iter().rev().cloned().collect::<Vec<_>>()).unwrap(),
            positions
        );
        assert!(drawing_set_positions(&[]).is_err());
        assert!(drawing_set_positions(&["combined_transformer".into()]).is_err());
    }

    const SHEETS: &str = r#"{"project_settings":[{"parameter":"design_export_format","value":["shp","xlsx","pdf__detail"]}],"printing_setups":[]}"#;

    fn print_receipt(country: &str) -> InputReceipt {
        InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: country.to_string(),
            sheets_json: SHEETS.to_string(),
            sheets_sha256: format!("{:x}", Sha256::digest(SHEETS.as_bytes())),
            reference_semantic_sha256:
                "70f0de17599e81d8d85809668cb172ee6ccf68c1edecac6e3709838ab0eb50c1".to_string(),
        }
    }

    fn simple_receipt(country: &str) -> InputReceipt {
        let sheets =
            r#"{"project_settings":[{"parameter":"design_export_format","value":["shp","xlsx"]}]}"#;
        InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: country.to_string(),
            sheets_json: sheets.to_string(),
            sheets_sha256: format!("{:x}", Sha256::digest(sheets.as_bytes())),
            reference_semantic_sha256:
                "70f0de17599e81d8d85809668cb172ee6ccf68c1edecac6e3709838ab0eb50c1".to_string(),
        }
    }

    #[test]
    fn survey_omission_is_verified_and_named_only_when_policy_appends_forms() {
        let mut receipt = simple_receipt("Benin");
        receipt.sheets_json = r#"{"project_settings":[{"parameter":"design_export_format","value":["shp","xlsx"]},{"parameter":"include_survey_data_in_transformer_exports","value":true},{"parameter":"survey_forms_in_transformer_exports","value":["field_notes","transformer_survey"]}]}"#.into();
        receipt.sheets_sha256 = format!("{:x}", Sha256::digest(receipt.sheets_json.as_bytes()));
        let plan = plan_transformer_export("proj-1", &receipt, "tx_a", 7, &layers(), None).unwrap();
        let omitted = plan.survey_layers_omitted.as_ref().unwrap();
        assert_eq!(omitted.code, SURVEY_LAYERS_OMITTED);
        assert_eq!(omitted.context_omitted.len(), 2);
        assert_eq!(omitted.context_omitted[0].layer, "field_notes");

        let out_dir = PathBuf::from("/tmp/survey-omission/tx_a");
        let staged = StagedInputs {
            transformer_document: Path::new("/tmp/survey-omission/room.json"),
            network_config: Path::new("/tmp/survey-omission/config.json"),
            out_dir: &out_dir,
            admin_bounds: None,
            media_grant: None,
            print_context: None,
            sheet: None,
        };
        assert_eq!(
            engine_request(&plan, &staged).unwrap()["survey_layers_omitted"],
            json!(["field_notes", "transformer_survey"])
        );
        let mut document = result_document(&plan, &out_dir);
        assert!(verify_result(&serde_json::to_vec(&document).unwrap(), &plan, &out_dir).is_err());
        document["survey_layers_omitted"] = json!(["field_notes", "transformer_survey"]);
        let verified =
            verify_result(&serde_json::to_vec(&document).unwrap(), &plan, &out_dir).unwrap();
        let run = run_receipt(&RunReceiptFacts {
            plan: &plan,
            client_run_id: &format!("report-{}", "a".repeat(32)),
            engine: &engine(),
            result: &verified,
            artifact_dir: "tx_a",
            now_ms: 1,
            expected_content_digest: None,
            print_context_sha256: None,
            print_context_layers: &[],
            print_context_omitted: &[],
        })
        .unwrap();
        assert_eq!(
            run["survey_layers_omitted"]["context_omitted"][1]["layer"],
            "transformer_survey"
        );
        let row = BatchResult::ok("tx_a", "tx_a/report-run.json".into(), 2)
            .with_survey_layers_omitted(plan.survey_layers_omitted.clone());
        let batch = batch_receipt(
            "proj-1",
            "stable",
            Some(&engine()),
            1,
            &["tx_a".into()],
            &[row],
            1,
        )
        .unwrap();
        assert_eq!(batch["status"], "completed");
        assert_eq!(
            batch["survey_layers_omitted"]["code"],
            SURVEY_LAYERS_OMITTED
        );
        assert_eq!(
            batch["results"][0]["survey_layers_omitted"]["forms"],
            json!(["field_notes", "transformer_survey"])
        );

        let bare = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        assert!(bare.survey_layers_omitted.is_none());
        assert!(engine_request(&bare, &staged)
            .unwrap()
            .get("survey_layers_omitted")
            .is_none());
        let bare_result = verify_result(
            &serde_json::to_vec(&result_document(&bare, &out_dir)).unwrap(),
            &bare,
            &out_dir,
        )
        .unwrap();
        let bare_run = run_receipt(&RunReceiptFacts {
            plan: &bare,
            client_run_id: &format!("report-{}", "a".repeat(32)),
            engine: &engine(),
            result: &bare_result,
            artifact_dir: "tx_a",
            now_ms: 1,
            expected_content_digest: None,
            print_context_sha256: None,
            print_context_layers: &[],
            print_context_omitted: &[],
        })
        .unwrap();
        assert!(bare_run.get("survey_layers_omitted").is_none());
        let bare_batch = batch_receipt(
            "proj-1",
            "stable",
            Some(&engine()),
            1,
            &["tx_a".into()],
            &[BatchResult::ok("tx_a", "tx_a/report-run.json".into(), 2)],
            1,
        )
        .unwrap();
        assert!(bare_batch.get("survey_layers_omitted").is_none());
    }

    fn layers() -> BTreeMap<String, Value> {
        let mut layers = BTreeMap::new();
        layers.insert(
            "lv_poles".to_string(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{"pole_number":"P1"},"geometry":{"type":"Point","coordinates":[30.06,-1.94]}}]}),
        );
        layers
    }

    fn engine() -> EngineIdentity {
        EngineIdentity {
            engine_version: format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
            build_manifest_sha256: "b".repeat(64),
            package_version: "0.1.0".into(),
            source_sha: "a".repeat(40),
            profile: "release".into(),
        }
    }

    #[test]
    fn the_fingerprint_matches_the_desktop_shell_and_ds_brain() {
        assert_eq!(
            input_base_fingerprint(
                "_001_main",
                7,
                "da506c8a9c8a9f31aa00eaeef23d49764b9ace97158a1a0a7aa628e6d446b0fb",
                "Rwanda",
                "70f0de17599e81d8d85809668cb172ee6ccf68c1edecac6e3709838ab0eb50c1",
            )
            .unwrap(),
            "43b3f123c37d0576a8fbb4bc2833e9e57e7c5abee1ec2389e792f397e28b91b3"
        );
        assert!(input_base_fingerprint(
            "Not Canonical",
            7,
            &"a".repeat(64),
            "Rwanda",
            &"b".repeat(64)
        )
        .is_err());
        assert!(
            input_base_fingerprint("tx_a", -1, &"a".repeat(64), "Rwanda", &"b".repeat(64)).is_err()
        );
        assert!(input_base_fingerprint("tx_a", 1, "short", "Rwanda", &"b".repeat(64)).is_err());
    }

    #[test]
    fn transformer_identities_are_the_canonical_publishable_ones() {
        for ok in ["tx_a", "_001_main", "agatare", "combined_transformer"] {
            assert!(valid_transformer_identity(ok), "{ok}");
        }
        for bad in [
            "",
            "TX_A",
            "tx a",
            "001_main",
            "_",
            "all_transformers",
            "tx-a",
            &"a".repeat(122),
        ] {
            assert!(!valid_transformer_identity(bad), "{bad:?}");
        }
        // Publishable but reserved: what a report produces or the project holds.
        for reserved in [
            "combined_transformer",
            "collisions",
            "mv_data",
            "compounded_report",
        ] {
            assert!(reportable_transformer(reserved).is_err(), "{reserved}");
        }
        assert!(reportable_transformer("tx_a").is_ok());
    }

    #[test]
    fn the_receipt_is_read_from_the_configuration_and_proves_its_sheets() {
        let receipt = print_receipt("Rwanda");
        let document = json!({"sheets": {}, "network_reporter_input_receipt": receipt});
        assert_eq!(InputReceipt::from_config(&document).unwrap(), receipt);
        assert!(receipt.requires_admin_bounds());
        assert!(!simple_receipt("Benin").requires_admin_bounds());
        assert!(InputReceipt::from_config(&json!({"sheets": {}}))
            .unwrap_err()
            .contains("network_reporter_input_receipt"));
        let mut wrong = receipt.clone();
        wrong.sheets_sha256 = "1".repeat(64);
        assert!(wrong.validate().unwrap_err().contains("does not match"));
        let mut unknown = serde_json::to_value(&receipt).unwrap();
        unknown["extra"] = json!(1);
        assert!(
            InputReceipt::from_config(&json!({"network_reporter_input_receipt": unknown})).is_err()
        );
        let mut schema = receipt.clone();
        schema.schema = 2;
        assert!(schema.validate().is_err());
    }

    /// cd193b7d: the config came back without the member and every host said
    /// "refresh the project configuration", which refreshes nothing when the
    /// server could not mint it. The server's refusal now travels with the
    /// document, and the kernel's refusal repeats it: reason, detail, remedy.
    #[test]
    fn a_missing_receipt_is_refused_with_the_servers_reason_and_remedy() {
        let refused = json!({"sheets": {}, "network_reporter_input_receipt_refusal": {
            "reason_key": "printing_style_unavailable",
            "detail": "the selected printing setup a0-northern-hub binds print style gt/rwanda_villages_print, which the governed style catalogue does not hold",
            "remedy": "rebind or drop the layers bound to the missing styles (`ds report layout style-ref`), then read `ds report project settings` again",
            "missing_print_styles": ["gt/rwanda_villages_print"],
            "printing_setups": ["a0-northern-hub"],
        }});
        let error = InputReceipt::from_config(&refused).unwrap_err();
        for needle in [
            "network_reporter_input_receipt",
            "printing_style_unavailable",
            "gt/rwanda_villages_print",
            "a0-northern-hub",
            "ds report layout style-ref",
        ] {
            assert!(error.contains(needle), "{error}");
        }
        assert!(
            !error.contains("refresh the project configuration"),
            "{error}"
        );
        let refusal = InputReceipt::refusal_from_config(&refused).unwrap();
        assert_eq!(refusal.missing_print_styles, ["gt/rwanda_villages_print"]);
        assert_eq!(refusal.printing_setups, ["a0-northern-hub"]);

        let readiness = InputReceipt::readiness(&refused);
        assert!(!readiness.ready);
        assert_eq!(readiness.issue.as_deref(), Some(error.as_str()));
        let rendered = readiness.refusal.unwrap();
        assert_eq!(rendered["code"], "report_input_receipt_unavailable");
        assert_eq!(rendered["message_key"], "printing_style_unavailable");
        assert_eq!(rendered["reason_key"], "printing_style_unavailable");
        assert_eq!(
            rendered["missing_print_styles"][0],
            "gt/rwanda_villages_print"
        );
        assert_eq!(rendered["printing_setups"][0], "a0-northern-hub");
        assert!(rendered["remedy"]
            .as_str()
            .unwrap()
            .contains("ds report project settings"));

        // No member and no refusal: still refused, still actionable, and the
        // remedy names the read that re-fetches rather than a phantom refresh.
        let bare = InputReceipt::readiness(&json!({"sheets": {}}));
        assert!(!bare.ready);
        let rendered = bare.refusal.unwrap();
        assert_eq!(rendered["message_key"], "report_input_receipt_unavailable");
        assert!(rendered.get("reason_key").is_none());
        assert!(bare.issue.unwrap().contains("ds report project settings"));
        // An unreadable refusal member never hides the absence.
        let junk =
            json!({"sheets": {}, "network_reporter_input_receipt_refusal": {"reason_key": " "}});
        assert!(InputReceipt::refusal_from_config(&junk).is_none());
        assert!(!InputReceipt::readiness(&junk).ready);
        // A minted receipt is ready and carries no refusal.
        let ready = InputReceipt::readiness(
            &json!({"network_reporter_input_receipt": print_receipt("Rwanda")}),
        );
        assert!(ready.ready && ready.issue.is_none() && ready.refusal.is_none());
    }

    #[test]
    fn the_configuration_bytes_splice_the_servers_own_sheets_unchanged() {
        let sheets = r#"{"adversarial":{"big":9007199254740993,"float":100000000000000000000,"negative_zero":-0,"separators":"line paragraph end"}}"#;
        let receipt = InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: "Rwanda".into(),
            sheets_json: sheets.into(),
            sheets_sha256: format!("{:x}", Sha256::digest(sheets.as_bytes())),
            reference_semantic_sha256: "c".repeat(64),
        };
        assert_eq!(
            String::from_utf8(receipt.network_config_bytes().unwrap()).unwrap(),
            r#"{"groups":{},"sheets":{"adversarial":{"big":9007199254740993,"float":100000000000000000000,"negative_zero":-0,"separators":"line paragraph end"}},"models":{}}"#
        );
    }

    #[test]
    fn a_plan_binds_the_run_to_its_inputs_and_policy() {
        let receipt = simple_receipt("Benin");
        let plan = plan_transformer_export("proj-1", &receipt, "tx_a", 7, &layers(), None).unwrap();
        assert_eq!(plan.formats, vec!["shp", "xlsx"]);
        assert_eq!(plan.outputs[0].output_id, "shp");
        assert!(!plan.requires_admin_bounds);
        assert_eq!(
            plan.room,
            RoomEvidence {
                layers: 1,
                features: 1
            }
        );
        assert_eq!(
            plan.room_content_sha256,
            jcs::room_content_sha256(&layers()).unwrap()
        );
        assert_eq!(
            plan.content_digest,
            jcs::layers_content_digest(&layers()).unwrap()
        );
        assert_eq!(
            plan.input_base_fingerprint,
            input_base_fingerprint(
                "tx_a",
                7,
                &receipt.sheets_sha256,
                "Benin",
                &receipt.reference_semantic_sha256
            )
            .unwrap()
        );
        // The same decisions the desktop shell refuses with.
        assert!(plan_transformer_export("../p", &receipt, "tx_a", 7, &layers(), None).is_err());
        assert!(
            plan_transformer_export("proj-1", &receipt, "collisions", 7, &layers(), None).is_err()
        );
        assert!(plan_transformer_export("proj-1", &receipt, "tx_a", -1, &layers(), None).is_err());
        assert!(
            plan_transformer_export("proj-1", &receipt, "tx_a", 7, &BTreeMap::new(), None).is_err()
        );
        let mut empty = layers();
        empty.get_mut("lv_poles").unwrap()["features"] = json!([]);
        assert!(plan_transformer_export("proj-1", &receipt, "tx_a", 7, &empty, None).is_err());
        let mut extra = layers();
        extra.get_mut("lv_poles").unwrap()["bbox"] = json!([0, 0, 1, 1]);
        assert!(plan_transformer_export("proj-1", &receipt, "tx_a", 7, &extra, None).is_err());
        // A local selection narrows the run without touching the policy.
        let selection: DesignOutputSelection = serde_json::from_value(
            json!({"schema":"ds.design-output-selection/v1","tabular":["xlsx"]}),
        )
        .unwrap();
        let narrowed =
            plan_transformer_export("proj-1", &receipt, "tx_a", 7, &layers(), Some(&selection))
                .unwrap();
        assert_eq!(narrowed.formats, vec!["xlsx"]);
        // A named print output needs its setup in the sheets.
        assert!(plan_transformer_export(
            "proj-1",
            &print_receipt("Rwanda"),
            "tx_a",
            7,
            &layers(),
            None
        )
        .unwrap_err()
        .contains("detail"));
    }

    #[test]
    fn the_engine_request_is_the_exact_typed_document() {
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Rwanda"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let staging = PathBuf::from("/tmp/run");
        let staged = StagedInputs {
            transformer_document: &staging.join("transformer.json"),
            network_config: &staging.join("config.json"),
            out_dir: &staging.join("out"),
            admin_bounds: Some((Path::new("/assets/asset.dsab"), &"d".repeat(64))),
            media_grant: None,
            print_context: None,
            sheet: None,
        };
        let request = engine_request(&plan, &staged).unwrap();
        assert_eq!(
            request,
            json!({
                "transformer_document": "/tmp/run/transformer.json",
                "network_config": "/tmp/run/config.json",
                "input_shape": "plain_local",
                "transformer": "tx_a",
                "out_dir": "/tmp/run/out",
                "country": "Rwanda",
                "admin_bounds_asset": "/assets/asset.dsab",
                "admin_bounds_sha256": "d".repeat(64),
                "formats": ["shp", "xlsx"],
            })
        );
        let without = StagedInputs {
            admin_bounds: None,
            ..staged.clone()
        };
        assert!(engine_request(&plan, &without)
            .unwrap_err()
            .contains("Rwanda"));
        // A place in the drawing set reaches the engine as two integers, and
        // only a real place does.
        let numbered = StagedInputs {
            sheet: Some((12, 86)),
            ..staged.clone()
        };
        let request = engine_request(&plan, &numbered).unwrap();
        assert_eq!(request["sheet_index"], json!(12));
        assert_eq!(request["sheet_count"], json!(86));
        for bad in [(0, 86), (87, 86), (1, 0)] {
            let wrong = StagedInputs {
                sheet: Some(bad),
                ..staged.clone()
            };
            assert!(engine_request(&plan, &wrong)
                .unwrap_err()
                .contains("drawing set"));
        }
        let benin = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        assert!(engine_request(&benin, &staged)
            .unwrap_err()
            .contains("another country"));
        let request = engine_request(&benin, &without).unwrap();
        assert!(request.get("admin_bounds_asset").is_none());
    }

    #[test]
    fn the_engine_identity_is_the_closed_build_document() {
        let info = json!({
            "schema": "ds.engine-build/v1", "service": "ds-network-reporter", "binary": "ds-report",
            "package_version": "0.1.0", "source_sha": "development",
            "engine_version": "ds-network-reporter@0.1.0+development",
            "cargo_lock_sha256": "d".repeat(64), "features": [], "rustc": "rustc 1.95.0",
            "target": "x86_64-unknown-linux-gnu", "profile": "debug",
            "schemas": ["ds-network-reporter.task-result/v1", "ds.engine-build/v1"],
            "build_manifest_sha256": "f".repeat(64)
        });
        let identity = engine_identity(&info).unwrap();
        assert_eq!(
            identity.publication_state().unwrap(),
            PublicationState::LocalOnly
        );
        let mut release = info.clone();
        release["source_sha"] = json!("a".repeat(40));
        release["engine_version"] = json!(format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)));
        release["profile"] = json!("release");
        assert_eq!(
            engine_identity(&release)
                .unwrap()
                .publication_state()
                .unwrap(),
            PublicationState::Pending
        );
        let mut mismatched = release.clone();
        mismatched["engine_version"] =
            json!("ds-network-reporter@0.2.0+".to_string() + &"a".repeat(40));
        assert!(engine_identity(&mismatched).is_err());
        let mut other = release.clone();
        other["binary"] = json!("ds-grid");
        assert!(engine_identity(&other).is_err());
        let mut unknown = release;
        unknown["extra"] = json!(true);
        assert!(engine_identity(&unknown).is_err());
    }

    fn result_document(plan: &ExportPlan, out_dir: &Path) -> Value {
        json!({
            "task": REPORT_TASK, "status": "completed", "transformer": plan.transformer,
            "country": plan.country, "layers_decoded": 1, "features_decoded": 1,
            "admin_bounds": {"configured": plan.requires_admin_bounds, "villages_loaded": if plan.requires_admin_bounds {14920} else {0}, "features_matched": 0, "features_requested": 0},
            "geometry_changed": false, "formats_requested": plan.formats,
            "artifacts": [
                {"format": "shp", "filename": "tx_a.shp.zip", "path": out_dir.join("tx_a.shp.zip"), "bytes": 10, "sha256": "1".repeat(64)},
                {"format": "xlsx", "filename": "tx_a.xlsx", "path": out_dir.join("tx_a.xlsx"), "bytes": 20, "sha256": "2".repeat(64)},
            ],
            "blockers": [], "elapsed_ms": 42
        })
    }

    #[test]
    fn a_result_is_verified_against_its_plan_and_refused_typed() {
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let out_dir = PathBuf::from("/tmp/run/out");
        let document = result_document(&plan, &out_dir);
        let verified =
            verify_result(&serde_json::to_vec(&document).unwrap(), &plan, &out_dir).unwrap();
        assert_eq!(verified.artifacts.len(), 2);
        assert_eq!(verified.artifacts[0].output_id, "shp");
        assert_eq!(verified.artifacts[0].content_type, "application/zip");
        assert_eq!(verified.artifacts[1].size_bytes, 20);

        assert!(verified.failed.is_empty());

        // A run that built nothing is the refusal; the engine's own blockers
        // say which format and why.
        let mut nothing = document.clone();
        nothing["status"] = json!("failed");
        nothing["artifacts"] = json!([]);
        nothing["blockers"] = json!([{"code": "format_build_failed", "message": "shp OOM", "format": "shp"},
                   {"code": "format_build_failed", "message": "xlsx OOM", "format": "xlsx"}]);
        match verify_result(&serde_json::to_vec(&nothing).unwrap(), &plan, &out_dir).unwrap_err() {
            ResultRefusal::Blocked {
                status,
                blockers,
                artifacts,
            } => {
                assert_eq!(status, "failed");
                assert_eq!(blockers[0].code, "format_build_failed");
                assert_eq!(artifacts, 0);
                assert_eq!(
                    blocker_summary(&blockers),
                    "format_build_failed/shp: shp OOM; format_build_failed/xlsx: xlsx OOM"
                );
            }
            other => panic!("{other:?}"),
        }
        // The typed failure read after a non-zero exit is the same refusal.
        assert!(matches!(
            typed_failure(&serde_json::to_vec(&nothing).unwrap(), &plan),
            Some(ResultRefusal::Blocked { .. })
        ));
        assert!(typed_failure(&serde_json::to_vec(&document).unwrap(), &plan).is_none());
        assert!(typed_failure(b"not json", &plan).is_none());

        let invalid = |mutate: &dyn Fn(&mut Value)| {
            let mut doc = document.clone();
            mutate(&mut doc);
            match verify_result(&serde_json::to_vec(&doc).unwrap(), &plan, &out_dir).unwrap_err() {
                ResultRefusal::Invalid { message } => message,
                other => panic!("{other:?}"),
            }
        };
        assert!(invalid(&|d| d["transformer"] = json!("tx_b")).contains("semantic request"));
        assert!(
            invalid(&|d| d["formats_requested"] = json!(["xlsx", "shp"]))
                .contains("in the plan's order")
        );
        // A format silently dropped is the failure this whole shape exists to
        // end: it is named, or the result is not an answer.
        assert!(invalid(&|d| {
            d["formats_requested"] = json!(["shp"]);
            d["artifacts"] = json!([d["artifacts"][0].clone()]);
            d["status"] = json!("partial");
            d["blockers"] = json!([]);
        })
        .contains("does not agree with its blockers"));
        assert!(invalid(&|d| {
            d["artifacts"] = json!([d["artifacts"][0].clone()]);
            d["status"] = json!("partial");
            d["blockers"] = json!([{"code": "x", "message": "y", "format": "shp"}]);
        })
        .contains("distinct unbuilt format"));
        assert!(invalid(&|d| {
            d["artifacts"] = json!([d["artifacts"][0].clone()]);
            d["status"] = json!("partial");
            d["blockers"] = json!([{"code": "x", "message": "y"}]);
        })
        .contains("distinct unbuilt format"));
        assert!(
            invalid(&|d| d["artifacts"][1]["filename"] = json!("tx_a.shp.zip"))
                .contains("invalid artifact")
        );
        assert!(
            invalid(&|d| d["artifacts"][0]["path"] = json!("/elsewhere/tx_a.shp.zip"))
                .contains("direct output child")
        );
        assert!(invalid(&|d| d["artifacts"][0]["bytes"] = json!(0)).contains("invalid artifact"));
        assert!(invalid(&|d| d["features_decoded"] = json!(0)).contains("no decoded"));
        assert!(
            invalid(&|d| d["admin_bounds"]["configured"] = json!(true)).contains("another country")
        );
        assert!(invalid(&|d| d["warnings"] = json!([{"code":"other","transformer":"tx_a","output_id":"shp","layout_id":"x","element_id":"e","total_rows":2,"omitted_rows":1,"message":""}])).contains("print warnings"));
        assert!(invalid(&|d| d["extra"] = json!(1)).contains("malformed"));

        // A Rwanda plan needs the administrative evidence the engine reports.
        let rwanda = plan_transformer_export(
            "proj-1",
            &simple_receipt("Rwanda"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let mut stamped = result_document(&rwanda, &out_dir);
        assert!(verify_result(&serde_json::to_vec(&stamped).unwrap(), &rwanda, &out_dir).is_ok());
        stamped["admin_bounds"]["villages_loaded"] = json!(0);
        assert!(matches!(
            verify_result(&serde_json::to_vec(&stamped).unwrap(), &rwanda, &out_dir),
            Err(ResultRefusal::Invalid { .. })
        ));
    }

    /// The Gisagara incident, as the batch of 86 transformers met it: one A0
    /// sheet fails to lay out and the A3, the KMZ, the shapefile and the
    /// workbook beside it are finished. They were discarded — 59 of 86 sheets
    /// of correct, computed work thrown away because something else failed
    /// next to them, and nothing left the machine.
    ///
    /// They are now the answer: four verified artifacts a host can place,
    /// receipt and publish, and one named failure an operator can act on —
    /// the output id AND the layout knob, because "the A0 failed" is not
    /// actionable and "the A0's `lv_schedule` needs 3 panels where 1 is
    /// permitted" is.
    #[test]
    fn a_partial_run_keeps_what_completed_and_names_the_sheet_that_failed() {
        let mut plan = plan_transformer_export(
            "gisagara",
            &simple_receipt("Rwanda"),
            "upgrade_ruturo",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let a0 = "pdf__a0-landscape-gisagara-cjic";
        for format in ["kmz", "pdf_a3", a0] {
            plan.formats.push(format.to_string());
            plan.outputs
                .push(report_formats::output(format).expect("a known output"));
        }
        let out_dir = PathBuf::from("/tmp/gisagara/out");
        let path = |name: &str| out_dir.join(name);
        let document = json!({
            "task": REPORT_TASK, "status": "partial", "transformer": "upgrade_ruturo",
            "country": "Rwanda", "layers_decoded": 9, "features_decoded": 312,
            "admin_bounds": {"configured": true, "villages_loaded": 14920, "features_matched": 3, "features_requested": 3},
            "geometry_changed": false, "formats_requested": plan.formats,
            "artifacts": [
                {"format": "shp", "filename": "upgrade_ruturo.shp.zip", "path": path("upgrade_ruturo.shp.zip"), "bytes": 10, "sha256": "1".repeat(64)},
                {"format": "xlsx", "filename": "upgrade_ruturo.xlsx", "path": path("upgrade_ruturo.xlsx"), "bytes": 20, "sha256": "2".repeat(64)},
                {"format": "kmz", "filename": "upgrade_ruturo.kmz", "path": path("upgrade_ruturo.kmz"), "bytes": 30, "sha256": "3".repeat(64)},
                {"format": "pdf_a3", "filename": "upgrade_ruturo.a3.pdf", "path": path("upgrade_ruturo.a3.pdf"), "bytes": 40, "sha256": "4".repeat(64)},
            ],
            "blockers": [{
                "code": "format_build_failed",
                "message": "render a0-landscape-gisagara-cjic: table lv_schedule has 212 rows; needs 3 panels at the authored type size (maximum 1)",
                "format": a0
            }],
            "elapsed_ms": 9_100
        });
        let verified =
            verify_result(&serde_json::to_vec(&document).unwrap(), &plan, &out_dir).unwrap();

        // Four completed, matched to their own outputs — the missing A0 in
        // the middle of the plan shifts nothing.
        let built: Vec<&str> = verified
            .artifacts
            .iter()
            .map(|artifact| artifact.format.as_str())
            .collect();
        assert_eq!(built, ["shp", "xlsx", "kmz", "pdf_a3"]);
        assert!(verified
            .artifacts
            .iter()
            .all(|artifact| artifact.output_id == artifact.format));
        assert_eq!(verified.artifacts[3].filename, "upgrade_ruturo.a3.pdf");

        // One failed format, named to the knob.
        assert_eq!(verified.failed.len(), 1);
        let failed = &verified.failed[0];
        assert_eq!(failed.output_id, a0);
        assert_eq!(failed.code, "format_build_failed");
        let knob = failed.layout.as_ref().expect("the layout knob");
        assert_eq!(knob.element_id.as_deref(), Some("lv_schedule"));
        assert_eq!(knob.knob, KNOB_PANELS);
        assert_eq!(knob.detail, "212 rows need 3 panels; the layout permits 1");
        assert!(failed.remedy.contains("raise `panels` on `lv_schedule`"));
        assert!(failed.remedy.contains("truncate_warn"));

        // The receipt beside the artifacts declares the same two sets, so a
        // reader knows what is publishable and what is missing without
        // opening the directory.
        let receipt = run_receipt(&RunReceiptFacts {
            plan: &plan,
            client_run_id: &format!("report-{}", "a".repeat(32)),
            engine: &engine(),
            result: &verified,
            artifact_dir: "upgrade_ruturo",
            now_ms: 1_700_000_000_000,
            expected_content_digest: None,
            print_context_sha256: None,
            print_context_layers: &[],
            print_context_omitted: &[],
        })
        .unwrap();
        assert_eq!(receipt["status"], "partial");
        assert_eq!(receipt["outputs"].as_array().unwrap().len(), 4);
        assert_eq!(receipt["failed_formats"][0]["output_id"], a0);
        assert_eq!(receipt["failed_formats"][0]["layout"]["knob"], "panels");
        assert_eq!(receipt["formats_requested"].as_array().unwrap().len(), 5);
        // Publication reads the receipt's publication facts, and they are the
        // same facts a complete run would carry: the four artifacts are
        // publishable exactly as they are.
        assert_eq!(receipt["publication"]["state"], "pending");
        assert_eq!(receipt["publication"]["transformer"], "upgrade_ruturo");

        // The batch row stays `ok` — the work is on disk and publishable —
        // and still says what is missing, so no batch reads as complete when
        // part of a sheet set is not there.
        let row = BatchResult::ok("upgrade_ruturo", "upgrade_ruturo/report-run.json".into(), 4)
            .with_failed_formats(verified.failed.clone());
        let batch = batch_receipt(
            "gisagara",
            "stable",
            Some(&engine()),
            4,
            &["upgrade_ruturo".to_string()],
            &[row],
            1_700_000_000_000,
        )
        .unwrap();
        assert_eq!(batch["status"], "completed");
        assert_eq!(batch["completed"], 1);
        assert_eq!(batch["failed"], 0);
        assert_eq!(batch["partial_formats"], 1);
        assert_eq!(batch["results"][0]["artifacts"], 4);
        assert_eq!(batch["results"][0]["failed_formats"][0]["output_id"], a0);

        // The format that is lost is not always the last one asked for. A gap
        // in the MIDDLE of the plan is what used to shift every later artifact
        // onto the wrong output and refuse the whole run as invalid, and it is
        // the reason a batch reported `report_result_invalid` rather than the
        // work it was holding. Lose the workbook and the A3 — one early, one
        // late — and the three that built must still land on their own
        // outputs, by name, in order.
        let mut gapped = document.clone();
        gapped["artifacts"] = json!([
            document["artifacts"][0].clone(),
            document["artifacts"][2].clone(),
            {"format": a0, "filename": "upgrade_ruturo.a0-landscape-gisagara-cjic.pdf", "path": path("upgrade_ruturo.a0-landscape-gisagara-cjic.pdf"), "bytes": 50, "sha256": "5".repeat(64)},
        ]);
        gapped["blockers"] = json!([
            {"code": "format_build_failed", "message": "workbook writer ran out of memory", "format": "xlsx"},
            {"code": "printing_execution_disabled", "message": "printing is not enabled on this device", "format": "pdf_a3"},
        ]);
        let verified =
            verify_result(&serde_json::to_vec(&gapped).unwrap(), &plan, &out_dir).unwrap();
        let placed: Vec<(&str, &str, &str)> = verified
            .artifacts
            .iter()
            .map(|artifact| {
                (
                    artifact.format.as_str(),
                    artifact.output_id.as_str(),
                    artifact.filename.as_str(),
                )
            })
            .collect();
        assert_eq!(
            placed,
            [
                ("shp", "shp", "upgrade_ruturo.shp.zip"),
                ("kmz", "kmz", "upgrade_ruturo.kmz"),
                (a0, a0, "upgrade_ruturo.a0-landscape-gisagara-cjic.pdf"),
            ]
        );
        let lost: Vec<&str> = verified
            .failed
            .iter()
            .map(|failed| failed.output_id.as_str())
            .collect();
        assert_eq!(lost, ["xlsx", "pdf_a3"]);
        // Each one keeps its own code and its own way forward; neither claims
        // a layout knob, because neither message names one.
        assert_eq!(verified.failed[0].code, "format_build_failed");
        assert_eq!(verified.failed[1].code, "printing_execution_disabled");
        assert!(verified.failed.iter().all(|failed| failed.layout.is_none()));
        assert!(verified.failed[1].remedy.contains("printing enabled"));
    }

    /// Every layout shortfall the kernel's own table planner raises is read
    /// back into the member an operator turns, wherever the engine wrapped
    /// the message. A failure it did not raise claims no knob.
    #[test]
    fn a_layout_shortfall_names_the_member_that_decides_it() {
        let ceiling = layout_knob(
            "table schedule has 900 rows; needs 9 panels at the authored type size (maximum 8)",
        )
        .expect("a knob");
        assert_eq!(ceiling.knob, KNOB_OVERFLOW);
        assert_eq!(ceiling.element_id.as_deref(), Some("schedule"));
        assert!(ceiling.detail.contains("8-panel ceiling"));
        assert!(remedy("format_build_failed", Some(&ceiling)).contains("`panels` cannot rise"));

        let type_size = layout_knob(
            "render a3-portrait: table lv_print_info row height cannot contain its readable type",
        )
        .expect("a knob");
        assert_eq!(type_size.knob, KNOB_ROW_MM);
        assert_eq!(type_size.element_id.as_deref(), Some("lv_print_info"));

        // The frame refusals name their own table, so the remedy says which
        // table to lower — and a message from an engine built before they did
        // still reads, naming no element rather than the wrong one.
        let frame =
            layout_knob("table lv_schedule frame needs room for its title, header and one row")
                .expect("a knob");
        assert_eq!(frame.knob, KNOB_ROW_MM);
        assert_eq!(frame.element_id.as_deref(), Some("lv_schedule"));
        assert!(remedy("format_build_failed", Some(&frame)).contains("`row_mm` on `lv_schedule`"));
        let older = layout_knob("table frame needs room for its title, header and one row")
            .expect("a knob");
        assert_eq!(older.knob, KNOB_ROW_MM);
        assert_eq!(older.element_id, None);

        // A workbook table is held to one panel whatever `panels` the setup
        // authors, so the knob it names is the one that can actually move.
        // Naming `panels` here sent an operator to re-run 86 sheets against a
        // number that cannot change.
        let workbook = layout_knob(
            "table lv_print_info has 212 rows; needs 10 panels at the authored type size \
             (maximum 1; a workbook table prints in one panel)",
        )
        .expect("a knob");
        assert_eq!(workbook.knob, KNOB_OVERFLOW);
        assert!(workbook.detail.contains(ONE_PANEL_PRESENTATION));
        assert!(!remedy("format_build_failed", Some(&workbook)).contains("raise `panels`"));

        // The permitted count is read from `(maximum N)` itself. A bracket in
        // whatever prefix an engine wraps the message with used to be read as
        // the ceiling, and told an operator to raise `panels` to the number
        // the layout already permits.
        let wrapped = layout_knob(
            "render a0 (sheet 2 of 3): table lv_schedule has 212 rows; needs 3 panels at the \
             authored type size (maximum 1)",
        )
        .expect("a knob");
        assert_eq!(wrapped.knob, KNOB_PANELS);
        assert_eq!(
            wrapped.detail,
            "212 rows need 3 panels; the layout permits 1"
        );

        assert!(layout_knob("Invalid table row geometry.").is_none());
        assert!(layout_knob("the reporter ran out of memory").is_none());
        // A failure with no knob still carries a way forward.
        for code in [
            "format_not_in_policy",
            "artifact_stage_failed",
            "printing_execution_disabled",
            "format_build_failed",
        ] {
            assert!(!remedy(code, None).is_empty());
        }
        // The exact refusal `printing::table_rows::plan` writes is the one
        // read here; if that wording moves, this fails rather than a batch.
        let raised = crate::printing::table_rows::plan(
            "lv_schedule",
            212,
            99.,
            7.,
            Some(4.),
            1,
            false,
            crate::printing::TableOverflow::Error,
        )
        .unwrap_err();
        let knob = layout_knob(&raised).expect("the planner's own message is readable");
        assert_eq!(knob.element_id.as_deref(), Some("lv_schedule"));
        assert_eq!(knob.knob, KNOB_PANELS);
        // Every shortfall the planner can raise, read back from the words it
        // actually wrote: the knob it names must be one the operator can turn.
        let raise = |rows, height: f64, row_mm, panels, styled, overflow| {
            let message = crate::printing::table_rows::plan(
                "lv_schedule",
                rows,
                height,
                7.,
                Some(row_mm),
                panels,
                styled,
                overflow,
            )
            .unwrap_err();
            let knob = layout_knob(&message)
                .unwrap_or_else(|| panic!("the planner's own message is readable: {message}"));
            assert_eq!(knob.element_id.as_deref(), Some("lv_schedule"), "{message}");
            (knob.knob, message)
        };
        use crate::printing::TableOverflow;
        // A workbook section cannot be cut into panels, whatever `panels`
        // the setup authors — here four.
        assert_eq!(
            raise(212, 99., 4., 4, true, TableOverflow::Error).0,
            KNOB_OVERFLOW
        );
        assert_eq!(
            raise(212, 99., 4., 4, false, TableOverflow::Error).0,
            KNOB_PANELS
        );
        assert_eq!(
            raise(10, 10., 4., 1, false, TableOverflow::Error).0,
            KNOB_ROW_MM
        );
        assert_eq!(
            raise(10, 12., 4., 1, false, TableOverflow::TruncateWarn).0,
            KNOB_ROW_MM
        );
    }

    fn context_layer(id: &str) -> (String, Value) {
        (
            id.to_string(),
            json!({"type":"FeatureCollection","features":[{
                "type":"Feature","properties":{"class":"national"},
                "geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.01,-2.0]]}
            }]}),
        )
    }

    fn coverage() -> Value {
        json!({"type":"Polygon","coordinates":[[[29.99,-2.01],[30.02,-2.01],[30.02,-1.99],[29.99,-1.99],[29.99,-2.01]]]})
    }

    #[test]
    fn the_print_context_document_is_the_engine_document_and_deterministic() {
        // The exact input the engine's own test stages
        // (`ds-network-reporter` `desktop_print_context_is_digest_bound_and_decoded_as_vectors`).
        let layers: BTreeMap<String, Value> = [context_layer("roads")].into_iter().collect();
        let sources = json!([{"layer":"roads","resource_id":"r","version":"v","cache":"local"}]);
        let bytes = print_context_document(&coverage(), &sources, &layers).unwrap();
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(document["schema"], PRINT_CONTEXT_SCHEMA);
        assert_eq!(
            document.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["schema", "coverage", "sources", "layers"]
        );
        assert_eq!(
            document["layers"]["roads"]["features"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // Byte-identical to the recipe the desktop shell applied on its own
        // until now, so a desktop and a server stage one digest.
        let desktop = serde_json::to_vec(&json!({
            "schema":"ds.print-context/v1", "coverage":coverage(), "sources":sources, "layers":layers
        }))
        .unwrap();
        assert_eq!(bytes, desktop);
        assert_eq!(
            sha256_hex(&bytes),
            format!("{:x}", Sha256::digest(&desktop))
        );
        // Layer order is the map's, whatever order a host gathered them in.
        let reversed: BTreeMap<String, Value> = [context_layer("rivers"), context_layer("roads")]
            .into_iter()
            .collect();
        let forward: BTreeMap<String, Value> = [context_layer("roads"), context_layer("rivers")]
            .into_iter()
            .collect();
        assert_eq!(
            print_context_document(&coverage(), &json!([]), &reversed).unwrap(),
            print_context_document(&coverage(), &json!([]), &forward).unwrap()
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_print_context_document_is_refused_where_the_engine_would_refuse_it() {
        let layers: BTreeMap<String, Value> = [context_layer("roads")].into_iter().collect();
        let refused = |coverage: &Value, sources: &Value, layers: &BTreeMap<String, Value>| {
            print_context_document(coverage, sources, layers).unwrap_err()
        };
        assert!(refused(
            &json!({"type":"Point","coordinates":[30.,-2.]}),
            &json!([]),
            &layers
        )
        .contains("coverage"));
        assert!(refused(&coverage(), &json!([]), &BTreeMap::new()).contains("no layers"));
        let bad_id: BTreeMap<String, Value> =
            [context_layer("roads national")].into_iter().collect();
        assert!(refused(&coverage(), &json!([]), &bad_id).contains("not an identifier"));
        let mut empty = layers.clone();
        empty.insert(
            "rivers".into(),
            json!({"type":"FeatureCollection","features":[]}),
        );
        assert!(refused(&coverage(), &json!([]), &empty).contains("is empty"));
        let mut decorated = layers.clone();
        decorated.insert(
            "rivers".into(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":null}],"name":"x"}),
        );
        assert!(
            refused(&coverage(), &json!([]), &decorated).contains("only type=FeatureCollection")
        );
        let mut shapeless = layers.clone();
        shapeless.insert(
            "rivers".into(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":[],"geometry":null}]}),
        );
        assert!(refused(&coverage(), &json!([]), &shapeless).contains("feature[0]"));
        assert!(refused(&coverage(), &json!({}), &layers).contains("sources must be an array"));
        assert!(refused(&coverage(), &json!([{"layer":"rivers"}]), &layers).contains("source[0]"));
        let mut many = BTreeMap::new();
        for index in 0..=MAX_PRINT_CONTEXT_LAYERS {
            many.insert(format!("layer_{index}"), context_layer("x").1);
        }
        assert!(refused(&coverage(), &json!([]), &many).contains("-layer bound"));
        let features: Vec<Value> = (0..=MAX_PRINT_CONTEXT_FEATURES)
            .map(|_| json!({"type":"Feature","properties":{},"geometry":null}))
            .collect();
        let crowded: BTreeMap<String, Value> = [(
            "roads".to_string(),
            json!({"type":"FeatureCollection","features":features}),
        )]
        .into_iter()
        .collect();
        assert!(refused(&coverage(), &json!([]), &crowded).contains("more than"));
    }

    #[test]
    fn receipts_record_the_print_context_they_were_rendered_with() {
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let out_dir = PathBuf::from("/tmp/run/out");
        let result = verify_result(
            &serde_json::to_vec(&result_document(&plan, &out_dir)).unwrap(),
            &plan,
            &out_dir,
        )
        .unwrap();
        let run_id = format!("report-{}", "0".repeat(32));
        let digest = sha256_hex(b"print-context");
        let supplied = ["roads".to_string(), "contours_index".to_string()];
        let omitted = ["project_dsgrid_mv".to_string()];
        let facts = |sha: Option<&str>, layers: &[String], omitted: &[String]| {
            run_receipt(&RunReceiptFacts {
                plan: &plan,
                client_run_id: &run_id,
                engine: &engine(),
                result: &result,
                artifact_dir: "tx_a",
                now_ms: 1_700_000_000_000,
                expected_content_digest: None,
                print_context_sha256: sha,
                print_context_layers: layers,
                print_context_omitted: omitted,
            })
        };
        let receipt = facts(Some(&digest), &supplied, &omitted).unwrap();
        assert_eq!(receipt["print_context_sha256"], digest);
        assert_eq!(
            receipt["print_context_layers"],
            json!(["roads", "contours_index"])
        );
        assert_eq!(
            receipt["print_context_omitted"],
            json!(["project_dsgrid_mv"])
        );
        // A run without context says so, and may still name what it omitted.
        let bare = facts(None, &[], &omitted).unwrap();
        assert_eq!(bare["print_context_sha256"], Value::Null);
        assert_eq!(bare["print_context_omitted"], json!(["project_dsgrid_mv"]));
        // A digest without layers, layers without a digest, a malformed
        // digest, and an id that is not an identifier or is named twice.
        assert!(facts(Some(&digest), &[], &[]).is_err());
        assert!(facts(None, &supplied, &[]).is_err());
        assert!(facts(Some("abc"), &supplied, &[]).is_err());
        assert!(facts(Some(&digest), &["roads x".to_string()], &[]).is_err());
        assert!(facts(Some(&digest), &supplied, &["roads".to_string()]).is_err());
    }

    #[test]
    fn artifact_filenames_follow_their_format() {
        assert!(valid_artifact_filename("tx_a.shp.zip", "shp"));
        assert!(valid_artifact_filename(
            "tx_a.network-information.json",
            "network_information"
        ));
        assert!(!valid_artifact_filename("tx_a.json", "network_information"));
        assert!(valid_artifact_filename("tx_a.detail.pdf", "pdf__detail"));
        assert!(valid_artifact_filename("tx_a.detail.png", "png__detail"));
        assert!(valid_artifact_filename("tx_a.detail.jpg", "jpeg__detail"));
        assert!(!valid_artifact_filename("tx_a.detail.pdf", "png__detail"));
        assert!(!valid_artifact_filename("tx_a.other.png", "png__detail"));
        assert!(!valid_artifact_filename(
            "../tx_a.detail.png",
            "png__detail"
        ));
        assert!(!valid_artifact_filename("tx_a.pdf", "pdf__detail"));
        assert!(!valid_artifact_filename("../tx_a.xlsx", "xlsx"));
        assert!(!valid_artifact_filename("tx_a.xlsx", "shp"));
        assert!(!valid_artifact_filename("", "xlsx"));
    }

    #[test]
    fn named_raster_results_keep_the_full_artifact_admission_contract() {
        let mut plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Chad"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let out_dir = PathBuf::from("/tmp/run/out");
        let layout = crate::printing::named_layout(
            Some("detail".into()),
            None,
            crate::printing::Paper::A3,
            crate::printing::Orientation::Landscape,
        )
        .unwrap();
        for format in ["png__detail", "jpeg__detail", "pdf__detail"] {
            let output = report_formats::output(format).unwrap();
            plan.formats = vec![format.into()];
            plan.outputs = vec![output.clone()];
            let filename = format!("tx_a{}", output.suffix);
            let mut document = result_document(&plan, &out_dir);
            document["artifacts"] = json!([{
                "format":format, "filename":filename, "path":out_dir.join(&filename),
                "bytes":123, "sha256":"a".repeat(64), "paper_size":"A3",
                "presentation":crate::printing::ArtifactPresentation::from_layout(&layout).unwrap()
            }]);
            document["warnings"] = json!([{
                "code":"table_truncated", "transformer":"tx_a", "output_id":format,
                "layout_id":"detail", "element_id":"poles", "total_rows":10,
                "omitted_rows":1, "message":"One pole row omitted."
            }]);
            let verify =
                |value: &Value| verify_result(&serde_json::to_vec(value).unwrap(), &plan, &out_dir);
            let result = verify(&document).unwrap();
            assert_eq!(result.artifacts[0].content_type, output.content_type);
            assert_eq!(result.artifacts[0].sha256, "a".repeat(64));
            assert_eq!(result.warnings.len(), 1);
            for (key, invalid) in [
                ("filename", json!("tx_a.other.png")),
                ("path", json!("/tmp/elsewhere.png")),
                ("sha256", json!("not-a-digest")),
                ("bytes", json!(0)),
                ("paper_size", json!("A0")),
            ] {
                let mut changed = document.clone();
                changed["artifacts"][0][key] = invalid;
                assert!(
                    verify(&changed).is_err(),
                    "{format}: accepted invalid {key}"
                );
            }
            document["warnings"][0]["layout_id"] = json!("another-layout");
            assert!(verify(&document).is_err());

            // Measured furniture diagnostics travel as warning rows too: a
            // named finding, and the bounded summary that names no element.
            // Neither has table rows behind it. (2026-09-14: the reporter
            // began emitting them and every sheet with a finding was refused
            // as "invalid print warnings".)
            document["warnings"] = json!([
                {
                    "code":"print_furniture_overlap", "transformer":"tx_a", "output_id":format,
                    "layout_id":"detail", "element_id":"legend", "total_rows":0,
                    "omitted_rows":0, "message":"legend overlaps house-schedule by 4.2 mm"
                },
                {
                    "code":"print_diagnostics_summary", "transformer":"tx_a", "output_id":format,
                    "layout_id":"detail", "element_id":"", "total_rows":0,
                    "omitted_rows":0, "message":"3 furniture findings; details truncated=false; 2 labels omitted"
                },
                {
                    "code":"print_text_unbound", "transformer":"tx_a", "output_id":format,
                    "layout_id":"detail", "element_id":"", "total_rows":0,
                    "omitted_rows":0, "message":"{note} has no value in the title block or the project; the cell printed empty"
                },
                {
                    "code":"print_label_omissions", "transformer":"tx_a", "output_id":format,
                    "layout_id":"detail", "element_id":"", "total_rows":0,
                    "omitted_rows":0, "message":"poles labels: 1 skipped by repeat-label spacing"
                }
            ]);
            assert_eq!(verify(&document).unwrap().warnings.len(), 4);
            for (index, key, invalid) in [
                (0, "code", json!("print_visual_approval")),
                (0, "element_id", json!("")),
                (0, "total_rows", json!(5)),
                (1, "element_id", json!("legend")),
                (1, "omitted_rows", json!(1)),
                (1, "transformer", json!("tx_b")),
                (2, "element_id", json!("title")),
                (2, "layout_id", json!("")),
                (2, "total_rows", json!(1)),
                (3, "element_id", json!("poles")),
                (3, "total_rows", json!(1)),
                (3, "omitted_rows", json!(1)),
                (3, "layout_id", json!("another-layout")),
                (3, "transformer", json!("tx_b")),
                (3, "output_id", json!("xlsx")),
            ] {
                let mut changed = document.clone();
                changed["warnings"][index][key] = invalid;
                assert!(
                    verify(&changed).is_err(),
                    "{format}: accepted a diagnostics row with invalid {key}"
                );
            }
        }
    }

    #[test]
    fn cable_conflict_rows_ride_the_workbook_not_a_paper() {
        // 2026-09-15: the reporter began mirroring report_consistency findings
        // as warning rows on the xlsx output, labelled with the saved
        // transformer name, and 59 of 86 Gisagara reports were refused as
        // "invalid print warnings".
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        assert!(plan.formats.iter().any(|format| format == "xlsx"));
        let out_dir = PathBuf::from("/tmp/run/out");
        let mut document = result_document(&plan, &out_dir);
        let verify = |document: &Value| {
            verify_result(&serde_json::to_vec(document).unwrap(), &plan, &out_dir)
        };
        document["warnings"] = json!([{
            "code":"lv_pole_line_cable_conflict", "transformer":"TX_A", "output_id":"xlsx",
            "layout_id":"", "element_id":"LV_Line_01", "total_rows":0, "omitted_rows":0,
            "message":"LV_Line_01 P01: saved pole cable '3 x 50 + 54.6mm² ABC' differs from saved LV-line cable(s) '3 x 35 + 54.6mm² ABC'; pending engineering review. Values preserved."
        }]);
        let verified = verify(&document).unwrap();
        assert_eq!(verified.warnings.len(), 1);
        assert_eq!(verified.warnings[0].transformer, "TX_A");
        for (key, invalid) in [
            ("transformer", json!("tx_b")),
            ("output_id", json!("shp")),
            ("layout_id", json!("detail")),
            ("element_id", json!("")),
            ("element_id", json!("x".repeat(241))),
            ("total_rows", json!(3)),
            ("omitted_rows", json!(1)),
            ("message", json!("m".repeat(2049))),
        ] {
            let mut changed = document.clone();
            changed["warnings"][0][key] = invalid;
            assert!(
                verify(&changed).is_err(),
                "accepted a cable-conflict row with invalid {key}"
            );
        }
    }

    #[test]
    fn lv_engineering_audit_rows_ride_the_workbook_on_their_layer() {
        // 4070556f / 8b2d9545: the reporter re-runs ds-network's transformer
        // protection and span-length audits over the saved layers and names
        // each finding on the Engineering Review sheet.
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let out_dir = PathBuf::from("/tmp/run/out");
        let mut document = result_document(&plan, &out_dir);
        let verify = |document: &Value| {
            verify_result(&serde_json::to_vec(document).unwrap(), &plan, &out_dir)
        };
        let row = |code: &str, layer: &str| {
            json!({"code":code, "transformer":"TX_A", "output_id":"xlsx", "layout_id":"",
                "element_id":layer, "total_rows":0, "omitted_rows":0, "message":"finding"})
        };
        document["warnings"] = json!([
            row("transformer_protection_mismatch", "tr"),
            row("lv_span_exceeds_max_length", "spans"),
        ]);
        assert_eq!(verify(&document).unwrap().warnings.len(), 2);
        for (index, key, invalid) in [
            (0, "element_id", json!("spans")),
            (1, "element_id", json!("tr")),
            (0, "output_id", json!("shp")),
            (1, "layout_id", json!("detail")),
            (0, "transformer", json!("tx_b")),
            (1, "total_rows", json!(1)),
            (0, "message", json!("m".repeat(2049))),
        ] {
            let mut changed = document.clone();
            changed["warnings"][index][key] = invalid;
            assert!(
                verify(&changed).is_err(),
                "accepted an engineering audit row with invalid {key}"
            );
        }
    }

    /// Every code the notification digest counts as approval-blocking is one
    /// the validator admits as an Engineering Review row on the workbook, so
    /// the list cannot name a finding no engine is allowed to raise.
    #[test]
    fn every_engineering_review_code_is_a_workbook_row_the_validator_admits() {
        let formats = vec!["xlsx".to_string()];
        for code in ENGINEERING_REVIEW_CODES {
            let element_id = match code {
                "transformer_protection_mismatch" => "tr",
                "lv_span_exceeds_max_length" => "spans",
                _ => "LV_Line_01",
            };
            let warning = PrintWarning {
                code: code.into(),
                transformer: "TX_A".into(),
                output_id: "xlsx".into(),
                layout_id: String::new(),
                element_id: element_id.into(),
                total_rows: 0,
                omitted_rows: 0,
                message: "finding".into(),
            };
            assert!(valid_print_warning(&warning, "tx_a", &formats), "{code}");
        }
    }

    #[test]
    fn receipts_carry_the_publication_identity_and_the_batch_order() {
        let plan = plan_transformer_export(
            "proj-1",
            &simple_receipt("Benin"),
            "tx_a",
            7,
            &layers(),
            None,
        )
        .unwrap();
        let out_dir = PathBuf::from("/tmp/run/out");
        let result = verify_result(
            &serde_json::to_vec(&result_document(&plan, &out_dir)).unwrap(),
            &plan,
            &out_dir,
        )
        .unwrap();
        let run_id = format!("report-{}", "0".repeat(32));
        let receipt = run_receipt(&RunReceiptFacts {
            plan: &plan,
            client_run_id: &run_id,
            engine: &engine(),
            result: &result,
            artifact_dir: "tx_a",
            now_ms: 1_700_000_000_000,
            expected_content_digest: Some(&plan.content_digest),
            print_context_sha256: None,
            print_context_layers: &[],
            print_context_omitted: &[],
        })
        .unwrap();
        assert_eq!(receipt["schema"], RUN_RECEIPT_SCHEMA);
        assert_eq!(receipt["print_context_sha256"], Value::Null);
        assert_eq!(receipt["print_context_layers"], json!([]));
        assert_eq!(receipt["print_context_omitted"], json!([]));
        assert_eq!(receipt["outputs"][0]["path"], "tx_a/tx_a.shp.zip");
        assert_eq!(
            receipt["outputs"][1]["content_type"],
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(receipt["content_digest_verified"], true);
        assert_eq!(receipt["publication"]["state"], "pending");
        assert_eq!(receipt["publication"]["schema"], 3);
        assert_eq!(receipt["publication"]["operation"], "export-tx_a");
        assert_eq!(
            receipt["publication"]["root"],
            "eds_project/proj-1/eds_design"
        );
        assert_eq!(receipt["publication"]["transformer_revision"], 7);
        assert_eq!(
            receipt["input_base_fingerprint"],
            plan.input_base_fingerprint
        );
        assert!(run_receipt(&RunReceiptFacts {
            plan: &plan,
            client_run_id: "run-1",
            engine: &engine(),
            result: &result,
            artifact_dir: "tx_a",
            now_ms: 0,
            expected_content_digest: None,
            print_context_sha256: None,
            print_context_layers: &[],
            print_context_omitted: &[],
        })
        .is_err());

        let mut development = engine();
        development.engine_version = crate::report::DEVELOPMENT_ENGINE.to_string();
        let local = publication_facts(&plan, &development).unwrap();
        assert_eq!(local["state"], "local_only");
        assert_eq!(local["schema"], LOCAL_ONLY_SCHEMA);

        let results = vec![
            BatchResult::error("tx_b", "export_blocked", "kmz failed", None),
            BatchResult::ok("tx_a", "tx_a/report-run.json".into(), 2),
        ];
        assert_eq!(batch_status(&results), "partial");
        let batch = batch_receipt(
            "proj-1",
            "stable",
            Some(&engine()),
            2,
            &["tx_b".into(), "tx_a".into()],
            &results,
            5,
        )
        .unwrap();
        assert_eq!(batch["status"], "partial");
        assert_eq!(batch["failed"], 1);
        assert_eq!(batch["completed"], 1);
        assert_eq!(batch["results"][0]["transformer"], "tx_a");
        assert_eq!(batch["results"][1]["error"]["code"], "export_blocked");
        assert_eq!(batch["publication_state"], "pending");
        assert_eq!(batch_status(&[]), "failed");
        assert_eq!(batch_status(&results[1..]), "completed");
    }

    #[test]
    fn a_batch_plan_is_the_kernels_scope_and_concurrency() {
        let plan = batch_plan(&["tx_b".into(), "tx_a".into(), "tx_b".into()], 16, 4).unwrap();
        assert_eq!(plan.names, vec!["tx_a", "tx_b"]);
        assert_eq!(plan.concurrency, 2);
        assert_eq!(
            batch_plan(
                &(0..10).map(|i| format!("tx_{i}")).collect::<Vec<_>>(),
                16,
                4
            )
            .unwrap()
            .concurrency,
            4
        );
        assert_eq!(
            batch_plan(
                &(0..10).map(|i| format!("tx_{i}")).collect::<Vec<_>>(),
                2,
                4
            )
            .unwrap()
            .concurrency,
            2
        );
        assert!(batch_plan(&[], 16, 4).is_err());
        assert!(batch_plan(&["collisions".into()], 16, 4).is_err());
        assert!(batch_plan(&["tx_a".into()], 16, 0).is_err());
        assert!(batch_plan(&["tx_a".into()], 16, 65).is_err());
    }

    #[test]
    fn run_ids_are_the_hosts_own_shape() {
        assert!(valid_run_id(&format!(
            "report-{}",
            "0123456789abcdef".repeat(2)
        )));
        assert!(!valid_run_id("report-ABC"));
        assert!(!valid_run_id(&"0".repeat(39)));
    }
}
