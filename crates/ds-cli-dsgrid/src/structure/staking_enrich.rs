//! Apply conservative canonical Comment 1 and transformer survey joins to an
//! exact engineering-number interval through the typed engine mutation door.
use std::collections::BTreeSet;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::staking_enrichment::propose_staking_enrichment;
use ds_grid_engine::{GridCommand, ProfilePropertyEdit};
use ds_grid_model::{StakingEarthing, StakingFunction, StructureId};
use serde_json::{Value, json};

use crate::mutation::{self, Planned, Target};

const OWN: &[Refusal] = &[
    Refusal {
        code: "structure_range_invalid",
        when: "endpoints are absent, duplicated, nonnumeric, reversed or over 1000 numbers apart",
        remedy: "use unique numeric engineering numbers from the current structure report",
    },
    Refusal {
        code: "transformer_join_invalid",
        when: "maximum join distance is not positive and finite",
        remedy: "pass a measured --max-join-m for the survey-to-structure XY join",
    },
    Refusal {
        code: "nothing_to_enrich",
        when: "the selected range has no newly supported canonical facts",
        remedy: "inspect the dry-run proposals; author unresolved functions or survey facts explicitly",
    },
];
const REFUSALS: &[Refusal; OWN.len() + mutation::REFUSALS.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + mutation::REFUSALS.len()] {
    let mut all = [OWN[0]; OWN.len() + mutation::REFUSALS.len()];
    let mut i = 0;
    while i < OWN.len() {
        all[i] = OWN[i];
        i += 1;
    }
    let mut j = 0;
    while j < mutation::REFUSALS.len() {
        all[OWN.len() + j] = mutation::REFUSALS[j];
        j += 1;
    }
    all
}
pub static COMMAND: Command = Command {
    id: "dsgrid.structure.staking-enrich",
    path: &["dsgrid", "structure", "staking-enrich"],
    contract: 1,
    summary: "Preview or author canonical Comment 1 and transformer load joins for a number range.",
    purpose: "Derive meaningful structure functions from authored network role or exact structure family, then join each transformer to a unique nearby surveyed transformer name and kVA. Ordinary LINE supports remain without a Comment 1 proposal. Existing authored load references win; ambiguous and distant survey matches stay unknown and are reported. The inclusive engineering-number selection is revision-gated and bounded. Dry-run and write exercise the same typed engine command.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::OUT_ARG,
        Arg::value(
            "from-number",
            "<number>",
            "First engineering number, inclusive.",
        )
        .required(),
        Arg::value(
            "to-number",
            "<number>",
            "Last engineering number, inclusive.",
        )
        .required(),
        Arg::value(
            "max-join-m",
            "<metres>",
            "Maximum plan distance to one surveyed transformer with name and kVA.",
        )
        .required(),
        mutation::REVISION_ARG,
        mutation::DRY_RUN_ARG,
        mutation::YES_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "Revision-bound selected structures, proposed function and transformer load, survey point and distance, unresolved warnings, and the standard typed mutation receipt.",
    examples: &[Example {
        command: "ds dsgrid structure staking-enrich --package design.dsgrid --from-number 406 --to-number 427 --max-join-m 30 --dry-run --output json",
        note: "Preview exact Comment 1 and transformer associations without writing.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "structure comment 1",
        "transformer kVA",
        "staking",
        "nearest transformer",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

fn number(inputs: &Inputs, key: &str) -> Result<u32, Failure> {
    inputs.require(key)?.parse::<u32>().map_err(|_| {
        Failure::invalid(
            "structure_range_invalid",
            format!("--{key} must be a positive numeric engineering number"),
        )
    })
}

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let from = number(inputs, "from-number")?;
    let to = number(inputs, "to-number")?;
    if from == 0 || to < from || to - from > 1000 {
        return Err(Failure::invalid(
            "structure_range_invalid",
            "range must be positive, ordered and at most 1000 numbers wide",
        ));
    }
    let max_join_m = inputs.require("max-join-m")?.parse::<f64>().map_err(|_| {
        Failure::invalid(
            "transformer_join_invalid",
            "--max-join-m must be a positive finite distance",
        )
    })?;
    if !max_join_m.is_finite() || max_join_m <= 0.0 {
        return Err(Failure::invalid(
            "transformer_join_invalid",
            "--max-join-m must be a positive finite distance",
        ));
    }
    let writing = mutation::write_mode(inputs, context)?;
    let target = Target::resolve(inputs, writing)?;
    let opened = mutation::open(target, inputs)?;
    let snapshot = opened.session.snapshot();
    let mut selected = BTreeSet::<StructureId>::new();
    let mut seen_numbers = BTreeSet::new();
    for row in &snapshot.structures {
        let Some(number) = row
            .engineering_number
            .as_deref()
            .and_then(|text| text.parse::<u32>().ok())
        else {
            continue;
        };
        if (from..=to).contains(&number) {
            if !seen_numbers.insert(number) {
                return Err(Failure::invalid(
                    "structure_range_invalid",
                    format!("engineering number {number} occurs more than once"),
                ));
            }
            selected.insert(row.id.clone());
        }
    }
    if !seen_numbers.contains(&from) || !seen_numbers.contains(&to) {
        return Err(Failure::invalid(
            "structure_range_invalid",
            "one or both endpoint engineering numbers are absent",
        )
        .detail(json!({"from":from,"to":to,"numbers_found":seen_numbers})));
    }
    let proposals = propose_staking_enrichment(snapshot, &selected, max_join_m)
        .map_err(|detail| Failure::invalid("transformer_join_invalid", detail))?;
    let mut edits = Vec::new();
    let mut warnings = Vec::new();
    for proposal in &proposals {
        let row = snapshot
            .structures
            .iter()
            .find(|row| row.id == proposal.structure_id)
            .expect("proposal is from this snapshot");
        if let Some(function) = proposal.function {
            if row.staking.function == StakingFunction::Line && function != StakingFunction::Line {
                edits.push(ProfilePropertyEdit {
                    entity_id: row.id.entity().clone(),
                    field_id: "staking.function".into(),
                    value: json!(function),
                });
            }
            if function == StakingFunction::Tfo && row.staking.earthing.is_none() {
                edits.push(ProfilePropertyEdit {
                    entity_id: row.id.entity().clone(),
                    field_id: "staking.earthing".into(),
                    value: json!(StakingEarthing::Tfo),
                });
            }
        }
        if row.staking.load_ref.is_none() && proposal.load_ref.is_some() {
            edits.push(ProfilePropertyEdit {
                entity_id: row.id.entity().clone(),
                field_id: "staking.load_ref".into(),
                value: json!(proposal.load_ref),
            });
        }
        if let Some(code) = &proposal.warning {
            warnings
                .push(json!({"code":code,"structure_id":row.id,"number":row.engineering_number}));
        }
    }
    let preview = json!({
        "from_number":from,
        "to_number":to,
        "max_join_m":max_join_m,
        "selected":selected.len(),
        "edits":edits.len(),
        "proposals":proposals,
    });
    if edits.is_empty() {
        if writing {
            return Err(Failure::invalid(
                "nothing_to_enrich",
                "no selected structure has a new canonical fact",
            ));
        }
        return Ok(
            json!({"preview":preview,"warnings":warnings,"would_apply":false,"persisted":false}),
        );
    }
    let planned = vec![Planned {
        command_id: mutation::command_id("staking-enrich", &opened.head, &format!("{from}-{to}")),
        command: GridCommand::EditProfileProperties { edits },
    }];
    mutation::run(
        opened,
        planned,
        writing,
        json!({"preview":preview}),
        warnings,
        &["DON"],
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "staking enrichment {}–{}: {} selected, {} edits, {} warnings\n",
        data["preview"]["from_number"].as_u64().unwrap_or(0),
        data["preview"]["to_number"].as_u64().unwrap_or(0),
        data["preview"]["selected"].as_u64().unwrap_or(0),
        data["preview"]["edits"].as_u64().unwrap_or(0),
        data["warnings"].as_array().map_or(0, Vec::len)
    )
}
