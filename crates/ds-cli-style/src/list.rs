//! `ds style list` — the style refs the paired application has open.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, HOST_ARG, LANE_ARG, PROJECT_ARG};

const QUERY_ARG: Arg = Arg {
    name: "query",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Keep refs or layer names containing this text.",
};
const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("100"),
    choices: &[],
    summary: "Rows to return (1-200). The total is always reported.",
};

pub static COMMAND: Command = Command {
    id: "style.list",
    path: &["style", "list"],
    contract: 2,
    summary: "Loaded style refs, including present Notes/PM geometry children.",
    purpose: "Reads a bounded backend catalogue of governed style editor refs. Native project authentication is the default; --host desktop uses one explicit project through the paired application without switching its GUI.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        QUERY_ARG,
        LIMIT_ARG,
        HOST_ARG,
        PROJECT_ARG,
        LANE_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "Project, total, bounded styles with refs/type/target/colour field, and more.",
    examples: &[
        Example {
            command: "ds style list --query lv_poles --output json",
            note: "Pick the ref whose target is design_vt for tiled design layers.",
            runnable: false,
        },
        Example {
            command: "ds style list --host desktop --project PROJECT --query roads --output json",
            note: "Use the paired signed-in application for this exact project without changing the GUI project.",
            runnable: false,
        },
    ],
    refusals: crate::native::REFUSALS,
    reference: Some("docs/reference/style.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    if let Some(query) = inputs.value("query") {
        arguments.insert("query".into(), json!(query));
    }
    if let Some(limit) = inputs.value("limit") {
        arguments.insert(
            "limit".into(),
            json!(crate::integer(limit, "limit", 1, 200)?),
        );
    }
    crate::native::execute(
        inputs,
        context,
        &crate::STYLE_LIST,
        Value::Object(arguments),
    )
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{} · {}\n",
        data["project"].as_str().unwrap_or("?"),
        crate::plural(total, "style")
    );
    for row in data["styles"].as_array().into_iter().flatten() {
        let dimensions: Vec<String> = row["dimensions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| {
                format!(
                    "{}→{}({})",
                    d["field"].as_str().unwrap_or("?"),
                    d["kind"].as_str().unwrap_or("?"),
                    d["values"].as_u64().unwrap_or(0)
                )
            })
            .collect();
        let runtime = row["runtime"].as_object().map(|receipt| {
            format!(
                " · {} / {}",
                receipt
                    .get("logicalRoot")
                    .and_then(Value::as_str)
                    .unwrap_or("runtime"),
                receipt
                    .get("freshness")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
            )
        });
        out.push_str(&format!(
            "  {:<32} {:<8} {:<10} {}{}\n",
            crate::truncate(row["ref"].as_str().unwrap_or("?"), 32),
            row["type"].as_str().unwrap_or("?"),
            row["target"].as_str().unwrap_or("—"),
            if dimensions.is_empty() {
                "flat".to_string()
            } else {
                dimensions.join(" + ")
            },
            runtime.unwrap_or_default(),
        ));
    }
    if data["more"].as_bool().unwrap_or(false) || data["truncated"].as_bool().unwrap_or(false) {
        out.push_str("  … more; narrow with --query or raise --limit\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::render;
    use ds_cli_contract::{Context, Format, Output, parse};
    use serde_json::json;

    fn context() -> Context {
        Context {
            confirmed: false,
            output: Output {
                format: Format::Json,
                pretty: false,
                color: false,
            },
        }
    }

    #[test]
    fn renders_runtime_source_freshness_without_changing_the_style_ref() {
        let text = render(&json!({
            "project": "p1",
            "total": 1,
            "truncated": false,
            "styles": [{
                "ref": "ud/project_work_polygon",
                "type": "fill",
                "target": "user_data",
                "dimensions": [],
                "runtime": { "logicalRoot": "project_work", "freshness": "partial" }
            }]
        }));
        assert!(text.contains("ud/project_work_polygon"));
        assert!(text.contains("project_work / partial"));
    }

    #[test]
    fn desktop_host_requires_an_explicit_project_and_never_becomes_a_native_fallback() {
        let desktop = parse(
            &super::COMMAND,
            &["--host", "desktop"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
        )
        .expect("desktop inputs");
        let failure = super::run(&desktop, &context()).expect_err("project is required");
        assert_eq!(failure.code(), "style_refused");
        assert!(failure.message().contains("requires --project"));

        let native = parse(
            &super::COMMAND,
            &["--project", "huye"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
        )
        .expect("native inputs");
        let failure = super::run(&native, &context()).expect_err("project is desktop-only");
        assert_eq!(failure.code(), "style_refused");
        assert!(failure.message().contains("requires --host desktop"));
    }
}
