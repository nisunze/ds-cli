//! `ds design tag query` — bounded project-wide typed tag predicates.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, MAX_TAG_QUERY_FILTERS, MAX_TAG_QUERY_ROWS};

const KIND_ARG: Arg = Arg {
    name: "kind",
    kind: ArgKind::Value,
    value: "<object-kind>",
    required: false,
    default: Some("lv_transformer"),
    choices: &["lv_transformer"],
    summary: "Project-wide queries currently scan current LV transformers.",
};

const MATCH_ARG: Arg = Arg {
    name: "match",
    kind: ArgKind::Value,
    value: "<mode>",
    required: false,
    default: Some("all"),
    choices: &["all", "any"],
    summary: "Require all predicates, or any one of them.",
};

const PRESENCE_ARG: Arg = Arg {
    name: "presence",
    kind: ArgKind::Repeated,
    value: "<definition>:<exists|missing>",
    required: false,
    default: None,
    choices: &[],
    summary: "Repeat for a definition-presence predicate; it carries no value.",
};

const CHOICE_ARG: Arg = Arg {
    name: "choice",
    kind: ArgKind::Repeated,
    value: "<definition>:<operator>:<value[,value]>",
    required: false,
    default: None,
    choices: &[],
    summary: "Repeat a choice predicate: equals, not_equals, any_of or all_of.",
};

const TEXT_ARG: Arg = Arg {
    name: "text",
    kind: ArgKind::Repeated,
    value: "<definition>:<operator>:<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Repeat a text predicate: equals, not_equals, contains or prefix.",
};

const INTEGER_ARG: Arg = Arg {
    name: "integer",
    kind: ArgKind::Repeated,
    value: "<definition>:<operator>:<integer>",
    required: false,
    default: None,
    choices: &[],
    summary: "Repeat an integer predicate: equals, not_equals, gt, gte, lt or lte.",
};

const NUMBER_ARG: Arg = Arg {
    name: "number",
    kind: ArgKind::Repeated,
    value: "<definition>:<operator>:<number>",
    required: false,
    default: None,
    choices: &[],
    summary: "Repeat a number predicate: equals, not_equals, gt, gte, lt or lte.",
};

const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("200"),
    choices: &[],
    summary: "Maximum complete match set (1-2000); exceeding it refuses, never truncates.",
};

pub static COMMAND: Command = Command {
    id: "design.tag.query",
    path: &["design", "tag", "query"],
    contract: 1,
    summary: "Find transformers with bounded typed tag predicates.",
    purpose: "\
Evaluates 1-20 typed predicates against the project's current Transformer \
Status projection. Each filter names its type explicitly, so a numeric \
comparison cannot turn into lexical string ordering and free text is never \
mistaken for a choice vocabulary. The server scans transformers once and \
assignments once per referenced definition. --limit is an admission bound: \
if more rows match, the call refuses rather than returning a partial selection.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        KIND_ARG,
        MATCH_ARG,
        PRESENCE_ARG,
        CHOICE_ARG,
        TEXT_ARG,
        INTEGER_ARG,
        NUMBER_ARG,
        LIMIT_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The complete matched object rows plus scanned object, assignment and read counts; never a silently truncated selection.",
    examples: &[
        Example {
            command: "ds design tag query --choice city:any_of:huye,kigali --output json",
            note: "Hypothetical city grouping: returns transformers assigned either governed city value.",
            runnable: false,
        },
        Example {
            command: "ds design tag query --choice phasing:equals:phase-1 --number completion:gte:80 --output json",
            note: "Hypothetical delivery cohort: phase 1 transformers whose numeric completion is at least 80.",
            runnable: false,
        },
        Example {
            command: "ds design tag query --text survey_note:contains:access --presence inspection_date:exists --output json",
            note: "Text and presence predicates remain typed and may be combined without opening the map.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_TAG_INPUT,
        crate::INVALID_VALUE_LIST,
        crate::TOO_MANY,
        crate::TOO_MANY_TAG_FILTERS,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut filters = Vec::new();
    for raw in inputs.repeated("presence") {
        filters.push(presence_filter(raw)?);
    }
    for raw in inputs.repeated("choice") {
        filters.push(choice_filter(raw)?);
    }
    for raw in inputs.repeated("text") {
        filters.push(typed_filter(raw, "text")?);
    }
    for raw in inputs.repeated("integer") {
        filters.push(typed_filter(raw, "integer")?);
    }
    for raw in inputs.repeated("number") {
        filters.push(typed_filter(raw, "number")?);
    }
    if filters.is_empty() {
        return Err(Failure::invalid(
            "invalid_tag_input",
            "a tag query requires at least one predicate",
        )
        .remedy("repeat one of --presence, --choice, --text, --integer or --number"));
    }
    if filters.len() > MAX_TAG_QUERY_FILTERS {
        return Err(Failure::invalid(
            "too_many_tag_filters",
            format!(
                "a tag query accepts at most {MAX_TAG_QUERY_FILTERS} predicates; {} were given",
                filters.len()
            ),
        )
        .remedy("narrow or split the query"));
    }
    let mut arguments = Map::new();
    arguments.insert("kind".into(), json!(inputs.require("kind")?));
    arguments.insert("match".into(), json!(inputs.require("match")?));
    arguments.insert("filters".into(), Value::Array(filters));
    arguments.insert(
        "limit".into(),
        json!(crate::integer(
            inputs.require("limit")?,
            "limit",
            1,
            MAX_TAG_QUERY_ROWS,
        )?),
    );
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TAG_QUERY,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

fn presence_filter(raw: &str) -> Result<Value, Failure> {
    let (definition, operator) = raw
        .split_once(':')
        .ok_or_else(|| invalid_filter("--presence", raw, "<definition>:<exists|missing>"))?;
    let definition = nonempty(definition, "--presence", raw)?;
    if operator != "exists" && operator != "missing" {
        return Err(invalid_filter(
            "--presence",
            raw,
            "<definition>:<exists|missing>",
        ));
    }
    Ok(json!({"definition_id": definition, "operator": operator}))
}

fn choice_filter(raw: &str) -> Result<Value, Failure> {
    let (definition, operator, value) = parts(raw, "--choice")?;
    if !["equals", "not_equals", "any_of", "all_of"].contains(&operator) {
        return Err(invalid_filter(
            "--choice",
            raw,
            "<definition>:<equals|not_equals|any_of|all_of>:<value[,value]>",
        ));
    }
    let values = crate::list_values(value, "choice", crate::MAX_TAG_VALUES)?;
    Ok(json!({
        "definition_id": definition,
        "operator": operator,
        "values": values,
    }))
}

fn typed_filter(raw: &str, value_type: &str) -> Result<Value, Failure> {
    let flag = format!("--{value_type}");
    let (definition, operator, value) = parts(raw, &flag)?;
    let allowed = match value_type {
        "text" => &["equals", "not_equals", "contains", "prefix"][..],
        "integer" | "number" => &["equals", "not_equals", "gt", "gte", "lt", "lte"][..],
        _ => unreachable!("callers use the closed typed filter set"),
    };
    if !allowed.contains(&operator) {
        return Err(invalid_filter(
            &flag,
            raw,
            "<definition>:<operator>:<value>",
        ));
    }
    let typed = match value_type {
        "text" => json!({"type": "text", "text": nonempty(value, &flag, raw)?}),
        "integer" => {
            let parsed = value.parse::<i64>().map_err(|_| {
                Failure::invalid("invalid_number", format!("`{flag}` requires an integer"))
                    .remedy("pass <definition>:<operator>:<whole-number>")
            })?;
            json!({"type": "integer", "integer": parsed})
        }
        "number" => {
            let parsed = value.parse::<f64>().map_err(|_| {
                Failure::invalid("invalid_number", format!("`{flag}` requires a number"))
                    .remedy("pass <definition>:<operator>:<finite-number>")
            })?;
            if !parsed.is_finite() {
                return Err(Failure::invalid(
                    "invalid_number",
                    format!("`{flag}` requires a finite number"),
                )
                .remedy("pass <definition>:<operator>:<finite-number>"));
            }
            json!({"type": "number", "number": parsed})
        }
        _ => unreachable!("callers use the closed typed filter set"),
    };
    Ok(json!({
        "definition_id": definition,
        "operator": operator,
        "typed_values": [typed],
    }))
}

fn parts<'a>(raw: &'a str, flag: &str) -> Result<(&'a str, &'a str, &'a str), Failure> {
    let mut parts = raw.splitn(3, ':');
    let definition = parts.next().unwrap_or_default();
    let operator = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default();
    if definition.trim().is_empty() || operator.trim().is_empty() || value.trim().is_empty() {
        return Err(invalid_filter(flag, raw, "<definition>:<operator>:<value>"));
    }
    Ok((definition.trim(), operator.trim(), value.trim()))
}

fn nonempty<'a>(value: &'a str, flag: &str, raw: &str) -> Result<&'a str, Failure> {
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid_filter(flag, raw, "a non-empty value"));
    }
    Ok(value)
}

fn invalid_filter(flag: &str, raw: &str, expected: &str) -> Failure {
    Failure::invalid(
        "invalid_tag_input",
        format!("`{flag} {raw}` is not a valid typed tag predicate"),
    )
    .remedy(format!("pass `{flag} {expected}`"))
}

pub fn render(data: &Value) -> String {
    let rows = data["matches"]
        .as_array()
        .or_else(|| data["query"]["matches"].as_array());
    let query = if data["query"].is_object() {
        &data["query"]
    } else {
        data
    };
    let mut out = format!(
        "{} tag matches · scanned {} transformers / {} assignments ({} reads)\n",
        rows.map_or(0, Vec::len),
        query["scanned_objects"].as_u64().unwrap_or(0),
        query["scanned_assignments"].as_u64().unwrap_or(0),
        query["assignment_reads"].as_u64().unwrap_or(0),
    );
    for row in rows.into_iter().flatten() {
        out.push_str(&format!(
            "  {}\n",
            row["object"]["id"].as_str().unwrap_or("?")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_closed_and_losslessly_typed() {
        assert_eq!(
            choice_filter("city:any_of:huye,kigali").unwrap(),
            json!({"definition_id":"city","operator":"any_of","values":["huye","kigali"]})
        );
        assert_eq!(
            typed_filter("completion:gte:82.5", "number").unwrap(),
            json!({"definition_id":"completion","operator":"gte","typed_values":[{"type":"number","number":82.5}]})
        );
        assert_eq!(
            typed_filter("survey_note:contains:road: blocked", "text").unwrap(),
            json!({"definition_id":"survey_note","operator":"contains","typed_values":[{"type":"text","text":"road: blocked"}]})
        );
    }

    #[test]
    fn malformed_or_non_finite_filters_refuse_before_pairing() {
        assert_eq!(
            presence_filter("city:equals").unwrap_err().code(),
            "invalid_tag_input"
        );
        assert_eq!(
            typed_filter("completion:gte:NaN", "number")
                .unwrap_err()
                .code(),
            "invalid_number"
        );
    }
}
