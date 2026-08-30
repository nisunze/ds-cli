//! `ds solar result compare` — validate and compare two sealed local results.

use std::ffi::OsString;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value};

use crate::{DISCOVERY_TIMEOUT, DS_SOLAR};

const RESULT_SCHEMA: &str = "ds-solar.result/v1";
const COMPARISON_SCHEMA: &str = "ds-solar.result-comparison/v1";
const TEXT_CHARS: usize = 512;

pub static COMMAND: Command = Command {
    id: "solar.result.compare",
    path: &["solar", "result", "compare"],
    contract: 1,
    summary: "Compare two sealed native Solar results headlessly.",
    purpose: "\
Validates both complete result documents through the native Solar authority, \
then compares their canonical result digests and returns a bounded provenance \
receipt. This is local artifact equality only: it grants no project membership, \
publication or mutation authority and does not contact the desktop or network.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "left",
            "<path>",
            "First sealed ds-solar.result/v1 document.",
        )
        .required(),
        Arg::value(
            "right",
            "<path>",
            "Second sealed ds-solar.result/v1 document.",
        )
        .required(),
    ],
    output: "Whether the canonical digests match, plus each result's bounded project/city, input, weather and engine provenance. Local paths and full calculation payloads are not returned.",
    examples: &[Example {
        command: "ds solar result compare --left ./server/result.json --right ./desktop/result.json --output json",
        note: "A valid difference is returned as equal: false, not as a refusal.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "solar_engine_missing",
            when: "the packaged `ds-solar` sidecar cannot be found",
            remedy: "reinstall DS GridDesign, or set DS_SOLAR_BIN for development",
        },
        Refusal {
            code: "left_result_not_found",
            when: "--left does not name a file",
            remedy: "supply a sealed ds-solar result document",
        },
        Refusal {
            code: "right_result_not_found",
            when: "--right does not name a file",
            remedy: "supply a sealed ds-solar result document",
        },
        Refusal {
            code: "engine_refused",
            when: "a result is unreadable, oversized, schema-incompatible or fails canonical digest validation",
            remedy: "read detail.engine; regenerate any changed result through the pinned Solar engine",
        },
        Refusal {
            code: "callee_contract_mismatch",
            when: "the packaged engine does not return the closed comparison receipt",
            remedy: "update ds and DS GridDesign to one matching release",
        },
        Refusal {
            code: "callee_timed_out",
            when: "the native comparison does not finish within 20 seconds",
            remedy: "retry on local storage or inspect the Solar engine",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    availability,
};

fn availability() -> Availability {
    DS_SOLAR.availability()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let left = inputs.require("left")?;
    let right = inputs.require("right")?;
    require_file(left, "left_result_not_found", "--left")?;
    require_file(right, "right_result_not_found", "--right")?;

    let args = vec![
        OsString::from("--left"),
        OsString::from(left),
        OsString::from("--right"),
        OsString::from(right),
        OsString::from("--json"),
    ];
    let receipt = DS_SOLAR.call_json("compare", &args, DISCOVERY_TIMEOUT)?;
    validate_receipt(&receipt)?;
    Ok(receipt)
}

fn require_file(value: &str, code: &'static str, flag: &str) -> Result<(), Failure> {
    if Path::new(value).is_file() {
        return Ok(());
    }
    Err(Failure::invalid(code, format!("`{value}` is not a file"))
        .remedy(format!("supply a sealed Solar result with {flag}")))
}

fn mismatch(detail: impl Into<String>) -> Failure {
    Failure::failed("callee_contract_mismatch", detail)
        .remedy("update ds and DS GridDesign to one matching release")
}

fn validate_receipt(receipt: &Value) -> Result<(), Failure> {
    let root = exact_object(
        receipt,
        &["schema", "equal", "left", "right"],
        "comparison receipt",
    )?;
    if root.get("schema").and_then(Value::as_str) != Some(COMPARISON_SCHEMA) {
        return Err(mismatch(format!(
            "comparison receipt schema is not {COMPARISON_SCHEMA}"
        )));
    }
    let equal = root
        .get("equal")
        .and_then(Value::as_bool)
        .ok_or_else(|| mismatch("comparison receipt equal is not a boolean"))?;
    validate_side(
        root.get("left")
            .ok_or_else(|| mismatch("comparison receipt is missing left"))?,
        "left",
    )?;
    validate_side(
        root.get("right")
            .ok_or_else(|| mismatch("comparison receipt is missing right"))?,
        "right",
    )?;
    let digests_match = receipt["left"]["result_digest"] == receipt["right"]["result_digest"];
    if equal != digests_match {
        return Err(mismatch(
            "comparison receipt equal disagrees with its canonical result digests",
        ));
    }
    Ok(())
}

fn validate_side(side: &Value, label: &str) -> Result<(), Failure> {
    let side = exact_object(
        side,
        &[
            "schema_version",
            "result_digest",
            "input_digest",
            "weather_digest",
            "identity",
            "engine",
        ],
        label,
    )?;
    if side.get("schema_version").and_then(Value::as_str) != Some(RESULT_SCHEMA) {
        return Err(mismatch(format!(
            "{label}.schema_version is not {RESULT_SCHEMA}"
        )));
    }
    for field in ["result_digest", "input_digest", "weather_digest"] {
        let value = side.get(field).and_then(Value::as_str);
        if value.is_none_or(|value| !crate::exports::valid_sha256_digest(value)) {
            return Err(mismatch(format!(
                "{label}.{field} is not a canonical SHA-256 digest"
            )));
        }
    }

    let identity = exact_object(
        side.get("identity")
            .ok_or_else(|| mismatch(format!("{label}.identity is missing")))?,
        &["project_id", "root", "city_id", "display_name"],
        &format!("{label}.identity"),
    )?;
    for field in ["project_id", "root", "city_id", "display_name"] {
        bounded_text(identity, field, &format!("{label}.identity"), false)?;
    }

    let engine = exact_object(
        side.get("engine")
            .ok_or_else(|| mismatch(format!("{label}.engine is missing")))?,
        &["name", "version", "source_sha", "build_manifest_sha256"],
        &format!("{label}.engine"),
    )?;
    if bounded_text(engine, "name", &format!("{label}.engine"), false)? != "ds-solar-engine" {
        return Err(mismatch(format!(
            "{label}.engine.name is not ds-solar-engine"
        )));
    }
    bounded_text(engine, "version", &format!("{label}.engine"), false)?;
    let source_sha = bounded_text(engine, "source_sha", &format!("{label}.engine"), true)?;
    if !source_sha.is_empty() && !lower_hex(source_sha, 40) {
        return Err(mismatch(format!("{label}.engine.source_sha is invalid")));
    }
    let manifest = bounded_text(
        engine,
        "build_manifest_sha256",
        &format!("{label}.engine"),
        true,
    )?;
    if !manifest.is_empty() && !lower_hex(manifest, 64) {
        return Err(mismatch(format!(
            "{label}.engine.build_manifest_sha256 is invalid"
        )));
    }
    Ok(())
}

fn lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn exact_object<'a>(
    value: &'a Value,
    fields: &[&str],
    label: &str,
) -> Result<&'a Map<String, Value>, Failure> {
    let object = value
        .as_object()
        .ok_or_else(|| mismatch(format!("{label} is not an object")))?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(mismatch(format!(
            "{label} fields do not match the closed contract"
        )));
    }
    Ok(object)
}

fn bounded_text<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    label: &str,
    allow_empty: bool,
) -> Result<&'a str, Failure> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| {
            (allow_empty || !value.is_empty())
                && value.trim() == *value
                && value.chars().count() <= TEXT_CHARS
                && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| mismatch(format!("{label}.{field} is invalid")))
}

pub fn render(data: &Value) -> String {
    let state = if data["equal"].as_bool().unwrap_or(false) {
        "identical"
    } else {
        "different"
    };
    let source = |side: &str| {
        data[side]["engine"]["source_sha"]
            .as_str()
            .filter(|value| !value.is_empty())
            .unwrap_or("development")
    };
    format!(
        "{state}\n  left   {}  {}  source {}\n  right  {}  {}  source {}",
        data["left"]["result_digest"].as_str().unwrap_or(""),
        data["left"]["identity"]["city_id"].as_str().unwrap_or(""),
        source("left"),
        data["right"]["result_digest"].as_str().unwrap_or(""),
        data["right"]["identity"]["city_id"].as_str().unwrap_or(""),
        source("right"),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::validate_receipt;

    fn receipt() -> Value {
        let side = json!({
            "schema_version": "ds-solar.result/v1",
            "result_digest": format!("sha256:{}", "a".repeat(64)),
            "input_digest": format!("sha256:{}", "b".repeat(64)),
            "weather_digest": format!("sha256:{}", "c".repeat(64)),
            "identity": {
                "project_id": "project-1",
                "root": "solar",
                "city_id": "kigali",
                "display_name": "Kigali",
            },
            "engine": {
                "name": "ds-solar-engine",
                "version": "0.1.0",
                "source_sha": "",
                "build_manifest_sha256": "",
            },
        });
        json!({
            "schema": "ds-solar.result-comparison/v1",
            "equal": true,
            "left": side.clone(),
            "right": side,
        })
    }

    #[test]
    fn accepts_the_closed_native_receipt() {
        validate_receipt(&receipt()).expect("closed receipt validates");
    }

    #[test]
    fn refuses_unknown_fields_and_noncanonical_digests() {
        let mut unknown = receipt();
        unknown["left"]["path"] = json!("/tmp/result.json");
        assert_eq!(
            validate_receipt(&unknown)
                .expect_err("unknown field must refuse")
                .code(),
            "callee_contract_mismatch"
        );

        let mut bad_digest = receipt();
        bad_digest["right"]["result_digest"] = json!("changed");
        assert_eq!(
            validate_receipt(&bad_digest)
                .expect_err("invalid digest must refuse")
                .code(),
            "callee_contract_mismatch"
        );

        let mut untrimmed = receipt();
        untrimmed["left"]["identity"]["display_name"] = json!(" Kigali ");
        assert_eq!(
            validate_receipt(&untrimmed)
                .expect_err("untrimmed provenance must refuse")
                .code(),
            "callee_contract_mismatch"
        );
    }
}
