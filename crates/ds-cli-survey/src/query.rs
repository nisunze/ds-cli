//! One selected-project Survey aggregate through the fixed native route.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{SurveyQueryFilter, SurveyQueryMetric, SurveyQueryOrder, SurveyQueryRequest};
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};
use std::fmt;

const MAX_FILTER_BYTES: usize = 8 * 1024;
const MAX_FILTERS: usize = 8;
const MAX_IN_VALUES: usize = 20;
const MAX_VALUE_CHARS: usize = 2_048;

const FORM: Arg = Arg::value("form", "<form-slug>", "Exact governed form slug.").required();
const METRIC: Arg = Arg::value("metric", "<count|count_distinct>", "Aggregate metric.")
    .default("count")
    .choices(&["count", "count_distinct"]);
const DISTINCT_FIELD: Arg = Arg::value(
    "distinct-field",
    "<field>",
    "Field counted by count_distinct; forbidden for count.",
);
const GROUP_BY: Arg = Arg {
    name: "group-by",
    kind: ArgKind::Repeated,
    value: "<field>",
    required: false,
    default: None,
    choices: &[],
    summary: "Grouping field; repeat at most twice. Public created_by is supported.",
};
const FILTER: Arg = Arg {
    name: "filter",
    kind: ArgKind::Repeated,
    value: "<json-object>",
    required: false,
    default: None,
    choices: &[],
    summary: "One closed filter object; repeat at most eight times.",
};
const ORDER: Arg = Arg::value("order", "<asc|desc>", "Aggregate row order.")
    .default("desc")
    .choices(&["asc", "desc"]);
const LIMIT: Arg = Arg::value("limit", "<1-200>", "Maximum aggregate rows.").default("50");
const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

const QUERY_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "survey_query_invalid",
        when: "the aggregate flags violate the closed metric, grouping, field, order, or limit grammar",
        remedy: "correct the declared flags before retrying",
    },
    Refusal {
        code: "survey_filter_invalid",
        when: "a repeated filter is oversized, not an exact JSON object, or violates its operator-specific fields",
        remedy: "pass one closed JSON object per --filter",
    },
    Refusal {
        code: "survey_scope_not_found",
        when: "the selected project or governed form is unavailable to the verified user",
        remedy: "verify the selected project and pass one exact available form slug",
    },
    Refusal {
        code: "survey_query_refused",
        when: "the backend refuses the already validated question, including a stale Survey view",
        remedy: "retry once, then verify the governed form/view state before changing the question",
    },
    Refusal {
        code: "native_profile_not_configured",
        when: "the exact packaged native profile is unavailable",
        remedy: "install one complete ds release",
    },
    Refusal {
        code: "native_profile_digest_mismatch",
        when: "the packaged catalogue differs from the build pin",
        remedy: "reinstall one complete ds release",
    },
    Refusal {
        code: "native_profile_unsafe",
        when: "the packaged native catalogue is unsafe or malformed",
        remedy: "reinstall one complete ds release",
    },
    Refusal {
        code: "headless_signed_out",
        when: "the selected lane has no restorable native user",
        remedy: "run ds auth login --email <address>",
    },
    Refusal {
        code: "headless_project_not_selected",
        when: "the user has no audience-fenced project selection",
        remedy: "run ds auth project use --project <exact-id>",
    },
    Refusal {
        code: "project_context_stale",
        when: "the saved project belongs to another identity, lane, or audience",
        remedy: "select the project again with ds auth project use",
    },
    Refusal {
        code: "native_state_unsafe",
        when: "protected native state is unsafe or unreadable",
        remedy: "repair the owner-only DS config directory",
    },
    Refusal {
        code: "native_state_unavailable",
        when: "protected native state cannot be accessed",
        remedy: "repair the owner-only DS config directory",
    },
    Refusal {
        code: "native_state_protection_unavailable",
        when: "this build has no protected-state adapter",
        remedy: "install a supported native ds build",
    },
    Refusal {
        code: "native_state_root_invalid",
        when: "the configured state root is not absolute",
        remedy: "unset it or provide an absolute path",
    },
    Refusal {
        code: "native_state_conflict",
        when: "another native operation holds the state lease",
        remedy: "retry after that operation finishes",
    },
    Refusal {
        code: "native_cleanup_required",
        when: "revoked identity cleanup cannot clear project context",
        remedy: "repair protected state and run auth logout",
    },
    Refusal {
        code: "auth_rejected",
        when: "the gateway rejects membership or the verified request",
        remedy: "verify account and form authority in the selected project",
    },
    Refusal {
        code: "auth_revoked",
        when: "Firebase permanently revokes the native session",
        remedy: "sign in again interactively",
    },
    Refusal {
        code: "auth_identity_mismatch",
        when: "Firebase returns an identity outside the bound session",
        remedy: "sign in again and report a repeated mismatch",
    },
    Refusal {
        code: "auth_transient",
        when: "the fixed native service is temporarily unavailable",
        remedy: "retry without changing local state",
    },
    Refusal {
        code: "auth_response_unreadable",
        when: "the Survey aggregate response violates its closed bounded contract",
        remedy: "retry once, then update ds if it persists",
    },
];

pub static COMMAND: Command = Command {
    id: "survey.query",
    path: &["survey", "query"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Run one governed Survey aggregate headlessly.",
    purpose: "Validates one closed aggregate question before auth, restores the native user, loads only its audience-fenced selected project under lease, releases the lease before the fixed Survey query call, and strictly decodes at most 200 rows. The server rechecks project/form/view authority and refreshes the survey mirror. No project, URL, body, token, raw-entry, or media override is accepted.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        FORM,
        METRIC,
        DISTINCT_FIELD,
        GROUP_BY,
        FILTER,
        ORDER,
        LIMIT,
        LANE,
    ],
    output: "Lane, selected-project identity, echoed form/metric/grouping, at most 200 aggregate rows, and truncation; never raw entries, billing claims, or credentials.",
    examples: &[
        Example {
            command: "ds survey query --form lv_poles_survey --metric count --group-by created_by --filter '{\"field\":\"created_by\",\"op\":\"eq\",\"value\":\"operator@example.com\"}' --output json",
            note: "Counts the selected project's governed rows by the public created_by field.",
            runnable: false,
        },
        Example {
            command: "ds survey query --form customers --metric count_distinct --distinct-field created_by --limit 50 --output json",
            note: "Counts distinct public creators without returning any Survey entry.",
            runnable: false,
        },
    ],
    refusals: QUERY_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // All caller-controlled grammar is parsed before profile discovery, auth,
    // project-context access, or network work.
    let query = parse(inputs)?;
    let headless = ds_cli_auth::survey_query(inputs.require("lane")?, &query)?;
    let result = headless.result();
    let rows = result
        .rows()
        .iter()
        .map(|row| json!({ "groups": row.groups(), "value": row.value() }))
        .collect::<Vec<_>>();
    Ok(json!({
        "lane": headless.lane(),
        "project": {
            "ds_project": headless.project_id(),
            "project_name": headless.project_name(),
            "status": headless.project_status(),
        },
        "form": result.form(),
        "metric": result.metric().as_str(),
        "group_by": result.group_by(),
        "rows": rows,
        "truncated": result.truncated(),
    }))
}

fn parse(inputs: &Inputs) -> Result<SurveyQueryRequest, Failure> {
    let metric = match inputs.require("metric")? {
        "count" => SurveyQueryMetric::Count,
        "count_distinct" => SurveyQueryMetric::CountDistinct,
        _ => return Err(query_failure("`--metric` must be count or count_distinct")),
    };
    let order = match inputs.require("order")? {
        "asc" => SurveyQueryOrder::Asc,
        "desc" => SurveyQueryOrder::Desc,
        _ => return Err(query_failure("`--order` must be asc or desc")),
    };
    let limit = inputs
        .require("limit")?
        .parse::<u16>()
        .ok()
        .filter(|limit| (1..=200).contains(limit))
        .ok_or_else(|| query_failure("`--limit` must be an integer from 1 through 200"))?;
    let group_by = inputs.repeated("group-by");
    if group_by.len() > 2 {
        return Err(query_failure("at most two `--group-by` flags may be given"));
    }
    let raw_filters = inputs.repeated("filter");
    if raw_filters.len() > MAX_FILTERS {
        return Err(filter_failure(
            "at most eight `--filter` flags may be given",
        ));
    }
    let filters = raw_filters
        .iter()
        .map(|raw| parse_filter(raw))
        .collect::<Result<Vec<_>, _>>()?;
    SurveyQueryRequest::new(
        inputs.require("form")?,
        metric,
        inputs.value("distinct-field").map(str::to_owned),
        group_by.iter().map(|value| (*value).to_owned()).collect(),
        filters,
        order,
        limit,
    )
    .map_err(|_| query_failure("the query fields violate the closed Survey aggregate grammar"))
}

fn parse_filter(raw: &str) -> Result<SurveyQueryFilter, Failure> {
    if raw.is_empty() || raw.len() > MAX_FILTER_BYTES {
        return Err(filter_failure(
            "each `--filter` must be 1 through 8192 UTF-8 bytes",
        ));
    }
    let object = serde_json::from_str::<ClosedFilterObject>(raw)
        .ok()
        .map(|value| value.0)
        .ok_or_else(|| filter_failure("each `--filter` must be one JSON object"))?;
    let field = string(&object, "field")?.to_owned();
    let op = string(&object, "op")?;
    match op {
        "eq" => exact_value_filter(&object, field, SurveyQueryFilter::eq),
        "neq" => exact_value_filter(&object, field, SurveyQueryFilter::neq),
        "gte" => exact_value_filter(&object, field, SurveyQueryFilter::gte),
        "lte" => exact_value_filter(&object, field, SurveyQueryFilter::lte),
        "in" => {
            exact_keys(&object, &["field", "op", "values"])?;
            let values = object
                .get("values")
                .and_then(Value::as_array)
                .filter(|values| (1..=MAX_IN_VALUES).contains(&values.len()))
                .ok_or_else(|| filter_failure("`in` requires 1 through 20 string values"))?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .filter(|value| bounded_value(value, false))
                        .map(str::to_owned)
                        .ok_or_else(|| filter_failure("every `in` value must be a bounded string"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(SurveyQueryFilter::in_values(field, values))
        }
        "between" => {
            exact_keys(&object, &["field", "from", "op", "to"])?;
            let from = bounded_string(&object, "from", true)?;
            let to = bounded_string(&object, "to", true)?;
            Ok(SurveyQueryFilter::between(field, from, to))
        }
        "is_null" => {
            exact_keys(&object, &["field", "op"])?;
            Ok(SurveyQueryFilter::is_null(field))
        }
        "not_null" => {
            exact_keys(&object, &["field", "op"])?;
            Ok(SurveyQueryFilter::not_null(field))
        }
        _ => Err(filter_failure(
            "filter `op` is not in the closed operator set",
        )),
    }
}

fn exact_value_filter(
    object: &Map<String, Value>,
    field: String,
    constructor: fn(String, String) -> SurveyQueryFilter,
) -> Result<SurveyQueryFilter, Failure> {
    exact_keys(object, &["field", "op", "value"])?;
    let value = bounded_string(object, "value", false)?;
    Ok(constructor(field, value))
}

fn string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, Failure> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| filter_failure(format!("filter `{key}` must be a string")))
}

fn bounded_string(
    object: &Map<String, Value>,
    key: &str,
    nonempty: bool,
) -> Result<String, Failure> {
    let value = string(object, key)?.to_owned();
    if bounded_value(&value, nonempty) {
        Ok(value)
    } else {
        Err(filter_failure(format!(
            "filter `{key}` is outside its string bound"
        )))
    }
}

fn bounded_value(value: &str, nonempty: bool) -> bool {
    (!nonempty || !value.is_empty())
        && value.chars().count() <= MAX_VALUE_CHARS
        && !value.chars().any(char::is_control)
}

fn exact_keys(object: &Map<String, Value>, expected: &[&str]) -> Result<(), Failure> {
    let mut actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    actual.sort_unstable();
    if actual == expected {
        Ok(())
    } else {
        Err(filter_failure(
            "filter fields do not exactly match the selected operator",
        ))
    }
}

struct ClosedFilterObject(Map<String, Value>);

impl<'de> Deserialize<'de> for ClosedFilterObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(ClosedFilterVisitor)
    }
}

struct ClosedFilterVisitor;

impl<'de> Visitor<'de> for ClosedFilterVisitor {
    type Value = ClosedFilterObject;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("one JSON object with unique keys")
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        while let Some(key) = access.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom("duplicate filter key"));
            }
            object.insert(key, access.next_value::<Value>()?);
        }
        Ok(ClosedFilterObject(object))
    }
}

fn query_failure(message: impl Into<String>) -> Failure {
    Failure::invalid("survey_query_invalid", message)
        .remedy("read `ds survey query --help` and pass only the bounded typed flags")
}

fn filter_failure(message: impl Into<String>) -> Failure {
    Failure::invalid("survey_filter_invalid", message)
        .remedy("pass one exact operator-specific JSON object per --filter")
}

pub fn render(data: &Value) -> String {
    format!(
        "{}/{}  {} rows{}\n",
        data["project"]["ds_project"].as_str().unwrap_or("project"),
        data["form"].as_str().unwrap_or("form"),
        data["rows"].as_array().map_or(0, Vec::len),
        if data["truncated"].as_bool().unwrap_or(false) {
            " (truncated)"
        } else {
            ""
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::args::parse;

    fn inputs(arguments: &[&str]) -> Inputs {
        parse(
            &COMMAND,
            &arguments
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn created_by_filter_and_group_are_public_grammar() {
        let inputs = inputs(&[
            "--form",
            "a_poles",
            "--group-by",
            "created_by",
            "--filter",
            r#"{"field":"created_by","op":"eq","value":"operator@example.com"}"#,
        ]);
        let query = super::parse(&inputs).unwrap();
        assert_eq!(query.group_by(), ["created_by"]);
        assert_eq!(query.filters()[0].field(), "created_by");
        assert_eq!(query.order(), SurveyQueryOrder::Desc);
        assert_eq!(query.limit(), 50);
        assert_eq!(COMMAND.arg("order").unwrap().default, Some("desc"));
        assert_eq!(COMMAND.arg("limit").unwrap().default, Some("50"));
    }

    #[test]
    fn metric_pairing_and_bounds_are_refused_before_auth() {
        for arguments in [
            vec![
                "--form",
                "a_poles",
                "--metric",
                "count",
                "--distinct-field",
                "created_by",
            ],
            vec!["--form", "a_poles", "--metric", "count_distinct"],
            vec!["--form", "a_poles", "--limit", "201"],
            vec![
                "--form",
                "a_poles",
                "--group-by",
                "one",
                "--group-by",
                "two",
                "--group-by",
                "three",
            ],
        ] {
            assert_eq!(
                super::parse(&inputs(&arguments)).unwrap_err().code(),
                "survey_query_invalid"
            );
        }
    }

    #[test]
    fn filters_require_exact_operator_specific_objects() {
        for raw in [
            "[]",
            r#"{"field":"created_by","op":"eq"}"#,
            r#"{"field":"created_by","op":"eq","value":"x","extra":true}"#,
            r#"{"field":"created_by","field":"other","op":"eq","value":"x"}"#,
            r#"{"field":"created_by","op":"is_null","value":null}"#,
            r#"{"field":"created_by","op":"in","values":[]}"#,
            r#"{"field":"created_by","op":"between","from":"","to":"z"}"#,
        ] {
            assert_eq!(
                parse_filter(raw).unwrap_err().code(),
                "survey_filter_invalid"
            );
        }
    }

    #[test]
    fn every_closed_filter_operator_builds_without_auth() {
        for raw in [
            r#"{"field":"created_by","op":"eq","value":"a"}"#,
            r#"{"field":"created_by","op":"neq","value":"a"}"#,
            r#"{"field":"created_by","op":"gte","value":"a"}"#,
            r#"{"field":"created_by","op":"lte","value":"z"}"#,
            r#"{"field":"created_by","op":"in","values":["a","b"]}"#,
            r#"{"field":"created_by","op":"between","from":"a","to":"z"}"#,
            r#"{"field":"created_by","op":"is_null"}"#,
            r#"{"field":"created_by","op":"not_null"}"#,
        ] {
            assert!(parse_filter(raw).is_ok(), "operator fixture failed: {raw}");
        }
    }

    #[test]
    fn descriptor_has_no_escape_hatches() {
        for forbidden in [
            "project",
            "url",
            "body",
            "token",
            "entry",
            "raw",
            "media",
            "desktop-descriptor",
        ] {
            assert!(COMMAND.arg(forbidden).is_none(), "unexpected --{forbidden}");
        }
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.effect, Effect::LocalAuthState);
        let refusal_codes = COMMAND
            .refusals
            .iter()
            .map(|refusal| refusal.code)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(refusal_codes.contains("survey_scope_not_found"));
        assert!(refusal_codes.contains("survey_query_refused"));
        assert!(!refusal_codes.contains("survey_form_not_found"));
        assert!(!refusal_codes.contains("auth_input_invalid"));
    }
}
