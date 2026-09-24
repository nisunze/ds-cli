//! `ds dsgrid model prepare-project` — which of the selected project's exact
//! governed MV heads this machine holds as working copies, and fill the rest.
//!
//! Until 2026-09-20 this asked the paired application whether its browser
//! cache held each head. Headless, the answer is a fact about THIS machine's
//! catalogue of working copies (`ds_command_kernel::local_models`, the store
//! `ds dsgrid model list` reads): a head is ready when a copy pinned to that
//! exact project/model/revision/digest is held, or when any copy's bytes are
//! that digest. `--download-missing` fetches each missing head through the
//! same verified door `ds dsgrid project download` uses and registers it as
//! an `Origin::Project` working copy, pinned to where it came from, never
//! activated. The kernel decides readiness (`local_models::project_readiness`)
//! and which governed rows are MV heads (`local_models::is_mv_head`).

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::local_models::{GovernedHead, Op, Origin, ProjectPin, Scope};
use serde_json::{Value, json};

use crate::model::workspace;

const DOWNLOAD_MISSING_ARG: Arg = Arg {
    name: "download-missing",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Download and verify each missing exact MV head into this machine's working copies, one at a time.",
};

const LANE_ARG: Arg = Arg::value("lane", "<stable|canary>", "Native credential lane.")
    .choices(&["stable", "canary"])
    .default("stable");

/// The listing is paged at the service's bound; a project past this many
/// heads is not folded silently as "complete".
const PAGE: u16 = 100;
const MAX_PAGES: usize = 10;

pub const INVENTORY_UNBOUNDED: Refusal = Refusal {
    code: "grid_project_inventory_unbounded",
    when: "the selected project lists more than 1000 governed MV heads",
    remedy: "prepare heads individually with `ds dsgrid project list` and `ds dsgrid project download`",
};
pub const HEAD_UNVERIFIED: Refusal = Refusal {
    code: "grid_project_head_unverified",
    when: "a downloaded head's declared digest or byte count did not match the bytes received; nothing was registered for it",
    remedy: "retry; if it repeats, the governed head itself is inconsistent — report the model and revision",
};

const HEADLESS: &[Refusal] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals;
const OWN: [Refusal; 2] = [INVENTORY_UNBOUNDED, HEAD_UNVERIFIED];
const TOTAL: usize = HEADLESS.len() + workspace::REFUSALS.len() + OWN.len();
const fn refusals() -> [Refusal; TOTAL] {
    let mut out = [INVENTORY_UNBOUNDED; TOTAL];
    let mut i = 0;
    while i < HEADLESS.len() {
        out[i] = HEADLESS[i];
        i += 1;
    }
    let mut j = 0;
    while j < workspace::REFUSALS.len() {
        out[i + j] = workspace::REFUSALS[j];
        j += 1;
    }
    i += workspace::REFUSALS.len();
    let mut k = 0;
    while k < OWN.len() {
        out[i + k] = OWN[k];
        k += 1;
    }
    out
}
const REFUSALS: [Refusal; TOTAL] = refusals();

pub static COMMAND: Command = Command {
    id: "dsgrid.model.prepare-project",
    path: &["dsgrid", "model", "prepare-project"],
    contract: 1,
    summary: "Show or prepare the selected project's exact DS Grid MV heads.",
    purpose: "Lists every governed MV model head of the CLI-selected project and reports whether this machine holds its exact immutable bytes as a working copy (`ds dsgrid model list`). With --download-missing, downloads and verifies each missing head one at a time and registers it as a project-pinned working copy, never activated. Design and physical printing on this machine consume the same working copies. No window, no browser cache; model bytes never leave this machine's store.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[DOWNLOAD_MISSING_ARG, LANE_ARG],
    output: "Project, total and ready counts, completeness, downloaded ids, and one bounded row per head with model_id, name, revision, digest, cached, local_id and bytes when held.",
    examples: &[],
    refusals: &REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn heads_of(rows: &[Value]) -> Vec<GovernedHead> {
    rows.iter()
        .filter(|row| {
            ds_command_kernel::local_models::is_mv_head(
                row["state"].as_str(),
                row["model_kind"].as_str(),
            )
        })
        .filter_map(|row| {
            Some(GovernedHead {
                model_id: row["model_id"].as_str()?.to_owned(),
                display_name: row["display_name"]
                    .as_str()
                    .unwrap_or(row["model_id"].as_str()?)
                    .to_owned(),
                head_revision_id: row["head_revision_id"].as_str()?.to_owned(),
                head_model_digest: row["head_model_digest"].as_str()?.to_owned(),
            })
        })
        .collect()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let download = inputs.switch("download-missing");

    // 1. Every governed head of the selected project, paged at the bound.
    let mut heads = Vec::new();
    let mut cursor: Option<String> = None;
    let mut project = String::new();
    let mut uid = String::new();
    for page in 0..=MAX_PAGES {
        if page == MAX_PAGES {
            return Err(Failure::invalid(
                INVENTORY_UNBOUNDED.code,
                format!(
                    "{project} lists more than {} governed MV heads",
                    PAGE as usize * MAX_PAGES
                ),
            )
            .remedy(INVENTORY_UNBOUNDED.remedy));
        }
        let report = ds_cli_auth::grid_models(
            lane,
            &ds_cli_auth::GridModelsCommand::List {
                limit: PAGE,
                cursor: cursor.clone(),
                include_deleted: false,
            },
        )?;
        project = report.project_id().to_owned();
        uid = report.identity().uid().to_owned();
        let data = report.into_result().data;
        heads.extend(heads_of(
            data["models"].as_array().map_or(&[][..], Vec::as_slice),
        ));
        if data["more"] == true {
            cursor = data["next_cursor"].as_str().map(str::to_owned);
        } else {
            break;
        }
    }

    // 2. This machine's working copies, under the credential's own scope.
    let scope = Scope {
        lane: lane.to_owned(),
        uid,
    };
    let catalogue = workspace::read_in(&scope)?;
    let mut rows = ds_command_kernel::local_models::project_readiness(&project, &heads, &catalogue);

    // 3. Fill the missing heads, one verified download and one registration
    // at a time, each pinned to where it came from.
    let mut downloaded = Vec::new();
    if download {
        for row in rows.iter_mut().filter(|row| !row.cached) {
            let receipt = ds_cli_auth::grid_models(
                lane,
                &ds_cli_auth::GridModelsCommand::Download {
                    model: row.model_id.clone(),
                    revision: row.revision.clone(),
                },
            )?
            .into_result();
            let bytes = receipt.bytes.ok_or_else(|| {
                Failure::invalid(
                    HEAD_UNVERIFIED.code,
                    "the verified owner returned no package",
                )
                .remedy(HEAD_UNVERIFIED.remedy)
            })?;
            let identity = workspace::identity(&bytes)?;
            if !identity.sha256.eq_ignore_ascii_case(&row.digest) {
                return Err(Failure::invalid(
                    HEAD_UNVERIFIED.code,
                    format!(
                        "{} {} declares digest {} but its bytes are {}",
                        row.model_id, row.revision, row.digest, identity.sha256
                    ),
                )
                .remedy(HEAD_UNVERIFIED.remedy));
            }
            let id = workspace::mint_id();
            let outcome = workspace::execute_in(
                &scope,
                Op::Register {
                    id: id.clone(),
                    display_name: row.name.clone(),
                    origin: Origin::Project,
                    crs: identity.crs,
                    model_revision: identity.model_revision,
                    bytes: identity.bytes,
                    sha256: identity.sha256,
                    created_at: None,
                    project: Some(ProjectPin {
                        project_id: project.clone(),
                        model_id: row.model_id.clone(),
                        revision_id: row.revision.clone(),
                        digest: row.digest.clone(),
                    }),
                    head_revision: Some(identity.authored_revision),
                    // Preparation is not a decision to work in it.
                    activate: false,
                },
                Some(&bytes),
            )?;
            let held = outcome.model.as_ref().ok_or_else(|| {
                Failure::internal("local_model_store_unavailable", "nothing was registered")
            })?;
            row.cached = true;
            row.local_id = Some(held.id.clone());
            row.bytes = Some(held.bytes);
            downloaded.push(id);
        }
    }

    let ready = rows.iter().filter(|row| row.cached).count();
    Ok(json!({
        "project": project,
        "lane": lane,
        "total": rows.len(),
        "ready": ready,
        "complete": ready == rows.len(),
        "downloaded": downloaded,
        "models": rows,
    }))
}

pub fn render(data: &Value) -> String {
    let ready = data["ready"].as_u64().unwrap_or(0);
    let total = data["total"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{ready}/{total} project MV model heads held on this machine ({})\n",
        data["project"].as_str().unwrap_or("?")
    );
    for row in data["models"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        out.push_str(&format!(
            "  {} · {} · {}\n",
            row["name"].as_str().unwrap_or("unnamed"),
            row["revision"].as_str().unwrap_or("unknown revision"),
            if row["cached"].as_bool().unwrap_or(false) {
                format!("held as {}", row["local_id"].as_str().unwrap_or("?"))
            } else {
                "missing".to_owned()
            }
        ));
    }
    if let Some(downloaded) = data["downloaded"]
        .as_array()
        .filter(|rows| !rows.is_empty())
    {
        out.push_str(&format!("  {} downloaded this run\n", downloaded.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::args::parse;

    #[test]
    fn the_descriptor_is_headless_and_declares_every_refusal_once() {
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.requires, Requires::Server);
        let mut codes: Vec<&str> = COMMAND.refusals.iter().map(|r| r.code).collect();
        let before = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), before, "a refusal is declared twice");
        assert!(codes.contains(&"headless_signed_out"));
        assert!(codes.contains(&"local_model_store_unavailable"));
        assert!(codes.contains(&INVENTORY_UNBOUNDED.code));
        assert!(
            !COMMAND
                .args
                .iter()
                .any(|arg| arg.name == "desktop-descriptor"),
            "the window path is retired"
        );
        let parsed = parse(&COMMAND, &["--download-missing".to_owned()]).expect("parses");
        assert!(parsed.switch("download-missing"));
    }

    #[test]
    fn only_live_mv_rows_become_heads() {
        let rows = vec![
            json!({"model_id":"a","display_name":"A","head_revision_id":"r1","head_model_digest":"sha256:aa","state":"active","model_kind":"mv_line"}),
            json!({"model_id":"b","display_name":"B","head_revision_id":"r2","head_model_digest":"bb","state":"deleted"}),
            json!({"model_id":"c","head_revision_id":"r3","head_model_digest":"cc"}),
            json!({"model_id":"d","display_name":"D","head_revision_id":"r4","head_model_digest":"dd","model_kind":"lv_network"}),
            json!({"model_id":"e","display_name":"E"}),
        ];
        let heads = heads_of(&rows);
        let ids: Vec<&str> = heads.iter().map(|h| h.model_id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
        assert_eq!(
            heads[1].display_name, "c",
            "a nameless row is named by its id"
        );
    }
}
