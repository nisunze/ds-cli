//! Headless tag vocabulary and digest-pinned report projection.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::Value;
const LANE: Arg = Arg::value("lane", "<stable|canary>", "Native authentication lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
pub static DEFINITIONS: Command = Command {
    id: "design.tag.project-list",
    path: &["design", "tag", "project-list"],
    contract: 1,
    summary: "Read project tag definitions for city and phase selection.",
    purpose: "Discover exact definition IDs, semantic roles and allowed values for the project named by --project before obtaining a report projection. No Desktop or map is needed. Does not infer assignments from transformer names.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[crate::PROJECT_ARG, LANE],
    output: "Selected project and active tag definitions; no mutation.",
    examples: &[],
    refusals: ds_cli_auth::PROJECT_STATUS_COMMAND.refusals,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PROJECTION: Command = Command {
    id: "design.group.project-export",
    path: &["design", "group", "project-export"],
    contract: 1,
    summary: "Read city and phase tag assignments for maps and Solar.",
    purpose: "Export one governed digest-pinned tag projection for explicit transformers and ordered definition IDs. Use city and phase definitions from design.tag.project-list for map hatching and network seeding. No Desktop is required. Preserve document bytes verbatim with their SHA-256; empty definition selection means one untagged group.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        LANE,
        Arg::repeated(
            "transformer",
            "<name>",
            "Exact saved transformer; repeat for the scope (max 2000).",
        )
        .required(),
        Arg::repeated(
            "definition",
            "<id>",
            "Exact ordered definition ID; repeat (max 64).",
        ),
    ],
    output: "Project, definition IDs, exact document text, verified SHA-256/bytes and coverage counts/exclusions.",
    examples: &[],
    refusals: PROJECTION_REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub fn definitions(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    Ok(ds_cli_auth::design_tags(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::DesignTagsCommand::Definitions,
    )?
    .into_result())
}
pub fn projection(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    Ok(ds_cli_auth::design_tags(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::DesignTagsCommand::Projection {
            transformers: i
                .repeated("transformer")
                .iter()
                .map(|s| s.to_string())
                .collect(),
            definitions: i
                .repeated("definition")
                .iter()
                .map(|s| s.to_string())
                .collect(),
        },
    )?
    .into_result())
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

const LOCAL: Refusal = Refusal {
    code: "design_tag_entries_invalid",
    when: "the entry file is missing, oversized or not an exact transformer/value array",
    remedy: "provide 1..200 distinct assignments within 1 MiB",
};
const STATUS_REFUSALS: &[Refusal] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals;
const UNKNOWN: Refusal = ds_cli_auth::TAG_DEFINITION_UNKNOWN_REFUSAL;
/// A command's own refusals followed by the shared project-status ones.
const fn with_status<const N: usize>(own: &[Refusal]) -> [Refusal; N] {
    let mut r = [LOCAL; N];
    let mut n = 0;
    while n < own.len() {
        r[n] = own[n];
        n += 1;
    }
    let mut s = 0;
    while s < STATUS_REFUSALS.len() {
        r[n + s] = STATUS_REFUSALS[s];
        s += 1;
    }
    assert!(n + s == N, "refusal list must fill the array exactly");
    r
}
const PROJECTION_REFUSALS: &[Refusal] = &with_status::<{ 1 + STATUS_REFUSALS.len() }>(&[UNKNOWN]);
const BATCH_REFUSALS: &[Refusal] = &with_status::<{ 2 + STATUS_REFUSALS.len() }>(&[LOCAL, UNKNOWN]);
const BATCH_ARGS: &[Arg] = &[
    crate::PROJECT_ARG,
    LANE,
    Arg::value("definition", "<id>", "Exact writable tag definition.").required(),
    Arg::value(
        "entries",
        "<json-path>",
        "Array of {transformer,value} assignments, 1..200 distinct transformers.",
    )
    .required(),
];
pub static PREVIEW: Command = Command {
    id: "design.group.project-preview",
    path: &["design", "group", "project-preview"],
    contract: 1,
    summary: "Preview city and phase tag assignments with a fencing digest.",
    purpose: "Read the exact project tag vocabulary first. Supply source-grounded per-transformer assignments; the server validates vocabulary, current assignment versions and permissions. Inspect every outcome before project-apply. No shared write.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: BATCH_ARGS,
    output: "Project, definition, every outcome, counts, state and exact plan_digest.",
    examples: &[],
    refusals: BATCH_REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static APPLY: Command = Command {
    id: "design.group.project-apply",
    path: &["design", "group", "project-apply"],
    contract: 1,
    summary: "Apply a previewed headless tag batch under its exact digest.",
    purpose: "Send the same definition and assignment file reviewed through project-preview, plus that plan_digest. Requires --yes. The server rechecks assignments and refuses a moved plan. Partial outcomes stay explicit; do not claim all rows assigned from HTTP success alone.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        LANE,
        Arg::value("definition", "<id>", "Exact previewed definition.").required(),
        Arg::value(
            "entries",
            "<json-path>",
            "The same assignment array previewed.",
        )
        .required(),
        Arg::value(
            "digest",
            "<sha256>",
            "Exact plan_digest from project-preview.",
        )
        .required(),
    ],
    output: "Project, definition, per-transformer outcomes, counts and applied or partial state.",
    examples: &[],
    refusals: BATCH_REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn batch(i: &Inputs, apply: bool) -> Result<Value, Failure> {
    let p = i.require("entries")?;
    let meta = std::fs::metadata(p).map_err(|_| {
        Failure::invalid(
            "design_tag_entries_invalid",
            "entries must be one readable file",
        )
    })?;
    if !meta.is_file() || meta.len() > 1024 * 1024 {
        return Err(Failure::invalid(
            "design_tag_entries_invalid",
            "entries must be within 1 MiB",
        ));
    }
    let bytes = std::fs::read(p)
        .map_err(|_| Failure::invalid("design_tag_entries_invalid", "entries unreadable"))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Failure::invalid("design_tag_entries_invalid", "entries must be JSON"))?;
    let values = value.as_array().ok_or_else(|| {
        Failure::invalid("design_tag_entries_invalid", "entries must be an array")
    })?;
    let mut entries = Vec::new();
    for v in values {
        let obj = v.as_object().filter(|o| o.len() == 2).ok_or_else(|| {
            Failure::invalid(
                "design_tag_entries_invalid",
                "each entry requires exactly transformer and value",
            )
        })?;
        entries.push((
            obj.get("transformer")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Failure::invalid("design_tag_entries_invalid", "transformer string required")
                })?
                .to_owned(),
            obj.get("value")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Failure::invalid("design_tag_entries_invalid", "value string required")
                })?
                .to_owned(),
        ));
    }
    Ok(ds_cli_auth::design_tags(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::DesignTagsCommand::Batch {
            group: i.require("definition")?.into(),
            entries,
            digest: if apply {
                Some(i.require("digest")?.into())
            } else {
                None
            },
        },
    )?
    .into_result())
}
pub fn preview(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    batch(i, false)
}
pub fn apply(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    batch(i, true)
}
