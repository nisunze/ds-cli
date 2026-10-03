//! Typed inputs for kernel-owned primary channels and screen scale.
use crate::{LANE_ARG, PROJECT_ARG, REF_ARG};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

const PRESET:Arg=Arg::value("name","<declared-preset>","Exact governed authoring preset from style read .instructions.presets, applied only to this screen ref.").required();
const FIELD: Arg = Arg::value(
    "field",
    "<field>",
    "Exact field from style read; the kernel validates its declared domain.",
)
.required();
const FIELD_TYPE: Arg = Arg::value(
    "field-type",
    "<string|number>",
    "Category label type if the published field domain is untyped; numbers must be integers.",
)
.choices(&["string", "number"]);
const CATEGORY_CHANNEL: Arg = Arg::value(
    "channel",
    "<color|icon|label>",
    "Primary colour, catalog icon, or categorical label text; each has an explicit fallback.",
)
.required()
.choices(&["color", "icon", "label"]);
const VALUES: Arg = Arg {
    name: "value",
    kind: ArgKind::Repeated,
    value: "<category>=<output>",
    required: true,
    default: None,
    choices: &[],
    summary: "Exact category and hex colour, catalog icon or label text. Repeat 1..50 times; labels use the field's scalar type.",
};
const OTHER: Arg = Arg::value(
    "other",
    "<output>",
    "Explicit colour/icon/text for unknown, missing and unnamed category values.",
)
.required();
const MERGE: Arg = Arg::switch(
    "merge",
    "Patch only named categories of the existing same-field match; preserve the other categories.",
);
const COLOR_STOPS: Arg = Arg {
    name: "stop",
    kind: ArgKind::Repeated,
    value: "<number>=<#hex>",
    required: true,
    default: None,
    choices: &[],
    summary: "Numeric input and colour, ordered strictly increasing; repeat 2..50 times. Interpolation is linear; missing data uses the first stop.",
};
const ZOOM_CHANNEL:Arg=Arg::value("channel","<size|opacity|icon_overlap>","Screen size, opacity or icon collision placement across zoom levels. Overlap updates both icon placement flags.").required().choices(&["size","opacity","icon_overlap"]);
const BASE:Arg=Arg::value("base","<number|on|off>","Value below the first step (or at zoom 0 for linear interpolation); opacity 0..1, size uses live schema bounds.").required();
const ZOOM_STOPS: Arg = Arg {
    name: "stop",
    kind: ArgKind::Repeated,
    value: "<zoom>=<number|on|off>",
    required: true,
    default: None,
    choices: &[],
    summary: "Zoom and value; repeat 1..50 times, strictly increasing within 0..24. Collision overlap uses on/off.",
};
const INTERPOLATION:Arg=Arg::value("interpolation","<step|linear>","Stepped scale by default; linear for numeric channels, starting at zoom 0. Icon overlap accepts step only.").choices(&["step","linear"]).default("step");

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("invalid_value_spec", message).remedy(
        "use one category=output or zoom=value per repeated flag; read the command help for bounds",
    )
}
fn pair(raw: &str) -> Result<(&str, &str), Failure> {
    raw.split_once('=')
        .filter(|(a, _)| !a.is_empty())
        .ok_or_else(|| invalid("each value or stop needs input=output"))
}
fn number(raw: &str) -> Result<f64, Failure> {
    raw.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| invalid("stop inputs and numeric outputs must be finite numbers"))
}

pub(crate) fn arguments(
    inputs: &Inputs,
    action: crate::native::Edit,
    apply: bool,
) -> Result<Value, Failure> {
    let repeated = inputs.repeated(if action == crate::native::Edit::Categorical {
        "value"
    } else {
        "stop"
    });
    if repeated.len() > 50 {
        return Err(invalid("at most 50 values or stops"));
    }
    let mut args = json!({"ref":inputs.require("ref")?,"apply":apply});
    match action {
        crate::native::Edit::Preset => {
            args["name"] = json!(inputs.require("name")?);
        }
        crate::native::Edit::Categorical => {
            args["channel"] = json!(inputs.require("channel")?);
            args["field"] = json!(inputs.require("field")?);
            args["field_type"] = json!(inputs.value("field-type"));
            args["other"] = json!(inputs.require("other")?);
            args["merge"] = json!(inputs.switch("merge"));
            args["values"] = json!(
                repeated
                    .iter()
                    .map(|raw| {
                        let (value, output) = pair(raw)?;
                        Ok(json!({"value":value,"output":output}))
                    })
                    .collect::<Result<Vec<_>, Failure>>()?
            );
        }
        crate::native::Edit::ColorRange => {
            args["field"] = json!(inputs.require("field")?);
            args["stops"] = json!(
                repeated
                    .iter()
                    .map(|raw| {
                        let (value, color) = pair(raw)?;
                        Ok(json!({"value":number(value)?,"color":crate::color(color,"stop")?}))
                    })
                    .collect::<Result<Vec<_>, Failure>>()?
            );
        }
        crate::native::Edit::Zoom => {
            let channel = inputs.require("channel")?;
            let output = |raw: &str| -> Result<Value, Failure> {
                if channel == "icon_overlap" {
                    match raw {
                        "on" | "true" => Ok(json!(true)),
                        "off" | "false" => Ok(json!(false)),
                        _ => Err(invalid("icon overlap needs on or off")),
                    }
                } else {
                    number(raw).map(|n| json!(n))
                }
            };
            args["channel"] = json!(channel);
            args["base"] = output(inputs.require("base")?)?;
            args["interpolation"] = json!(inputs.require("interpolation")?);
            args["stops"] = json!(
                repeated
                    .iter()
                    .map(|raw| {
                        let (zoom, value) = pair(raw)?;
                        Ok(json!({"zoom":number(zoom)?,"value":output(value)?}))
                    })
                    .collect::<Result<Vec<_>, Failure>>()?
            );
        }
        _ => unreachable!("multiscale command"),
    }
    Ok(args)
}
fn render(data: &Value) -> String {
    format!(
        "{} · {}\n",
        data["ref"].as_str().unwrap_or("?"),
        if data["published"] == true {
            "published"
        } else {
            "plan only"
        }
    )
}

macro_rules! commands {
    ($module:ident,$path:literal,$edit:ident,$plan:literal,$set:literal,$purpose:literal,$args:expr,$example:literal) => {
        pub mod $module { use super::*;
            pub mod plan {use super::*;
                pub static COMMAND:Command=Command{id:concat!("style.",$path,".plan"),path:&["style",$path,"plan"],contract:1,summary:$plan,purpose:$purpose,chapter:Chapter::MapPresentation,effect:Effect::LocalAuthState,authority:Authority::HeadlessProject,execution:Execution::Sync,args:$args,output:"Exact kernel-planned document, field/type or zoom expression, dryRun:true and published:false.",examples:&[Example{command:$example,note:"Review the expression and fallback before publishing the same flags with set --yes.",runnable:false}],refusals:crate::native::REFUSALS,reference:Some("docs/reference/style.md"),search:&[],requires:Requires::Server,availability:ds_cli_auth::native_availability};
                pub fn run(inputs:&Inputs,_:&Context)->Result<Value,Failure>{crate::native::edit(inputs,crate::native::Edit::$edit,arguments(inputs,crate::native::Edit::$edit,false)?)}
                pub fn render(data:&Value)->String{super::super::render(data)}
            }
            pub mod set {use super::*;
                pub static COMMAND:Command=Command{id:concat!("style.",$path,".set"),path:&["style",$path,"set"],contract:1,summary:$set,purpose:$purpose,chapter:Chapter::MapPresentation,effect:Effect::GlobalWrite,authority:Authority::HeadlessProject,execution:Execution::Sync,args:$args,output:"The kernel plan with published:true, service warnings and exact persisted document.",examples:&[],refusals:crate::native::PUBLISH_REFUSALS,reference:Some("docs/reference/style.md"),search:&[],requires:Requires::Server,availability:ds_cli_auth::native_availability};
                pub fn run(inputs:&Inputs,_:&Context)->Result<Value,Failure>{crate::native::edit(inputs,crate::native::Edit::$edit,arguments(inputs,crate::native::Edit::$edit,true)?)}
                pub fn render(data:&Value)->String{super::super::render(data)}
            }
        }
    }
}
commands!(
    preset,
    "preset",
    Preset,
    "Plan opt-in satellite or existing-assets screen contrast.",
    "Publish a standardized screen contrast preset.",
    "Apply a versioned kernel recipe to one explicitly named screen ref. Satellite preserves category identities and gives each band contrast with visible unknown values and white point halos. Existing-assets uses neutral asset colour. Plan shows every changed property; print documents are independent.",
    &[PROJECT_ARG, REF_ARG, PRESET, LANE_ARG],
    "ds style preset plan --project <id> --ref master/lv_poles --name satellite --output json"
);
commands!(
    categorical,
    "categorical",
    Categorical,
    "Plan primary categorical colours, icons or label text.",
    "Publish primary categorical colours, icons or label text.",
    "Author exact typed categories and an explicit unknown/missing fallback through the shared kernel. Colour supports line/fill/circle/symbol refs and tiled equivalents; icons must be in the published catalog. Labels retain their placement. --merge changes named categories without flattening the palette. Screen and print documents are independent.",
    &[
        PROJECT_ARG,
        REF_ARG,
        CATEGORY_CHANNEL,
        FIELD,
        FIELD_TYPE,
        VALUES,
        OTHER,
        MERGE,
        LANE_ARG
    ],
    "ds style categorical plan --project <id> --ref master/lv_lines_vt --channel color --field cable_size --value ABC50=#E11D48 --other '#64748B' --output json"
);
commands!(
    color_range,
    "color-range",
    ColorRange,
    "Plan interpolated colour by a numeric field.",
    "Publish interpolated colour by a numeric field.",
    "Use a declared numeric field and ordered colour stops to author a linear primary colour expression. Values outside the range clamp to the end colours; missing values use the first stop. Nothing else in the document changes.",
    &[PROJECT_ARG, REF_ARG, FIELD, COLOR_STOPS, LANE_ARG],
    "ds style color-range plan --project <id> --ref <line-ref> --field <numeric-field> --stop 25=#22C55E --stop 100=#E11D48 --output json"
);
commands!(
    zoom,
    "zoom",
    Zoom,
    "Plan screen size, opacity and collision placement by zoom.",
    "Publish screen size, opacity or collision placement by zoom.",
    "A continuous screen map needs different size and collision behavior at overview and working zoom. Author bounded zoom stops through the same kernel the Style Center uses. Step defaults to base below the first stop; linear interpolates from base at zoom 0. Icon overlap changes allow-overlap and ignore-placement together. Line size retains authored casing thickness.",
    &[
        PROJECT_ARG,
        REF_ARG,
        ZOOM_CHANNEL,
        BASE,
        ZOOM_STOPS,
        INTERPOLATION,
        LANE_ARG
    ],
    "ds style zoom plan --project <id> --ref edcl_customers_survey --channel icon_overlap --base off --stop 16=on --output json"
);

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(command: &Command, flags: &[&str]) -> Inputs {
        let mut args = vec!["--project".into(), "gisagara".into()];
        args.extend(flags.iter().map(|s| s.to_string()));
        ds_cli_contract::parse(command, &args).unwrap()
    }
    #[test]
    fn flag_mapping_preserves_typed_fallbacks_and_zoom() {
        let inputs = parse(
            &categorical::plan::COMMAND,
            &[
                "--ref",
                "master/tr",
                "--channel",
                "label",
                "--field",
                "tr_size",
                "--value",
                "25=25 kVA",
                "--other",
                "Unknown",
            ],
        );
        let args = arguments(&inputs, crate::native::Edit::Categorical, false).unwrap();
        let encoded = serde_json::to_value(
            crate::native::instruction(crate::native::Edit::Categorical, args, &inputs).unwrap(),
        )
        .unwrap();
        assert_eq!(
            encoded["values"][0],
            json!({"value":"25","output":"25 kVA"})
        );
        assert_eq!(encoded["other"], "Unknown");
        let inputs = parse(
            &zoom::plan::COMMAND,
            &[
                "--ref",
                "survey",
                "--channel",
                "icon_overlap",
                "--base",
                "off",
                "--stop",
                "16=on",
            ],
        );
        let args = arguments(&inputs, crate::native::Edit::Zoom, false).unwrap();
        let instruction =
            crate::native::instruction(crate::native::Edit::Zoom, args, &inputs).unwrap();
        assert_eq!(
            serde_json::to_value(instruction).unwrap()["stops"][0]["value"],
            true
        );
        let inputs = parse(
            &color_range::plan::COMMAND,
            &[
                "--ref",
                "master/tr",
                "--field",
                "tr_size",
                "--stop",
                "25=#22C55E",
                "--stop",
                "100=#E11D48",
            ],
        );
        let args = arguments(&inputs, crate::native::Edit::ColorRange, false).unwrap();
        assert_eq!(args["stops"][0]["value"], 25.);
    }
}
