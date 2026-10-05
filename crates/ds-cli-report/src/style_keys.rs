//! `ds report layout style-keys` — migrate legacy printing templates to exact
//! governed style keys. A template that still names its pens through legacy
//! `style_refs` refuses every preview with `style_resolution_required`.
//! ds-command-kernel decides each template; ds-brain holds the templates and
//! the governed catalogue, fences the reviewed plan and keeps every replaced
//! template. This domain only reads plans and applies a reviewed one.
use ds_cli_auth::{
    PRINT_STYLE_KEYS_CATALOGUE_UNSEEDED_REFUSAL, PRINT_STYLE_KEYS_KERNEL_UNAVAILABLE_REFUSAL,
    PRINT_STYLE_KEYS_PLAN_CHANGED_REFUSAL, PRINT_STYLE_KEYS_REQUEST_INVALID_REFUSAL,
    PRINT_STYLE_KEYS_ROUTE_UNAVAILABLE_REFUSAL, PRINT_STYLE_KEYS_SETUP_CHANGED_REFUSAL,
    PrintStyleKeyChoices, PrintStyleKeysRequest,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;
use std::io::Read;

const LANE: Arg = ds_cli_contract::spec::LANE
    .placeholder("<lane>")
    .choices(&["canary", "stable"])
    .default("canary");
const SCOPE: Arg = Arg::value(
    "scope",
    "<scope>",
    "The global template library, or the library of the project named by --project.",
)
.choices(&["global", "project"]);
const PROJECT: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Exact ds_project whose library is migrated, with --scope project.",
);
const ID: Arg = Arg::value(
    "id",
    "<setup-id>",
    "One template id from report.layout.list; omit for every template of the library.",
);
const CHOICES: Arg = Arg::value(
    "choices",
    "<json-file>",
    "Owner choices: {\"<template id>\": {\"<layer>\": <a key from the plan's candidates>}}.",
);
const PURPOSE: &str = "Moves printing templates whose pens still use legacy style_refs (previews refuse with style_resolution_required) to the governed print keys ds-command-kernel decides. An undecidable layer is a named finding settled only by --choices. Plans write nothing; apply writes exactly a reviewed plan_sha256 and keeps every replaced template.";

const NOT_FOUND: Refusal = Refusal {
    code: "print_setup_not_found",
    when: "the named template does not exist in the addressed library",
    remedy: "list the library with report.layout.list and pass one exact template id",
};
const PROJECT_REQUIRED: Refusal = Refusal {
    code: "project_required",
    when: "--scope project names no --project, or --scope global names one",
    remedy: "pass --scope project with --project <exact-id>, or --scope global alone; the saved selection is never read",
};
const INPUT_INVALID: Refusal = Refusal {
    code: "printing_invalid",
    when: "the flags do not name one library, or the choices file is not one bounded JSON object of exact keys",
    remedy: "pass --scope (and --project), or --all-projects alone; write --choices from the plan's candidates",
};
const REFUSALS: &[Refusal] = &[
    INPUT_INVALID,
    PROJECT_REQUIRED,
    NOT_FOUND,
    PRINT_STYLE_KEYS_REQUEST_INVALID_REFUSAL,
    PRINT_STYLE_KEYS_PLAN_CHANGED_REFUSAL,
    PRINT_STYLE_KEYS_SETUP_CHANGED_REFUSAL,
    PRINT_STYLE_KEYS_CATALOGUE_UNSEEDED_REFUSAL,
    PRINT_STYLE_KEYS_KERNEL_UNAVAILABLE_REFUSAL,
    PRINT_STYLE_KEYS_ROUTE_UNAVAILABLE_REFUSAL,
];

pub static PLAN: Command = Command {
    id: "report.layout.style-keys.plan",
    path: &["report", "layout", "style-keys", "plan"],
    contract: 1,
    summary: "Plan the migration of legacy printing templates to exact style keys.",
    purpose: PURPOSE,
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        SCOPE,
        PROJECT,
        ID,
        CHOICES,
        Arg::switch(
            "all-projects",
            "Census of every library, a page at a time; platform administrators only.",
        ),
        Arg::value("cursor", "<cursor>", "next_cursor of the previous census page."),
        Arg::value("limit", "<n>", "Libraries per census page, 1-25 (default 10)."),
        LANE,
    ],
    output: "Per template: status current|migrate|blocked|refused, Kernel keys, document changes, findings with governed candidates and the plan_sha256 apply checks; --all-projects pages libraries with counts and plan_sha256.",
    examples: &[
        Example {
            command: "ds report layout style-keys plan --scope project --project <exact-id> --output json",
            note: "Every template of one project; read .data.setups[].plan.findings and .data.plan_sha256.",
            runnable: false,
        },
        Example {
            command: "ds report layout style-keys plan --all-projects --output json",
            note: "Platform census; continue with --cursor <next_cursor>.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[
        "template",
        "template migration",
        "resolution required",
        "style refs",
        "legacy pens",
        "template preview",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static APPLY: Command = Command {
    id: "report.layout.style-keys.apply",
    path: &["report", "layout", "style-keys", "apply"],
    contract: 1,
    summary: "Apply a reviewed printing template style-key migration plan.",
    purpose: PURPOSE,
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        SCOPE.required(),
        PROJECT,
        ID,
        CHOICES,
        Arg::value(
            "expected-plan",
            "<64hex>",
            "Exact plan_sha256 of the reviewed plan with the same scope, project, id and choices.",
        )
        .required(),
        LANE,
    ],
    output: "Receipt: each migrated template's previous and new revision and its retained copy; templates already current and templates left unchanged.",
    examples: &[Example {
        command: "ds report layout style-keys apply --scope project --project <exact-id> --choices choices.json --expected-plan <plan_sha256> --yes --output json",
        note: "Writes only the plan's migrate templates, each beside an immutable copy of what it replaced.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["template migration", "resolution required"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(INPUT_INVALID.code, message.into()).remedy(INPUT_INVALID.remedy)
}

/// The library the flags name: `None` for the global one.
fn library(i: &Inputs) -> Result<Option<String>, Failure> {
    let refuse = |message: &str| {
        Failure::invalid(PROJECT_REQUIRED.code, message.to_owned()).remedy(PROJECT_REQUIRED.remedy)
    };
    match (i.value("scope"), i.value("project")) {
        (Some("global"), None) => Ok(None),
        (Some("project"), Some(project)) => Ok(Some(project.to_owned())),
        (Some("project"), None) => Err(refuse("--scope project addresses the library named by --project")),
        (Some(_), Some(_)) => Err(refuse("--scope global names no --project")),
        _ => Err(invalid("name one library with --scope, or pass --all-projects alone")),
    }
}

fn choices(i: &Inputs) -> Result<PrintStyleKeyChoices, Failure> {
    let Some(path) = i.value("choices") else {
        return Ok(PrintStyleKeyChoices::new());
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| invalid(format!("{path}: {e}")))?
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(format!("{path}: {e}")))?;
    if bytes.len() > 256 * 1024 {
        return Err(invalid("the choices file exceeds 256 KiB"));
    }
    serde_json::from_slice(&bytes).map_err(|e| invalid(format!("{path}: {e}")))
}

fn setup(i: &Inputs) -> Option<String> {
    i.value("id").map(str::to_owned)
}

pub fn plan(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let lane = i.require("lane")?;
    if i.switch("all-projects") {
        if ["scope", "project", "id", "choices"]
            .iter()
            .any(|name| i.value(name).is_some())
        {
            return Err(invalid(
                "--all-projects plans every library; pass no --scope, --project, --id or --choices",
            ));
        }
        let limit = match i.value("limit") {
            Some(text) => Some(
                text.parse::<u32>()
                    .ok()
                    .filter(|n| (1..=25).contains(n))
                    .ok_or_else(|| invalid("--limit is 1-25"))?,
            ),
            None => None,
        };
        let request = PrintStyleKeysRequest::Census {
            cursor: i.value("cursor").map(str::to_owned),
            limit,
        };
        return ds_cli_auth::print_style_keys(lane, None, &request);
    }
    if i.value("cursor").is_some() || i.value("limit").is_some() {
        return Err(invalid("--cursor and --limit page only --all-projects"));
    }
    let project = library(i)?;
    let request = PrintStyleKeysRequest::Plan {
        id: setup(i),
        choices: choices(i)?,
    };
    ds_cli_auth::print_style_keys(lane, project.as_deref(), &request)
}

pub fn apply(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let project = library(i)?;
    let request = PrintStyleKeysRequest::Apply {
        id: setup(i),
        choices: choices(i)?,
        expected_plan_sha256: i.require("expected-plan")?.to_owned(),
    };
    ds_cli_auth::print_style_keys(i.require("lane")?, project.as_deref(), &request)
}

/// A plan reads as one line per template; a census as one line per library;
/// a receipt as one line per migrated template.
pub fn render(data: &Value) -> String {
    let mut text = String::new();
    if let Some(setups) = data["setups"].as_array() {
        for setup in setups {
            let findings = setup["plan"]["findings"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .map(|f| {
                            format!(
                                "{}:{}",
                                f["layer"].as_str().unwrap_or("-"),
                                f["code"].as_str().unwrap_or("-")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            text.push_str(&format!(
                "{} {} {}\n",
                setup["id"].as_str().unwrap_or("-"),
                setup["status"].as_str().unwrap_or("-"),
                findings
            ));
        }
        text.push_str(&format!(
            "plan_sha256 {}\n",
            data["plan_sha256"].as_str().unwrap_or("-")
        ));
        return text;
    }
    if let Some(libraries) = data["libraries"].as_array() {
        for library in libraries {
            let counts = &library["counts"];
            text.push_str(&format!(
                "{} current={} migrate={} blocked={} refused={} {}\n",
                library["project_id"].as_str().unwrap_or("global"),
                counts["current"],
                counts["migrate"],
                counts["blocked"],
                counts["refused"],
                library["plan_sha256"].as_str().unwrap_or("-")
            ));
        }
        if let Some(next) = data["next_cursor"].as_str() {
            text.push_str(&format!("next_cursor {next}\n"));
        }
        return text;
    }
    if let Some(migrated) = data["migrated"].as_array() {
        for row in migrated {
            text.push_str(&format!(
                "{} {} -> {} retained {}\n",
                row["id"].as_str().unwrap_or("-"),
                row["from_revision"].as_str().unwrap_or("-"),
                row["to_revision"].as_str().unwrap_or("-"),
                row["retained"].as_str().unwrap_or("-")
            ));
        }
        text.push_str(&format!("{} migrated\n", migrated.len()));
        return text;
    }
    format!("{data}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            confirmed: false,
            output: ds_cli_contract::Output {
                format: ds_cli_contract::Format::Json,
                pretty: false,
                color: false,
            },
        }
    }

    fn parse(command: &Command, args: &[&str]) -> Inputs {
        let owned = args.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        ds_cli_contract::parse(command, &owned).unwrap()
    }

    #[test]
    fn the_flags_name_exactly_one_library_before_anything_is_sent() {
        assert_eq!(
            library(&parse(&PLAN, &["--scope", "global"])).unwrap(),
            None
        );
        assert_eq!(
            library(&parse(&PLAN, &["--scope", "project", "--project", "czgmdwth_gisagara"]))
                .unwrap()
                .as_deref(),
            Some("czgmdwth_gisagara")
        );
        for args in [
            &["--scope", "project"][..],
            &["--scope", "global", "--project", "p"][..],
        ] {
            assert_eq!(library(&parse(&PLAN, args)).unwrap_err().code(), "project_required");
        }
        assert_eq!(
            library(&parse(&PLAN, &[])).unwrap_err().code(),
            "printing_invalid"
        );
        // A census names no library and is not mixed with one.
        let mixed = plan(
            &parse(&PLAN, &["--all-projects", "--project", "p"]),
            &context(),
        );
        assert_eq!(mixed.unwrap_err().code(), "printing_invalid");
        let paged = plan(
            &parse(&PLAN, &["--scope", "global", "--cursor", "x"]),
            &context(),
        );
        assert_eq!(paged.unwrap_err().code(), "printing_invalid");
    }

    #[test]
    fn choices_are_one_bounded_object_of_exact_keys() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("choices.json");
        std::fs::write(
            &good,
            r#"{"a0-landscape-gisagara-cjic":{"village_boundaries":{"entity_class":"village_boundaries","source_kind":"cold_existing","target":"print","role":"project"}}}"#,
        )
        .unwrap();
        let parsed = choices(&parse(&PLAN, &["--choices", good.to_str().unwrap()])).unwrap();
        assert_eq!(
            parsed["a0-landscape-gisagara-cjic"]["village_boundaries"].role,
            "project"
        );
        let loose = dir.path().join("loose.json");
        std::fs::write(
            &loose,
            r#"{"a0":{"village_boundaries":{"entity_class":"village_boundaries","source_kind":"cold_existing","target":"print","role":"project","guess":true}}}"#,
        )
        .unwrap();
        assert_eq!(
            choices(&parse(&PLAN, &["--choices", loose.to_str().unwrap()]))
                .unwrap_err()
                .code(),
            "printing_invalid"
        );
    }

    #[test]
    fn apply_is_a_confirmed_write_fenced_on_the_reviewed_plan() {
        assert_eq!(APPLY.effect, Effect::GlobalWrite);
        assert_eq!(PLAN.effect, Effect::LocalAuthState);
        assert!(APPLY.args.iter().any(|a| a.name == "expected-plan" && a.required));
        assert!(APPLY.args.iter().any(|a| a.name == "scope" && a.required));
        assert!(!APPLY.args.iter().any(|a| a.name == "all-projects"));
        assert!(PLAN.search.contains(&"template"));
    }
}
