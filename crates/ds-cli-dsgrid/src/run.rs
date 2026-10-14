//! `ds dsgrid run` — transport one Network-owned read, solve or proposal.
//! Package decoding, descriptor admission and engineering remain in Network.
use crate::{native, package};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_contracts::grid::{
    NATIVE_GRID_QUERY_SCHEMA_V1, NativeGridQueryRequestV1, NativeGridQuerySourcePin,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
const MAX_PARAMS_BYTES: u64 = 16 * 1024 * 1024;
const COMMAND_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "model_not_found",
        when: "the source path does not exist or is not a file",
        remedy: "check the path; --model takes one .dsgrid file",
    },
    Refusal {
        code: "model_too_large",
        when: "the source is above the 512 MiB read bound",
        remedy: "confirm the file is a .dsgrid package and not a disk image",
    },
    Refusal {
        code: "model_unreadable",
        when: "the source exists but cannot be read",
        remedy: "check file permissions",
    },
    Refusal {
        code: "not_a_dsgrid_package",
        when: "the source bytes are not a readable .dsgrid container",
        remedy: "convert the native source through dsgrid-exchange first",
    },
    Refusal {
        code: "package_decode_failed",
        when: "the source manifest or canonical tables do not verify",
        remedy: "run `ds dsgrid validate --model <path>` and repair the package",
    },
    Refusal {
        code: "unknown_operation",
        when: "the compiled engine publishes no operation with that id",
        remedy: "run `ds dsgrid describe --kind operations` and choose an exact id",
    },
    Refusal {
        code: "operation_not_read_only",
        when: "the operation journals, mutates, imports, or exports model state",
        remedy: "use `ds dsgrid apply` for a deliberate revision-gated mutation",
    },
    Refusal {
        code: "params_not_found",
        when: "--params does not name one regular file",
        remedy: "write one JSON object matching the operation descriptor",
    },
    Refusal {
        code: "params_too_large",
        when: "the params document exceeds 16 MiB",
        remedy: "use one bounded operation request",
    },
    Refusal {
        code: "params_unreadable",
        when: "the params file exists but cannot be read",
        remedy: "check file permissions",
    },
    Refusal {
        code: "params_invalid",
        when: "the JSON is not an object matching the live descriptor",
        remedy: "read `ds dsgrid describe --kind operations --id <id>` and supply its exact fields",
    },
    Refusal {
        code: "operation_failed",
        when: "the native engine refuses the typed request against this authored revision",
        remedy: "read detail.refusal when present, then use ids and authored values from this exact package revision",
    },
    Refusal {
        code: "invalid_limit",
        when: "--limit is not a whole number in 1..10000",
        remedy: "pass a limit inside the range, or omit it for the default of 50",
    },
];

pub static COMMAND: Command = Command {
    id: "dsgrid.run",
    path: &["dsgrid", "run"],
    contract: 1,
    summary: "Run one native DS Grid read, solve, or proposal headlessly.",
    purpose: "\
Runs one native read, solve or proposal on a verified .dsgrid package without \
journaling. Results retain exact source/revision pins and truncation receipts. \
Discover structure usage and Analyze/Issues through dsgrid describe; missing \
screening inputs remain blocking findings with exact affected counts. Profile \
indices require the default project_profile_atlas revision and axis pin. \
project_profile_atlas returns attachment boxes, usage labels and native summaries.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "The source .dsgrid package.").required(),
        Arg::value(
            "operation",
            "<id>",
            "One non-journaled read, solve, or propose operation id.",
        )
        .required(),
        Arg::value(
            "params",
            "<json-path>",
            "JSON object matching the live operation descriptor; omit for parameterless operations.",
        ),
        Arg::value(
            "limit",
            "<n>",
            "Cap every returned JSON collection outside a digest-sealed plan.",
        )
        .default(package::DEFAULT_LIMIT),
        Arg::switch(
            "progress",
            "Stream native spotting progress and ETA as JSON lines on stderr; stopping the read-only run is safe.",
        ),
    ],
    output: "\
The exact source package identity and authored revision, engine and operation \
descriptor identity, a typed bounded result, staged:false and persisted:false. \
`more.truncated` names every collection shortened by --limit with exact totals. \
A plan sealed by plan_digest is never shortened, so the digest-verified apply \
path can check it; its request's max_reported_rejections bounds its rows.",
    examples: &[
        Example {
            command: "ds dsgrid run --model ./model.dsgrid --operation project_plan --output json",
            note: "Read stable plan entity ids from the authored revision.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid run --model ./model.dsgrid --operation project_profile --params ./profile.json --limit 200 --output json",
            note: "Run a parameterized native projection with bounded output.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid run --model ./model.dsgrid --operation analyze_model_defaults --params ./analysis.json --output json",
            note: "Supply the native DefaultAnalysisRequest inside request; copy its revision and engineering root from project_criteria_workbench on this file.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid run --model ./model.dsgrid --operation compute_structure_conductor_loads --params ./conductor-loads.json --output json",
            note: "Read conductor-only attachment and support forces for explicit structures at the pinned model head; upward demand is not anchored capacity or a strength failure.",
            runnable: false,
        },
    ],
    refusals: &native::extend_refusals::<{ COMMAND_REFUSALS.len() + native::REFUSALS.len() }>(
        COMMAND_REFUSALS,
    ),
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "conductor loads",
        "attachment forces",
        "upward demand",
        "support resistance",
        "anchorage evidence",
        "model analysis",
        "structure usage",
        "structure screening",
        "screening inputs",
        "capacity handshake",
        "profile review",
        "attachment boxes",
    ],
    requires: Requires::Server,
    availability: available,
};

fn available() -> Availability {
    ds_network_client::available()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let raw_path = inputs.require("model")?;
    let operation_id = inputs.require("operation")?;
    let limit = match inputs.value("limit").unwrap_or("50").parse::<usize>() {
        Ok(value @ 1..=10_000) => value,
        _ => {
            return Err(Failure::invalid(
                "invalid_limit",
                "--limit must be 1..10000",
            ));
        }
    };
    let mut params = read_params(inputs.value("params"))?;
    // Execution memory admission remains a host fact, supplied as prepared
    // input. Network owns the spotting interpretation and equations.
    if matches!(
        operation_id,
        "plan_whole_model_spotting" | "plan_optimum_spotting_batch"
    ) && params.get("memory_budget_bytes").is_none_or(Value::is_null)
        && let Some(budget) = crate::host_memory::spotting_memory_budget_bytes()
    {
        params["memory_budget_bytes"] = json!(budget);
    }
    let bytes = package::read_bytes(raw_path)?;
    let request = NativeGridQueryRequestV1 {
        schema: NATIVE_GRID_QUERY_SCHEMA_V1.into(),
        source: NativeGridQuerySourcePin {
            package_sha256: format!("{:x}", Sha256::digest(&bytes)),
            expected_revision: None,
            engineering_input_root: None,
        },
        operation_id: operation_id.into(),
        params,
    };
    let scratch = native::scratch()?;
    let client = native::capture()?;
    let observer = |line: &str| {
        if inputs.switch("progress") {
            use std::io::Write;
            let mut stderr = std::io::stderr().lock();
            let _ = stderr.write_all(line.as_bytes());
            let _ = stderr.flush();
        }
    };
    let outcome = client
        .grid_query_observed(&request, &bytes, scratch.path(), &|| false, &observer)
        .map_err(native::map_failure)?;
    let query = outcome.result;
    let descriptor = query.operation;
    let (result, truncated) = bound_result(query.result, limit);
    let mut answer = json!({
        "source": {
            "path": raw_path,
            "model_id": query.source.model_id.as_str(),
            "package_revision": query.model_revision,
            "authored_revision": query.source.expected_revision,
            "package_sha256": format!("sha256:{}", query.source.package_sha256),
        },
        "engine": query.engine_version,
        "operation": {
            "id": descriptor["operation_id"],
            "semantic_version": descriptor["semantic_version"],
            "effect": descriptor["effect_class"],
            "result_type": descriptor["result_type"],
            "journaled": descriptor["journaled"],
        },
        "staged": false,
        "persisted": false,
        "result": result,
    });
    if !truncated.is_empty() {
        answer["more"] = json!({ "truncated": truncated });
    }
    Ok(answer)
}

fn read_params(raw_path: Option<&str>) -> Result<Value, Failure> {
    let Some(raw_path) = raw_path else {
        return Ok(json!({}));
    };
    let path = Path::new(raw_path);
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::invalid("params_not_found", format!("cannot read `{raw_path}`"))
            .remedy("--params takes one JSON object file")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    if !metadata.is_file() {
        return Err(
            Failure::invalid("params_not_found", format!("`{raw_path}` is not a file"))
                .remedy("--params takes one JSON object file"),
        );
    }
    if metadata.len() > MAX_PARAMS_BYTES {
        return Err(Failure::invalid(
            "params_too_large",
            format!("`{raw_path}` exceeds the params bound"),
        )
        .remedy("use one bounded operation request")
        .detail(json!({ "byte_len": metadata.len(), "max_byte_len": MAX_PARAMS_BYTES })));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        Failure::failed("params_unreadable", format!("cannot read `{raw_path}`"))
            .remedy("check file permissions")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Failure::invalid("params_invalid", "the params document is not valid JSON")
            .remedy("supply one JSON object matching the live operation descriptor")
            .detail(json!({ "detail": error.to_string() }))
    })?;
    if !value.is_object() {
        return Err(Failure::invalid(
            "params_invalid",
            "the params document must be a JSON object",
        )
        .remedy("read the live operation descriptor and supply its named fields"));
    }
    Ok(value)
}

fn bound_result(mut result: Value, limit: usize) -> (Value, Vec<Value>) {
    let mut truncated = Vec::new();
    bound_value(&mut result, "result", limit, &mut truncated);
    (result, truncated)
}

pub(crate) fn bound_value(value: &mut Value, path: &str, limit: usize, truncated: &mut Vec<Value>) {
    match value {
        // A spotting plan's digest covers its whole content, diagnostic
        // `rejected.rows` and provisional `blocked_by` included: one shortened
        // collection leaves a plan no apply door can verify. The engine
        // already bounds those rows by the request's max_reported_rejections,
        // so a sealed plan crosses whole.
        Value::Object(object) if is_digest_sealed(object) => {}
        Value::Array(items) => {
            let total = items.len();
            if total > limit {
                items.truncate(limit);
                truncated.push(json!({
                    "field": path,
                    "total": total,
                    "shown": limit,
                    "withheld": total - limit,
                    "limit": limit,
                }));
            }
            for (index, item) in items.iter_mut().enumerate() {
                bound_value(item, &format!("{path}[{index}]"), limit, truncated);
            }
        }
        Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                bound_value(child, &format!("{path}.{key}"), limit, truncated);
            }
        }
        _ => {}
    }
}

fn is_digest_sealed(object: &serde_json::Map<String, Value>) -> bool {
    object
        .get("plan_digest")
        .and_then(Value::as_str)
        .is_some_and(|digest| digest.starts_with("sha256:"))
}

pub fn render(data: &Value) -> String {
    let source = &data["source"];
    let operation = &data["operation"];
    let mut out = format!(
        "{}  {}\n  model {} · authored {}\n  {} · {} · persisted false\n",
        operation["id"].as_str().unwrap_or("?"),
        operation["effect"].as_str().unwrap_or("?"),
        source["model_id"].as_str().unwrap_or("?"),
        source["authored_revision"].as_str().unwrap_or("?"),
        data["engine"].as_str().unwrap_or("?"),
        operation["result_type"].as_str().unwrap_or("?"),
    );
    if let Some(rows) = data["more"]["truncated"].as_array() {
        out.push_str(&format!(
            "  {} bounded collection(s); use --limit to adjust\n",
            rows.len()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recursive_bounding_is_explicit_at_every_shortened_path() {
        let value = json!({
            "rows": [1, 2, 3],
            "nested": { "items": [4, 5, 6] },
        });
        let (bounded, truncated) = bound_result(value, 2);
        assert_eq!(bounded["rows"], json!([1, 2]));
        assert_eq!(bounded["nested"]["items"], json!([4, 5]));
        assert_eq!(truncated.len(), 2);
        let rows = truncated
            .iter()
            .find(|receipt| receipt["field"] == "result.rows")
            .expect("rows truncation is explicit");
        assert_eq!(rows["total"], 3);
        assert_eq!(rows["withheld"], 1);
    }

    #[test]
    fn a_digest_sealed_plan_crosses_the_limit_whole() {
        // The whole-model receipt shape that lost 27 plans to --limit 10000:
        // cutting rejected.rows left plan_digest unverifiable.
        let plan = json!({
            "plan_digest": format!("sha256:{}", "a".repeat(64)),
            "commands": [1, 2, 3],
            "rejected": { "truncated": false, "rows": [1, 2, 3] },
            "provisional_intervals": [{ "blocked_by": [1, 2, 3] }],
        });
        let value = json!({
            "batch": { "items": [{ "plan": plan.clone() }] },
            "refused_derivations": [1, 2, 3],
        });
        let (bounded, truncated) = bound_result(value, 2);
        assert_eq!(bounded["batch"]["items"][0]["plan"], plan);
        assert_eq!(bounded["refused_derivations"], json!([1, 2]));
        let fields: Vec<&str> = truncated
            .iter()
            .map(|receipt| receipt["field"].as_str().unwrap())
            .collect();
        assert_eq!(fields, ["result.refused_derivations"]);

        // An unsealed (infeasible, empty-digest) plan is ordinary output.
        let (bounded, truncated) = bound_result(json!({ "plan_digest": "", "rows": [1, 2, 3] }), 2);
        assert_eq!(bounded["rows"], json!([1, 2]));
        assert_eq!(truncated.len(), 1);
    }
}
