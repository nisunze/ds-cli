//! Browser transformer and Combined Report exports through the governed
//! Reporter unit.
//!
//! Owner 24 Oct: the Web exports transformer reports and Combined Reports;
//! it does not print. The Kernel admits the native export request with the
//! held inputs it names (room, cached configuration, administrative bounds
//! and their digest, survey layers, media grant, saved voltage drop) and
//! names the effects a host executes: `{ unit: "reporter", entry, payload }`
//! for the export and `{ unit: "network", entry: "grid.model", payload }` for
//! each Network report-engineering fact the export names. The Reporter unit
//! runs the native task over held inputs
//! (`ds-network-reporter/docs/contracts/held-report-exports.md`); the Network
//! unit answers each fact with the engine's exact `engineering.json` bytes.
//! The Kernel computes no report value and links neither engine: the request
//! and every answer cross as text, so `-0` and float text stay the engines'.
//! Each output is checked against the artifact the task's own result names
//! before a host reads it.
use base64::Engine as _;
use ds_network_contracts::grid_unit::{
    decode_reply as decode_grid_reply, GridArg, GridEntry, GridModelMethod, GridOperation,
    GridRequest, GridResult, METHOD_UNAVAILABLE,
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// The governed unit that runs browser report exports.
pub const UNIT: &str = "reporter";
/// The Network unit that answers report engineering.
pub const NETWORK_UNIT: &str = "network";
/// The browser projection of one verified export.
pub const REPLY_SCHEMA: &str = "ds.reporter-browser-export/v1";

pub const REQUEST_INVALID: &str = "reporter_request_invalid";
pub const RESPONSE_INVALID: &str = "reporter_response_invalid";
pub const ENGINE_UNAVAILABLE: &str = "reporter_engine_unavailable";
pub const ENGINE_UPDATE_REQUIRED: &str = "reporter_engine_update_required";
pub const NETWORK_UPDATE_REQUIRED: &str = "network_engine_update_required";
pub const NETWORK_RESPONSE_INVALID: &str = "network_response_invalid";
/// The unit's own refusal naming the Network facts it needs.
pub const ENGINEERING_REQUIRED: &str = "report_engineering_required";
/// The unit kept naming Network facts after they were answered.
pub const ENGINEERING_UNANSWERED: &str = "report_engineering_unanswered";

const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_INPUTS: usize = 4096;
const MAX_INPUT_BYTES: usize = 384 * 1024 * 1024;
const MAX_NAME_CHARS: usize = 4096;
const MAX_REPLY_BYTES: usize = 768 * 1024 * 1024;
const MAX_OUTPUTS: usize = 64;
const MAX_ROUNDS: u32 = 8;
const MAX_ANSWERS: usize = 4096;
const MAX_IDENTITY_BYTES: usize = 16 * 1024;

/// A typed refusal: `code: message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReporterUnitError {
    pub code: String,
    pub message: String,
}
impl std::fmt::Display for ReporterUnitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
fn refuse(code: &str, message: impl Into<String>) -> ReporterUnitError {
    ReporterUnitError {
        code: code.into(),
        message: message.into(),
    }
}

/// The two exports the Web offers, each with its unit entry, envelope and
/// the capability a unit must declare to serve it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportExport {
    Transformer,
    Combined,
}
impl ReportExport {
    pub fn parse(name: &str) -> Result<Self, ReporterUnitError> {
        match name {
            "transformer" => Ok(Self::Transformer),
            "combined" => Ok(Self::Combined),
            _ => Err(refuse(
                REQUEST_INVALID,
                format!("unknown report export {name:?}; expected transformer or combined"),
            )),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Transformer => "transformer",
            Self::Combined => "combined",
        }
    }
    /// The declared unit entry the host calls.
    pub fn entry(self) -> &'static str {
        match self {
            Self::Transformer => "reporter.export.transformer",
            Self::Combined => "reporter.export.combined",
        }
    }
    pub fn capability(self) -> &'static str {
        match self {
            Self::Transformer => "export_transformer_report_v1",
            Self::Combined => "export_combined_report_v1",
        }
    }
    fn envelope_schema(self) -> &'static str {
        match self {
            Self::Transformer => "ds.reporter-held-transformer-export/v1",
            Self::Combined => "ds.reporter-held-combined-export/v1",
        }
    }
    fn result_schema(self) -> &'static str {
        match self {
            Self::Transformer => "ds.reporter-held-transformer-export-result/v1",
            Self::Combined => "ds.reporter-held-combined-export-result/v1",
        }
    }
}

/// One declared effect: the unit, its entry and the request text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReporterEffect {
    pub unit: &'static str,
    pub entry: &'static str,
    pub payload: String,
}

/// One Network fact the export needs: the effect that asks the Network unit
/// and the digest its answer is held under.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkAsk {
    pub request_digest: String,
    pub entry: GridEntry,
    pub payload: Vec<u8>,
}

/// One verified export output.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportFile {
    /// The file name to save (the artifact's own `filename`).
    pub name: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

/// The verified export: its browser projection and every output.
#[derive(Debug, Clone)]
pub struct ReportExportReply {
    pub summary: Value,
    pub files: Vec<ReportFile>,
}

/// What the host does next.
#[derive(Debug, Clone)]
pub enum ReportStep {
    /// Ask the Network unit each fact, answer it with [`ReporterExport::network_reply`],
    /// then call the Reporter unit again with [`ReporterExport::effect`].
    Network(Vec<NetworkAsk>),
    Completed(Box<ReportExportReply>),
}

#[derive(Serialize)]
struct HeldInput<'a> {
    name: &'a str,
    body_base64: &'a str,
}
#[derive(Serialize)]
struct HeldAnswer {
    request_digest: String,
    result_json: String,
    receipt: Value,
}
#[derive(Serialize)]
struct Envelope<'a> {
    schema: &'static str,
    request: &'a RawValue,
    inputs: Vec<HeldInput<'a>>,
    network_results: &'a [HeldAnswer],
}

/// One browser export: the admitted request, its held inputs and the Network
/// answers gathered so far.
pub struct ReporterExport {
    export: ReportExport,
    request: Box<RawValue>,
    inputs: Vec<(String, String)>,
    input_bytes: usize,
    answers: Vec<HeldAnswer>,
    answered: BTreeSet<String>,
    pending: BTreeSet<String>,
    rounds: u32,
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Admit the loaded unit by the capability the export calls. An older unit
/// that lacks it is `reporter_engine_update_required`.
pub fn admit_unit(export: ReportExport, identity: &str) -> Result<(), ReporterUnitError> {
    if identity.len() > MAX_IDENTITY_BYTES {
        return Err(refuse(
            ENGINE_UNAVAILABLE,
            "the unit identity exceeds its bound",
        ));
    }
    let identity: Value = serde_json::from_str(identity)
        .map_err(|_| refuse(ENGINE_UNAVAILABLE, "the unit reports no readable identity"))?;
    if identity["schema"] != "ds.reporter-engine/v1" {
        return Err(refuse(
            ENGINE_UNAVAILABLE,
            "the loaded unit is not the governed Reporter unit",
        ));
    }
    let declared = identity["capabilities"]
        .as_array()
        .is_some_and(|all| all.iter().any(|item| item == export.capability()));
    if !declared {
        return Err(refuse(
            ENGINE_UPDATE_REQUIRED,
            format!(
                "the selected Reporter unit does not export {} reports ({}); update the Reporter unit",
                export.name(),
                export.capability()
            ),
        ));
    }
    Ok(())
}

impl ReporterExport {
    /// Admit one export: its kind, the native task request (a JSON object
    /// whose paths are held names) and the loaded unit's identity.
    pub fn new(export: &str, request: &[u8], identity: &str) -> Result<Self, ReporterUnitError> {
        let export = ReportExport::parse(export)?;
        admit_unit(export, identity)?;
        if request.len() > MAX_REQUEST_BYTES {
            return Err(refuse(
                REQUEST_INVALID,
                "the export request exceeds its bound",
            ));
        }
        let text = std::str::from_utf8(request)
            .map_err(|_| refuse(REQUEST_INVALID, "the export request is not UTF-8 JSON"))?;
        let raw: Box<RawValue> = serde_json::from_str(text.trim())
            .map_err(|error| refuse(REQUEST_INVALID, format!("the export request: {error}")))?;
        if !raw.get().starts_with('{') {
            return Err(refuse(
                REQUEST_INVALID,
                "the export request is not a JSON object",
            ));
        }
        Ok(Self {
            export,
            request: raw,
            inputs: Vec::new(),
            input_bytes: 0,
            answers: Vec::new(),
            answered: BTreeSet::new(),
            pending: BTreeSet::new(),
            rounds: 0,
        })
    }

    pub fn export(&self) -> ReportExport {
        self.export
    }

    /// Hold one named input the request uses, under the exact name it uses.
    pub fn hold(&mut self, name: &str, bytes: &[u8]) -> Result<(), ReporterUnitError> {
        if name.is_empty() || name.chars().count() > MAX_NAME_CHARS || name.contains('\0') {
            return Err(refuse(
                REQUEST_INVALID,
                "a held input needs a 1..4096 character name",
            ));
        }
        if self.inputs.iter().any(|(held, _)| held == name) {
            return Err(refuse(
                REQUEST_INVALID,
                format!("held input {name} is supplied twice"),
            ));
        }
        if self.inputs.len() >= MAX_INPUTS || self.input_bytes + bytes.len() > MAX_INPUT_BYTES {
            return Err(refuse(
                REQUEST_INVALID,
                "the held inputs exceed their bound",
            ));
        }
        self.input_bytes += bytes.len();
        self.inputs.push((
            name.to_owned(),
            base64::engine::general_purpose::STANDARD.encode(bytes),
        ));
        Ok(())
    }

    /// The Reporter unit effect with every input and Network answer held.
    pub fn effect(&self) -> Result<ReporterEffect, ReporterUnitError> {
        if !self.pending.is_empty() {
            return Err(refuse(
                REQUEST_INVALID,
                format!(
                    "{} Network fact(s) are still unanswered",
                    self.pending.len()
                ),
            ));
        }
        let envelope = Envelope {
            schema: self.export.envelope_schema(),
            request: &self.request,
            inputs: self
                .inputs
                .iter()
                .map(|(name, body)| HeldInput {
                    name,
                    body_base64: body,
                })
                .collect(),
            network_results: &self.answers,
        };
        let payload = serde_json::to_string(&envelope)
            .map_err(|error| refuse(REQUEST_INVALID, error.to_string()))?;
        Ok(ReporterEffect {
            unit: UNIT,
            entry: self.export.entry(),
            payload,
        })
    }

    /// Decode the Reporter unit's answer; `refused` is the text it threw.
    /// A refusal naming Network facts becomes the Network step; any other
    /// refusal keeps the unit's own typed code.
    pub fn reporter_reply(
        &mut self,
        reply: &str,
        refused: bool,
    ) -> Result<ReportStep, ReporterUnitError> {
        if reply.len() > MAX_REPLY_BYTES {
            return Err(refuse(RESPONSE_INVALID, "the unit reply exceeds its bound"));
        }
        if refused {
            return self.refusal(reply).map(ReportStep::Network);
        }
        self.completed(reply)
            .map(|reply| ReportStep::Completed(Box::new(reply)))
    }

    fn refusal(&mut self, reply: &str) -> Result<Vec<NetworkAsk>, ReporterUnitError> {
        #[derive(Deserialize)]
        struct Required {
            request_digest: String,
            request_json: String,
        }
        #[derive(Deserialize)]
        struct UnitRefusal {
            code: String,
            message: String,
            #[serde(default)]
            requests: Option<Vec<Required>>,
        }
        let unit: UnitRefusal = serde_json::from_str(reply)
            .map_err(|_| refuse(RESPONSE_INVALID, "the unit refused without a typed error"))?;
        if unit.code.is_empty()
            || unit.code.len() > 64
            || !unit
                .code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(refuse(
                RESPONSE_INVALID,
                "the unit refused with an unnamed error",
            ));
        }
        if unit.code != ENGINEERING_REQUIRED {
            let message: String = unit.message.chars().take(2000).collect();
            return Err(refuse(&unit.code, message));
        }
        let requests = unit.requests.unwrap_or_default();
        if requests.is_empty() {
            return Err(refuse(
                RESPONSE_INVALID,
                "the unit named no Network fact it needs",
            ));
        }
        self.rounds += 1;
        if self.rounds > MAX_ROUNDS
            || self.answered.len() + requests.len() > MAX_ANSWERS
            || requests
                .iter()
                .any(|required| self.answered.contains(&required.request_digest))
        {
            return Err(refuse(
                ENGINEERING_UNANSWERED,
                "the Reporter unit keeps naming Network facts it was given",
            ));
        }
        let mut asks = Vec::with_capacity(requests.len());
        for required in requests {
            if !hex64(&required.request_digest)
                || !self.pending.insert(required.request_digest.clone())
            {
                return Err(refuse(
                    RESPONSE_INVALID,
                    "a Network fact needs one distinct 64-hex request digest",
                ));
            }
            let operation = GridOperation::Model(GridModelMethod::ReportEngineering);
            // The request crosses as the unit wrote it: its digest is over
            // its canonical JSON, and the Network unit decodes these bytes.
            let request = GridRequest::new(
                operation,
                vec![GridArg::Bytes(required.request_json.into_bytes())],
            )
            .map_err(|refusal| refuse(RESPONSE_INVALID, refusal.message))?;
            asks.push(NetworkAsk {
                request_digest: required.request_digest,
                entry: operation.entry(),
                payload: request.encode(),
            });
        }
        Ok(asks)
    }

    /// Hold the Network unit's answer to one named fact: the engine's exact
    /// `engineering.json` bytes, admitted against their receipt.
    pub fn network_reply(
        &mut self,
        request_digest: &str,
        reply: &[u8],
    ) -> Result<(), ReporterUnitError> {
        if !self.pending.contains(request_digest) {
            return Err(refuse(
                REQUEST_INVALID,
                "this Network answer is for no fact the export asked",
            ));
        }
        let operation = GridOperation::Model(GridModelMethod::ReportEngineering);
        let (bytes, receipt_json) = match decode_grid_reply(operation, reply) {
            Err(malformed) => return Err(refuse(NETWORK_RESPONSE_INVALID, malformed.message)),
            Ok(Err(refusal)) if refusal.code == METHOD_UNAVAILABLE => {
                return Err(refuse(
                    NETWORK_UPDATE_REQUIRED,
                    "the selected Network unit does not answer report engineering; update the Network unit",
                ))
            }
            Ok(Err(refusal)) => return Err(refuse(&refusal.code, refusal.message)),
            Ok(Ok(GridResult::Artifact {
                bytes,
                receipt_json,
            })) => (bytes, receipt_json),
            Ok(Ok(_)) => {
                return Err(refuse(
                    NETWORK_RESPONSE_INVALID,
                    "report engineering answers its artifact and receipt",
                ))
            }
        };
        let receipt: Value = serde_json::from_str(&receipt_json)
            .map_err(|_| refuse(NETWORK_RESPONSE_INVALID, "the Network receipt is not JSON"))?;
        let artifact = &receipt["artifact"];
        if artifact["sha256"].as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str())
            || artifact["byteCount"].as_u64() != Some(bytes.len() as u64)
        {
            return Err(refuse(
                NETWORK_RESPONSE_INVALID,
                "the Network answer is not the artifact its receipt names",
            ));
        }
        let result_json = String::from_utf8(bytes)
            .map_err(|_| refuse(NETWORK_RESPONSE_INVALID, "the Network answer is not UTF-8"))?;
        self.pending.remove(request_digest);
        self.answered.insert(request_digest.to_owned());
        self.answers.push(HeldAnswer {
            request_digest: request_digest.to_owned(),
            result_json,
            receipt,
        });
        Ok(())
    }

    fn completed(&self, reply: &str) -> Result<ReportExportReply, ReporterUnitError> {
        let invalid = |message: String| refuse(RESPONSE_INVALID, message);
        let mut value: Value = serde_json::from_str(reply)
            .map_err(|_| invalid("the unit reply is not an export result".into()))?;
        if value["schema"] != self.export.result_schema() {
            return Err(invalid(format!(
                "the unit reply is not {}",
                self.export.result_schema()
            )));
        }
        let result = value["result"].take();
        let artifacts = result["artifacts"]
            .as_array()
            .ok_or_else(|| invalid("the export result names no artifacts".into()))?;
        let outputs = match value["outputs"].take() {
            Value::Array(outputs) if outputs.len() <= MAX_OUTPUTS => outputs,
            _ => return Err(invalid("the unit reply has no bounded output list".into())),
        };
        if outputs.len() != artifacts.len() {
            return Err(invalid(format!(
                "the unit answered {} outputs for {} artifacts",
                outputs.len(),
                artifacts.len()
            )));
        }
        let mut files: Vec<ReportFile> = Vec::with_capacity(outputs.len());
        for output in outputs {
            let path = output["path"]
                .as_str()
                .ok_or_else(|| invalid("an output has no path".into()))?;
            let artifact = artifacts
                .iter()
                .find(|artifact| artifact["path"] == path)
                .ok_or_else(|| invalid(format!("{path} is no artifact the result names")))?;
            let name = artifact["filename"]
                .as_str()
                .filter(|name| {
                    !name.is_empty()
                        && name.len() <= 255
                        && !name.contains(['/', '\\', '\0'])
                        && *name != "."
                        && *name != ".."
                })
                .ok_or_else(|| invalid(format!("{path} has no safe file name")))?;
            let bytes = output["body_base64"]
                .as_str()
                .and_then(|body| base64::engine::general_purpose::STANDARD.decode(body).ok())
                .ok_or_else(|| invalid(format!("{name} carries no base64 bytes")))?;
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            if artifact["sha256"].as_str() != Some(sha256.as_str())
                || artifact["bytes"].as_u64() != Some(bytes.len() as u64)
                || files.iter().any(|file| file.name == name)
            {
                return Err(invalid(format!("{name} differs from its artifact")));
            }
            files.push(ReportFile {
                name: name.to_owned(),
                sha256,
                bytes,
            });
        }
        let summary = json!({
            "schema": REPLY_SCHEMA,
            "export": self.export.name(),
            "status": result["status"],
            "blockers": result["blockers"],
            "network_answers": self.answers.len(),
            "files": files.iter().map(|file| json!({
                "name": file.name, "sha256": file.sha256, "byte_count": file.bytes.len(),
            })).collect::<Vec<_>>(),
            "result": result,
        });
        Ok(ReportExportReply { summary, files })
    }
}

/// The refusal a host reports when no admitted unit with the entry is held.
pub fn unavailable(reason: &str) -> ReporterUnitError {
    refuse(
        ENGINE_UNAVAILABLE,
        format!("Report exports need the governed Reporter unit: {reason}"),
    )
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use wasm_bindgen::prelude::*;

    fn error(refusal: ReporterUnitError) -> JsError {
        JsError::new(&refusal.to_string())
    }
    fn set(target: &js_sys::Object, key: &str, value: &JsValue) -> Result<(), JsError> {
        js_sys::Reflect::set(target, &JsValue::from_str(key), value)
            .map(|_| ())
            .map_err(|_| JsError::new("reporter_request_invalid: could not assemble the effect"))
    }

    /// One browser export the host drives: `effect()` to the Reporter unit,
    /// `reporterReply()` with its answer, each Network ask's reply through
    /// `networkReply()`, until the step is `completed`.
    #[wasm_bindgen]
    pub struct ReporterExportSession {
        export: ReporterExport,
        reply: Option<ReportExportReply>,
    }

    #[wasm_bindgen]
    impl ReporterExportSession {
        /// Admit the export (`transformer` | `combined`), the native task
        /// request and the identity the loaded Reporter unit exported.
        #[wasm_bindgen(constructor)]
        pub fn new(
            export: &str,
            request: &[u8],
            identity: &str,
        ) -> Result<ReporterExportSession, JsError> {
            Ok(Self {
                export: ReporterExport::new(export, request, identity).map_err(error)?,
                reply: None,
            })
        }
        /// Hold one input under the exact name the request uses.
        pub fn hold(&mut self, name: &str, bytes: &[u8]) -> Result<(), JsError> {
            self.export.hold(name, bytes).map_err(error)
        }
        /// The Reporter unit effect: `{ unit: 'reporter', entry, payload }`.
        #[wasm_bindgen(
            unchecked_return_type = "{ unit: 'reporter'; entry: 'reporter.export.transformer' | 'reporter.export.combined'; payload: string }"
        )]
        pub fn effect(&self) -> Result<JsValue, JsError> {
            let effect = self.export.effect().map_err(error)?;
            let value = js_sys::Object::new();
            set(&value, "unit", &JsValue::from_str(effect.unit))?;
            set(&value, "entry", &JsValue::from_str(effect.entry))?;
            set(&value, "payload", &JsValue::from_str(&effect.payload))?;
            Ok(value.into())
        }
        /// The unit's answer (`refused`: the text it threw). Returns the Network
        /// asks, `{ request_digest, unit: 'network', entry: 'grid.model', payload }[]`,
        /// or an empty list once the export completed.
        #[wasm_bindgen(js_name = reporterReply, unchecked_return_type = "{ request_digest: string; unit: 'network'; entry: 'grid.model'; payload: Uint8Array }[]")]
        pub fn reporter_reply(&mut self, reply: &str, refused: bool) -> Result<JsValue, JsError> {
            let asks = js_sys::Array::new();
            match self.export.reporter_reply(reply, refused).map_err(error)? {
                ReportStep::Completed(reply) => self.reply = Some(*reply),
                ReportStep::Network(network) => {
                    for ask in network {
                        let value = js_sys::Object::new();
                        set(
                            &value,
                            "request_digest",
                            &JsValue::from_str(&ask.request_digest),
                        )?;
                        set(&value, "unit", &JsValue::from_str(NETWORK_UNIT))?;
                        set(&value, "entry", &JsValue::from_str("grid.model"))?;
                        set(
                            &value,
                            "payload",
                            &js_sys::Uint8Array::from(ask.payload.as_slice()),
                        )?;
                        asks.push(&value);
                    }
                }
            }
            Ok(asks.into())
        }
        /// The Network unit's reply bytes to one ask.
        #[wasm_bindgen(js_name = networkReply)]
        pub fn network_reply(&mut self, request_digest: &str, reply: &[u8]) -> Result<(), JsError> {
            self.export
                .network_reply(request_digest, reply)
                .map_err(error)
        }
        /// `ds.reporter-browser-export/v1` JSON text, once completed.
        pub fn summary(&self) -> Option<String> {
            self.reply.as_ref().map(|reply| reply.summary.to_string())
        }
        /// The verified file names to download, once completed.
        #[wasm_bindgen(js_name = fileNames)]
        pub fn file_names(&self) -> Vec<String> {
            self.reply
                .iter()
                .flat_map(|reply| reply.files.iter().map(|file| file.name.clone()))
                .collect()
        }
        /// The verified bytes of one named file.
        pub fn file(&self, name: &str) -> Option<Vec<u8>> {
            self.reply
                .as_ref()?
                .files
                .iter()
                .find(|file| file.name == name)
                .map(|file| file.bytes.clone())
        }
    }

    /// The typed refusal for a host that holds no admitted unit or entry.
    #[wasm_bindgen(js_name = reporterEngineUnavailable)]
    pub fn engine_unavailable(reason: &str) -> JsError {
        error(unavailable(reason))
    }
}

#[cfg(test)]
#[path = "reporter_unit_tests.rs"]
mod tests;
