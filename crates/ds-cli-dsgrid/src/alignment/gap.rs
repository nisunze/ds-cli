//! `ds dsgrid alignment gap show|set` — the multiple-alignment gap as a model
//! fact. The engine's `set_alignment_station_gaps` authors it as one revision;
//! the exchange's NUM projection is the one place global stations are
//! derived, and `show` reads them from it rather than stationing a second way.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::GridCommand;
use ds_grid_exchange::pls_cadd_num_projection::project_snapshot_to_pls_cadd_num_runs;
use ds_grid_exchange::pls_cadd_workspace::WorkspaceIngestOptions;
use ds_grid_model::{AlignmentId, GridModelSnapshot};
use ds_io::PLS_CADD_NUM_DEFAULT_ROUTE_GAP_M;
use serde_json::{Value, json};

use crate::mutation::{self, Planned, Target};

const GAP_ARG: Arg = Arg::value(
    "gap-m",
    "<metres>",
    "The gap before each alignment's first global station, metres (> 0); PLS-CADD's multiple-alignment gap.",
);
const CLEAR_ARG: Arg = Arg::switch(
    "clear",
    "Clear the authored gap so the export writes its 1 m default again.",
);
const ALIGNMENT_ARG: Arg = Arg::repeated(
    "alignment",
    "<id|label>",
    "Only this alignment (id or unique label); repeat for several. Omitted: every alignment.",
);

const OWN: &[Refusal] = &[
    Refusal {
        code: "gap_required",
        when: "neither --gap-m nor --clear was given, or both were",
        remedy: "pass --gap-m <metres> to author the gap, or --clear to remove it",
    },
    Refusal {
        code: "gap_invalid",
        when: "--gap-m is not a positive finite number of metres",
        remedy: "pass a positive length such as --gap-m 100",
    },
    Refusal {
        code: "alignment_unknown",
        when: "an --alignment is neither an alignment id nor a unique label in this revision",
        remedy: "use an id or label from `ds dsgrid alignment gap show`",
    },
];

const SET_REFUSALS: &[Refusal; OWN.len() + mutation::REFUSALS.len()] = &splice();
const fn splice() -> [Refusal; OWN.len() + mutation::REFUSALS.len()] {
    let mut all = [OWN[0]; OWN.len() + mutation::REFUSALS.len()];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < mutation::REFUSALS.len() {
        all[OWN.len() + shared] = mutation::REFUSALS[shared];
        shared += 1;
    }
    all
}

const SHOW_REFUSALS: &[Refusal] = &[
    mutation::REFUSALS[0],  // target_required
    mutation::REFUSALS[6],  // local_model_not_found
    mutation::REFUSALS[7],  // local_model_ambiguous
    mutation::REFUSALS[8],  // local_model_store_unavailable
    mutation::REFUSALS[9],  // model_not_found
    mutation::REFUSALS[10], // not_a_dsgrid_package
    mutation::REFUSALS[11], // package_decode_failed
];

pub static SHOW: Command = Command {
    id: "dsgrid.alignment.gap.show",
    path: &["dsgrid", "alignment", "gap", "show"],
    contract: 1,
    summary: "Read the multiple-alignment gap and global stations.",
    purpose: "Shows, in the order the PLS-CADD export writes alignments, each alignment's authored gap before its start (PLS-CADD's multiple-alignment gap), the gap the export writes (its 1 m default where none is authored), and the global start and end stations those gaps give it in the NUM and DON. Positions, local stations and assignments are not affected by the gap. Reads a working copy or a package; writes nothing.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "`target`, `revision`, `default_gap_m`, `authored` (how many alignments carry a gap), and `alignments[]` in export order {ordinal, id, label, parent_id, authored_gap_m, written_gap_before_m, global_station_start_m, global_station_end_m}; `global_stations_unavailable` names a route the export cannot project.",
    examples: &[Example {
        command: "ds dsgrid alignment gap show --package ./model.dsgrid --output json",
        note: "Read the gap and the global stations the export will write.",
        runnable: false,
    }],
    refusals: SHOW_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["multiple alignment", "global station", "stationing", "num"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub static SET: Command = Command {
    id: "dsgrid.alignment.gap.set",
    path: &["dsgrid", "alignment", "gap", "set"],
    contract: 1,
    summary: "Author the multiple-alignment gap as one revision.",
    purpose: "Sets the gap the model's global stationing leaves before each alignment's start — PLS-CADD's multiple-alignment gap (Terrain › Alignment › Multiple Alignment Options) — for every alignment or the named ones, through the engine's `set_alignment_station_gaps` as ONE revision of a working copy or one new package. The PLS-CADD export writes it on the NUM break rows and into every later DON global station; `--clear` returns to the export's 1 m default. Nothing else moves: positions, local stations, sections.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        mutation::MODEL_ARG,
        mutation::PACKAGE_ARG,
        mutation::OUT_ARG,
        GAP_ARG,
        CLEAR_ARG,
        ALIGNMENT_ARG,
        mutation::REVISION_ARG,
        mutation::DRY_RUN_ARG,
        mutation::YES_ARG,
        mutation::LANE_ARG,
        mutation::ACCOUNT_ARG,
    ],
    output: "The family receipt plus `gap` {authored_gap_m or null, alignments} and `alignments[]` {id, label, gap before → after}.",
    examples: &[
        Example {
            command: "ds dsgrid alignment gap set --package ./model.dsgrid --out ./model-gap.dsgrid --gap-m 100 --dry-run",
            note: "Every alignment 100 m apart in global stationing; nothing written.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid alignment gap set --package ./model.dsgrid --out ./model-gap.dsgrid --gap-m 100 --yes --output json",
            note: "The same gap written as one revision.",
            runnable: false,
        },
    ],
    refusals: SET_REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["multiple alignment", "global station", "stationing", "num"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn show(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let (target, path) = mutation::read_target_path(inputs)?;
    let bytes = crate::package::read_bytes(&path)?;
    let package = crate::package::decode(&path, &bytes)?;
    let session = ds_grid_engine::GridSession::open(package.snapshot.clone());
    let snapshot = session.snapshot();
    let (alignments, unavailable) = export_order(snapshot);
    Ok(json!({
        "target": target,
        "revision": session.current_revision().revision_id.as_str(),
        "default_gap_m": PLS_CADD_NUM_DEFAULT_ROUTE_GAP_M,
        "authored": snapshot
            .alignments
            .iter()
            .filter(|row| row.global_station_gap_m.is_some())
            .count(),
        "alignments": alignments,
        "global_stations_unavailable": unavailable,
    }))
}

/// The alignments in the export's order with the stations its NUM
/// projection gives them; the model's own order when the route cannot be
/// projected, with the reason.
fn export_order(snapshot: &GridModelSnapshot) -> (Vec<Value>, Option<String>) {
    match project_snapshot_to_pls_cadd_num_runs(snapshot, &WorkspaceIngestOptions::default()) {
        Ok(projection) => {
            let mut previous_gap = None;
            let rows = projection
                .alignments
                .iter()
                .enumerate()
                .map(|(index, run)| {
                    let row = snapshot
                        .alignments
                        .iter()
                        .find(|row| row.id == run.alignment_id);
                    let written_before = previous_gap;
                    previous_gap = run.native_run.gap_after_m;
                    json!({
                        "ordinal": index + 1,
                        "id": run.alignment_id.as_str(),
                        "label": row.map(|row| row.label.as_str()),
                        "parent_id": row.and_then(|row| row.parent_id.as_ref()).map(|id| id.as_str()),
                        "authored_gap_m": row.and_then(|row| row.global_station_gap_m),
                        "written_gap_before_m": written_before,
                        "global_station_start_m": run.native_global_station_start_m,
                        "global_station_end_m": run.native_global_station_end_m,
                    })
                })
                .collect();
            (rows, None)
        }
        Err(error) => (
            snapshot
                .alignments
                .iter()
                .map(|row| {
                    json!({
                        "id": row.id.as_str(),
                        "label": row.label,
                        "parent_id": row.parent_id.as_ref().map(|id| id.as_str()),
                        "authored_gap_m": row.global_station_gap_m,
                    })
                })
                .collect(),
            Some(error.to_string()),
        ),
    }
}

pub fn set(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let gap = match (inputs.value("gap-m"), inputs.switch("clear")) {
        (Some(raw), false) => Some(
            raw.trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value > 0.0)
                .ok_or_else(|| {
                    Failure::invalid(
                        "gap_invalid",
                        format!("--gap-m {raw:?} is not a positive length"),
                    )
                    .remedy("pass a positive length such as --gap-m 100")
                })?,
        ),
        (None, true) => None,
        _ => {
            return Err(Failure::invalid(
                "gap_required",
                "pass exactly one of --gap-m <metres> or --clear",
            )
            .remedy("pass --gap-m <metres> to author the gap, or --clear to remove it"));
        }
    };
    let writing = mutation::write_mode(inputs, context)?;
    let target = Target::resolve(inputs, writing)?;
    let opened = mutation::open(target, inputs)?;
    let snapshot = opened.session.snapshot();
    let mut alignment_ids: Vec<AlignmentId> = Vec::new();
    for raw in inputs.repeated("alignment") {
        let id = resolve_alignment(snapshot, raw)?;
        if !alignment_ids.contains(&id) {
            alignment_ids.push(id);
        }
    }
    let selected = |id: &AlignmentId| alignment_ids.is_empty() || alignment_ids.contains(id);
    let changes = snapshot
        .alignments
        .iter()
        .filter(|row| selected(&row.id))
        .map(|row| {
            json!({
                "id": row.id.as_str(),
                "label": row.label,
                "gap_before_m": row.global_station_gap_m,
                "gap_after_m": gap,
            })
        })
        .collect::<Vec<_>>();
    let subject = format!(
        "{gap:?}:{}",
        alignment_ids
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>()
            .join(",")
    );
    let planned = vec![Planned {
        command_id: mutation::command_id("alignment-gap", &opened.head, &subject),
        command: GridCommand::SetAlignmentStationGaps {
            alignment_ids: alignment_ids.clone(),
            global_station_gap_m: gap,
        },
    }];
    let extra = json!({
        "gap": { "authored_gap_m": gap, "alignments": changes.len() },
        "alignments": changes,
    });
    mutation::run(opened, planned, writing, extra, Vec::new(), &["NUM", "DON"])
}

fn resolve_alignment(snapshot: &GridModelSnapshot, raw: &str) -> Result<AlignmentId, Failure> {
    let wanted = raw.trim();
    if let Some(row) = snapshot
        .alignments
        .iter()
        .find(|row| row.id.as_str() == wanted)
    {
        return Ok(row.id.clone());
    }
    let by_label = snapshot
        .alignments
        .iter()
        .filter(|row| row.label == wanted)
        .collect::<Vec<_>>();
    match by_label.as_slice() {
        [row] => Ok(row.id.clone()),
        rows => Err(Failure::invalid(
            "alignment_unknown",
            format!(
                "`{wanted}` names {} alignments in this revision",
                rows.len()
            ),
        )
        .remedy("use an id or unique label from `ds dsgrid alignment gap show`")
        .detail(json!({
            "candidates": rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        }))),
    }
}

pub fn render_show(data: &Value) -> String {
    let mut out = format!(
        "{} alignment(s), {} with an authored gap (default {} m)\n",
        data["alignments"].as_array().map_or(0, Vec::len),
        data["authored"],
        data["default_gap_m"],
    );
    for row in data["alignments"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:>3} {:<24} gap {:>8} start {:>10} end {:>10}\n",
            row["ordinal"],
            row["label"].as_str().unwrap_or("?"),
            row["written_gap_before_m"],
            row["global_station_start_m"],
            row["global_station_end_m"],
        ));
    }
    if let Some(reason) = data["global_stations_unavailable"].as_str() {
        out.push_str(&format!("global stations unavailable: {reason}\n"));
    }
    out
}

pub fn render_set(data: &Value) -> String {
    mutation::render_receipt(data)
}
