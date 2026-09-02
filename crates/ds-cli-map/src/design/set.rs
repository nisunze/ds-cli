//! `ds map design set` — stage a property change on selected design features.
//!
//! This is the command the family exists for. Marking an as-built network
//! approved — `drafting_status=approved` — is what stops the kernel from
//! redesigning an installed transformer, and nothing in the application could
//! write it in bulk, so as-built rows stayed unmarked and every process run
//! was free to resize them.
//!
//! It stages. The change lands in the operator's local room and marks it
//! dirty; the project is untouched until `ds map design save`. `staged` and
//! `persisted` are both reported on every call so the two can never be
//! confused, and `--dry-run` answers "how many would change?" without writing
//! even locally.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::{BBOX_ARG, ID_ARG, LAYER_ARG, TRANSFORMER_ARG, WHERE_ARG};

const SET_ARG: Arg = Arg {
    name: "set",
    kind: ArgKind::Repeated,
    value: "<key=value>",
    required: true,
    default: None,
    choices: &[],
    summary: "Property to write. Repeat for more than one.",
};

const PROTECTED_PROPERTY: Refusal = Refusal {
    code: "protected_property",
    when: "a --set assignment targets an immutable design identity property",
    remedy: "write an ordinary mutable property; identity rewrite requires a separate confirmation-gated command",
};

pub static COMMAND: Command = Command {
    id: "map.design.set",
    path: &["map", "design", "set"],
    contract: 2,
    summary: "Stage a property change on selected design features.",
    purpose: "\
Writes properties onto every design feature a selector matches — the bulk edit \
the application has no other way to make, and the one that marks an as-built \
network approved so the kernel stops redesigning it. It stages into the \
operator's local room and marks it dirty; nothing reaches the project until \
`ds map design save`. Immutable identity properties refuse before dry-run or \
staging; use --dry-run to count ordinary property changes first.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        SET_ARG,
        LAYER_ARG,
        WHERE_ARG,
        BBOX_ARG,
        ID_ARG,
        Arg::switch("dry-run", "Report what would change; stage nothing."),
        DESCRIPTOR_ARG,
    ],
    output: "\
How many features matched, how many changed and how many already carried the \
value, the change count per layer, and `staged` and `persisted` separately — \
`persisted` is false here always.",
    examples: &[
        Example {
            command: "ds map design set --transformer T-1042 --layer lv_lines --where drafting_status= --set drafting_status=approved --dry-run",
            note: "Count the unmarked as-built rows before touching them.",
            runnable: false,
        },
        Example {
            command: "ds map design set --transformer T-1042 --layer lv_lines --where drafting_status= --set drafting_status=approved --output json",
            note: "Stage it. Read .data.staged; the project is still untouched.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_PAIR,
        crate::INVALID_BBOX,
        super::TOO_MANY_IDS,
        PROTECTED_PROPERTY,
        Refusal {
            code: "no_properties",
            when: "every --set was parsed away, leaving nothing to write",
            remedy: "pass at least one --set name=value",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let dry_run = inputs.switch("dry-run");

    let properties = crate::pairs(inputs.repeated("set"), "set")?;
    if properties.is_empty() {
        return Err(
            Failure::invalid("no_properties", "no property was given to write")
                .remedy("pass at least one --set name=value"),
        );
    }

    let selector = super::selector(inputs, "")?;
    let mut arguments = Map::new();
    arguments.insert("transformer".into(), json!(transformer));
    arguments.insert("properties".into(), Value::Object(properties.clone()));
    arguments.insert("dryRun".into(), json!(dry_run));

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_SET,
        super::with_selector(arguments, selector.clone()),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(classify_set_failure)?;

    Ok(json!({
        "transformer": transformer,
        "project": result["project"],
        "selector": super::describe(&selector),
        "properties": Value::Object(properties),
        "dry_run": result["dryRun"].as_bool().unwrap_or(dry_run),
        "matched": result["matched"].as_u64().unwrap_or(0),
        "changed": result["changed"].as_u64().unwrap_or(0),
        "unchanged": result["unchanged"].as_u64().unwrap_or(0),
        "changed_by_layer": result["changedByLayer"],
        "staged": result["staged"].as_bool().unwrap_or(false),
        "persisted": result["persisted"].as_bool().unwrap_or(false),
    }))
}

fn classify_set_failure(failure: Failure) -> Failure {
    let failure = crate::classify_design_failure(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|value| value["detail"].as_str())
        .unwrap_or_default();
    let Some(rest) = detail.strip_prefix("protected_property:") else {
        return failure;
    };
    let Some((property, _message)) = rest.split_once(':') else {
        return failure;
    };
    if property.is_empty() {
        return failure;
    }
    Failure::invalid(
        "protected_property",
        format!("`{property}` is an immutable design identity property"),
    )
    .remedy(PROTECTED_PROPERTY.remedy)
    .detail(json!({ "property": property }))
}

pub fn render(data: &Value) -> String {
    let changed = data["changed"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{} matched  ·  {} would change\n  selector  {}\n",
        data["matched"],
        changed,
        data["selector"].as_str().unwrap_or(""),
    );
    if let Some(unchanged) = data["unchanged"].as_u64().filter(|count| *count > 0) {
        out.push_str(&format!("  {unchanged} already carry the value\n"));
    }
    if let Some(layers) = data["changed_by_layer"].as_object() {
        for (layer, count) in layers {
            out.push_str(&format!("  {layer:<28} {count:>7}\n"));
        }
    }
    if data["dry_run"].as_bool().unwrap_or(false) {
        out.push_str("\ndry run; nothing was staged\n");
        return out;
    }
    out.push('\n');
    out.push_str(super::staging_note(data));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_property_bridge_refusal_keeps_the_exact_key() {
        let failure = Failure::failed("desktop_refused", "the paired session refused the write")
            .detail(json!({
                "detail": "protected_property:source_feature_id: design identity properties are immutable"
            }));

        let classified = classify_set_failure(failure);
        assert_eq!(classified.code(), "protected_property");
        assert_eq!(
            classified.detail_value(),
            Some(&json!({ "property": "source_feature_id" }))
        );
        assert_eq!(classified.remedy_text(), Some(PROTECTED_PROPERTY.remedy));
    }

    #[test]
    fn unrelated_bridge_refusal_stays_generic() {
        let failure = Failure::failed("desktop_refused", "the paired session refused the write")
            .detail(json!({ "detail": "transformer not found" }));

        assert_eq!(classify_set_failure(failure).code(), "desktop_refused");
    }
}
