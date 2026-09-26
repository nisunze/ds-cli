//! `ds report project combined` — publish one **Combined Report** archive in
//! the background against the CLI-named project.
//!
//! This is the command that used to be called `compounded`. One deliverable
//! had two names — the governed service already titled it "Combined Report"
//! for humans while every id, receipt key and help screen said "compounded" —
//! and an operator cannot ask a question about a thing whose name changes
//! between the screen and the command line. The old id worked, deprecated,
//! for one release (stable 487c432 and canary 5318beb both carried it) and is
//! gone; `compounded` stays a search word so an agent using it lands here.
//!
//! The other change is that this command REFUSES.
//!
//! The governed service can publish an archive over a scope it has quietly
//! shrunk: a room whose report is missing, months old, or sealed on somebody's
//! PC and never drained contributes nothing, and the archive is still labelled
//! a success. An operator then hands over a Combined Report that is missing
//! half a project, or that contains June's files, believing it is the answer.
//! So the receipt is read through
//! [`ds_command_kernel::combined_readiness`] — the same predicate the Desktop
//! dialog refuses from, so the two cannot disagree — and a Combined Report
//! whose inputs are not current fails, naming the rooms and the command that
//! makes them current.

use ds_cli_auth::{CompoundedReportRequest, ReportFileLevel};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::combined_readiness::{self, RoomInput};
use serde_json::{Value, json};

use super::grouping::{self, GROUP_BY_ARG, WHERE_ARG};
use super::{LANE_ARG, PROJECT_ARG, TRANSFORMER_ARG};

pub(super) const FILE_LEVEL_ARG: Arg = Arg::value(
    "file-level",
    "<transformer|sector|district|root>",
    "Requested folder level; district/sector folders need resolved administrative values.",
)
.default("transformer")
.choices(&["transformer", "sector", "district", "root"]);
pub(super) const COMBINE_PER_GROUP_ARG: Arg = Arg::switch(
    "combine-per-group",
    "Also file one combined set for each first-level applied report group.",
);
pub(super) const FORCE_ARG: Arg = Arg::switch(
    "force",
    "Regenerate every individual artifact instead of reusing fresh ones.",
);

pub(super) const ARGS: &[Arg] = &[
    TRANSFORMER_ARG,
    GROUP_BY_ARG,
    WHERE_ARG,
    FILE_LEVEL_ARG,
    COMBINE_PER_GROUP_ARG,
    FORCE_ARG,
    LANE_ARG,
    PROJECT_ARG,
];

pub(super) const PURPOSE: &str = "\
After confirmation, asks the governed report service for a Combined Report \
archive over the named project: it resolves the scope, composes the sets and publishes one \
ZIP with a registry row. District and sector folders come from the project's \
applied `report_archive` grouping, not from this request. Retired \
transformers are never in scope. Rooms not current refuse the run. Blocks \
until the service answers \
(up to ten minutes). A project is required; no URL, body or action override is accepted. \
--group-by/--where publish an archive per leaf tag group, in turn \
(`_unassigned` holds the untagged); nothing is saved on the project.";

pub(super) const OUTPUT: &str = "\
Lane, named project ID, requested scope, and \
`archive_layout` — the layout asked for, never the tree achieved: \
unresolved administrative values collapse to `_unassigned/`, and `ds report \
project archives` confirms the tree built. Then the receipt: status, \
`prefix`, cloud locators, individual coverage, missing individuals with \
causes, bounded errors and registry-write failure. Grouped: `grouping` \
and one `groups` receipt per archive.";

pub static COMMAND: Command = Command {
    id: "report.project.combined",
    path: &["report", "project", "combined"],
    contract: 1,
    summary: "Publish a Combined Report archive, or one per tag group (needs --yes).",
    purpose: PURPOSE,
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: ARGS,
    output: OUTPUT,
    examples: &[
        Example {
            command: "ds report project combined --file-level sector --yes --output json --project <exact-id>",
            note: "`ds report project archives` then confirms the foldering built.",
            runnable: false,
        },
        Example {
            command: "ds report project combined --group-by district --group-by city --where phase=i --yes --output json --project <exact-id>",
            note: "An archive per district/city pair, phase i only.",
            runnable: false,
        },
    ],
    refusals: super::COMBINED_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[
        "compounded",
        "combined report",
        "zip",
        "per city",
        "group by",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let file_level = ReportFileLevel::parse(inputs.require("file-level")?)
        .expect("the command parser enforces the file-level choices");
    let combine_per_group = inputs.switch("combine-per-group");
    let force = inputs.switch("force");
    let (lane, project) = (inputs.require("lane")?, inputs.require("project")?);
    if let Some(requested) = grouping::requested(inputs)? {
        return run_grouped(
            lane,
            project,
            &requested,
            file_level,
            combine_per_group,
            force,
        );
    }
    let transformers = super::transformer_set(inputs)?;
    let request = CompoundedReportRequest::new(transformers, file_level, combine_per_group, force);
    publish(lane, project, &request)
}

/// One leaf tag group per archive, run one after another through the SAME
/// publish a single run uses, each over that group's explicit scope.
///
/// Every group's scope is validated before the first archive is published.
/// A refusal about one group's rooms lets the next group run; any other
/// refusal would refuse every group alike, so the rest are left `not_run`.
/// A run that did not publish every group never exits zero.
fn run_grouped(
    lane: &str,
    project: &str,
    requested: &grouping::Requested,
    file_level: ReportFileLevel,
    combine_per_group: bool,
    force: bool,
) -> Result<Value, Failure> {
    let grouped = grouping::resolve(lane, project, requested)?;
    let scopes = grouping::group_scopes(&grouped.plan)?;
    let summary = grouping::grouping_json(&grouped, false);
    let mut receipts: Vec<Value> = Vec::new();
    let mut refusals: Vec<Failure> = Vec::new();
    let mut stopped = false;
    for (group, scope) in grouped.plan.groups.iter().zip(scopes) {
        if stopped {
            receipts.push(grouping::not_run(group));
            continue;
        }
        let request = CompoundedReportRequest::new(scope, file_level, combine_per_group, force);
        match publish(lane, project, &request) {
            Ok(output) => receipts.push(grouping::published(group, &output)),
            Err(failure) => {
                receipts.push(grouping::refused(group, &failure));
                stopped = !grouping::group_scoped(&failure);
                refusals.push(failure);
            }
        }
    }
    let published = receipts.len() - refusals.len() - not_run_count(&receipts);
    if published == 0 && refusals.len() == 1 {
        // Exactly one attempt, and it refused: the caller needs THAT refusal,
        // with its own code, remedy and detail, not a summary of one.
        return Err(refusals.remove(0));
    }
    if published < receipts.len() {
        return Err(grouping::partial(summary, receipts));
    }
    let mut output = grouped.receipt;
    let fields = json!({
        "scope": {"mode": "grouped", "requested": []},
        "grouping": summary,
        "archive_layout": archive_layout(file_level.token(), combine_per_group),
        "force": force,
        "published_count": published,
        "groups": receipts,
    });
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(fields.as_object().expect("fields are an object").clone());
    Ok(output)
}

fn not_run_count(receipts: &[Value]) -> usize {
    receipts
        .iter()
        .filter(|receipt| receipt["status"] == "not_run")
        .count()
}

/// One Combined Report archive over one request's scope, read for what it
/// does not contain.
fn publish(lane: &str, project: &str, request: &CompoundedReportRequest) -> Result<Value, Failure> {
    let file_level = request.file_level();
    let combine_per_group = request.combine_per_district();
    let force = request.force();
    let headless = ds_cli_auth::compounded_report_for_project(lane, project, request)?;
    let receipt = headless.result();
    let mut output = super::project_receipt(&headless);
    let causes: Vec<Value> = receipt
        .missing_individual_artifact_causes()
        .iter()
        .map(|cause| {
            json!({
                "transformer": cause.transformer(),
                "code": cause.code(),
                "detail": cause.detail(),
            })
        })
        .collect();
    let fields = json!({
        "scope": {
            "mode": if request.transformers().is_empty() { "all_active" } else { "explicit" },
            "requested": request.transformers().names(),
        },
        "archive_layout": archive_layout(file_level.token(), combine_per_group),
        "force": force,
        "status": receipt.status().token(),
        "prefix": receipt.prefix(),
        "archives": receipt.archive_paths(),
        "cached": receipt.cached(),
        "individual_artifact_transformer_count": receipt.individual_artifact_transformer_count(),
        "missing_individual_artifact_count": receipt.missing_individual_artifact_count(),
        "missing_individual_artifacts": receipt.missing_individual_artifacts(),
        "missing_individual_artifact_causes": causes.clone(),
        "errors": receipt.errors(),
        "registry_write_failed": receipt.registry_write_failed(),
        "registry_write_error": receipt.registry_write_error(),
    });
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(fields.as_object().expect("fields are an object").clone());

    // The receipt is read for what it does NOT contain. A published archive
    // with missing rooms is not a Combined Report an operator may hand over;
    // succeeding here is how a project was delivered with June's files in it.
    if let Some(failure) = readiness_refusal(&causes, &output)? {
        return Err(failure);
    }
    Ok(output)
}

/// Turn the service's per-room causes into the kernel's readiness question.
///
/// The publication vocabulary is READ, not redefined here: a room the service
/// reports as pending publication carries that through to the predicate, which
/// is what makes "drain the queue" the remedy instead of a re-export that
/// would seal a second artifact beside the first.
fn readiness_rooms(causes: &[Value]) -> Vec<RoomInput> {
    causes
        .iter()
        .map(|cause| {
            let name = cause["transformer"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            match cause["code"].as_str().unwrap_or_default() {
                "report_publication_pending" => RoomInput {
                    name,
                    publication: Some("pending".into()),
                    ..Default::default()
                },
                "report_stale" => RoomInput {
                    name,
                    report_state: Some("stale".into()),
                    ..Default::default()
                },
                // Every other cause — a failed export, an unusable pointer, a
                // room that was never published — is "no current report here".
                // The predicate's remedy for all of them is the same command.
                _ => RoomInput {
                    name,
                    report_state: Some("none".into()),
                    ..Default::default()
                },
            }
        })
        .collect()
}

/// The refusal, when the archive that was published does not cover its scope.
fn readiness_refusal(causes: &[Value], receipt: &Value) -> Result<Option<Failure>, Failure> {
    if causes.is_empty() {
        return Ok(None);
    }
    let rooms = readiness_rooms(causes);
    let verdict = combined_readiness::evaluate_value(&rooms, combined_readiness::DEFAULT_SHOWN)
        .map_err(|error| {
            Failure::unavailable(
                super::COMBINED_READINESS_UNAVAILABLE.code,
                format!("The Combined Report readiness predicate could not answer: {error}"),
            )
            .remedy(super::COMBINED_READINESS_UNAVAILABLE.remedy)
        })?;
    let Some(refusal) = verdict.get("refusal").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let command = refusal["command"]
        .as_str()
        .unwrap_or("ds report project scope");
    let named: Vec<&str> = refusal["rooms"]["names"]
        .as_array()
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let more = refusal["rooms"]["more"].as_u64().unwrap_or(0);
    let mut message = format!(
        "The Combined Report was published without {} room(s): {}",
        refusal["rooms"]["total"]
            .as_u64()
            .unwrap_or(named.len() as u64),
        named.join(", "),
    );
    if more > 0 {
        message.push_str(&format!(" and {more} more"));
    }
    message.push_str(". It does not cover its scope; do not hand it over as the answer.");
    // The kernel decides WHICH refusal applies; each one is constructed here
    // from its own declared `Refusal`, so `ds capabilities` lists every code
    // this command can emit and nothing has to infer them from a variable. A
    // token this build does not know is NOT passed through as an undocumented
    // code — it becomes the conservative one, "these rooms are not current",
    // whose remedy is a fresh run and is never wrong.
    let failure = match refusal["token"].as_str().unwrap_or_default() {
        "combined_inputs_publication_pending" => {
            Failure::conflict(super::COMBINED_INPUTS_PUBLICATION_PENDING.code, message)
        }
        "combined_inputs_empty" => Failure::conflict(super::COMBINED_INPUTS_EMPTY.code, message),
        "combined_no_inputs" => Failure::conflict(super::COMBINED_NO_INPUTS.code, message),
        _ => Failure::conflict(super::COMBINED_INPUTS_NOT_CURRENT.code, message),
    };
    Ok(Some(
        failure
            .remedy(format!("run `{command}`, then run this command again"))
            .next(command.to_string())
            .next("ds report project scope")
            .detail(json!({
                "readiness": verdict,
                "published": {
                    "prefix": receipt["prefix"].clone(),
                    "archives": receipt["archives"].clone(),
                },
            })),
    ))
}

/// The requested layout, in the report layer's own words.
///
/// `combine_per_group` is the current name for the choice the registry has
/// always recorded as `combine_per_district`; both are reported so a caller
/// reading a fresh receipt and one reading an old registry row see the same
/// archive described the same way.
pub(super) fn archive_layout(file_level: &str, combine_per_group: bool) -> Value {
    // The vocabulary is the shared fold's; the RECORDED members are this
    // receipt's own and win, so describing an archive can never restate what
    // was asked for. (`describe` echoes `combine_per_group` from the layout it
    // was handed, which is the registry's spelling and not this flag.)
    let mut layout = super::archive_layout_vocabulary(Some(file_level), None, combine_per_group);
    let recorded = json!({
        "file_level": file_level,
        "combine_per_group": combine_per_group,
    });
    layout
        .as_object_mut()
        .expect("layout is an object")
        .extend(recorded.as_object().expect("recorded is an object").clone());
    layout
}

pub fn render(data: &Value) -> String {
    if data["groups"].is_array() {
        return render_grouped(data);
    }
    let mut out = String::new();
    out.push_str(&format!(
        "project {} ({}) · {} · {} · Combined Report {} · {} individual artifact(s), {} missing\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["status"].as_str().unwrap_or("?"),
        data["prefix"].as_str().unwrap_or("?"),
        data["individual_artifact_transformer_count"]
            .as_u64()
            .unwrap_or(0),
        data["missing_individual_artifact_count"]
            .as_u64()
            .unwrap_or(0),
    ));
    if let Some(archives) = data["archives"].as_array() {
        for archive in archives {
            out.push_str(&format!("  {}\n", archive.as_str().unwrap_or("?")));
        }
    }
    if let Some(causes) = data["missing_individual_artifact_causes"].as_array() {
        for cause in causes {
            out.push_str(&format!(
                "  missing {:<28} {}\n",
                cause["transformer"].as_str().unwrap_or("?"),
                cause["detail"]
                    .as_str()
                    .or_else(|| cause["code"].as_str())
                    .unwrap_or("?"),
            ));
        }
    }
    if data["registry_write_failed"].as_bool().unwrap_or(false) {
        out.push_str("  registry row not written; future runs will not see this archive\n");
    }
    out
}

/// One line per group archive, in the order they were published.
fn render_grouped(data: &Value) -> String {
    let groups = data["groups"].as_array().cloned().unwrap_or_default();
    let mut out = format!(
        "project {} · {} · {} Combined Report archive(s) of {} group(s) over {} transformer(s)\n",
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["published_count"].as_u64().unwrap_or(0),
        groups.len(),
        data["grouping"]["transformer_count"].as_u64().unwrap_or(0),
    );
    for group in &groups {
        out.push_str(&format!(
            "  {:<40} {:>4} · {} · {}\n",
            super::scope::path_line(&group["path"]),
            group["transformer_count"].as_u64().unwrap_or(0),
            group["status"].as_str().unwrap_or("?"),
            group["prefix"].as_str().unwrap_or("-"),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each archive is one line naming its group, so an operator can see
    /// which city a prefix belongs to without opening JSON.
    #[test]
    fn a_grouped_run_renders_one_line_per_group_archive() {
        let data = json!({
            "project": {"ds_project": "p1"}, "lane": "canary", "published_count": 2,
            "grouping": {"transformer_count": 20},
            "groups": [
                {"path": [{"key": "city", "value": "bere"}], "transformer_count": 12,
                 "status": "success", "prefix": "run-bere"},
                {"path": [{"key": "city", "value": "_unassigned"}], "transformer_count": 8,
                 "status": "success", "prefix": "run-rest"},
            ],
        });
        let rendered = render(&data);
        assert!(
            rendered.contains("2 Combined Report archive(s) of 2 group(s)"),
            "{rendered}"
        );
        assert!(rendered.contains("city=bere"), "{rendered}");
        assert!(rendered.contains("run-bere"), "{rendered}");
        assert!(rendered.contains("city=_unassigned"), "{rendered}");
    }

    /// Grouping and naming transformers are alternatives, and every grouping
    /// refusal is one this command declares.
    #[test]
    fn the_grouping_flags_are_declared_once_with_their_refusals() {
        for flag in ["group-by", "where", "transformer"] {
            assert!(COMMAND.arg(flag).is_some(), "--{flag}");
        }
        let declared: Vec<&str> = COMMAND.refusals.iter().map(|r| r.code).collect();
        for code in [
            "combined_group_scope_conflict",
            "combined_groups_too_many",
            "combined_groups_empty",
            "combined_groups_partial",
            // Read from the published receipt; declared here since 09-26.
            "combined_inputs_not_current",
        ] {
            assert!(declared.contains(&code), "{code} undeclared");
        }
    }

    fn cause(name: &str, code: &str) -> Value {
        json!({"transformer": name, "code": code, "detail": ""})
    }

    /// The deliverable has ONE name on the command line, and that name is the
    /// one the governed service already shows humans.
    #[test]
    fn the_command_is_named_combined_everywhere_a_caller_reads_it() {
        assert_eq!(COMMAND.id, "report.project.combined");
        assert_eq!(COMMAND.path, &["report", "project", "combined"]);
        assert!(
            COMMAND.summary.contains("Combined Report"),
            "{}",
            COMMAND.summary
        );
        assert!(!COMMAND.summary.to_lowercase().contains("compounded"));
        // A stranger who only knows the old word must still find it.
        assert!(COMMAND.search.contains(&"compounded"));
    }

    /// Rooms whose reports are sealed on a PC and not drained must be told to
    /// DRAIN, not to export again — exporting seals a second artifact beside
    /// the first, which is the hoarding this campaign removes.
    #[test]
    fn pending_rooms_refuse_with_the_drain_command() {
        let causes = vec![cause("tx_a", "report_publication_pending")];
        let failure = readiness_refusal(&causes, &json!({}))
            .expect("the predicate answers")
            .expect("a pending room refuses");
        assert_eq!(failure.code(), "combined_inputs_publication_pending");
        assert!(failure.message().contains("tx_a"), "{}", failure.message());
        assert!(
            failure
                .next_commands()
                .contains(&"ds report outbox drain".to_string()),
            "{:?}",
            failure.next_commands()
        );
    }

    /// A Combined Report published over a scope it silently shrank must not
    /// exit zero. That success is what let a project be handed over missing
    /// half its rooms.
    #[test]
    fn a_published_archive_with_missing_rooms_refuses_and_names_them() {
        let causes = vec![
            cause("tx_a", "report_not_published"),
            cause("tx_b", "report_stale"),
        ];
        let receipt = json!({"prefix": "run-1", "archives": ["gs://b/run-1.zip"]});
        let failure = readiness_refusal(&causes, &receipt)
            .expect("the predicate answers")
            .expect("missing rooms refuse");
        assert_eq!(failure.code(), "combined_inputs_not_current");
        assert!(failure.message().contains("tx_a"), "{}", failure.message());
        assert!(failure.message().contains("tx_b"), "{}", failure.message());
        assert!(
            failure
                .remedy_text()
                .unwrap_or_default()
                .contains("ds report project export --transformer tx_a --transformer tx_b"),
            "{:?}",
            failure.remedy_text()
        );
        // The archive that WAS published is still named, so nobody hunts for a
        // file the refusal did not mention.
        let detail = failure.detail_value().expect("detail travels");
        assert_eq!(detail["published"]["prefix"], "run-1");
    }

    /// A complete run is not turned into a refusal by the gate.
    #[test]
    fn a_complete_run_is_not_refused() {
        assert!(
            readiness_refusal(&[], &json!({}))
                .expect("the predicate answers")
                .is_none()
        );
    }

    /// The receipt reports what was ASKED FOR. The shared vocabulary echoes a
    /// registry layout's own `combine_per_group`, which this receipt does not
    /// carry — so merging the vocabulary over the recorded members used to
    /// report every `--combine-per-group` run as `false`.
    #[test]
    fn the_requested_grouping_survives_the_shared_vocabulary() {
        for requested in [true, false] {
            let layout = archive_layout("transformer", requested);
            assert_eq!(
                layout["combine_per_group"],
                json!(requested),
                "the receipt must report the grouping the caller asked for"
            );
            assert_eq!(layout["file_level"], json!("transformer"));
            // The vocabulary still arrives beside it, in keys.
            assert_eq!(layout["level_key"], "pctl_layout_transformer");
        }
        assert_eq!(
            archive_layout("district", true)["label_key"],
            "pctl_layout_per_district"
        );
    }

    /// The `archive_layout` key is pinned by the contract, so the honesty has
    /// to live in the prose beside it: the receipt reports what was asked
    /// for, and only the registry says what was built.
    #[test]
    fn the_descriptor_calls_archive_layout_a_request_not_a_tree() {
        assert!(
            COMMAND
                .output
                .contains("the layout asked for, never the tree achieved"),
            "{}",
            COMMAND.output
        );
        assert!(
            COMMAND.output.contains("`_unassigned/`"),
            "{}",
            COMMAND.output
        );
        assert!(
            COMMAND.output.contains("ds report project archives"),
            "{}",
            COMMAND.output
        );
        assert!(
            COMMAND
                .purpose
                .contains("applied `report_archive` grouping"),
            "{}",
            COMMAND.purpose
        );
        assert!(
            FILE_LEVEL_ARG.summary.starts_with("Requested folder level"),
            "{}",
            FILE_LEVEL_ARG.summary
        );
    }

    /// The refusal a reader sees first is the one line the render prints.
    #[test]
    fn the_render_says_combined_report_and_never_the_retired_name() {
        let data = json!({
            "project": {"project_name": "p", "ds_project": "p1"},
            "lane": "stable", "status": "success", "prefix": "run-1",
            "individual_artifact_transformer_count": 2,
            "missing_individual_artifact_count": 0,
        });
        let rendered = render(&data);
        assert!(rendered.contains("Combined Report"), "{rendered}");
        assert!(
            !rendered.to_lowercase().contains("compounded"),
            "{rendered}"
        );
    }
}
