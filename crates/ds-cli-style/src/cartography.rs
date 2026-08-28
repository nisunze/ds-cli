//! `ds style cartography plan | set` — line type, direction, casing, hatching.
//!
//! The third thing a Style Center document carries, after the flat appearance
//! and the second field-driven dimension: how a line or a fill *reads as a
//! map*. A dashed versus solid line, arrows that show which way water flows, a
//! dark casing that keeps a thin line legible over satellite imagery, and a
//! hatch that says "proposed" without spending the colour dimension.
//!
//! Both commands send the same typed operation with `apply` false or true, so
//! the document a caller reviews is the document that gets published. Every
//! property is a named cartographic instruction — never raw paint JSON, and
//! never a dash array or a pattern image composed here. The application owns
//! the dash vocabulary ds-brain publishes and the pattern tile it rasterises.
//!
//! An omitted flag leaves that part of the document alone, which is why the
//! only locally decidable errors are: asking for nothing, and asking for a
//! detail that contradicts the line type or fill pattern set in the same call.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, REF_ARG};

/// The one line type that draws markers instead of a dash pattern.
const DIRECTIONAL: &str = "directional";
/// The one fill pattern that means "no hatch", for both flags.
const UNPATTERNED: &str = "solid";

const LINE_TYPE_ARG: Arg = Arg {
    name: "line-type",
    kind: ArgKind::Value,
    value: "<line-type>",
    required: false,
    default: None,
    choices: &[
        "solid",
        "dashed",
        "dotted",
        "dash-dot",
        "long-dash",
        "dash-dot-dot",
        "directional",
    ],
    summary: "Line preset. `solid` clears dashes; `directional` draws flow arrows.",
};
const DIRECTION_SIZE_ARG: Arg = Arg {
    name: "direction-size",
    kind: ArgKind::Value,
    value: "<px>",
    required: false,
    default: None,
    choices: &[],
    summary: "Flow-arrow size in px, 6..48; directional lines only.",
};
const DIRECTION_SPACING_ARG: Arg = Arg {
    name: "direction-spacing",
    kind: ArgKind::Value,
    value: "<px>",
    required: false,
    default: None,
    choices: &[],
    summary: "Flow-arrow spacing in px, 20..1000; directional lines only.",
};
const CASING_COLOR_ARG: Arg = Arg {
    name: "casing-color",
    kind: ArgKind::Value,
    value: "<#hex>",
    required: false,
    default: None,
    choices: &[],
    summary: "Contrast casing colour, e.g. #0F172A over satellite imagery.",
};
const CASING_WIDTH_ARG: Arg = Arg {
    name: "casing-width",
    kind: ArgKind::Value,
    value: "<px>",
    required: false,
    default: None,
    choices: &[],
    summary: "Casing width in px, 0..20; 0 removes it. Halves are allowed.",
};
const FILL_PATTERN_ARG: Arg = Arg {
    name: "fill-pattern",
    kind: ArgKind::Value,
    value: "<fill-pattern>",
    required: false,
    default: None,
    choices: &[
        "solid",
        "diagonal-forward",
        "diagonal-back",
        "crosshatch",
        "dots",
    ],
    summary: "Fill hatch preset; `solid` clears the hatch.",
};
const PATTERN_COLOR_ARG: Arg = Arg {
    name: "pattern-color",
    kind: ArgKind::Value,
    value: "<#hex>",
    required: false,
    default: None,
    choices: &[],
    summary: "Colour of the hatch strokes or dots.",
};
const PATTERN_BACKGROUND_ARG: Arg = Arg {
    name: "pattern-background",
    kind: ArgKind::Value,
    value: "<#hex>",
    required: false,
    default: None,
    choices: &[],
    summary: "Hatch background; use #FFFFFF00 for transparency.",
};
const PATTERN_SPACING_ARG: Arg = Arg {
    name: "pattern-spacing",
    kind: ArgKind::Value,
    value: "<px>",
    required: false,
    default: None,
    choices: &["4", "8", "16", "32"],
    summary: "Pattern tile size in px: 4, 8, 16 or 32.",
};
const PATTERN_STROKE_ARG: Arg = Arg {
    name: "pattern-stroke",
    kind: ArgKind::Value,
    value: "<px>",
    required: false,
    default: None,
    choices: &[],
    summary: "Hatch stroke or dot width in whole px, 1..6.",
};

/// The declared inputs, in the order help prints them. One list, used by both
/// commands, so the two cannot drift into accepting different flags.
const ARGS: &[Arg] = &[
    REF_ARG,
    LINE_TYPE_ARG,
    DIRECTION_SIZE_ARG,
    DIRECTION_SPACING_ARG,
    CASING_COLOR_ARG,
    CASING_WIDTH_ARG,
    FILL_PATTERN_ARG,
    PATTERN_COLOR_ARG,
    PATTERN_BACKGROUND_ARG,
    PATTERN_SPACING_ARG,
    PATTERN_STROKE_ARG,
    DESCRIPTOR_ARG,
];

const PLAN_REFUSALS: &[ds_cli_contract::spec::Refusal] = &[
    crate::NOT_PAIRED,
    crate::AMBIGUOUS,
    crate::UNREACHABLE,
    crate::PAIRING_REJECTED,
    crate::STYLE_REFUSED,
    crate::UNSUPPORTED,
    crate::UNREADABLE,
    crate::SIGNED_OUT,
    crate::INVALID_CARTOGRAPHY,
    crate::INVALID_COLOR,
    crate::INVALID_NUMBER,
];

const SET_REFUSALS: &[ds_cli_contract::spec::Refusal] = &[
    crate::NOT_PAIRED,
    crate::AMBIGUOUS,
    crate::UNREACHABLE,
    crate::PAIRING_REJECTED,
    crate::STYLE_REFUSED,
    crate::UNSUPPORTED,
    crate::UNREADABLE,
    crate::SIGNED_OUT,
    crate::CONFIRMATION_REQUIRED,
    crate::INVALID_CARTOGRAPHY,
    crate::INVALID_COLOR,
    crate::INVALID_NUMBER,
];

/// A bounded finite number, for the one property that is genuinely fractional.
///
/// Casing width is a MapLibre line width, and half-pixel casings are ordinary
/// practice for a thin line over imagery. Everything else here is a whole
/// number of pixels because it ends up in a rasterised tile or a marker count.
fn bounded(raw: &str, flag: &str, min: f64, max: f64) -> Result<f64, Failure> {
    let parsed: f64 = raw.trim().parse().map_err(|_| {
        Failure::invalid("invalid_number", format!("`--{flag}` must be a number"))
            .remedy(format!("pass {min}..{max}"))
            .detail(json!({ "given": raw }))
    })?;
    if !parsed.is_finite() || parsed < min || parsed > max {
        return Err(
            Failure::invalid("invalid_number", format!("`--{flag}` is outside its bound"))
                .remedy(format!("pass {min}..{max}"))
                .detail(json!({ "given": raw, "min": min, "max": max })),
        );
    }
    Ok(parsed)
}

fn refuse(why: &str) -> Failure {
    Failure::invalid("invalid_cartography", why.to_string())
        .remedy(crate::INVALID_CARTOGRAPHY.remedy)
}

fn arguments(inputs: &Inputs, apply: bool) -> Result<Value, Failure> {
    let line_type = inputs
        .value("line-type")
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let direction_size = inputs
        .value("direction-size")
        .map(|raw| crate::integer(raw, "direction-size", 6, 48))
        .transpose()?;
    let direction_spacing = inputs
        .value("direction-spacing")
        .map(|raw| crate::integer(raw, "direction-spacing", 20, 1_000))
        .transpose()?;
    let casing_color = inputs
        .value("casing-color")
        .map(|raw| crate::color(raw, "casing-color"))
        .transpose()?;
    let casing_width = inputs
        .value("casing-width")
        .map(|raw| bounded(raw, "casing-width", 0.0, 20.0))
        .transpose()?;
    let fill_pattern = inputs
        .value("fill-pattern")
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let pattern_color = inputs
        .value("pattern-color")
        .map(|raw| crate::color(raw, "pattern-color"))
        .transpose()?;
    let pattern_background = inputs
        .value("pattern-background")
        .map(|raw| crate::color(raw, "pattern-background"))
        .transpose()?;
    let pattern_spacing = inputs
        .value("pattern-spacing")
        .map(|raw| crate::integer(raw, "pattern-spacing", 4, 32))
        .transpose()?;
    let pattern_stroke = inputs
        .value("pattern-stroke")
        .map(|raw| crate::integer(raw, "pattern-stroke", 1, 6))
        .transpose()?;

    // Direction detail only means something for the directional line type. A
    // call that names another type in the same breath cannot be right whatever
    // the document currently says, so it is refused here rather than by the
    // application.
    if let Some(line_type) = line_type
        && line_type != DIRECTIONAL
        && (direction_size.is_some() || direction_spacing.is_some())
    {
        return Err(refuse(&format!(
            "`--direction-size`/`--direction-spacing` need the directional line type, but this call sets `--line-type {line_type}`"
        )));
    }
    // The same reasoning for a hatch: `solid` is the instruction that removes
    // one, so detail for a pattern that is being removed is a contradiction.
    let pattern_detail = pattern_color.is_some()
        || pattern_background.is_some()
        || pattern_spacing.is_some()
        || pattern_stroke.is_some();
    if fill_pattern == Some(UNPATTERNED) && pattern_detail {
        return Err(refuse(
            "`--pattern-*` detail cannot be sent with `--fill-pattern solid`, which removes the hatch",
        ));
    }

    let mut arguments = Map::new();
    arguments.insert("ref".into(), json!(inputs.require("ref")?));
    if let Some(value) = line_type {
        arguments.insert("lineType".into(), json!(value));
    }
    if let Some(value) = direction_size {
        arguments.insert("directionSize".into(), json!(value));
    }
    if let Some(value) = direction_spacing {
        arguments.insert("directionSpacing".into(), json!(value));
    }
    if let Some(value) = casing_color {
        arguments.insert("casingColor".into(), json!(value));
    }
    if let Some(value) = casing_width {
        arguments.insert("casingWidth".into(), json!(value));
    }
    if let Some(value) = fill_pattern {
        arguments.insert("fillPattern".into(), json!(value));
    }
    if let Some(value) = pattern_color {
        arguments.insert("patternColor".into(), json!(value));
    }
    if let Some(value) = pattern_background {
        arguments.insert("patternBackground".into(), json!(value));
    }
    if let Some(value) = pattern_spacing {
        arguments.insert("patternSpacing".into(), json!(value));
    }
    if let Some(value) = pattern_stroke {
        arguments.insert("patternStroke".into(), json!(value));
    }

    // `ref` is the subject, not a change; `apply` is not yet inserted.
    if arguments.len() == 1 {
        return Err(refuse("no cartography change was requested"));
    }

    arguments.insert("apply".into(), json!(apply));
    Ok(Value::Object(arguments))
}

/// One human line per change, read back from what was actually sent.
fn render_cartography(data: &Value) -> String {
    let requested = &data["requested"];
    let mut changes = Vec::new();
    if let Some(value) = requested["lineType"].as_str() {
        changes.push(format!("line {value}"));
    }
    if let Some(value) = requested["directionSize"].as_i64() {
        changes.push(format!("arrow {value}px"));
    }
    if let Some(value) = requested["directionSpacing"].as_i64() {
        changes.push(format!("every {value}px"));
    }
    let casing = match (
        requested["casingWidth"].as_f64(),
        requested["casingColor"].as_str(),
    ) {
        (Some(width), Some(color)) => Some(format!("casing {width}px {color}")),
        (Some(width), None) => Some(format!("casing {width}px")),
        (None, Some(color)) => Some(format!("casing {color}")),
        (None, None) => None,
    };
    changes.extend(casing);
    if let Some(value) = requested["fillPattern"].as_str() {
        changes.push(format!("fill {value}"));
    }
    if let Some(value) = requested["patternColor"].as_str() {
        changes.push(format!("hatch {value}"));
    }
    if let Some(value) = requested["patternBackground"].as_str() {
        changes.push(format!("on {value}"));
    }
    if let Some(value) = requested["patternSpacing"].as_i64() {
        changes.push(format!("tile {value}px"));
    }
    if let Some(value) = requested["patternStroke"].as_i64() {
        changes.push(format!("stroke {value}px"));
    }

    let mut out = format!(
        "{} · {} · {}\n",
        data["ref"].as_str().unwrap_or("?"),
        changes.join(" · "),
        if data["published"].as_bool().unwrap_or(false) {
            "published"
        } else {
            "plan only — nothing published"
        },
    );
    let properties: Vec<&str> = data["properties"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !properties.is_empty() {
        out.push_str(&format!(
            "  properties: {}\n",
            crate::truncate(&properties.join(", "), 110)
        ));
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
        id: "style.cartography.plan",
        path: &["style", "cartography", "plan"],
        contract: 1,
        summary: "Plan dashed line types, casing, direction or fill hatching.",
        purpose: "\
Plans governed line presets, direction arrows, contrast casing and fill \
hatching. Crosshatch can mark proposed service areas. It returns the resulting \
document without saving. Omitted flags leave those properties unchanged; \
apply the reviewed flags with `set`.",
        chapter: Chapter::MapPresentation,
        effect: Effect::ReadOnly,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: ARGS,
        output: "\
`ref`, `layerType`, `requested` (exactly what was sent), the resolved \
`cartography` (dash preset or arrow marker, casing, pattern tile), the \
`properties` written, `dryRun: true`, `published: false` and the full \
`document`.",
        examples: &[
            Example {
                command: "ds style cartography plan --ref master/water_mains --line-type directional --direction-size 14 --direction-spacing 140 --output json",
                note: "Show water-flow direction without changing colour.",
                runnable: false,
            },
            Example {
                command: "ds style cartography plan --ref master/mv_lines --casing-color '#0F172A' --casing-width 2 --output json",
                note: "Keep a bright line legible over satellite imagery.",
                runnable: false,
            },
            Example {
                command: "ds style cartography plan --ref master/service_areas --fill-pattern crosshatch --pattern-color '#B45309' --pattern-background '#FFFFFF00' --pattern-spacing 8 --pattern-stroke 1 --output json",
                note: "Crosshatch a proposed area over a transparent background.",
                runnable: false,
            },
        ],
        refusals: PLAN_REFUSALS,
        reference: Some("docs/reference/style.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let arguments = arguments(inputs, false)?;
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::CARTOGRAPHY_SET,
            arguments,
            crate::READ_TIMEOUT,
        )
        .map_err(crate::classify_style_failure)
    }

    pub fn render(data: &Value) -> String {
        render_cartography(data)
    }
}

pub mod set {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "style.cartography.set",
        path: &["style", "cartography", "set"],
        contract: 1,
        summary: "Publish a layer's line, direction, casing or fill hatch.",
        purpose: "\
Publishes the cartography shown by `plan` through the governed save. ds-brain \
validates it and the map renders it, including contrast casing over satellite \
imagery. Colour and field-driven dimensions stay unchanged; omitted flags \
leave their properties unchanged.",
        chapter: Chapter::MapPresentation,
        effect: Effect::GlobalWrite,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: ARGS,
        output: "The plan receipt with `published: true`, ds-brain `warnings`, and the `document` as persisted.",
        examples: &[
            Example {
                command: "ds style cartography set --ref master/water_mains --line-type directional --direction-size 14 --direction-spacing 140 --yes",
                note: "Publish water-flow markers instead of a dash preset.",
                runnable: false,
            },
            Example {
                command: "ds style cartography set --ref master/mv_lines --casing-color '#0F172A' --casing-width 2 --yes",
                note: "Add contrast casing; width 0 removes it.",
                runnable: false,
            },
            Example {
                command: "ds style cartography set --ref master/service_areas --fill-pattern crosshatch --pattern-color '#B45309' --pattern-spacing 8 --yes",
                note: "Crosshatch a proposed service area with a governed tile size.",
                runnable: false,
            },
        ],
        refusals: SET_REFUSALS,
        reference: Some("docs/reference/style.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let arguments = arguments(inputs, true)?;
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::CARTOGRAPHY_SET,
            arguments,
            crate::WRITE_TIMEOUT,
        )
        .map_err(crate::classify_style_failure)
    }

    pub fn render(data: &Value) -> String {
        render_cartography(data)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ds_cli_contract::parse;

    use super::*;

    /// Every flag at once, so the mapping test sees every declared key.
    const EVERY_FLAG: &[&str] = &[
        "--ref",
        "master/water_mains",
        "--line-type",
        "directional",
        "--direction-size",
        "14",
        "--direction-spacing",
        "140",
        "--casing-color",
        "#0f172a",
        "--casing-width",
        "2.5",
        "--fill-pattern",
        "crosshatch",
        "--pattern-color",
        "#b45309",
        "--pattern-background",
        "#ffffff00",
        "--pattern-spacing",
        "8",
        "--pattern-stroke",
        "1",
    ];

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn every_declared_bridge_key_is_produced_and_plan_set_differ_only_by_apply() {
        // The hand copy that matters: a flag whose camelCase key is misspelled
        // compiles, helps correctly, and is refused by the application at
        // runtime. Prove the produced keys are exactly the declared ones here,
        // where no paired desktop is needed to find out.
        let tokens = argv(EVERY_FLAG);
        let plan_inputs = parse(&plan::COMMAND, &tokens).expect("plan inputs");
        let set_inputs = parse(&set::COMMAND, &tokens).expect("set inputs");
        let planned = arguments(&plan_inputs, false).expect("plan");
        let applied = arguments(&set_inputs, true).expect("set");

        let produced: BTreeSet<&str> = planned
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        let declared: BTreeSet<&str> = crate::CARTOGRAPHY_SET.arguments.iter().copied().collect();
        assert_eq!(
            produced, declared,
            "the flags a caller can pass must produce exactly the keys `style.cartography.set` declares"
        );

        assert_eq!(
            planned,
            json!({
                "ref": "master/water_mains",
                "lineType": "directional",
                "directionSize": 14,
                "directionSpacing": 140,
                "casingColor": "#0F172A",
                "casingWidth": 2.5,
                "fillPattern": "crosshatch",
                "patternColor": "#B45309",
                "patternBackground": "#FFFFFF00",
                "patternSpacing": 8,
                "patternStroke": 1,
                "apply": false,
            })
        );

        let mut expected = planned.clone();
        expected["apply"] = json!(true);
        assert_eq!(
            applied, expected,
            "plan and set are one operation; only `apply` may differ"
        );
    }

    #[test]
    fn a_call_that_changes_nothing_is_refused_before_the_bridge() {
        let tokens = argv(&["--ref", "master/mv_lines"]);
        let inputs = parse(&plan::COMMAND, &tokens).expect("inputs");
        assert_eq!(
            arguments(&inputs, false).expect_err("must refuse").code(),
            "invalid_cartography"
        );

        // A descriptor path is not a cartography change either.
        let tokens = argv(&["--ref", "master/mv_lines", "--desktop-descriptor", "/tmp/x"]);
        let inputs = parse(&plan::COMMAND, &tokens).expect("inputs");
        assert_eq!(
            arguments(&inputs, false).expect_err("must refuse").code(),
            "invalid_cartography"
        );
    }

    #[test]
    fn detail_that_contradicts_the_type_it_is_sent_with_is_refused() {
        for contradiction in [
            vec![
                "--ref",
                "r",
                "--line-type",
                "dashed",
                "--direction-size",
                "14",
            ],
            vec![
                "--ref",
                "r",
                "--line-type",
                "solid",
                "--direction-spacing",
                "140",
            ],
            vec![
                "--ref",
                "r",
                "--fill-pattern",
                "solid",
                "--pattern-color",
                "#B45309",
            ],
            vec![
                "--ref",
                "r",
                "--fill-pattern",
                "solid",
                "--pattern-spacing",
                "8",
            ],
        ] {
            let tokens = argv(&contradiction);
            let inputs = parse(&plan::COMMAND, &tokens).expect("inputs");
            assert_eq!(
                arguments(&inputs, false).expect_err("must refuse").code(),
                "invalid_cartography",
                "`{}` was accepted",
                contradiction.join(" ")
            );
        }

        // Adjusting arrow detail on a ref that already carries the directional
        // type is legitimate: `ds` has not read the document and must not
        // guess that it has not.
        let tokens = argv(&["--ref", "r", "--direction-spacing", "140"]);
        let inputs = parse(&plan::COMMAND, &tokens).expect("inputs");
        assert_eq!(
            arguments(&inputs, false).expect("adjust is allowed")["directionSpacing"],
            json!(140)
        );
    }

    #[test]
    fn every_bound_is_enforced_locally_including_seamless_pattern_spacing() {
        for (flag, value, code) in [
            ("--direction-size", "5", "invalid_number"),
            ("--direction-size", "49", "invalid_number"),
            ("--direction-spacing", "19", "invalid_number"),
            ("--direction-spacing", "1001", "invalid_number"),
            ("--casing-width", "20.5", "invalid_number"),
            ("--casing-width", "-1", "invalid_number"),
            ("--casing-width", "NaN", "invalid_number"),
            ("--pattern-stroke", "0", "invalid_number"),
            ("--pattern-stroke", "7", "invalid_number"),
            ("--casing-color", "navy", "invalid_color"),
            ("--pattern-background", "#FFF", "invalid_color"),
        ] {
            let tokens = argv(&["--ref", "r", "--line-type", DIRECTIONAL, flag, value]);
            let inputs = parse(&plan::COMMAND, &tokens).expect("inputs");
            assert_eq!(
                arguments(&inputs, false).expect_err("must refuse").code(),
                code,
                "`{flag} {value}` was accepted"
            );
        }

        // MapLibre repeats a pattern image by tiling it, so a tile that is not
        // a power of two seams visibly. The closed choice is the enforcement:
        // it is refused by the parser, before the handler is reached at all.
        for spacing in ["6", "12", "24", "33", "0"] {
            let tokens = argv(&["--ref", "r", "--pattern-spacing", spacing]);
            assert_eq!(
                parse(&plan::COMMAND, &tokens)
                    .expect_err("must refuse")
                    .code(),
                "invalid_choice",
                "`--pattern-spacing {spacing}` is not one of the seamless sizes"
            );
        }
        for spacing in ["4", "8", "16", "32"] {
            let tokens = argv(&["--ref", "r", "--pattern-spacing", spacing]);
            let inputs = parse(&plan::COMMAND, &tokens).expect("inputs");
            assert_eq!(
                arguments(&inputs, false).expect("seamless size")["patternSpacing"],
                json!(spacing.parse::<i64>().expect("integer choice"))
            );
        }
    }

    #[test]
    fn the_line_type_and_fill_pattern_vocabularies_are_closed() {
        for bad in ["dash_dot", "arrows", "Directional", ""] {
            let tokens = argv(&["--ref", "r", "--line-type", bad]);
            assert_eq!(
                parse(&plan::COMMAND, &tokens)
                    .expect_err("must refuse")
                    .code(),
                "invalid_choice",
                "`--line-type {bad}` was accepted"
            );
        }
        for bad in ["hatch", "diagonal", "crosshatched"] {
            let tokens = argv(&["--ref", "r", "--fill-pattern", bad]);
            assert_eq!(
                parse(&plan::COMMAND, &tokens)
                    .expect_err("must refuse")
                    .code(),
                "invalid_choice",
                "`--fill-pattern {bad}` was accepted"
            );
        }
        assert!(
            LINE_TYPE_ARG.choices.contains(&DIRECTIONAL),
            "the directional marker type must stay in the line-type vocabulary"
        );
        assert!(
            LINE_TYPE_ARG.choices.contains(&UNPATTERNED)
                && FILL_PATTERN_ARG.choices.contains(&UNPATTERNED),
            "`solid` is how a caller removes a dash or a hatch on either flag"
        );
    }

    #[test]
    fn plan_and_set_declare_the_same_inputs_and_part_only_on_confirmation() {
        let plan_flags: Vec<&str> = plan::COMMAND.args.iter().map(|arg| arg.name).collect();
        let set_flags: Vec<&str> = set::COMMAND.args.iter().map(|arg| arg.name).collect();
        assert_eq!(
            plan_flags, set_flags,
            "a caller must be able to re-run a reviewed plan verbatim with --yes"
        );
        assert!(!plan::COMMAND.effect.needs_confirmation());
        assert!(set::COMMAND.effect.needs_confirmation());
        let set_codes: Vec<&str> = SET_REFUSALS.iter().map(|refusal| refusal.code).collect();
        let plan_codes: Vec<&str> = PLAN_REFUSALS.iter().map(|refusal| refusal.code).collect();
        assert!(set_codes.contains(&"confirmation_required"));
        assert!(!plan_codes.contains(&"confirmation_required"));
    }
}
