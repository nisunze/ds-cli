//! `ds data project-cache` — the project's own extracts of canonical datasets.
//!
//! A project holds bounded, spatially indexed extracts of the datasets its
//! workflow declares, instead of downloading a whole national layer it will
//! mostly not use. Coverage is the design's own footprint, buffered and fused,
//! so neighbouring transformers share one acquisition.
//!
//! Two commands, deliberately asymmetric:
//!
//! * `status` reads and costs nothing. It reports each dataset separately —
//!   what was requested, what actually completed, how many features, whether
//!   the index answers, and the last error.
//! * `seed` is the ONE place a geographic source is queried. It is confirmed,
//!   because it spends real money at the provider, and it acquires only the
//!   coverage the project does not already hold.
//!
//! Both take an explicit `--project` and neither opens or changes the paired
//! Desktop's map project. Printing and reporting never appear here: they read
//! the held index and must never reach a provider.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, NOT_PAIRED, PAIRING_REJECTED, REFUSED, SIGNED_OUT,
    UNREACHABLE, UNREADABLE as DESKTOP_UNREADABLE, UNSUPPORTED as DESKTOP_UNSUPPORTED,
    classify_signed_out, invoke, paired, paired_availability,
};
use serde_json::{Map, Value, json};

pub const STATUS_OPERATION: BridgeOp = BridgeOp {
    operation: "data.project_cache.status",
    arguments: &["project", "dataset"],
};

pub const SEED_OPERATION: BridgeOp = BridgeOp {
    operation: "data.project_cache.seed",
    arguments: &["project", "dataset"],
};

/// Required, never inferred. A dataset acquisition spends money against one
/// project's coverage, so the project it applies to is stated by the caller
/// rather than borrowed from whatever the Desktop happens to have open.
const PROJECT_ARG: Arg = Arg {
    name: "project",
    kind: ArgKind::Value,
    value: "<exact-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "Exact project to read or seed. Never changes the paired Desktop's map project.",
};

const DATASET_ARG: Arg = Arg::value(
    "dataset",
    "<dataset-id>",
    "One canonical dataset id, from `ds data project-cache status`. Defaults to every dataset this project declares.",
);

const INVALID_SCOPE: Refusal = Refusal {
    code: "project_dataset_scope_invalid",
    when: "the project is missing, or the named dataset is not one this project declares",
    remedy: "pass one exact --project, and a --dataset id listed by `ds data project-cache status`",
};

const NOT_THIS_SESSION: Refusal = Refusal {
    code: "project_not_selected",
    when: "the named project is not the signed-in Desktop session's selected project",
    remedy: "select that project in DS GridDesign, then retry; this command never switches it for you",
};

const NO_DESIGN_EXTENT: Refusal = Refusal {
    code: "project_has_no_extent",
    when: "the project holds no transformer design to derive coverage from",
    remedy: "load or draw at least one transformer, then seed",
};

const PROVIDER_UNAVAILABLE: Refusal = Refusal {
    code: "dataset_provider_unavailable",
    when: "the governed provider could not answer the acquisition",
    remedy: "restore the connection and retry; previously held data is kept and never partially claimed as ready",
};

pub static STATUS_COMMAND: Command = Command {
    id: "data.project-cache.status",
    path: &["data", "project-cache", "status"],
    contract: 1,
    summary: "Report the project's held extracts of canonical geographic datasets.",
    purpose: "Reads what this project holds locally, per dataset: requested and completed coverage (kept separate, so a failed acquisition never reads as a holding), feature count, index state, held and available source versions, buffer policy and last error. Coverage held under a replaced source version is reported stale, never deleted or quietly reused; abandoned acquisitions read as expired, not pending. Local state only: no provider, no BigQuery, no cost.",

    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, DATASET_ARG, DESCRIPTOR_ARG],
    output: "Per dataset: id, provider, quality, held and available source versions, stale coverage, buffer policy, requested and completed coverage, feature count, index state, pending and expired acquisitions, timestamps and last error.",
    examples: &[
        Example {
            command: "ds data project-cache status --project my-project --output json",
            note: "Reads every dataset this project holds. Costs nothing.",
            runnable: false,
        },
        Example {
            command: "ds data project-cache status --project my-project --dataset google_open_buildings --output json",
            note: "Reads one dataset's own coverage and readiness.",
            runnable: false,
        },
    ],
    refusals: &[
        INVALID_SCOPE,
        NOT_THIS_SESSION,
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        SIGNED_OUT,
        DESKTOP_UNREADABLE,
        DESKTOP_UNSUPPORTED,
    ],
    reference: Some("docs/reference/data.md"),
    availability: paired_availability,
};

pub static SEED_COMMAND: Command = Command {
    id: "data.project-cache.seed",
    path: &["data", "project-cache", "seed"],
    contract: 1,
    summary: "Acquire the geographic datasets this project's design needs.",
    purpose: "Derives coverage from every transformer's design extent, buffers each by the project's policy, fuses overlapping clusters, and acquires ONLY the parts not already held. Disconnected sites stay separate rather than fusing into one enormous rectangle, and a re-run over unchanged design acquires nothing. This is the one command that queries a geographic source, so it is confirmed: it spends provider cost. Printing and reporting read the resulting local index and never query a source themselves. Held data survives a failure, and a partial acquisition is never reported as ready.",
    chapter: Chapter::Data,
    effect: Effect::ArtifactWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, DATASET_ARG, DESCRIPTOR_ARG],
    output: "Per dataset: fused clusters and their buffers, coverage acquired in this run, total held coverage, feature count, query digests and any warnings.",
    examples: &[
        Example {
            command: "ds data project-cache seed --project my-project --dataset google_open_buildings --yes --output json",
            note: "Seeds building footprints for the whole project's fused coverage.",
            runnable: false,
        },
        Example {
            command: "ds data project-cache seed --project my-project --yes --output json",
            note: "Seeds every dataset this project's workflow declares.",
            runnable: false,
        },
    ],
    refusals: &[
        INVALID_SCOPE,
        NOT_THIS_SESSION,
        NO_DESIGN_EXTENT,
        PROVIDER_UNAVAILABLE,
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        SIGNED_OUT,
        DESKTOP_UNREADABLE,
        DESKTOP_UNSUPPORTED,
    ],
    reference: Some("docs/reference/data.md"),
    availability: paired_availability,
};

fn arguments(inputs: &Inputs) -> Result<Map<String, Value>, Failure> {
    let project = inputs.require("project")?.trim().to_owned();
    if project.is_empty() || project.len() > 128 {
        return Err(Failure::invalid(
            "project_dataset_scope_invalid",
            "`--project` must be one exact project id",
        )
        .remedy(INVALID_SCOPE.remedy));
    }
    let mut arguments = Map::new();
    arguments.insert("project".into(), json!(project));
    if let Some(dataset) = inputs.value("dataset") {
        let dataset = dataset.trim();
        if dataset.is_empty() || dataset.len() > 128 {
            return Err(Failure::invalid(
                "project_dataset_scope_invalid",
                "`--dataset` must be one exact canonical dataset id",
            )
            .remedy(INVALID_SCOPE.remedy));
        }
        arguments.insert("dataset".into(), json!(dataset));
    }
    Ok(arguments)
}

fn call(operation: &BridgeOp, inputs: &Inputs, timeout: Duration) -> Result<Value, Failure> {
    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    invoke(
        &descriptor,
        operation,
        Value::Object(arguments(inputs)?),
        timeout,
    )
    .map_err(classify_signed_out)
}

pub fn run_status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    call(&STATUS_OPERATION, inputs, Duration::from_secs(2 * 60))
}

pub fn run_seed(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // An acquisition legitimately runs for a long time over a large project;
    // the desktop reports progress and the room fences duplicate work.
    call(&SEED_OPERATION, inputs, Duration::from_secs(2 * 60 * 60))
}

fn coverage_cells(value: &Value) -> usize {
    value["cells"].as_array().map(Vec::len).unwrap_or(0)
}

fn dataset_lines(dataset: &Value) -> String {
    let held = coverage_cells(&dataset["completed"]);
    let asked = coverage_cells(&dataset["requested"]);
    let mut line = format!(
        "  {} · {} feature(s) · index {} · {} covered area(s) of {} requested",
        dataset["dataset_id"].as_str().unwrap_or("?"),
        dataset["feature_count"].as_u64().unwrap_or(0),
        dataset["index_state"].as_str().unwrap_or("?"),
        held,
        asked,
    );
    if let Some(version) = dataset["source_version"].as_str().filter(|v| !v.is_empty()) {
        line.push_str(&format!("\n    source version {version}"));
    }
    // Freshness is reported, never acted on. Naming the stale areas is what
    // lets an operator decide to spend money on a refresh; saying nothing
    // would let obsolete rows pass for current ones.
    let stale = coverage_cells(&dataset["stale"]);
    if stale > 0 {
        let available = dataset["available_version"].as_str().unwrap_or("");
        line.push_str(&format!(
            "\n    {stale} held area(s) completed under a superseded version{}; refresh to re-acquire, nothing is removed until it lands",
            if available.is_empty() {
                String::new()
            } else {
                format!(" (provider now publishes {available})")
            },
        ));
    }
    let expired = dataset["expired_queries"].as_u64().unwrap_or(0);
    if expired > 0 {
        line.push_str(&format!(
            "\n    {expired} abandoned acquisition(s) expired and no longer count as pending"
        ));
    }
    if let Some(policy) = dataset["buffer_policy"].as_object() {
        line.push_str(&format!(
            "\n    buffers {} m design / {} m isolated · rule {}{}",
            policy["design_buffer_m"].as_f64().unwrap_or(0.0),
            policy["isolated_buffer_m"].as_f64().unwrap_or(0.0),
            policy["isolated_rule"].as_str().unwrap_or("?"),
            if policy["provisional"] == Value::Bool(true) {
                " (provisional)"
            } else {
                ""
            },
        ));
    }
    if let Some(error) = dataset["last_error"].as_str() {
        line.push_str(&format!("\n    last error: {error}"));
    }
    line
}

pub fn render_status(data: &Value) -> String {
    let datasets = data["datasets"].as_array().cloned().unwrap_or_default();
    if datasets.is_empty() {
        return format!(
            "{} holds no project dataset extracts yet\n",
            data["project"].as_str().unwrap_or("this project")
        );
    }
    let mut out = format!(
        "{} holds {} project dataset extract(s)\n",
        data["project"].as_str().unwrap_or("?"),
        datasets.len()
    );
    for dataset in &datasets {
        out.push_str(&dataset_lines(dataset));
        out.push('\n');
    }
    out
}

pub fn render_seed(data: &Value) -> String {
    let datasets = data["datasets"].as_array().cloned().unwrap_or_default();
    let mut out = format!(
        "seeded {} dataset(s) for {}\n",
        datasets.len(),
        data["project"].as_str().unwrap_or("?")
    );
    for dataset in &datasets {
        out.push_str(&format!(
            "  {} · {} cluster(s) · {} acquisition(s) this run · {} feature(s) held\n",
            dataset["dataset_id"].as_str().unwrap_or("?"),
            dataset["clusters"].as_u64().unwrap_or(0),
            dataset["acquired"].as_u64().unwrap_or(0),
            dataset["feature_count"].as_u64().unwrap_or(0),
        ));
        for warning in dataset["warnings"].as_array().cloned().unwrap_or_default() {
            if let Some(text) = warning.as_str() {
                out.push_str(&format!("    note: {text}\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bridge_contract_is_closed_and_mapless() {
        assert_eq!(STATUS_OPERATION.operation, "data.project_cache.status");
        assert_eq!(SEED_OPERATION.operation, "data.project_cache.seed");
        for operation in [&STATUS_OPERATION, &SEED_OPERATION] {
            assert_eq!(operation.arguments, &["project", "dataset"]);
        }
    }

    #[test]
    fn reading_is_free_and_acquiring_is_confirmed() {
        // Status must never be able to spend provider cost, and seeding must
        // never be able to run without an explicit human decision.
        assert_eq!(STATUS_COMMAND.effect, Effect::ReadOnly);
        assert!(!STATUS_COMMAND.effect.needs_confirmation());
        assert_eq!(SEED_COMMAND.effect, Effect::ArtifactWrite);
        assert!(SEED_COMMAND.effect.needs_confirmation());
    }

    #[test]
    fn the_project_is_always_explicit() {
        for command in [&STATUS_COMMAND, &SEED_COMMAND] {
            let project = command
                .args
                .iter()
                .find(|arg| arg.name == "project")
                .expect("both commands name their project");
            assert!(project.required, "the project must never be implicit");
        }
    }

    #[test]
    fn an_absent_or_oversized_scope_is_refused_before_the_bridge() {
        // Parsed through the real declared arguments, so the test cannot drift
        // from the contract the command actually publishes.
        let parsed = |tokens: &[&str]| {
            let owned: Vec<String> = tokens.iter().map(|token| (*token).to_owned()).collect();
            ds_cli_contract::parse(&STATUS_COMMAND, &owned).expect("declared tokens parse")
        };
        let failure =
            arguments(&parsed(&["--project", ""])).expect_err("an empty project is refused");
        assert_eq!(failure.code(), "project_dataset_scope_invalid");
        assert!(arguments(&parsed(&["--project", "p", "--dataset", " "])).is_err());
        let arguments = arguments(&parsed(&[
            "--project",
            "p",
            "--dataset",
            "google_open_buildings",
        ]))
        .expect("an exact scope is accepted");
        assert_eq!(arguments["project"], "p");
        assert_eq!(arguments["dataset"], "google_open_buildings");
    }

    #[test]
    fn status_render_separates_requested_from_completed() {
        let rendered = render_status(&json!({
            "project": "p",
            "datasets": [{
                "dataset_id": "google_open_buildings",
                "feature_count": 12,
                "index_state": "ready",
                "requested": {"cells": [[0,0,1,1],[2,2,3,3]]},
                "completed": {"cells": [[0,0,1,1]]},
                "source_version": "v1",
                "buffer_policy": {"design_buffer_m": 500.0, "isolated_buffer_m": 1000.0,
                    "isolated_rule": "point_only", "provisional": true},
                "last_error": "the provider was unreachable"
            }]
        }));
        assert!(rendered.contains("1 covered area(s) of 2 requested"));
        assert!(rendered.contains("(provisional)"));
        assert!(rendered.contains("last error: the provider was unreachable"));
    }

    #[test]
    fn status_render_names_obsolete_coverage_and_abandoned_acquisitions() {
        let rendered = render_status(&json!({
            "project": "p",
            "datasets": [{
                "dataset_id": "google_open_buildings",
                "feature_count": 12,
                "index_state": "ready",
                "requested": {"cells": [[0,0,1,1]]},
                "completed": {"cells": [[0,0,1,1]]},
                "source_version": "v1",
                "available_version": "v2",
                "stale": {"cells": [[0,0,1,1]]},
                "expired_queries": 2,
            }]
        }));
        // The operator is told what is out of date and what it would cost to
        // fix, and told plainly that reading this destroys nothing.
        assert!(rendered.contains("1 held area(s) completed under a superseded version"));
        assert!(rendered.contains("provider now publishes v2"));
        assert!(rendered.contains("nothing is removed until it lands"));
        // An abandoned attempt is reported as expired, never as pending work.
        assert!(rendered.contains("2 abandoned acquisition(s) expired"));

        // A room holding one current version says none of that.
        let current = render_status(&json!({
            "project": "p",
            "datasets": [{"dataset_id": "google_open_buildings", "feature_count": 1,
                "index_state": "ready", "requested": {"cells": []}, "completed": {"cells": []},
                "source_version": "v2", "available_version": "v2", "stale": {"cells": []},
                "expired_queries": 0}]
        }));
        assert!(!current.contains("superseded"));
        assert!(!current.contains("expired"));
    }
}
