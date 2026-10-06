//! Local adapter to the shared, deliberate Rust composition contract.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs, outcome::Failure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub static COMMAND: Command = Command {
    id: "dsgrid.model.combine",
    path: &["dsgrid", "model", "combine"],
    contract: 1,
    summary: "Preview and deliberately save an editable combined snapshot.",
    purpose: "Combine 2..100 exact local .dsgrid packages through shared Rust composition. Preview first; saving requires --apply --plan-id from that preview and a new --out path. Rust enforces source bytes, order, CRS, library conflicts and lineage. The output is an ordinary editable model. Sources stay unchanged; no automatic reconciliation or cloud publication occurs.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::repeated(
            "source",
            "<file.dsgrid>",
            "Exact local source package; repeat in the desired order.",
        )
        .required(),
        Arg::switch("apply", "Save the reviewed result; omitted, preview only."),
        Arg::value("plan-id", "<id>", "Exact plan_id returned by the preview."),
        Arg::value(
            "out",
            "<new.dsgrid>",
            "New output path; existing files are never overwritten.",
        ),
    ],
    output: "Rust plan, executable verdict, ordered source lineage, blockers and warnings. Applying also returns persisted:true and the exact output path, SHA-256 and byte count.",
    examples: &[Example {
        command: "ds dsgrid model combine --source /work/one.dsgrid --source /work/two.dsgrid --output json",
        note: "Review the plan before applying to a new file.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["combine models", "editable snapshot", "composition"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

const OWN_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "composition_source_count",
        when: "fewer than two or more than 100 sources",
        remedy: "choose 2..100 source packages",
    },
    Refusal {
        code: "composition_refused",
        when: "Rust rejects the exact sources or plan",
        remedy: "inspect the engine_code and detail, resolve conflicts and preview again",
    },
    Refusal {
        code: "composition_review_required",
        when: "applying without a reviewed plan id",
        remedy: "preview, then pass its plan_id",
    },
    Refusal {
        code: "composition_plan_mismatch",
        when: "sources or order changed since review",
        remedy: "preview the current sources and review the new plan",
    },
    Refusal {
        code: "composition_output_required",
        when: "applying without an output path",
        remedy: "choose a new .dsgrid output path",
    },
    Refusal {
        code: "output_exists",
        when: "output already exists",
        remedy: "choose a new output path",
    },
    Refusal {
        code: "output_parent_missing",
        when: "output parent is absent",
        remedy: "create the intended directory",
    },
    Refusal {
        code: "output_unwritable",
        when: "output cannot be fully written",
        remedy: "check permissions and free space",
    },
];
const fn refusals() -> [Refusal; OWN_REFUSALS.len() + crate::package::SHARED_REFUSALS.len()] {
    let mut result = [OWN_REFUSALS[0]; OWN_REFUSALS.len() + crate::package::SHARED_REFUSALS.len()];
    let mut i = 0;
    while i < OWN_REFUSALS.len() {
        result[i] = OWN_REFUSALS[i];
        i += 1;
    }
    let mut j = 0;
    while j < crate::package::SHARED_REFUSALS.len() {
        result[i + j] = crate::package::SHARED_REFUSALS[j];
        j += 1;
    }
    result
}
pub const REFUSALS: &[Refusal] = &refusals();

fn refuse(error: ds_grid_composition::Error) -> Failure {
    let failure = match error.code {
        "composition_review_required" => {
            Failure::invalid("composition_review_required", error.message)
        }
        "composition_plan_mismatch" => {
            Failure::conflict("composition_plan_mismatch", error.message)
        }
        _ => Failure::invalid("composition_refused", error.message),
    };
    failure
        .remedy("resolve the named finding and review a fresh preview")
        .detail(json!({"engine_code": error.code}))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let paths = inputs.repeated("source");
    if !(2..=100).contains(&paths.len()) {
        return Err(Failure::invalid(
            "composition_source_count",
            "Choose 2..100 exact model snapshots.",
        )
        .remedy("repeat --source for each package"));
    }
    let applying = inputs.switch("apply");
    let plan_id = inputs.value("plan-id");
    if applying && plan_id.is_none() {
        return Err(Failure::invalid(
            "composition_review_required",
            "Review the composition plan before saving.",
        )
        .remedy("preview first, then pass --plan-id"));
    }
    let out = inputs.value("out");
    if applying {
        crate::apply::validate_output_path(out.ok_or_else(|| {
            Failure::invalid("composition_output_required", "Saving requires --out.")
                .remedy("choose a new .dsgrid path")
        })?)?;
    }
    let sources = paths
        .iter()
        .map(|path| crate::package::read_bytes(path))
        .collect::<Result<Vec<_>, _>>()?;
    let pins = sources
        .iter()
        .enumerate()
        .map(|(position, bytes)| {
            json!({
                "source_kind": "temporary_model", "model_id": paths[position],
                "model_digest": format!("sha256:{:x}", Sha256::digest(bytes)),
            })
        })
        .collect::<Vec<_>>();
    let request = json!({"sources": pins, "reviewed_plan_id": plan_id}).to_string();
    let preview = ds_grid_composition::preview_json(&request, &sources).map_err(refuse)?;
    let mut result: Value = serde_json::from_str(&preview).expect("Rust preview emits valid JSON");
    result["persisted"] = json!(false);
    if applying {
        let bytes = ds_grid_composition::execute_json(&request, &sources).map_err(refuse)?;
        crate::apply::write_new(out.expect("validated output"), &bytes)?;
        result["persisted"] = json!(true);
        result["artifact"] = json!({"path": out, "byte_len": bytes.len(), "sha256": format!("sha256:{:x}", Sha256::digest(&bytes))});
    }
    Ok(result)
}

pub fn render(data: &Value) -> String {
    format!(
        "plan {}\nexecutable {}\nsaved {}\n",
        data["plan_id"], data["executable"], data["persisted"]
    )
}
