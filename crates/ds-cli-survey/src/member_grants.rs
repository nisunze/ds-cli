//! Typed adapter for the existing server-owned member form grant transaction.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::member_form_grants::{MAX_BYTES, Plan, Request};
use serde_json::{Value, json};
use std::io::Read;

const MEMBER: Arg = Arg::value(
    "member",
    "<email>",
    "Exact existing active member email; no accounts or memberships are created.",
)
.required();
const OWN_REFUSALS: [Refusal; 7] = [
    Refusal {
        code: "member_grant_stale",
        when: "membership, actor, forms or captured revision changed",
        remedy: "read and plan again; never silently rebase",
    },
    Refusal {
        code: "member_grant_not_permitted",
        when: "current actor capability or hierarchy does not authorize this target",
        remedy: "ask a higher project authority to manage this member",
    },
    Refusal {
        code: "member_grant_member_required",
        when: "the target is inactive, revoked or lacks agreeing membership edges",
        remedy: "use an existing active project member",
    },
    Refusal {
        code: "member_grant_bound_exceeded",
        when: "the project contains more than 100 form bindings",
        remedy: "use the existing governed member editor for this larger project",
    },
    Refusal {
        code: "member_grant_invalid",
        when: "a requested form does not participate or the captured plan is invalid",
        remedy: "use exact participating form slugs and a fresh complete plan",
    },
    Refusal {
        code: "member_grant_unavailable",
        when: "the project-access authority is temporarily unavailable",
        remedy: "retry reads; verify effective state before retrying an uncertain apply",
    },
    Refusal {
        code: "confirmation_required",
        when: "apply was not explicitly confirmed",
        remedy: "review the captured plan, then pass --yes",
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
pub static READ_COMMAND: Command = Command {
    id: "survey.member-grant.read",
    path: &["survey", "member-grant", "read"],
    contract: 1,
    summary: "Read one member's explicit and effective survey form grant.",
    purpose: "Read one existing active member's paired grant under your current project hierarchy. The named project is captured explicitly. Returns unchanged project roles, finite participating forms, current scope and effective form read/edit exclusions. No account creation, role promotion, saved project selection or wildcard grant.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[crate::PROJECT, MEMBER, crate::LANE],
    output: "One bounded grant snapshot with members base_version, roles, before/after explicit forms, participating bound forms, effective per-form read/edit scope and snapshot hash; at most 100 bindings.",
    examples: &[Example {
        command: "ds survey member-grant read --project <id> --member <email> --output json",
        note: "Effective readback uses the same server authority as apply.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &["permission", "member", "grant"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PLAN_COMMAND: Command = Command {
    id: "survey.member-grant.plan",
    path: &["survey", "member-grant", "plan"],
    contract: 1,
    summary: "Dry-run a finite member form grant against current authority.",
    purpose: "Capture one non-writing plan before confirmation. Choose an exact comma-separated form list or --all, which expands only currently participating project forms under the 100-binding bound. The plan pins actor, target, unchanged roles, members version, form identities and current effective grant. Redirect JSON output to a file for apply. It never creates a member, promotes roles or authorizes future forms.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT,
        MEMBER,
        Arg::value(
            "forms",
            "<slug,...>",
            "Nonempty exact participating forms replacing this member's explicit grant; mutually exclusive with --all.",
        ),
        Arg::switch(
            "all",
            "Expand currently participating forms into a finite captured grant; never a wildcard.",
        ),
        crate::LANE,
    ],
    output: "Non-writing, actor-bound plan with exact before/after grant, effective scope, base_version and form snapshot hash. At most 100 bindings; larger projects refuse without truncation.",
    examples: &[Example {
        command: "ds survey member-grant plan --project <id> --member <email> --forms <slug> --output json",
        note: "Review the bounded dry run and save its JSON output before apply.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &["permission", "member", "grant", "dry run"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static APPLY_COMMAND: Command = Command {
    id: "survey.member-grant.apply",
    path: &["survey", "member-grant", "apply"],
    contract: 1,
    summary: "Apply one confirmed, revision-pinned member form grant.",
    purpose: "Submit the exact saved plan after --yes. The server rechecks the actor hierarchy, existing target membership pair, participating forms and optimistic revision inside the write transaction. Only forms and membership revision change; roles remain unchanged. Stale plans refuse without rebase. An uncertain response requires effective readback before another apply; no separate ledger is added.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT,
        MEMBER,
        Arg::value(
            "plan",
            "<json-path>",
            "Captured plan or exact JSON output from member-grant plan, at most 128 KiB.",
        )
        .required(),
        crate::LANE,
    ],
    output: "The exact accepted plan, applied/no-op verdict, committed members version and effective grant. Follow with member-grant read for fresh effective readback.",
    examples: &[Example {
        command: "ds survey member-grant apply --project <id> --member <email> --plan ./grant-plan.json --yes --output json",
        note: "Apply exactly the reviewed dry run; no target/project grant is performed by discovery.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &["permission", "member", "grant"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn document(message: &str) -> Failure {
    Failure::invalid("invalid_document", message)
        .remedy("save the exact member-grant plan JSON output and provide that bounded file")
}
fn load_plan(path: &str) -> Result<Plan, Failure> {
    let file =
        std::fs::File::open(path).map_err(|_| document("captured plan file is unavailable"))?;
    let info = file
        .metadata()
        .map_err(|_| document("captured plan file metadata is unavailable"))?;
    if !info.is_file() || info.len() > MAX_BYTES as u64 {
        return Err(document(
            "captured plan must be a regular file at most 128 KiB",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| document("captured plan file could not be read"))?;
    if bytes.len() > MAX_BYTES {
        return Err(document("captured plan file exceeds 128 KiB"));
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| document("captured plan is not JSON"))?;
    let plan = value
        .get("data")
        .and_then(|v| v.get("plan"))
        .or_else(|| value.get("plan"))
        .unwrap_or(&value);
    parse_plan(plan.clone())
}
fn parse_plan(value: Value) -> Result<Plan, Failure> {
    let plan: Plan = serde_json::from_value(value)
        .map_err(|_| document("captured plan violates its closed schema"))?;
    plan.validate()
        .map_err(|_| document("captured plan violates its identity or bounds"))?;
    if plan.operation != "set" {
        return Err(document("read-only snapshot is not an apply plan"));
    }
    Ok(plan)
}
fn invoke(inputs: &Inputs, request: &Request) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    request.validate(project).map_err(|_| {
        Failure::invalid(
            "member_grant_invalid",
            "use exact member/project identities and one nonempty finite form list",
        )
        .remedy("read the command contract; --forms and --all are mutually exclusive")
    })?;
    let result =
        ds_cli_auth::member_form_grant_for_project(inputs.require("lane")?, project, request)?;
    Ok(json!(result.result()))
}
pub fn read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(
        inputs,
        &Request::Read {
            member: inputs.require("member")?.into(),
        },
    )
}
pub fn plan(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let forms = inputs
        .value("forms")
        .map(|s| s.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    invoke(
        inputs,
        &Request::Plan {
            member: inputs.require("member")?.into(),
            forms,
            all: inputs.switch("all"),
        },
    )
}
pub fn apply(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if !context.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "member form grant apply requires --yes",
        )
        .remedy("review the saved plan, then explicitly confirm the exact effect"));
    }
    let captured = load_plan(inputs.require("plan")?)?;
    invoke(
        inputs,
        &Request::Apply {
            member: inputs.require("member")?.into(),
            plan: captured,
        },
    )
}
pub fn render(data: &Value) -> String {
    let plan = &data["plan"];
    let forms = |key: &str| {
        plan[key]
            .as_array()
            .map(|v| {
                v.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    let mut output = format!(
        "{} · {}\nVersion {} → {} · applied {}\nRoles preserved: {}\nBefore: {}\nAfter: {}\n",
        plan["project_id"].as_str().unwrap_or(""),
        plan["target_user_email"].as_str().unwrap_or(""),
        plan["base_version"],
        data["current_version"],
        data["applied"],
        forms("roles"),
        forms("before_forms"),
        forms("after_forms")
    );
    for row in data["plan"]["effective_after"]
        .as_array()
        .into_iter()
        .flatten()
    {
        output.push_str(&format!(
            "{}: read {} · edit {}\n",
            row["form"].as_str().unwrap_or(""),
            if row["read_allowed"] != true {
                "no"
            } else if row["only_own"] == true {
                "own entries"
            } else {
                "all entries"
            },
            if row["edit_allowed"] == true {
                "form allowed"
            } else {
                "no"
            }
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn member_grant_contract_keeps_dry_run_and_confirmation_separate() {
        assert!(!PLAN_COMMAND.effect.needs_confirmation());
        assert!(APPLY_COMMAND.effect.needs_confirmation());
        for command in [&READ_COMMAND, &PLAN_COMMAND, &APPLY_COMMAND] {
            assert_eq!(command.authority, Authority::HeadlessProject);
            assert!(command.arg("project").unwrap().required);
            assert!(command.arg("member").unwrap().required);
            assert!(command.arg("role").is_none());
        }
        assert!(PLAN_COMMAND.arg("all").is_some());
        assert!(APPLY_COMMAND.arg("forms").is_none());
    }
    #[test]
    fn member_grant_rejects_retired_or_unknown_plan_shapes() {
        assert!(parse_plan(json!({"project_id":"p","target_user_email":"member@example.com","role":"project_admin"})).is_err());
        assert!(parse_plan(json!({"schema":1,"operation":"read"})).is_err());
    }
}
