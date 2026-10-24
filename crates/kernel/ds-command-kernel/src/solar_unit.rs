//! Browser Solar city calculation through the governed Solar unit.
//!
//! Owner 24 Oct: Web computes Solar. The Kernel admits one Core-prepared city
//! and the run settings a native Core run uses, and names the effect a host
//! executes: `{ unit: "solar", entry, payload }`. The host calls only that
//! entry of the unit it holds and returns the reply text here, where every
//! output is checked against its own descriptor before any host reads it. The
//! Kernel computes no Solar result: the prepared input crosses verbatim, so
//! its content digests are the preparer's, and the reply is the unit's.
//! Section reads use the dashboard's own bounded projection.
use base64::Engine as _;
use ds_solar_contracts::PreparedSolarCityInput;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

/// The governed unit that answers browser Solar calculations.
pub const UNIT: &str = "solar";
/// Its declared prepared-city entry (`calculate_prepared_city_json`).
pub const ENTRY: &str = "solar.prepared.city";
/// The browser projection of one admitted city reply.
pub const REPLY_SCHEMA: &str = "ds.solar-browser-calculation/v1";
/// The unit's own request bound (`ds_solar_wasm::MAX_REQUEST_BYTES`).
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
const MAX_REPLY_BYTES: usize = 256 * 1024 * 1024;
const MAX_FILES: usize = 512;
const MAX_ID_CHARS: usize = 256;
const DRAFTS: [&str; 4] = ["apd", "network", "plant", "financial"];

pub const REQUEST_INVALID: &str = "solar_request_invalid";
pub const RESPONSE_INVALID: &str = "solar_response_invalid";
pub const ENGINE_UNAVAILABLE: &str = "solar_engine_unavailable";

/// A typed refusal: `code: message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolarUnitError {
    pub code: String,
    pub message: String,
}
impl std::fmt::Display for SolarUnitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
fn refuse(code: &str, message: impl Into<String>) -> SolarUnitError {
    SolarUnitError {
        code: code.into(),
        message: message.into(),
    }
}

/// The page's run settings: the same choices and defaults as a native Core
/// calculation (`RunSettings`): charts and documents on, the APD draft.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSettings {
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub render_charts: Option<bool>,
    #[serde(default)]
    pub generate_documents: Option<bool>,
    #[serde(default)]
    pub report_drafts: Vec<String>,
}

/// One declared effect: the unit, its entry and the request text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SolarEffect {
    pub unit: &'static str,
    pub entry: &'static str,
    pub payload: String,
}

/// The unit options for the page's run settings, admitted before any input
/// is read.
pub fn run_options(settings: &[u8]) -> Result<Value, SolarUnitError> {
    let settings: RunSettings = if settings.is_empty() {
        RunSettings::default()
    } else {
        serde_json::from_slice(settings).map_err(|error| {
            refuse(
                REQUEST_INVALID,
                format!("invalid Solar run settings: {error}"),
            )
        })?
    };
    if settings.run_id.as_ref().is_some_and(|id| {
        id.is_empty()
            || id.chars().count() > MAX_ID_CHARS
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    }) {
        return Err(refuse(
            REQUEST_INVALID,
            "run id must be 1..256 ASCII letters, digits, '-', '_' or '.'",
        ));
    }
    let requested = if settings.report_drafts.is_empty() {
        vec!["apd".to_string()]
    } else {
        settings.report_drafts
    };
    let mut drafts = Vec::new();
    for draft in requested {
        if !DRAFTS.contains(&draft.as_str()) {
            return Err(refuse(
                REQUEST_INVALID,
                format!("unsupported Solar draft report intent: {draft}"),
            ));
        }
        if !drafts.contains(&draft) {
            drafts.push(draft);
        }
    }
    Ok(json!({
        "run_id": settings.run_id,
        "render_charts": settings.render_charts.unwrap_or(true),
        "generate_documents": settings.generate_documents.unwrap_or(true),
        "report_drafts": drafts,
    }))
}

/// Admit one prepared city and the run settings, and build the unit request.
pub fn calculate_effect(prepared: &[u8], settings: &[u8]) -> Result<SolarEffect, SolarUnitError> {
    let options = run_options(settings)?;
    let text = std::str::from_utf8(prepared)
        .map_err(|_| refuse(REQUEST_INVALID, "the prepared city is not UTF-8 JSON"))?;
    let city: PreparedSolarCityInput = serde_json::from_str(text).map_err(|error| {
        refuse(
            REQUEST_INVALID,
            format!("the input is not a Core-prepared Solar city: {error}"),
        )
    })?;
    if city.input_digest.is_empty() {
        return Err(refuse(
            REQUEST_INVALID,
            "the city has no prepared input digest; prepare it on Core first",
        ));
    }
    // The prepared text crosses verbatim: re-encoding could reorder keys or
    // change float text, and its content digests are over these bytes.
    let payload = format!(r#"{{"prepared":{},"options":{options}}}"#, text.trim());
    if payload.len() > MAX_REQUEST_BYTES {
        return Err(refuse(
            REQUEST_INVALID,
            "the prepared city exceeds the unit's 64 MiB request bound",
        ));
    }
    Ok(SolarEffect {
        unit: UNIT,
        entry: ENTRY,
        payload,
    })
}

/// One verified output: its descriptor and bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct SolarFile {
    pub entry: Value,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The admitted reply: the browser projection and every verified output.
#[derive(Debug, Clone)]
pub struct SolarCityReply {
    pub summary: Value,
    pub files: Vec<SolarFile>,
}

fn digest_text(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| {
        text.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    })
}

fn invalid(message: impl Into<String>) -> SolarUnitError {
    refuse(RESPONSE_INVALID, message)
}

/// Decode the unit's answer. A refusal (`refused`) keeps the unit's own typed
/// Solar code; a malformed, incomplete or corrupt answer is
/// `solar_response_invalid`.
pub fn decode_reply(reply: &str, refused: bool) -> Result<SolarCityReply, SolarUnitError> {
    if reply.len() > MAX_REPLY_BYTES {
        return Err(invalid("the unit reply exceeds its bound"));
    }
    if refused {
        #[derive(Deserialize)]
        struct UnitRefusal {
            code: String,
            message: String,
        }
        let unit: UnitRefusal = serde_json::from_str(reply)
            .map_err(|_| invalid("the unit refused without a typed Solar error"))?;
        if unit.code.is_empty()
            || unit.code.len() > 64
            || !unit
                .code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_')
        {
            return Err(invalid("the unit refused with an unnamed Solar error"));
        }
        let message: String = unit.message.chars().take(2000).collect();
        return Err(refuse(&format!("solar_{}", unit.code), message));
    }
    let mut value: Map<String, Value> = serde_json::from_str(reply)
        .map_err(|_| invalid("the unit reply is not a Solar calculation"))?;
    if !value
        .get("schema_version")
        .and_then(Value::as_str)
        .is_some_and(|schema| schema.starts_with("ds-solar.result/"))
    {
        return Err(invalid("the unit reply is not a Solar result"));
    }
    let digest = value
        .get("result_digest")
        .and_then(digest_text)
        .ok_or_else(|| invalid("the unit reply has no sealed result digest"))?
        .to_owned();
    let outputs = match value.remove("outputs") {
        Some(Value::Array(outputs)) if outputs.len() <= MAX_FILES => outputs,
        _ => return Err(invalid("the unit reply has no bounded output list")),
    };
    let mut files = Vec::with_capacity(outputs.len());
    for output in outputs {
        let entry = output
            .get("entry")
            .cloned()
            .ok_or_else(|| invalid("an output has no descriptor"))?;
        let name = entry["name"]
            .as_str()
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 1024
                    && name
                        .split('/')
                        .all(|part| !part.is_empty() && part != "." && part != "..")
            })
            .ok_or_else(|| invalid("an output name is not a safe relative name"))?
            .to_owned();
        let body = output["body_base64"]
            .as_str()
            .ok_or_else(|| invalid(format!("{name} carries no bytes")))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(body)
            .map_err(|_| invalid(format!("{name} bytes are not base64")))?;
        let expected = digest_text(&entry["content_digest"])
            .ok_or_else(|| invalid(format!("{name} has no content digest")))?;
        if entry["byte_count"].as_u64() != Some(bytes.len() as u64)
            || expected != format!("sha256:{:x}", Sha256::digest(&bytes))
            || files.iter().any(|file: &SolarFile| file.name == name)
        {
            return Err(invalid(format!("{name} differs from its descriptor")));
        }
        files.push(SolarFile { entry, name, bytes });
    }
    if !files.iter().any(|file| file.name == "report_input.json") {
        return Err(invalid("the unit reply has no report input"));
    }
    let timings = value.remove("timings").unwrap_or(Value::Null);
    let plots = value.remove("plots_generated").unwrap_or(Value::Null);
    let summary = json!({
        "schema": REPLY_SCHEMA,
        "result_digest": digest,
        "plots_generated": plots,
        "timings": timings,
        "files": files.iter().map(|file| file.entry.clone()).collect::<Vec<_>>(),
        "result": Value::Object(value),
    });
    Ok(SolarCityReply { summary, files })
}

/// One bounded result section of the verified report input, the same
/// projection `ds solar results read` and the dashboards use.
pub fn read_section(
    report_input: &[u8],
    section: &str,
    path: &[String],
) -> Result<Value, SolarUnitError> {
    let report: Value = serde_json::from_slice(report_input)
        .map_err(|_| refuse(REQUEST_INVALID, "the report input is not JSON"))?;
    crate::solar_dashboard::read_section(&report, section, path)
        .map_err(|error| refuse("solar_dashboard_path_invalid", error))
}

/// The refusal a host reports when no admitted unit with the entry is held.
pub fn unavailable(reason: &str) -> SolarUnitError {
    refuse(
        ENGINE_UNAVAILABLE,
        format!("Solar calculation needs the governed Solar unit: {reason}"),
    )
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use wasm_bindgen::prelude::*;

    fn error(refusal: SolarUnitError) -> JsError {
        JsError::new(&refusal.to_string())
    }

    /// The Solar unit effect for one prepared city and its run settings.
    #[wasm_bindgen(js_name = solarCalculateEffect, unchecked_return_type = "{ unit: 'solar'; entry: 'solar.prepared.city'; payload: string }")]
    pub fn effect(prepared: &[u8], settings: &[u8]) -> Result<JsValue, JsError> {
        let effect = calculate_effect(prepared, settings).map_err(error)?;
        let value = js_sys::Object::new();
        for (key, item) in [
            ("unit", JsValue::from_str(effect.unit)),
            ("entry", JsValue::from_str(effect.entry)),
            ("payload", JsValue::from_str(&effect.payload)),
        ] {
            js_sys::Reflect::set(&value, &JsValue::from_str(key), &item).map_err(|_| {
                JsError::new("solar_request_invalid: could not assemble the effect")
            })?;
        }
        Ok(value.into())
    }

    /// The admitted reply: its projection and each verified output.
    #[wasm_bindgen]
    pub struct SolarCalculation {
        reply: SolarCityReply,
    }
    #[wasm_bindgen]
    impl SolarCalculation {
        /// `ds.solar-browser-calculation/v1` JSON text.
        pub fn summary(&self) -> String {
            self.reply.summary.to_string()
        }
        /// The verified bytes of one named output.
        pub fn file(&self, name: &str) -> Option<Vec<u8>> {
            self.reply
                .files
                .iter()
                .find(|file| file.name == name)
                .map(|file| file.bytes.clone())
        }
    }

    /// Decode the unit's reply text; `refused` is the text the unit threw.
    #[wasm_bindgen(js_name = decodeSolarCalculateReply)]
    pub fn decode(reply: &str, refused: bool) -> Result<SolarCalculation, JsError> {
        decode_reply(reply, refused)
            .map(|reply| SolarCalculation { reply })
            .map_err(error)
    }

    /// One bounded section of a verified report input.
    #[wasm_bindgen(js_name = solarResultSection)]
    pub fn section(
        report_input: &[u8],
        section: &str,
        path: Vec<String>,
    ) -> Result<String, JsError> {
        read_section(report_input, section, &path)
            .map(|value| value.to_string())
            .map_err(error)
    }

    /// The typed refusal for a host that holds no admitted unit or entry.
    #[wasm_bindgen(js_name = solarEngineUnavailable)]
    pub fn engine_unavailable(reason: &str) -> JsError {
        error(unavailable(reason))
    }
}

#[cfg(test)]
#[path = "solar_unit_tests.rs"]
mod tests;
