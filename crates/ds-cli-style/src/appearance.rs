//! `ds style appearance plan | set` — guided colour, icon and base size.
//!
//! Both commands send the same typed Style Center operation. `plan` asks for
//! the exact resulting document without saving; `set` publishes that document
//! through the application's governed save path.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, REF_ARG};

const COLOR_ARG: Arg = Arg {
    name: "color",
    kind: ArgKind::Value,
    value: "<#hex>",
    required: false,
    default: None,
    choices: &[],
    summary: "Flat primary colour. Replaces a field-driven primary colour expression; plan first.",
};
const ICON_ARG: Arg = Arg {
    name: "icon",
    kind: ArgKind::Value,
    value: "<catalog-icon>",
    required: false,
    default: None,
    choices: &[],
    summary: "Flat symbol icon from `ds style read` .data.appearance.icon quick picks or the live Style Center catalog.",
};
const SIZE_ARG: Arg = Arg {
    name: "size",
    kind: ArgKind::Value,
    value: "<number>",
    required: false,
    default: None,
    choices: &[],
    summary: "Base circle radius, line width or symbol scale. Live Style Center bounds are returned by `ds style read`.",
};

fn arguments(inputs: &Inputs, apply: bool) -> Result<Value, Failure> {
    let colour = inputs
        .value("color")
        .map(|value| crate::color(value, "color"))
        .transpose()?;
    let icon = inputs
        .value("icon")
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let size = inputs
        .value("size")
        .map(|raw| {
            raw.trim().parse::<f64>().map_err(|_| {
                Failure::invalid("invalid_number", "`--size` must be a finite number")
                    .remedy("read .data.appearance.size from `ds style read`, then pass a number inside its min/max")
                    .detail(json!({ "given": raw }))
            })
        })
        .transpose()?;
    if size.is_some_and(|value| !value.is_finite()) {
        return Err(Failure::invalid(
            "invalid_number",
            "`--size` must be a finite number",
        )
        .remedy("read .data.appearance.size from `ds style read`, then pass a number inside its min/max"));
    }
    if colour.is_none() && icon.is_none() && size.is_none() {
        return Err(Failure::invalid(
            "invalid_appearance",
            "no base appearance change was requested",
        )
        .remedy("pass at least one of --color, --icon or --size"));
    }

    let mut arguments = Map::new();
    arguments.insert("ref".into(), json!(inputs.require("ref")?));
    if let Some(colour) = colour {
        arguments.insert("color".into(), json!(colour));
    }
    if let Some(icon) = icon {
        arguments.insert("icon".into(), json!(icon));
    }
    if let Some(size) = size {
        arguments.insert("size".into(), json!(size));
    }
    arguments.insert("apply".into(), json!(apply));
    Ok(Value::Object(arguments))
}

fn render_appearance(data: &Value) -> String {
    let requested = &data["requested"];
    let mut changes = Vec::new();
    if let Some(value) = requested["color"].as_str() {
        changes.push(format!("colour {value}"));
    }
    if let Some(value) = requested["icon"].as_str() {
        changes.push(format!("icon {value}"));
    }
    if let Some(value) = requested["size"].as_f64() {
        changes.push(format!("size {value}"));
    }
    format!(
        "{} · {} · {}\n",
        data["ref"].as_str().unwrap_or("?"),
        changes.join(" · "),
        if data["published"].as_bool().unwrap_or(false) {
            "published"
        } else {
            "plan only — nothing published"
        }
    )
}

pub mod plan {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "style.appearance.plan",
        path: &["style", "appearance", "plan"],
        contract: 1,
        summary: "Plan a layer's flat colour, icon and base size; publishes nothing.",
        purpose: "\
Uses the Style Center's guided property schema and returns the exact document \
that colour, icon and base-size instructions would produce. A base size updates \
the fallback when size already carries the second dimension. Nothing is saved.",
        chapter: Chapter::MapPresentation,
        effect: Effect::ReadOnly,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[REF_ARG, COLOR_ARG, ICON_ARG, SIZE_ARG, DESCRIPTOR_ARG],
        output: "`requested`, the resolved guided `appearance`, whether base size updated an existing fallback, `dryRun: true`, `published: false`, and the exact `document`.",
        examples: &[Example {
            command: "ds style appearance plan --ref gt/secondary_schools --color #008695 --icon school --size 1.2 --output json",
            note: "Plans a teal school icon without changing a second halo/opacity dimension.",
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
            crate::INVALID_APPEARANCE,
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
            &crate::APPEARANCE_SET,
            arguments,
            crate::READ_TIMEOUT,
        )
        .map_err(crate::classify_style_failure)
    }

    pub fn render(data: &Value) -> String {
        render_appearance(data)
    }
}

pub mod set {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "style.appearance.set",
        path: &["style", "appearance", "set"],
        contract: 1,
        summary: "Publish a layer's flat colour, icon or base size through Style Center.",
        purpose: "\
Applies the same guided colour, icon and size properties the Style Center owns, \
then publishes through its governed global save. Only supplied properties move. \
Flat colour or icon replaces a field-driven primary expression; plan first.",
        chapter: Chapter::MapPresentation,
        effect: Effect::GlobalWrite,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[REF_ARG, COLOR_ARG, ICON_ARG, SIZE_ARG, DESCRIPTOR_ARG],
        output: "The plan receipt with `published: true`, ds-brain `warnings`, and the exact persisted `document`.",
        examples: &[Example {
            command: "ds style appearance set --ref gt/secondary_schools --color #008695 --icon school --size 1.2 --yes",
            note: "Publishes one governed base appearance; use `style dimension set` separately for a second field.",
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
            crate::INVALID_APPEARANCE,
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
            &crate::APPEARANCE_SET,
            arguments,
            crate::WRITE_TIMEOUT,
        )
        .map_err(crate::classify_style_failure)
    }

    pub fn render(data: &Value) -> String {
        render_appearance(data)
    }
}

#[cfg(test)]
mod tests {
    use ds_cli_contract::parse;

    use super::*;

    #[test]
    fn appearance_arguments_are_typed_and_plan_set_differ_only_by_apply() {
        let tokens = [
            "--ref",
            "gt/secondary_schools",
            "--color",
            "#008695",
            "--icon",
            "school",
            "--size",
            "1.2",
        ]
        .map(str::to_string);
        let plan_inputs = parse(&plan::COMMAND, &tokens).expect("plan inputs");
        let set_inputs = parse(&set::COMMAND, &tokens).expect("set inputs");
        assert_eq!(
            arguments(&plan_inputs, false).expect("plan"),
            json!({
                "ref": "gt/secondary_schools", "color": "#008695", "icon": "school",
                "size": 1.2, "apply": false,
            })
        );
        assert_eq!(
            arguments(&set_inputs, true).expect("set"),
            json!({
                "ref": "gt/secondary_schools", "color": "#008695", "icon": "school",
                "size": 1.2, "apply": true,
            })
        );
    }

    #[test]
    fn appearance_requires_a_change_and_a_finite_size() {
        let empty = ["--ref", "gt/secondary_schools"].map(str::to_string);
        let inputs = parse(&plan::COMMAND, &empty).expect("inputs");
        assert_eq!(
            arguments(&inputs, false).expect_err("must refuse").code(),
            "invalid_appearance"
        );

        let invalid = ["--ref", "gt/secondary_schools", "--size", "NaN"].map(str::to_string);
        let inputs = parse(&plan::COMMAND, &invalid).expect("inputs");
        assert_eq!(
            arguments(&inputs, false).expect_err("must refuse").code(),
            "invalid_number"
        );
    }
}
