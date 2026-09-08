//! `ds style label plan | set` — bind a governed label to one declared field.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, HOST_ARG, LANE_ARG, PROJECT_ARG, REF_ARG};

const FIELD_ARG: Arg = Arg {
    name: "field",
    kind: ArgKind::Value,
    value: "<field>",
    required: true,
    default: None,
    choices: &[],
    summary: "Exact label field from `ds style read <ref>` .data.fields.",
};

fn arguments(inputs: &Inputs, apply: bool) -> Result<Value, Failure> {
    let raw = inputs.require("field")?;
    if raw.is_empty() || raw.len() > 120 || raw.trim() != raw {
        return Err(Failure::invalid(
            "invalid_label",
            "`--field` must be one non-empty field of at most 120 characters without surrounding whitespace",
        )
        .remedy(crate::INVALID_LABEL.remedy)
        .detail(json!({ "given": raw })));
    }
    Ok(json!({
        "ref": inputs.require("ref")?,
        "field": raw,
        "apply": apply,
    }))
}

fn render_label(data: &Value) -> String {
    let mut out = format!(
        "{} · label {} · {}\n",
        data["ref"].as_str().unwrap_or("?"),
        data["labelField"].as_str().unwrap_or("?"),
        if data["published"].as_bool().unwrap_or(false) {
            "published"
        } else {
            "plan only — nothing published"
        },
    );
    for warning in data["warnings"].as_array().into_iter().flatten() {
        if let Some(text) = warning.as_str() {
            out.push_str(&format!("  warning: {}\n", crate::truncate(text, 110)));
        }
    }
    out
}

pub mod plan {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "style.label.plan",
        path: &["style", "label", "plan"],
        contract: 1,
        summary: "Preview binding a label to one declared data field.",
        purpose: "Uses the shared Style Center planner to change only the label's text field. Existing placement, font, scale, halo, visibility, print scaling and symbols are preserved; a missing label starts from the backend's label model.",
        chapter: Chapter::MapPresentation,
        effect: Effect::LocalAuthState,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[
            REF_ARG,
            FIELD_ARG,
            HOST_ARG,
            PROJECT_ARG,
            LANE_ARG,
            DESCRIPTOR_ARG,
        ],
        output: "The exact resulting document, `labelField`, `changed`, and `published: false`.",
        examples: &[Example {
            command: "ds style label plan --ref gt/roads_print --field road_no --host desktop --project <project-id> --output json",
            note: "Refused when road_no is absent from the style's published fields.",
            runnable: false,
        }],
        refusals: crate::native::REFUSALS,
        reference: Some("docs/reference/style.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
        crate::native::execute(
            inputs,
            context,
            &crate::LABEL_SET,
            arguments(inputs, false)?,
        )
    }

    pub fn render(data: &Value) -> String {
        render_label(data)
    }
}

pub mod set {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "style.label.set",
        path: &["style", "label", "set"],
        contract: 1,
        summary: "Publish a governed label bound to one declared data field.",
        purpose: "Publishes the exact document returned by `ds style label plan` through the Style Center save route. The shared Rust planner owns the field validation and label transformation.",
        chapter: Chapter::MapPresentation,
        effect: Effect::GlobalWrite,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[
            REF_ARG,
            FIELD_ARG,
            HOST_ARG,
            PROJECT_ARG,
            LANE_ARG,
            DESCRIPTOR_ARG,
        ],
        output: "The plan receipt with `published: true`, backend warnings, and the exact persisted document.",
        examples: &[Example {
            command: "ds style label set --ref gt/roads_print --field road_no --host desktop --project <project-id> --yes --output json",
            note: "Labels roads with the published road number field while retaining the print style's other authorship.",
            runnable: false,
        }],
        refusals: crate::native::REFUSALS,
        reference: Some("docs/reference/style.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
        crate::native::execute(inputs, context, &crate::LABEL_SET, arguments(inputs, true)?)
    }

    pub fn render(data: &Value) -> String {
        render_label(data)
    }
}

#[cfg(test)]
mod tests {
    use ds_cli_contract::parse;

    use super::*;

    #[test]
    fn plan_and_set_share_one_closed_label_instruction() {
        let tokens = ["--ref", "gt/roads_print", "--field", "road_no"].map(str::to_string);
        let plan_inputs = parse(&plan::COMMAND, &tokens).expect("plan inputs");
        let set_inputs = parse(&set::COMMAND, &tokens).expect("set inputs");
        assert_eq!(
            arguments(&plan_inputs, false).expect("plan"),
            json!({"ref":"gt/roads_print","field":"road_no","apply":false})
        );
        assert_eq!(
            arguments(&set_inputs, true).expect("set"),
            json!({"ref":"gt/roads_print","field":"road_no","apply":true})
        );
    }

    #[test]
    fn label_field_is_exact_and_bounded_before_transport() {
        for field in [" road_no", "road_no ", ""] {
            let tokens = ["--ref", "gt/roads_print", "--field", field].map(str::to_string);
            let inputs = parse(&plan::COMMAND, &tokens).expect("parsed");
            assert_eq!(
                arguments(&inputs, false).expect_err("refused").code(),
                "invalid_label"
            );
        }
    }
}
