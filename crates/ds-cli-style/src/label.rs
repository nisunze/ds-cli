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

const VISIBLE_ARG: Arg = Arg {
    name: "visible",
    kind: ArgKind::Value,
    value: "<on|off>",
    required: false,
    default: None,
    choices: &["on", "off"],
    summary: "Enable or hide the label without deleting its content.",
};
const SIZE_ARG: Arg = Arg {
    name: "size",
    kind: ArgKind::Value,
    value: "<number>",
    required: false,
    default: None,
    choices: &[],
    summary: "Label text size in the style's units; bounds are in style read .data.labelSchema.numerics.text-size.",
};
const FONT_ARG: Arg = Arg {
    name: "font",
    kind: ArgKind::Value,
    value: "<font>",
    required: false,
    default: None,
    choices: &[],
    summary: "Font from style read .data.labelSchema.fonts, including italic and bold faces.",
};
const COLOR_ARG: Arg = Arg {
    name: "color",
    kind: ArgKind::Value,
    value: "<#RRGGBB>",
    required: false,
    default: None,
    choices: &[],
    summary: "Label text color as a six-digit hexadecimal value.",
};
const PAPER_ARG: Arg = Arg {
    name: "paper",
    kind: ArgKind::Repeated,
    value: "<paper>",
    required: false,
    default: None,
    choices: &["A0", "A1", "A2", "A3", "A4", "A5", "Custom", "all"],
    summary: "Print labels only on these papers; repeat for several. Use all alone to clear the restriction. Other opacity authorship is retained.",
};
const PLACEMENT_ARG: Arg = Arg {
    name: "placement",
    kind: ArgKind::Value,
    value: "<auto|fixed>",
    required: false,
    default: None,
    choices: &["auto", "fixed"],
    summary: "Auto tries backend-declared point anchors with collision checks. Fixed restores the authored single anchor.",
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
    let size = inputs
        .value("size")
        .map(|raw| {
            raw.parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or_else(|| {
                    Failure::invalid("invalid_number", "label size must be finite")
                        .remedy("read style read .data.labelSchema for label numeric bounds")
                })
        })
        .transpose()?;
    let papers = inputs.repeated("paper");
    let mut numerics = serde_json::Map::new();
    for item in inputs.repeated("number") {
        let (key, value) = item.split_once('=').ok_or_else(|| {
            Failure::invalid("invalid_number", "label number needs property=value")
        })?;
        let value = value
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| Failure::invalid("invalid_number", "label number must be finite"))?;
        if key.is_empty() || numerics.insert(key.into(), json!(value)).is_some() {
            return Err(Failure::invalid(
                "invalid_number",
                "label numeric properties must be unique",
            ));
        }
    }
    let mut result = json!({
        "ref": inputs.require("ref")?,
        "field": raw,
        "apply": apply,
    });
    if inputs.value("visible").is_some()
        || !inputs.repeated("append-field").is_empty()
        || [
            "separator",
            "suffix",
            "format",
            "halo-color",
            "overlap",
            "alignment",
        ]
        .iter()
        .any(|key| inputs.value(key).is_some())
        || !numerics.is_empty()
        || size.is_some()
        || inputs.value("font").is_some()
        || inputs.value("color").is_some()
        || !papers.is_empty()
        || inputs.value("placement").is_some()
    {
        result["options"] = json!({
            "visible": inputs.value("visible").map(|v| v == "on"),
            "size": size,
            "font": inputs.value("font"),
            "color": inputs.value("color"),
            "papers": if papers.is_empty() { None } else { Some(papers) },
            "automatic_placement": inputs.value("placement").map(|v| v == "auto"),
        });
    }
    if !numerics.is_empty() {
        result["options"]["numerics"] = json!(numerics);
    }
    for key in ["separator", "suffix", "format", "alignment"] {
        if let Some(value) = inputs.value(key) {
            result["options"][key] = json!(value);
        }
    }
    if let Some(value) = inputs.value("halo-color") {
        result["options"]["halo_color"] = json!(value);
    }
    if let Some(value) = inputs.value("overlap") {
        result["options"]["allow_overlap"] = json!(value == "on");
    }
    if !inputs.repeated("append-field").is_empty() {
        result["options"]["append_fields"] = json!(inputs.repeated("append-field"));
    }

    Ok(result)
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
        contract: 3,
        summary: "Preview binding a label to one declared data field.",
        purpose: "Uses the shared Style Center planner to bind a label field and optionally set visibility, size, font, text color, paper scope and automatic point placement. Omitted options preserve existing authorship. A missing label starts from the backend label model; style read includes its live numeric bounds.",
        chapter: Chapter::MapPresentation,
        effect: Effect::LocalAuthState,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[
            REF_ARG,
            FIELD_ARG,
            VISIBLE_ARG,
            SIZE_ARG,
            Arg {
                name: "number",
                kind: ArgKind::Repeated,
                value: "<property=value>",
                required: false,
                default: None,
                choices: &[],
                summary: "Label numeric property from labelSchema.numerics; repeat for priority, offsets, spacing or halo. Backend bounds apply.",
            },
            Arg {
                name: "append-field",
                kind: ArgKind::Repeated,
                value: "<field>",
                required: false,
                default: None,
                choices: &[],
                summary: "Append a declared field to the label; repeat in display order.",
            },
            Arg {
                name: "separator",
                kind: ArgKind::Value,
                value: "<text>",
                required: false,
                default: None,
                choices: &[],
                summary: "Separator between composed label fields; default is a centred dot.",
            },
            Arg {
                name: "suffix",
                kind: ArgKind::Value,
                value: "<text>",
                required: false,
                default: None,
                choices: &[],
                summary: "Literal suffix, for example ' m' or ' kVA'.",
            },
            Arg {
                name: "format",
                kind: ArgKind::Value,
                value: "<format>",
                required: false,
                default: None,
                choices: &["raw", "round", "int"],
                summary: "Format the primary field using the published label vocabulary.",
            },
            Arg {
                name: "halo-color",
                kind: ArgKind::Value,
                value: "<#RRGGBB>",
                required: false,
                default: None,
                choices: &[],
                summary: "Text halo color; width is controlled with --number text-halo-width=value.",
            },
            Arg {
                name: "overlap",
                kind: ArgKind::Value,
                value: "<on|off>",
                required: false,
                default: None,
                choices: &["on", "off"],
                summary: "Allow required labels to overlap other map features; clipping at the map edge still applies.",
            },
            Arg {
                name: "alignment",
                kind: ArgKind::Value,
                value: "<placement>",
                required: false,
                default: None,
                choices: &["point", "line", "line-center"],
                summary: "Point placement or labels rotated along the line and kept upright.",
            },
            FONT_ARG,
            COLOR_ARG,
            PAPER_ARG,
            PLACEMENT_ARG,
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
        contract: 3,
        summary: "Publish a governed label bound to one declared data field.",
        purpose: "Publishes the exact document returned by `ds style label plan` through the Style Center save route. The shared Rust planner owns the field validation and label transformation.",
        chapter: Chapter::MapPresentation,
        effect: Effect::GlobalWrite,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[
            REF_ARG,
            FIELD_ARG,
            VISIBLE_ARG,
            SIZE_ARG,
            Arg {
                name: "number",
                kind: ArgKind::Repeated,
                value: "<property=value>",
                required: false,
                default: None,
                choices: &[],
                summary: "Label numeric property from labelSchema.numerics; repeat for priority, offsets, spacing or halo. Backend bounds apply.",
            },
            Arg {
                name: "append-field",
                kind: ArgKind::Repeated,
                value: "<field>",
                required: false,
                default: None,
                choices: &[],
                summary: "Append a declared field to the label; repeat in display order.",
            },
            Arg {
                name: "separator",
                kind: ArgKind::Value,
                value: "<text>",
                required: false,
                default: None,
                choices: &[],
                summary: "Separator between composed label fields; default is a centred dot.",
            },
            Arg {
                name: "suffix",
                kind: ArgKind::Value,
                value: "<text>",
                required: false,
                default: None,
                choices: &[],
                summary: "Literal suffix, for example ' m' or ' kVA'.",
            },
            Arg {
                name: "format",
                kind: ArgKind::Value,
                value: "<format>",
                required: false,
                default: None,
                choices: &["raw", "round", "int"],
                summary: "Format the primary field using the published label vocabulary.",
            },
            Arg {
                name: "halo-color",
                kind: ArgKind::Value,
                value: "<#RRGGBB>",
                required: false,
                default: None,
                choices: &[],
                summary: "Text halo color; width is controlled with --number text-halo-width=value.",
            },
            Arg {
                name: "overlap",
                kind: ArgKind::Value,
                value: "<on|off>",
                required: false,
                default: None,
                choices: &["on", "off"],
                summary: "Allow required labels to overlap other map features; clipping at the map edge still applies.",
            },
            Arg {
                name: "alignment",
                kind: ArgKind::Value,
                value: "<placement>",
                required: false,
                default: None,
                choices: &["point", "line", "line-center"],
                summary: "Point placement or labels rotated along the line and kept upright.",
            },
            FONT_ARG,
            COLOR_ARG,
            PAPER_ARG,
            PLACEMENT_ARG,
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

    #[test]
    fn paper_visibility_and_placement_are_typed_for_both_hosts() {
        let tokens = [
            "--ref",
            "master/lv_poles_print",
            "--field",
            "pole_number",
            "--visible",
            "on",
            "--size",
            "8",
            "--paper",
            "A0",
            "--placement",
            "auto",
        ]
        .map(str::to_string);
        let input = parse(&plan::COMMAND, &tokens).unwrap();
        let value = arguments(&input, false).unwrap();
        assert_eq!(
            value["options"],
            json!({"visible":true,"size":8.,"font":null,"color":null,"papers":["A0"],"automatic_placement":true})
        );
        let set_input = parse(&set::COMMAND, &tokens).unwrap();
        assert_eq!(
            arguments(&set_input, true).unwrap()["options"],
            value["options"]
        );
    }
    #[test]
    fn composed_label_and_numeric_controls_survive_the_cli_boundary() {
        let tokens = [
            "--ref",
            "master/tr_print",
            "--field",
            "transfo",
            "--append-field",
            "tr_size",
            "--suffix",
            " kVA",
            "--overlap",
            "on",
            "--number",
            "symbol-sort-key=-1000",
        ]
        .map(str::to_string);
        let input = parse(&set::COMMAND, &tokens).unwrap();
        let args = arguments(&input, true).unwrap();
        assert_eq!(args["options"]["append_fields"], json!(["tr_size"]));
        assert_eq!(args["options"]["suffix"], " kVA");
        assert_eq!(args["options"]["allow_overlap"], true);
        assert_eq!(args["options"]["numerics"]["symbol-sort-key"], -1000.);
    }
}
