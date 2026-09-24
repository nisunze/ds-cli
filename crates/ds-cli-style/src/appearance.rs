//! `ds style appearance plan | set` — guided colour, icon and base size.
//!
//! Both commands send the same typed Style Center operation. `plan` asks for
//! the exact resulting document without saving; `set` publishes that document
//! through the application's governed save path.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{LANE_ARG, PROJECT_ARG, REF_ARG};

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

const ICON_OVERLAP_ARG: Arg = Arg::value("icon-overlap", "<on|off>", "Symbol icon collision policy. On sets both icon-allow-overlap and icon-ignore-placement, so its own label cannot displace the icon; off restores collision placement. Changes only the addressed screen or print document.").choices(&["on", "off"]);

pub(crate) fn arguments(inputs: &Inputs, apply: bool) -> Result<Value, Failure> {
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
    let icon_overlap = inputs.value("icon-overlap").map(|value| value == "on");
    if colour.is_none() && icon.is_none() && size.is_none() && icon_overlap.is_none() {
        return Err(Failure::invalid(
            "invalid_appearance",
            "no base appearance change was requested",
        )
        .remedy("pass at least one of --color, --icon, --size or --icon-overlap"));
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
    if let Some(overlap) = icon_overlap {
        arguments.insert("icon_overlap".into(), json!(overlap));
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
    if let Some(value) = requested["icon_overlap"].as_bool() {
        changes.push(format!("icon overlap {}", if value { "on" } else { "off" }));
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
        contract: 3,
        summary: "Plan a layer's flat colour, icon and base size; publishes nothing.",
        purpose: "\
Uses the Style Center's guided property schema and returns the exact document \
that colour, icon and base-size instructions would produce. A base size updates \
the fallback when size already carries the second dimension. Icon overlap changes both placement flags together without changing text overlap. Screen and print refs remain independent. Nothing is saved.",
        chapter: Chapter::MapPresentation,
        effect: Effect::LocalAuthState,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[
            PROJECT_ARG,
            REF_ARG,
            COLOR_ARG,
            ICON_ARG,
            SIZE_ARG,
            ICON_OVERLAP_ARG,
            LANE_ARG,
        ],
        output: "`requested`, the resolved guided `appearance`, whether base size updated an existing fallback, `dryRun: true`, `published: false`, and the exact `document`.",
        examples: &[Example {
            command: "ds style appearance plan --project <id> --ref gt/secondary_schools --color #008695 --icon school --size 1.2 --output json",
            note: "Plans a teal school icon without changing a second halo/opacity dimension.",
            runnable: false,
        }],
        refusals: crate::native::REFUSALS,
        reference: Some("docs/reference/style.md"),
        search: &[],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    };

    pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        crate::native::edit(
            inputs,
            crate::native::Edit::Appearance,
            arguments(inputs, false)?,
        )
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
        contract: 3,
        summary: "Publish a layer's flat colour, icon or base size natively.",
        purpose: "\
Applies the same guided colour, icon and size properties the Style Center owns, \
then publishes through its governed global save. Only supplied properties move. \
Flat colour or icon replaces a field-driven primary expression; plan first. Icon overlap changes both placement flags together without changing text overlap or another screen/print document.",
        chapter: Chapter::MapPresentation,
        effect: Effect::GlobalWrite,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[
            PROJECT_ARG,
            REF_ARG,
            COLOR_ARG,
            ICON_ARG,
            SIZE_ARG,
            ICON_OVERLAP_ARG,
            LANE_ARG,
        ],
        output: "The plan receipt with `published: true`, ds-brain `warnings`, and the exact persisted `document`.",
        examples: &[Example {
            command: "ds style appearance set --project <id> --ref gt/secondary_schools --color #008695 --icon school --size 1.2 --yes",
            note: "Publishes one governed base appearance; use `style dimension set` separately for a second field.",
            runnable: false,
        }],
        refusals: crate::native::PUBLISH_REFUSALS,
        reference: Some("docs/reference/style.md"),
        search: &[],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    };

    pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        crate::native::edit(
            inputs,
            crate::native::Edit::Appearance,
            arguments(inputs, true)?,
        )
    }

    pub fn render(data: &Value) -> String {
        render_appearance(data)
    }
}

#[cfg(test)]
mod tests {
    /// Every style call names its project; the saved selection is never read.
    fn parse(
        command: &ds_cli_contract::spec::Command,
        tokens: &[String],
    ) -> Result<ds_cli_contract::Inputs, ds_cli_contract::outcome::Failure> {
        let mut all = vec!["--project".to_owned(), "test-project".to_owned()];
        all.extend_from_slice(tokens);
        ds_cli_contract::parse(command, &all)
    }

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

#[cfg(test)]
mod overlap_tests {
    use super::*;
    /// Every style call names its project; the saved selection is never read.
    fn parse(
        command: &ds_cli_contract::spec::Command,
        tokens: &[String],
    ) -> Result<ds_cli_contract::Inputs, ds_cli_contract::outcome::Failure> {
        let mut all = vec!["--project".to_owned(), "test-project".to_owned()];
        all.extend_from_slice(tokens);
        ds_cli_contract::parse(command, &all)
    }
    #[test]
    fn overlap_only_is_a_typed_change_not_a_blank_appearance() {
        for (flag, value) in [("on", true), ("off", false)] {
            let tokens =
                ["--ref", "gt/primary_schools_print", "--icon-overlap", flag].map(str::to_owned);
            let inputs = parse(&plan::COMMAND, &tokens).unwrap();
            assert_eq!(
                arguments(&inputs, false).unwrap(),
                json!({"ref":"gt/primary_schools_print","icon_overlap":value,"apply":false})
            );
        }
        let tokens = ["--ref", "gt/primary_schools", "--icon-overlap", "maybe"].map(str::to_owned);
        assert!(parse(&plan::COMMAND, &tokens).is_err());
    }
}
