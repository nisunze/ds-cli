//! Declarative JSON instructions use the same closed kernel as guided flags.
use crate::{LANE_ARG, PROJECT_ARG, REF_ARG};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use std::io::Read;
const FILE:Arg=Arg::value("file","<instruction.json>","One JSON StyleInstruction, at most 2 MiB; use instruction schema for the exact closed vocabulary.").required();
const EXPECTED: Arg = Arg::value(
    "expected-digest",
    "<64hex>",
    "Storage contentSha256 from style read; refuses a changed document at plan and atomically at publish.",
);
pub(crate) fn arguments(inputs: &Inputs, apply: bool) -> Result<Value, Failure> {
    let invalid = |error: String| {
        Failure::invalid("style_refused", error)
            .remedy("read style instruction schema, correct the JSON, then plan before applying")
    };
    let mut bytes = Vec::new();
    std::fs::File::open(inputs.require("file")?)
        .map_err(|e| invalid(e.to_string()))?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(e.to_string()))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(invalid("instruction exceeds 2 MiB".into()));
    }
    let mut instruction: ds_command_kernel::style_plan::StyleInstruction =
        serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    if let Some(expected_digest) = inputs.value("expected-digest") {
        instruction = ds_command_kernel::style_plan::StyleInstruction::Guarded {
            expected_digest: expected_digest.into(),
            instruction: Box::new(instruction),
        };
    }
    Ok(json!({"ref":inputs.require("ref")?,"apply":apply,"instruction":instruction}))
}
macro_rules! command {($module:ident,$summary:literal,$effect:ident,$refusals:expr)=>{pub mod $module{use super::*;
    pub static COMMAND:Command=Command{id:concat!("style.instruction.",stringify!($module)),path:&["style","instruction",stringify!($module)],contract:1,summary:$summary,purpose:"Replay governed JSON through the same bounded StyleInstruction that CLI flags and Style Center use. Field domains, colours, icons, stops, fallback and scope are validated by the kernel; unknown keys are refused. --expected-digest fences against the reviewed storage content digest. Only the explicitly named style ref changes; engineering data and print/screen sibling documents remain independent.",chapter:Chapter::MapPresentation,effect:Effect::$effect,authority:Authority::HeadlessProject,execution:Execution::Sync,args:&[PROJECT_ARG,REF_ARG,FILE,EXPECTED,LANE_ARG],output:"Exact resulting document, dryRun/published state, validation and expressions; a guarded save includes baseDigest.",examples:&[],refusals:$refusals,reference:Some("docs/reference/style.md"),search:&[],requires:Requires::Server,availability:ds_cli_auth::native_availability};
    pub fn run(inputs:&Inputs,_:&Context)->Result<Value,Failure>{crate::native::edit(inputs,crate::native::Edit::Instruction,arguments(inputs,stringify!($module)=="set")?)}
    pub fn render(data:&Value)->String{format!("{} · {}\n",data["ref"].as_str().unwrap_or("?"),if data["published"]==true{"published"}else{"plan only"})}
}}}
command!(
    plan,
    "Validate and plan one declarative JSON style instruction.",
    LocalAuthState,
    crate::native::REFUSALS
);
command!(
    set,
    "Apply one reviewed declarative JSON style instruction.",
    GlobalWrite,
    crate::native::PUBLISH_REFUSALS
);
pub mod schema {
    use super::*;
    pub static COMMAND: Command = Command {
        id: "style.instruction.schema",
        path: &["style", "instruction", "schema"],
        contract: 1,
        summary: "Read the kernel JSON schema for guided style instructions.",
        purpose: "The exact compiled StyleInstruction schema, including categorical palettes/icons/text, interpolated colour, flat halos, zoom steps, contrast presets and guarded replay. No project, login or window is needed. Property bounds and field vocabularies remain project/layer facts from style read.",
        chapter: Chapter::MapPresentation,
        effect: Effect::ReadOnly,
        authority: Authority::None,
        execution: Execution::Sync,
        args: &[],
        output: "The compiled JSON Schema for the kernel StyleInstruction.",
        examples: &[],
        refusals: &[],
        reference: Some("docs/reference/style.md"),
        search: &[],
        requires: Requires::Server,
        availability: available,
    };
    fn available() -> Availability {
        Availability::Available
    }
    pub fn run(_: &Inputs, _: &Context) -> Result<Value, Failure> {
        Ok(ds_command_kernel::style_plan::instruction_schema())
    }
    pub fn render(data: &Value) -> String {
        format!(
            "{}\n",
            serde_json::to_string_pretty(data).unwrap_or_default()
        )
    }
}
