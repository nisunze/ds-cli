//! Explicit library update propagation over named local followers. The
//! native owner (`ds_grid_exchange::library_impact`) computes every plan and
//! revision; this module reads explicit paths, writes a fresh directory and
//! shapes the answer. Cloud publication of a written revision stays the
//! separate, head-fenced `dsgrid.publish-version`.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::library_impact::{
    FollowerImpact, FollowerStatus, LibraryImpactPlan, MAX_FOLLOWERS, ProposedRelease,
    apply_follower, impact_plan_id, plan_follower,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const RELEASE: Arg = Arg::value(
    "release",
    "<path.dsgrid-library>",
    "The proposed immutable library release.",
)
.required();
const EXPECTED_RELEASE: Arg = Arg::value(
    "expected-library-sha256",
    "<sha256:hex>",
    "SHA-256 of the exact proposed release bundle.",
)
.required();
const FOLLOWER: Arg = Arg::repeated(
    "follower",
    "<sha256:hex>=<path.dsgrid>",
    "One follower model and the exact digest you observed (its expected head); repeat, 1..64.",
);
const ELEMENT_LIMIT: Arg = Arg::value(
    "limit",
    "<n>",
    "Changed or missing elements listed per follower, 1..500; the rest are counted.",
)
.default("50");

const SEARCH: &[&str] = &["library update", "propagation", "impact plan"];

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "digest_conflict",
        when: "the proposed release bytes differ from --expected-library-sha256",
        remedy: "verify the intended exact release and pass its SHA-256",
    },
    Refusal {
        code: "library_resolution_required",
        when: "the proposed release has unresolved dependencies of its own",
        remedy: "propose a self-contained release whose dependencies are resolved",
    },
    Refusal {
        code: "library_kind_invalid",
        when: "a defaults template is proposed as an asset library",
        remedy: "propose an asset-bundle release",
    },
    Refusal {
        code: "library_identity_conflict",
        when: "the proposed release claims one engineering name for differing definitions",
        remedy: "repair the release at its source; no follower can adopt an ambiguous release",
    },
    Refusal {
        code: "library_selection_invalid",
        when: "no follower, more than 64, a repeated path, or a --follower not shaped <sha256:hex>=<path>",
        remedy: "name 1..64 distinct followers as --follower sha256:<64 hex>=<path.dsgrid>",
    },
    Refusal {
        code: "library_operation_failed",
        when: "native library verification refuses the proposed release",
        remedy: "inspect the returned native refusal; nothing was written",
    },
    Refusal {
        code: "library_path_not_found",
        when: "an explicit input path is unreadable",
        remedy: "use the intended existing local artifact",
    },
    Refusal {
        code: "library_path_not_file",
        when: "an input is not a file",
        remedy: "pass the exact model or release file",
    },
    Refusal {
        code: "library_file_too_large",
        when: "an input exceeds the local read bound",
        remedy: "use a bounded portable model/library artifact",
    },
    Refusal {
        code: "library_read_failed",
        when: "local input reading fails",
        remedy: "check access to the explicit artifact",
    },
];

const APPLY_REFUSALS: &[Refusal] = &[
    REFUSALS[0],
    REFUSALS[1],
    REFUSALS[2],
    REFUSALS[3],
    REFUSALS[4],
    REFUSALS[5],
    REFUSALS[6],
    REFUSALS[7],
    REFUSALS[8],
    REFUSALS[9],
    Refusal {
        code: "impact_plan_mismatch",
        when: "--plan-id is not the plan these exact release and follower arguments produce",
        remedy: "run impact-plan with the same arguments, review it and pass its plan_id",
    },
    Refusal {
        code: "output_exists",
        when: "the output directory already exists",
        remedy: "choose a new output directory",
    },
    Refusal {
        code: "output_unwritable",
        when: "the output directory cannot be created",
        remedy: "choose an accessible new output path",
    },
    Refusal {
        code: "impact_apply_incomplete",
        when: "a named follower was refused (stale head or blocked) or left unprocessed",
        remedy: "read detail.followers: applied revisions are written; re-read refused followers and plan again",
    },
];

pub static PLAN: Command = Command {
    id: "library.model.impact-plan",
    path: &["library", "model", "impact-plan"],
    contract: 1,
    summary: "Plan a library update across named follower models, read-only.",
    purpose: "Before any model follows a new library release, shows per named follower (with the exact digest you observed) whether it can move: its current and proposed pins, each required element whose definition changes with row-level before/after values, native bytes the release brings, the engineering input root before and after, and what blocks it. Publishing a release never revises a model; impact-apply writes the reviewed plan. No solver or engineering approval.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[RELEASE, EXPECTED_RELEASE, FOLLOWER, ELEMENT_LIMIT],
    output: "plan_id, the proposed release pin, applicable/blocked/unaffected counts, and per follower: status (applicable, already_current, not_following, stale_head, blocked), current and proposed pins, changed or missing elements with row changes, native resources added, engineering input roots, resulting revision and blockers.",
    examples: &[Example {
        command: "ds library model impact-plan --release ./reusable-r2.dsgrid-library --expected-library-sha256 sha256:<release> --follower sha256:<model>=./pinned.dsgrid --output json",
        note: "Review each follower's changes and blockers, then apply with the plan_id.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: SEARCH,
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub static APPLY: Command = Command {
    id: "library.model.impact-apply",
    path: &["library", "model", "impact-apply"],
    contract: 1,
    summary: "Apply a reviewed library update plan, writing new follower revisions.",
    purpose: "Recomputes the plan from the same release and follower arguments and refuses unless it is the reviewed --plan-id. Writes one new revision per applicable follower into a fresh directory, replacing only the closure the old pin supplied, retaining old native bytes and releases, and refusing a follower whose bytes moved since the plan. The receipt separates applied, refused and unprocessed followers. Publish each revision separately with its project and expected head.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        RELEASE,
        EXPECTED_RELEASE,
        FOLLOWER,
        Arg::value(
            "plan-id",
            "<sha256:hex>",
            "The plan_id impact-plan returned for these exact arguments.",
        )
        .required(),
        Arg::value(
            "out-dir",
            "<new-dir>",
            "Fresh directory for the new revisions; never overwritten.",
        )
        .required(),
    ],
    output: "plan_id, complete, applied/refused/unaffected/unprocessed counts, and per follower: outcome, planned status, written path, resulting digest and revision, pins and blockers. Local only; cloud_write is false.",
    examples: &[Example {
        command: "ds library model impact-apply --release ./reusable-r2.dsgrid-library --expected-library-sha256 sha256:<release> --follower sha256:<model>=./pinned.dsgrid --plan-id sha256:<plan> --out-dir ./following-r2 --output json",
        note: "Same release and follower arguments as the reviewed plan.",
        runnable: false,
    }],
    refusals: APPLY_REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: SEARCH,
    requires: Requires::Server,
    availability: || Availability::Available,
};

fn owner_error(message: String) -> Failure {
    let prefix = message.split(':').next().unwrap_or("");
    match prefix {
        "digest_conflict" => Failure::failed("digest_conflict", message),
        "library_resolution_required" => Failure::failed("library_resolution_required", message),
        "library_kind_invalid" => Failure::failed("library_kind_invalid", message),
        "library_identity_conflict" => Failure::failed("library_identity_conflict", message),
        _ => Failure::failed("library_operation_failed", message),
    }
}

fn selection(message: impl Into<String>) -> Failure {
    Failure::invalid("library_selection_invalid", message)
        .remedy("name 1..64 distinct followers as --follower sha256:<64 hex>=<path.dsgrid>")
}

/// The named followers as (label = path, expected digest), in order.
fn followers(inputs: &Inputs) -> Result<Vec<(String, String)>, Failure> {
    let raw = inputs.repeated("follower");
    if raw.is_empty() || raw.len() > MAX_FOLLOWERS {
        return Err(selection(format!(
            "{} followers named; name 1..{MAX_FOLLOWERS}",
            raw.len()
        )));
    }
    let mut named = Vec::with_capacity(raw.len());
    for value in raw {
        let Some((digest, path)) = value.split_once('=') else {
            return Err(selection(format!("`{value}` is not <sha256:hex>=<path>")));
        };
        let hex = digest.strip_prefix("sha256:").unwrap_or("");
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(selection(format!(
                "`{digest}` is not sha256:<64 lowercase hex>"
            )));
        }
        if path.is_empty() {
            return Err(selection("a follower path is empty"));
        }
        if named
            .iter()
            .any(|(label, _): &(String, String)| label == path)
        {
            return Err(selection(format!("follower `{path}` is named twice")));
        }
        named.push((path.to_string(), digest.to_string()));
    }
    Ok(named)
}

fn proposed(inputs: &Inputs) -> Result<ProposedRelease, Failure> {
    let bytes = crate::read(inputs.require("release")?)?;
    ProposedRelease::open(bytes, inputs.require("expected-library-sha256")?).map_err(owner_error)
}

/// Bound one follower's element list to the changed or missing ones.
fn shaped(mut impact: FollowerImpact, limit: usize) -> Value {
    let unchanged = impact
        .elements
        .iter()
        .filter(|element| {
            element.change == ds_grid_exchange::library_impact::ElementChange::Unchanged
        })
        .count();
    impact.elements.retain(|element| {
        element.change != ds_grid_exchange::library_impact::ElementChange::Unchanged
    });
    let listed = impact.elements.len().min(limit);
    let omitted = impact.elements.len() - listed;
    impact.elements.truncate(listed);
    let mut value = serde_json::to_value(&impact).unwrap_or(Value::Null);
    value["elements_unchanged"] = json!(unchanged);
    value["elements_omitted"] = json!(omitted);
    value
}

pub fn plan(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let named = followers(inputs)?;
    let limit = inputs
        .value("limit")
        .unwrap_or("50")
        .parse::<usize>()
        .ok()
        .filter(|limit| (1..=500).contains(limit))
        .ok_or_else(|| {
            Failure::invalid("library_selection_invalid", "--limit is outside 1..500")
        })?;
    let proposed = proposed(inputs)?;
    let mut impacts = Vec::with_capacity(named.len());
    for (label, digest) in &named {
        let bytes = crate::read(label)?;
        impacts.push(plan_follower(&proposed, label, digest, &bytes));
    }
    let plan = LibraryImpactPlan::new(&proposed, impacts);
    let followers = plan
        .followers
        .iter()
        .cloned()
        .map(|impact| shaped(impact, limit))
        .collect::<Vec<_>>();
    Ok(json!({
        "schema": plan.schema,
        "plan_id": plan.plan_id,
        "proposed": plan.proposed,
        "applicable": plan.applicable,
        "blocked": plan.blocked,
        "unaffected": plan.unaffected,
        "followers": followers,
        "cloud_write": false,
        "solver_approval": false,
    }))
}

pub fn apply(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let named = followers(inputs)?;
    let proposed = proposed(inputs)?;
    let plan_id = impact_plan_id(&proposed, &named);
    if plan_id != inputs.require("plan-id")? {
        return Err(Failure::conflict(
            "impact_plan_mismatch",
            "these release and follower arguments do not produce the reviewed plan",
        )
        .remedy("run impact-plan with the same arguments, review it and pass its plan_id")
        .detail(json!({ "plan_id": plan_id })));
    }
    let out = PathBuf::from(inputs.require("out-dir")?);
    if out.exists() {
        return Err(Failure::conflict(
            "output_exists",
            format!("`{}` already exists", out.display()),
        )
        .remedy("choose a new output directory"));
    }
    std::fs::create_dir_all(&out).map_err(|error| {
        Failure::failed("output_unwritable", "could not create the output directory")
            .detail(json!({ "io": error.kind().to_string() }))
    })?;

    let mut rows = Vec::with_capacity(named.len());
    let mut stopped = false;
    let (mut applied, mut refused, mut unaffected, mut unprocessed) = (0, 0, 0, 0);
    for (index, (label, digest)) in named.iter().enumerate() {
        if stopped {
            unprocessed += 1;
            rows.push(json!({ "label": label, "outcome": "unprocessed" }));
            continue;
        }
        let bytes = match crate::read(label) {
            Ok(bytes) => bytes,
            Err(failure) => {
                refused += 1;
                rows.push(json!({
                    "label": label,
                    "outcome": "refused",
                    "blockers": [format!("{}: {}", failure.code(), failure.message())],
                }));
                continue;
            }
        };
        let (impact, revision) = apply_follower(&proposed, label, digest, &bytes);
        let mut row = json!({
            "label": label,
            "planned_status": impact.status,
            "current_pin": impact.current_pin,
            "proposed_pin": impact.proposed_pin,
            "blockers": impact.blockers,
        });
        match (impact.status, revision) {
            (FollowerStatus::Applicable, Some(revision)) => {
                let path = out.join(format!("follower-{:02}.dsgrid", index + 1));
                match write_new(&path, &revision) {
                    Ok(()) => {
                        applied += 1;
                        row["outcome"] = json!("applied");
                        row["written"] = json!(path.display().to_string());
                        row["resulting_digest"] =
                            json!(ds_grid_exchange::library::bundle_digest(&revision));
                        row["resulting_model_revision"] = json!(impact.resulting_model_revision);
                        row["engineering_input_root"] = json!(impact.engineering_input_root);
                        row["stored_geometry_recomputed"] =
                            json!(impact.stored_geometry_recomputed);
                    }
                    Err(failure) => {
                        // A host that cannot write stops here; later
                        // followers are reported unprocessed, never skipped.
                        refused += 1;
                        stopped = true;
                        row["outcome"] = json!("refused");
                        row["blockers"] =
                            json!([format!("{}: {}", failure.code(), failure.message())]);
                    }
                }
            }
            (FollowerStatus::AlreadyCurrent | FollowerStatus::NotFollowing, _) => {
                unaffected += 1;
                row["outcome"] = json!("unaffected");
            }
            _ => {
                refused += 1;
                row["outcome"] = json!("refused");
            }
        }
        rows.push(row);
    }
    let complete = refused == 0 && unprocessed == 0;
    let receipt = json!({
        "plan_id": plan_id,
        "complete": complete,
        "applied": applied,
        "refused": refused,
        "unaffected": unaffected,
        "unprocessed": unprocessed,
        "out_dir": out.display().to_string(),
        "followers": rows,
        "cloud_write": false,
        "solver_approval": false,
    });
    if complete {
        Ok(receipt)
    } else {
        Err(Failure::conflict(
            "impact_apply_incomplete",
            format!("{refused} follower(s) refused and {unprocessed} unprocessed; {applied} applied"),
        )
        .remedy("read detail.followers: applied revisions are written; re-read refused followers and plan again")
        .detail(receipt))
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    crate::write_new(path, bytes)
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}
