//! What AutoProcess would do with committed edits, from its Rust owners.
//!
//! The host half of AutoProcess (the accumulator, timer and engine latch)
//! stays in the browser edit session. The RULES are not: whether an edit warrants
//! re-running the LV network, how wide the next run must be, and when queued
//! work dispatches belong to `ds_command_kernel::autoprocess` and
//! `ds_network::network::differential`. An agent can ask them without a Desktop
//! and get the answer the page acts on.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use std::io::Read;

// Differential scope now carries complete transformer layers, like native LV input.
const MAX_BYTES: usize = 64 * 1024 * 1024;

fn local() -> Availability {
    Availability::Available
}

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "autoprocess_request_invalid",
        when: "The changes document is malformed, too large, or a section is not an owner request",
        remedy: "Pass {mode?, trigger?, differential_scope?, cadence?} with the fields `ds capabilities design.autoprocess.plan` names",
    },
    Refusal {
        code: "network_engine_missing",
        when: "the compatible native Network engine is absent",
        remedy: "install the matching Network engine addon",
    },
    Refusal {
        code: "network_engine_incompatible",
        when: "the captured Network executable lacks the required protocol or capability",
        remedy: "install the compatible Network engine",
    },
    Refusal {
        code: "network_serialization_incompatible",
        when: "the captured engine differs from this consumer's preserved JSON byte profile",
        remedy: "install the insertion-order Network component for this host",
    },
    Refusal {
        code: "network_engine_path_invalid",
        when: "the explicit development engine path is not absolute",
        remedy: "set DS_NETWORK_ENGINE_BIN to an absolute compatible executable path",
    },
    Refusal {
        code: "network_engine_invalid",
        when: "the captured executable cannot be read within its byte bound",
        remedy: "install an intact compatible Network component",
    },
    Refusal {
        code: "network_engine_unqualified",
        when: "a normally installed candidate lacks source or dependency-closure qualification",
        remedy: "install a qualified component; explicit development overrides remain labeled",
    },
    Refusal {
        code: "network_engine_identity_invalid",
        when: "the identity probe is malformed, oversized or unsuccessful",
        remedy: "install an intact compatible Network component",
    },
    Refusal {
        code: "network_engine_changed",
        when: "the captured executable changes during admission or execution",
        remedy: "retain immutable version directories and retry with an intact component",
    },
    Refusal {
        code: "network_engine_refused",
        when: "the executable exits with an unrecognized owner refusal",
        remedy: "inspect the bounded failure and use a compatible component",
    },
    Refusal {
        code: "network_scratch_invalid",
        when: "private staging cannot be created beside the output",
        remedy: "choose an absent output in a writable ordinary directory",
    },
    Refusal {
        code: "network_request_io",
        when: "the admitted request cannot be staged durably",
        remedy: "choose a writable output directory and retry from unchanged input",
    },
    Refusal {
        code: "network_input_changed",
        when: "the staged request changes during execution",
        remedy: "retain the unchanged input and retry in a protected output directory",
    },
    Refusal {
        code: "network_receipt_bounded",
        when: "the engine exceeds its control-receipt bound",
        remedy: "install a compatible component; complete results belong in artifacts",
    },
    Refusal {
        code: "network_receipt_invalid",
        when: "the terminal receipt is absent, malformed or inconsistent with admission",
        remedy: "retain the unchanged input and use an intact compatible component",
    },
    Refusal {
        code: "network_refusal_invalid",
        when: "a failed engine does not return an admitted refusal",
        remedy: "retain the unchanged input and use an intact compatible component",
    },
    Refusal {
        code: "network_result_invalid",
        when: "the result is incomplete, unsafe or inconsistent with ordered inputs",
        remedy: "retain the unchanged input and use an intact compatible component",
    },
    Refusal {
        code: "network_result_digest_mismatch",
        when: "the artifact bytes differ from the terminal receipt",
        remedy: "retain the unchanged input and use an intact compatible component",
    },
    Refusal {
        code: "network_workers_invalid",
        when: "the engine rejects the positive worker budget",
        remedy: "use a compatible component with the admitted worker contract",
    },
    Refusal {
        code: "network_task_cancelled",
        when: "the batch does not complete after cooperative cancellation",
        remedy: "retry the unchanged input at a new output path",
    },
    Refusal {
        code: "network_control_unavailable",
        when: "the supervised control channel cannot be established",
        remedy: "retry on a host supporting the admitted native supervision boundary",
    },
    Refusal {
        code: "network_control_invalid",
        when: "the owner rejects the fixed task control document",
        remedy: "use a compatible Network component",
    },
    Refusal {
        code: "network_input_invalid",
        when: "the owner cannot admit its staged input",
        remedy: "retain the unchanged input and use a compatible component",
    },
    Refusal {
        code: "network_output_invalid",
        when: "the owner cannot admit its private output directory",
        remedy: "choose a writable ordinary output directory",
    },
    Refusal {
        code: "network_artifact_io",
        when: "the owner cannot durably emit its complete artifact",
        remedy: "provide disk space and retry unchanged input at a new output path",
    },
    Refusal {
        code: "callee_cancelled",
        when: "the host cancels the child or it completes after cancellation",
        remedy: "retry the unchanged input at a new output path",
    },
    Refusal {
        code: "callee_timed_out",
        when: "the supervised identity probe or task exceeds its bound",
        remedy: "use a compatible component and split large batches",
    },
    Refusal {
        code: "callee_wait_failed",
        when: "the host cannot collect the owned child status",
        remedy: "inspect host process availability and retry unchanged input",
    },
    Refusal {
        code: "callee_supervision_failed",
        when: "the host cannot establish process-tree supervision",
        remedy: "use a host supporting the admitted native supervision boundary",
    },
    Refusal {
        code: "callee_stdin_failed",
        when: "the child cannot receive its fixed control input",
        remedy: "use an intact compatible component",
    },
];

const CHANGES: Arg = Arg::value(
    "changes",
    "<json-file>",
    "{mode?, trigger?, differential_scope?:{gdfs,differential:{selected_feeders,auto_process?:{differential_enabled,is_mv_session,accumulator_bound,force_full,changed_features:[{layer_name,feature_id}]}},customer_source_addresses}, cadence?}, at most 64 MiB.",
)
.required();
const NOW_MS: Arg = Arg::value(
    "now-ms",
    "<ms>",
    "Epoch milliseconds for the cadence answer; defaults to this clock.",
);

pub static COMMAND: Command = Command {
    id: "design.autoprocess.plan",
    path: &["design", "autoprocess", "plan"],
    contract: 4,
    summary: "Plan what AutoProcess would do with committed edits.",
    purpose: "Answers AutoProcess's four admission questions from one document: auto or manual in this editing context, whether a committed edit re-runs the LV network, differential or full scope, and dispatch now or wait. The clock is an input, so a document always plans the same way; it never runs AutoProcess.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[CHANGES, NOW_MS],
    output: "One answer per section present (mode, trigger, differential_scope, cadence), each with a reason_key; fields in docs/reference/design.md.",
    examples: &[Example {
        command: "ds design autoprocess plan --changes edits.json --output json",
        note: "`.data.cadence.wait_ms` is when the host should evaluate again.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: local,
};

fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("autoprocess_request_invalid", e.to_string()).remedy(
        "Pass {mode?, trigger?, differential_scope?, cadence?} as `ds capabilities design.autoprocess.plan` names them",
    )
}

fn document(path: &str) -> Result<(Value, Vec<u8>), Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(invalid)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("changes document exceeds 64 MiB"));
    }
    Ok((serde_json::from_slice(&bytes).map_err(invalid)?, bytes))
}

/// One section through its Rust owner. `op` is added for kernel sections so
/// the document keeps plain request bodies rather than a tagged union.
fn section(op: &str, mut body: Value) -> Result<Value, Failure> {
    let Some(map) = body.as_object_mut() else {
        return Err(invalid(format!("`{op}` must be an object")));
    };
    if op == "differential_scope" {
        serde_json::from_value::<ds_network_contracts::differential::DifferentialPlanRequest>(
            body.clone(),
        )
        .map_err(invalid)?;
        return differential_section(&serde_json::to_vec(&body).map_err(invalid)?);
    }
    map.insert("op".into(), json!(op));
    let reply =
        ds_command_kernel::autoprocess::evaluate(&serde_json::to_vec(&body).map_err(invalid)?)
            .map_err(invalid)?;
    let parsed: Value = serde_json::from_str(&reply).map_err(invalid)?;
    Ok(parsed["result"].clone())
}

fn differential_section(body: &[u8]) -> Result<Value, Failure> {
    let value: Value = serde_json::from_slice(body).map_err(invalid)?;
    serde_json::from_value::<ds_network_contracts::differential::DifferentialPlanRequest>(value)
        .map_err(invalid)?;
    let captured =
        ds_network_client::CapturedNetworkClient::capture(1, &|| false).map_err(network_failure)?;
    // Private staging is on the current volume, never system tmpfs.
    let scratch = std::env::current_dir().map_err(invalid)?;
    let answer = captured
        .lv_differential_plan_bytes(body, &scratch, &|| false)
        .map_err(network_failure)?;
    serde_json::to_value(answer.result.plan).map_err(invalid)
}

fn network_failure(error: ds_network_client::NetworkFailure) -> Failure {
    if error.owner_code() == Some("autoprocess_request_invalid") {
        return invalid(error);
    }
    let code = error.code();
    let failure = if code == "network_engine_missing" {
        Failure::unavailable(code, error.to_string())
    } else {
        Failure::failed(code, error.to_string())
    };
    match REFUSALS.iter().find(|refusal| refusal.code == code) {
        Some(refusal) => failure.remedy(refusal.remedy),
        None => failure,
    }
}

fn host_now_ms() -> Result<u64, Failure> {
    ds_cli_contract::util::epoch_ms().map_err(invalid)
}

pub fn run(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let (mut request, original) = document(i.require("changes")?)?;
    let Some(sections) = request.as_object_mut() else {
        return Err(invalid("the changes document must be an object"));
    };
    let pinned_now: Option<u64> = match i.value("now-ms") {
        Some(raw) => Some(raw.trim().parse().map_err(invalid)?),
        None => None,
    };
    if let Some(cadence) = sections.get_mut("cadence").and_then(Value::as_object_mut) {
        // `now` is an input the KERNEL never reads for itself. Supplying it is
        // the host's job, and on this surface the host is `ds`: a pinned
        // --now-ms wins, a document that carries its own is left alone, and a
        // caller who declared neither gets this machine's clock — which is what
        // the argument says it gets, instead of a missing-field refusal.
        if let Some(now) = pinned_now {
            cadence.insert("now_ms".into(), json!(now));
        } else if !cadence.contains_key("now_ms") {
            cadence.insert("now_ms".into(), json!(host_now_ms()?));
        }
    }
    let known = ["mode", "trigger", "differential_scope", "cadence"];
    if let Some(unknown) = sections.keys().find(|key| !known.contains(&key.as_str())) {
        return Err(invalid(format!("unknown section `{unknown}`")));
    }
    // Admit every section's shape before any optional addon process. The
    // Network owns semantic differential decisions; this is wire admission.
    for (op, body) in sections.iter() {
        if !body.is_object() {
            return Err(invalid(format!("`{op}` must be an object")));
        }
        if op == "differential_scope" {
            serde_json::from_value::<ds_network_contracts::differential::DifferentialPlanRequest>(
                body.clone(),
            )
            .map_err(invalid)?;
        }
    }
    let raw_sections: std::collections::BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_slice(&original).map_err(invalid)?;
    // Admit the Kernel sections too before starting optional engineering.
    // Keep the original output section order after their answers are retained.
    let mut kernel_answers = serde_json::Map::new();
    for op in known.into_iter().filter(|op| *op != "differential_scope") {
        if let Some(body) = sections.get(op) {
            kernel_answers.insert(op.into(), section(op, body.clone())?);
        }
    }
    let mut out = json!({});
    for op in known {
        if sections.contains_key(op) {
            out[op] = if op == "differential_scope" {
                differential_section(raw_sections[op].get().as_bytes())?
            } else {
                kernel_answers.remove(op).expect("admitted Kernel answer")
            };
        }
    }
    if out.as_object().is_some_and(serde_json::Map::is_empty) {
        return Err(invalid("no section to plan"));
    }
    Ok(out)
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    for (key, label) in [
        ("mode", "mode"),
        ("trigger", "trigger"),
        ("differential_scope", "scope"),
        ("cadence", "cadence"),
    ] {
        let section = &data[key];
        if section.is_null() {
            continue;
        }
        let verdict = section["mode"]
            .as_str()
            .or_else(|| section["decision"].as_str())
            .or_else(|| section["scope"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| section["schedule"].to_string());
        out.push_str(&format!(
            "  {label:<18} {verdict:<12} {}\n",
            section["reason_key"].as_str().unwrap_or("")
        ));
        if let Some(wait) = section["wait_ms"].as_u64() {
            out.push_str(&format!("  {:<18} {wait} ms\n", "wait"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_is_a_headless_kernel_section_and_renders() {
        let answer = section(
            "mode",
            json!({
                "process_active": true,
                "auto_process_enabled": true,
            }),
        )
        .unwrap();
        assert_eq!(answer["mode"], "auto");
        assert_eq!(answer["reason_key"], "autoprocess_mode_enabled");
        assert!(render(&json!({"mode": answer})).contains("mode               auto"));
    }

    #[test]
    fn differential_scope_reads_layers_and_rejects_host_authored_mapping() {
        let request: ds_network_contracts::differential::DifferentialPlanRequest = serde_json::from_value( json!({
            "gdfs": {
                "lv_lines":{"features":[{"id":"f1","properties":{"path_id":"path1"}},{"id":"f2","properties":{"path_id":"path2"}}]},
                "lv_poles":{"features":[{"id":"p1","properties":{"path_id":"path1"}}]}
            },
            "differential":{"selected_feeders":[],"auto_process":{
                "differential_enabled":true,"is_mv_session":false,"accumulator_bound":true,"force_full":false,
                "changed_features":[{"layer_name":"lv_poles","feature_id":"p1"}]
            }},
            "customer_source_addresses":[]
        })).unwrap();
        assert_eq!(
            request.differential.auto_process.unwrap().changed_features[0].feature_id,
            "p1"
        );
        assert_eq!(section("differential_scope",json!({"differential_enabled":true,"change_count":1,"blocking_diagnostics":false,"mapping":{"unmapped":false,"feeder_ids":["f1"]}})).unwrap_err().code(),"autoprocess_request_invalid");
    }

    #[test]
    fn mode_has_no_lane_member_to_switch_on() {
        // There is one way to process a transformer: the only question left
        // is whether it runs by itself. A document written against the old
        // lane switch is refused by name, never answered as if it were valid.
        let manual = section(
            "mode",
            json!({ "process_active": false, "auto_process_enabled": true }),
        )
        .unwrap();
        assert_eq!(manual["mode"], "manual");
        assert_eq!(manual["reason_key"], "autoprocess_mode_manual");
        let stale = section(
            "mode",
            json!({
                "is_fast_lane": true,
                "fast_process_active": true,
                "auto_process_enabled": true,
            }),
        );
        assert_eq!(stale.unwrap_err().code(), "autoprocess_request_invalid");
    }
}
