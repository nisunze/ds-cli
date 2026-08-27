//! `ds style dimension plan | set | clear` — the second categorical dimension.
//!
//! `plan` and `set` send the same operation with `apply` false or true, so the
//! expressions a caller reviews are the expressions that get published. Both
//! run through the application's own pure module, which enforces what keeps a
//! document renderable: one label type per match, no arm-less match, the
//! channel's property pair for this layer type, and labels typed the way the
//! map carries the field.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, REF_ARG};

const FIELD_ARG: Arg = Arg {
    name: "field",
    kind: ArgKind::Value,
    value: "<field>",
    required: true,
    default: None,
    choices: &[],
    summary: "The second field, from `ds style read` .data.fields. Not the colour field.",
};
const CHANNEL_ARG: Arg = Arg {
    name: "channel",
    kind: ArgKind::Value,
    value: "<channel>",
    required: false,
    default: Some("halo"),
    choices: &["halo", "opacity", "size"],
    summary: "How the field is told apart: a stroke/halo ring (line: casing, fill: outline), the opacity, or the size.",
};
const VALUE_ARG: Arg = Arg {
    name: "value",
    kind: ArgKind::Repeated,
    value: "<value>[=<amount>[:<#hex>]]",
    required: true,
    default: None,
    choices: &[],
    summary: "One value of the field and its amount (ring px, opacity 0-1, or size) and ring colour. Repeat per value; an omitted amount uses the channel's muted preset.",
};
const OTHER_ARG: Arg = Arg {
    name: "other",
    kind: ArgKind::Value,
    value: "<amount>",
    required: false,
    default: None,
    choices: &[],
    summary: "Amount for every value not named (default: the channel's fallback, e.g. no ring).",
};
const COLOR_ARG: Arg = Arg {
    name: "color",
    kind: ArgKind::Value,
    value: "<#hex>",
    required: false,
    default: None,
    choices: &[],
    summary: "Ring colour for values without their own (halo channel; default #FFFFFF).",
};

fn arguments(inputs: &Inputs, apply: bool) -> Result<Value, Failure> {
    let values = inputs.repeated("value");
    if values.len() > crate::MAX_VALUES {
        return Err(Failure::invalid(
            "invalid_value_spec",
            format!("at most {} --value flags may be given", crate::MAX_VALUES),
        )
        .remedy("route the long tail through --other instead of naming every value"));
    }
    let mut specs = Vec::with_capacity(values.len());
    for raw in values {
        specs.push(crate::value_spec(raw)?);
    }
    let mut arguments = Map::new();
    arguments.insert("ref".into(), json!(inputs.require("ref")?));
    arguments.insert("field".into(), json!(inputs.require("field")?));
    arguments.insert(
        "channel".into(),
        json!(inputs.value("channel").unwrap_or("halo")),
    );
    arguments.insert("values".into(), Value::Array(specs));
    if let Some(other) = inputs.value("other") {
        let parsed: f64 = other.trim().parse().map_err(|_| {
            Failure::invalid("invalid_number", "`--other` must be a number")
                .remedy("pass e.g. --other 0")
                .detail(json!({ "given": other }))
        })?;
        arguments.insert("other".into(), json!(parsed));
    }
    if let Some(colour) = inputs.value("color") {
        arguments.insert("color".into(), json!(crate::color(colour, "color")?));
    }
    arguments.insert("apply".into(), json!(apply));
    Ok(Value::Object(arguments))
}

fn render_dimension(data: &Value) -> String {
    let mut out = format!(
        "{} · {} · {}\n",
        data["ref"].as_str().unwrap_or("?"),
        data["second"].as_str().unwrap_or("?"),
        if data["published"].as_bool().unwrap_or(false) {
            "published"
        } else {
            "plan only — nothing published"
        },
    );
    if let Some(expressions) = data["expressions"].as_object() {
        for (property, expression) in expressions {
            out.push_str(&format!("  {property}: {expression}\n"));
        }
    }
    if let Some(on_map) = data["onMap"].as_object() {
        let counts: Vec<String> = on_map["counts"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(value, n)| format!("{value}={n}"))
            .collect();
        if !counts.is_empty() {
            out.push_str(&format!(
                "  on map: {}\n",
                crate::truncate(&counts.join(", "), 110)
            ));
        }
    }
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
        id: "style.dimension.plan",
        path: &["style", "dimension", "plan"],
        contract: 1,
        summary: "The expressions a second dimension would write; publishes nothing.",
        purpose: "\
Runs the exact authoring the Style Center's Second-dimension panel performs \
and returns the match expressions and the whole resulting document, without \
saving. Read it, then `ds style dimension set` with the same flags.",
        chapter: Chapter::MapPresentation,
        effect: Effect::ReadOnly,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[
            REF_ARG,
            FIELD_ARG,
            CHANNEL_ARG,
            VALUE_ARG,
            OTHER_ARG,
            COLOR_ARG,
            DESCRIPTOR_ARG,
        ],
        output: "\
`ref`, `field`, `fieldType` (as the map carries it, or null), `channel`, \
`expressions` (property → match expression), `second`, `onMap` counts per \
value, `dryRun: true`, `published: false` and the full `document`.",
        examples: &[Example {
            command: "ds style dimension plan --ref master/lv_poles --field drafting_status --channel halo --value draft=3:#FFFFFF --other 0 --output json",
            note: "Draft poles in the bare Design GeoJSON layer get a 3px white ring; every other value gets none.",
            runnable: false,
        }],
        refusals: &[
            crate::NOT_PAIRED,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            crate::STYLE_REFUSED,
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            crate::SIGNED_OUT,
            crate::INVALID_VALUE_SPEC,
            crate::INVALID_COLOR,
            crate::INVALID_NUMBER,
        ],
        reference: Some("docs/reference/style.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let arguments = arguments(inputs, false)?;
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::DIMENSION_SET,
            arguments,
            crate::READ_TIMEOUT,
        )
        .map_err(crate::classify_style_failure)
    }

    pub fn render(data: &Value) -> String {
        render_dimension(data)
    }
}

pub mod set {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "style.dimension.set",
        path: &["style", "dimension", "set"],
        contract: 1,
        summary: "Publish a halo, opacity or size dimension by a second field.",
        purpose: "\
Authors the same expressions `ds style dimension plan` shows and publishes \
the document through the application's own governed save: ds-brain \
validates it, the local map renders it, the legend reads it. Any other \
second dimension on the ref is replaced; the colour dimension is untouched.",
        chapter: Chapter::MapPresentation,
        effect: Effect::GlobalWrite,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[
            REF_ARG,
            FIELD_ARG,
            CHANNEL_ARG,
            VALUE_ARG,
            OTHER_ARG,
            COLOR_ARG,
            DESCRIPTOR_ARG,
        ],
        output: "\
The plan receipt with `published: true`, ds-brain `warnings`, and the \
`document` as persisted.",
        examples: &[Example {
            command: "ds style dimension set --ref master/lv_poles --field drafting_status --channel halo --value draft=3:#FFFFFF --other 0 --yes",
            note: "Symbol layers: the ring is baked into the raster icon by ds-brain.",
            runnable: false,
        }],
        refusals: &[
            crate::NOT_PAIRED,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            crate::STYLE_REFUSED,
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            crate::SIGNED_OUT,
            crate::CONFIRMATION_REQUIRED,
            crate::INVALID_VALUE_SPEC,
            crate::INVALID_COLOR,
            crate::INVALID_NUMBER,
        ],
        reference: Some("docs/reference/style.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let arguments = arguments(inputs, true)?;
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::DIMENSION_SET,
            arguments,
            crate::WRITE_TIMEOUT,
        )
        .map_err(crate::classify_style_failure)
    }

    pub fn render(data: &Value) -> String {
        render_dimension(data)
    }
}

pub mod clear {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "style.dimension.clear",
        path: &["style", "dimension", "clear"],
        contract: 1,
        summary: "Remove the second dimension and publish the document.",
        purpose: "\
Deletes the channel properties the second dimension wrote so the schema \
defaults apply again, and publishes. The colour dimension is untouched.",
        chapter: Chapter::MapPresentation,
        effect: Effect::GlobalWrite,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[REF_ARG, DESCRIPTOR_ARG],
        output: "`ref`, `cleared` (what was removed), `properties`, `published: true`, `warnings`, `document`.",
        examples: &[Example {
            command: "ds style dimension clear --ref master/lv_poles --yes",
            note: "Refused with desktop_refused when no second dimension is authored.",
            runnable: false,
        }],
        refusals: &[
            crate::NOT_PAIRED,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            crate::STYLE_REFUSED,
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            crate::SIGNED_OUT,
            crate::CONFIRMATION_REQUIRED,
        ],
        reference: Some("docs/reference/style.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::DIMENSION_CLEAR,
            json!({ "ref": inputs.require("ref")?, "apply": true }),
            crate::WRITE_TIMEOUT,
        )
        .map_err(crate::classify_style_failure)
    }

    pub fn render(data: &Value) -> String {
        format!(
            "{} · cleared {} ({})\n",
            data["ref"].as_str().unwrap_or("?"),
            data["cleared"].as_str().unwrap_or("?"),
            data["properties"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}
