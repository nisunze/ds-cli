//! Revision-pinned Solar portfolio catalog mutations shared with browser WASM.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::paired;

const TIMEOUT: Duration = Duration::from_secs(30);
const CREATE_OPERATION: &str = "solar.portfolio.create";
const UPDATE_OPERATION: &str = "solar.portfolio.update";
const DELETE_OPERATION: &str = "solar.portfolio.delete";

const DESCRIPTOR_ARG: Arg = Arg::value(
    "desktop-descriptor",
    "<path>",
    "Use this bridge descriptor instead of discovering one.",
);

static REFUSALS: &[Refusal] = &[
    Refusal {
        code: "invalid_portfolio_mutation",
        when: "a name, id, revision, description, or ordered city membership is outside the shared Rust contract",
        remedy: "use canonical ids, an exact listed membership revision, a bounded name/description, and 2..200 unique --city values",
    },
    Refusal {
        code: "confirmation_required",
        when: "the global portfolio mutation was not explicitly confirmed",
        remedy: "review the exact portfolio inputs, then repeat with --yes",
    },
    Refusal {
        code: "desktop_not_paired",
        when: "no DS GridDesign session is running on this machine",
        remedy: "start DS GridDesign, sign in, and retry",
    },
    Refusal {
        code: "desktop_ambiguous",
        when: "more than one DS GridDesign session is running",
        remedy: "name one with --desktop-descriptor <path>",
    },
    Refusal {
        code: "desktop_unreachable",
        when: "the bridge descriptor names a session that does not answer",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_unreadable",
        when: "the paired session's reply could not be read",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_operation_unsupported",
        when: "this DS GridDesign build does not offer the named Solar operation",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "desktop_refused",
        when: "the active project rejected the mutation, including a stale membership revision",
        remedy: "list portfolios again, reconcile the current membership, and retry",
    },
    Refusal {
        code: "desktop_contract_mismatch",
        when: "the paired session returned a mutation receipt outside this command contract",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "pairing_rejected",
        when: "the descriptor's pairing secret is stale",
        remedy: "restart DS GridDesign to publish a fresh descriptor",
    },
];

pub static CREATE_COMMAND: Command = Command {
    id: CREATE_OPERATION,
    path: &["solar", "portfolio", "create"],
    contract: 1,
    summary: "Create one governed Solar portfolio.",
    purpose: "Creates one named portfolio from an explicit ordered membership through the same Rust mutation contract and project authority used by the Design page. It does not calculate the portfolio.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("name", "<name>", "Human-readable immutable portfolio name.").required(),
        Arg::value(
            "description",
            "<text>",
            "Optional description, up to 2,000 characters.",
        ),
        Arg::repeated("city", "<id>", "Member city in order. Repeat 2..200 times.").required(),
        DESCRIPTOR_ARG,
    ],
    output: "A confirmed mutation receipt naming the server-created portfolio id.",
    examples: &[Example {
        command: "ds solar portfolio create --name 'Northern portfolio' --city city_a --city city_b --yes --output json",
        note: "Creates membership only; run it separately with `solar run start --portfolio`.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static UPDATE_COMMAND: Command = Command {
    id: UPDATE_OPERATION,
    path: &["solar", "portfolio", "update"],
    contract: 1,
    summary: "Edit one revision-pinned Solar portfolio.",
    purpose: "Changes a portfolio display name, description, or complete ordered membership only when the exact membership revision last read by the caller is still current. Repeated --city values replace membership atomically.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("portfolio", "<id>", "Exact portfolio id from list.").required(),
        Arg::value(
            "membership-revision",
            "<sha256:digest>",
            "Exact current membership revision from list.",
        )
        .required(),
        Arg::value("display-name", "<name>", "New display name."),
        Arg::value(
            "description",
            "<text>",
            "New description; pass an empty value to clear it.",
        ),
        Arg::repeated(
            "city",
            "<id>",
            "Replacement member city in order. Repeat 2..200 times.",
        ),
        DESCRIPTOR_ARG,
    ],
    output: "The confirmed updated portfolio and its new membership revision.",
    examples: &[Example {
        command: "ds solar portfolio update --portfolio pf_1 --membership-revision sha256:<digest> --city city_a --city city_c --yes --output json",
        note: "Replaces membership only if the listed revision is still current.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static DELETE_COMMAND: Command = Command {
    id: DELETE_OPERATION,
    path: &["solar", "portfolio", "delete"],
    contract: 1,
    summary: "Delete one revision-pinned Solar portfolio.",
    purpose: "Deletes exactly one portfolio only when its listed membership revision is still current. Existing immutable calculation artifacts remain separately addressed by their run receipts.",
    chapter: Chapter::Solar,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("portfolio", "<id>", "Exact portfolio id from list.").required(),
        Arg::value(
            "membership-revision",
            "<sha256:digest>",
            "Exact current membership revision from list.",
        )
        .required(),
        DESCRIPTOR_ARG,
    ],
    output: "A confirmed deletion receipt naming the removed portfolio id.",
    examples: &[Example {
        command: "ds solar portfolio delete --portfolio pf_1 --membership-revision sha256:<digest> --yes --output json",
        note: "A stale revision is refused instead of deleting changed membership.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub fn create(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut intent = Map::new();
    intent.insert("operation".into(), json!("create"));
    intent.insert("name".into(), json!(inputs.require("name")?));
    if let Some(description) = inputs.value("description") {
        intent.insert("description".into(), json!(description));
    }
    intent.insert("cities".into(), json!(inputs.repeated("city")));
    invoke(inputs, CREATE_OPERATION, intent, None)
}

pub fn update(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let portfolio_id = inputs.require("portfolio")?;
    let mut intent = Map::new();
    intent.insert("operation".into(), json!("update"));
    intent.insert("portfolio_id".into(), json!(portfolio_id));
    intent.insert(
        "expected_membership_revision".into(),
        json!(inputs.require("membership-revision")?),
    );
    if let Some(display_name) = inputs.value("display-name") {
        intent.insert("display_name".into(), json!(display_name));
    }
    if let Some(description) = inputs.value("description") {
        intent.insert("description".into(), json!(description));
    }
    if !inputs.repeated("city").is_empty() {
        intent.insert("cities".into(), json!(inputs.repeated("city")));
    }
    invoke(inputs, UPDATE_OPERATION, intent, Some(portfolio_id))
}

pub fn delete(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let portfolio_id = inputs.require("portfolio")?;
    let intent = json!({
        "operation": "delete",
        "portfolio_id": portfolio_id,
        "expected_membership_revision": inputs.require("membership-revision")?,
    });
    invoke(
        inputs,
        DELETE_OPERATION,
        intent
            .as_object()
            .cloned()
            .expect("delete intent is object"),
        Some(portfolio_id),
    )
}

fn invoke(
    inputs: &Inputs,
    operation: &'static str,
    intent: Map<String, Value>,
    expected_id: Option<&str>,
) -> Result<Value, Failure> {
    let intent_value = Value::Object(intent.clone());
    ds_command_kernel::plan_solar_portfolio_mutation(
        &serde_json::to_vec(&intent_value).expect("portfolio intent serializes"),
    )
    .map_err(|error| {
        Failure::invalid("invalid_portfolio_mutation", error.to_string()).remedy(
            "correct the portfolio fields and use the exact membership revision returned by list",
        )
    })?;
    let result = paired::invoke(inputs, operation, intent_value, TIMEOUT)?;
    if result.get("operation").and_then(Value::as_str)
        != Some(operation.rsplit('.').next().expect("operation has leaf"))
        || expected_id
            .is_some_and(|id| result.get("portfolio_id").and_then(Value::as_str) != Some(id))
        || result
            .get("portfolio_id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(Failure::unavailable(
            "desktop_contract_mismatch",
            "the paired session returned an invalid portfolio mutation receipt",
        )
        .remedy("update DS GridDesign and ds to matching releases"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mutation_is_global_revision_governed_work() {
        for command in [&CREATE_COMMAND, &UPDATE_COMMAND, &DELETE_COMMAND] {
            assert_eq!(command.effect, Effect::GlobalWrite);
            assert!(
                command
                    .refusals
                    .iter()
                    .any(|refusal| refusal.code == "confirmation_required")
            );
            assert!(
                command
                    .refusals
                    .iter()
                    .any(|refusal| refusal.code == "invalid_portfolio_mutation")
            );
        }
    }
}
