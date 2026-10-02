//! Exact-slug projection of the backend-owned complete form purge.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

const OWN_REFUSALS: [Refusal; 3] = [
    Refusal {
        code: "form_purge_invalid",
        when: "the slug, digest or confirmation violates the complete purge contract",
        remedy: "use one exact slug and its complete purge plan digest",
    },
    Refusal {
        code: "form_purge_scope_changed",
        when: "the complete scope changed before the purge began",
        remedy: "read and inspect the changed plan before applying its new digest",
    },
    Refusal {
        code: "form_purge_unavailable",
        when: "a complete inventory or purge dependency is unavailable",
        remedy: "restore the named dependency; resume a partial purge using its original digest",
    },
];
const REFUSALS: &[Refusal] = &{
    let mut rows = [OWN_REFUSALS[0]; crate::COMMON_REFUSALS.len() + OWN_REFUSALS.len()];
    let mut i = 0;
    while i < crate::COMMON_REFUSALS.len() {
        rows[i] = crate::COMMON_REFUSALS[i];
        i += 1;
    }
    let mut j = 0;
    while j < OWN_REFUSALS.len() {
        rows[i + j] = OWN_REFUSALS[j];
        j += 1;
    }
    rows
};
pub static PLAN_COMMAND: Command = Command {
    id: "survey.form.purge-plan",
    path: &["survey", "form", "purge-plan"],
    contract: 1,
    summary: "Inventory one retired legacy form before permanent deletion.",
    purpose: "Requires platform.admin and an exact slug containing literal _-_. Reads the complete backend-owned scope across active and archived projects, observations, descendants, media versions, templates and derived projections. Refuses incomplete inventories. Returns a digest that pins this destructive scope; no data is deleted.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "slug",
            "<form-slug>",
            "One exact retired legacy slug containing literal _-_.",
        )
        .required(),
        crate::LANE,
    ],
    output: "Complete scope counts, project/template identities, plan_digest, and any existing resumable purge state.",
    examples: &[Example {
        command: "ds survey form purge-plan --slug <form-slug> --output json",
        note: "Use ds survey forms purge-candidates for retired slugs; ds survey forms list for current slugs.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static CANDIDATES_COMMAND: Command = Command {
    id: "survey.forms.purge-candidates",
    path: &["survey", "forms", "purge-candidates"],
    contract: 1,
    summary: "Find retired form identities containing literal _-_.",
    purpose: "Requires platform.admin. Inventories current and retired schemas, project form trees including missing/contextual parents, references, BigQuery and canonical media. The fixed legacy marker is not a client-selected deletion pattern. Refuses incomplete scans; returned slugs require their own exact purge-plan before deletion.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[crate::LANE],
    output: "Sorted distinct bounded exact legacy slugs, matching total, pattern _-_, and complete true; no observations or schema content.",
    examples: &[Example {
        command: "ds survey forms purge-candidates --output json",
        note: "Find retired legacy identities omitted from the ordinary master catalogue.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static APPLY_COMMAND: Command = Command {
    id: "survey.form.purge",
    path: &["survey", "form", "purge"],
    contract: 1,
    summary: "Permanently purge one retired legacy form and its pinned data.",
    purpose: "Requires platform.admin, an exact slug containing literal _-_, and explicit confirmation. Fences collection, deletes exact canonical data and media versions, cleans derived projections, and verifies residual counts. Schema snapshots are removed. A partial receipt remains partial; retry that purge with the same digest instead of silently replacing its scope.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "slug",
            "<form-slug>",
            "Exact retired legacy slug from purge-plan.",
        )
        .required(),
        Arg::value(
            "plan-digest",
            "<sha256:digest>",
            "Complete purge scope digest; retain it to resume a partial purge.",
        )
        .required(),
        crate::LANE,
    ],
    output: "Verified receipt with original scope, matching digest, complete/partial state, remaining counts and projection errors. Complete means every remaining count is zero.",
    examples: &[Example {
        command: "ds survey form purge --slug <form-slug> --plan-digest <sha256:digest> --yes --output json",
        note: "Copy the exact slug from ds survey forms purge-candidates or ds survey forms list; apply its plan digest.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub fn plan(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    crate::invoke(
        inputs,
        PLAN_COMMAND.id,
        Map::from_iter([(
            "slug".into(),
            json!(crate::text(inputs.require("slug")?, "slug", 160)?),
        )]),
    )
}
pub fn candidates(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    crate::invoke(inputs, CANDIDATES_COMMAND.id, Map::new())
}
pub fn apply(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    crate::invoke(
        inputs,
        APPLY_COMMAND.id,
        Map::from_iter([
            (
                "slug".into(),
                json!(crate::text(inputs.require("slug")?, "slug", 160)?),
            ),
            (
                "planDigest".into(),
                json!(crate::text(
                    inputs.require("plan-digest")?,
                    "plan-digest",
                    71
                )?),
            ),
        ]),
    )
}
pub fn render(data: &Value) -> String {
    if let Some(slugs) = data["slugs"].as_array() {
        return format!(
            "{} legacy forms\n{}\n",
            slugs.len(),
            slugs
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    if let Some(receipt) = data.get("receipt") {
        format!(
            "{} purge {}\n{}\n",
            receipt["slug"].as_str().unwrap_or("form"),
            receipt["state"].as_str().unwrap_or("unknown"),
            receipt["remaining"]
        )
    } else {
        format!(
            "{}\n{}\n{}\n",
            data["plan"]["slug"].as_str().unwrap_or("form"),
            data["plan"]["plan_digest"].as_str().unwrap_or(""),
            data["plan"]["scope"]
        )
    }
}
