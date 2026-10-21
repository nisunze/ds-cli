//! Exact print revision reads and a fenced restore; no window or saved context.
use crate::{LANE_ARG, PROJECT_ARG, REF_ARG};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;
const REVISION: Arg = Arg::value(
    "revision",
    "<64hex>",
    "Exact immutable revision id from style print versions list.",
)
.required();
const HEAD: Arg = Arg::value(
    "expected-head",
    "<64hex>",
    "Current head from the version list; a changed head refuses restore atomically.",
)
.required();
fn run(inputs: &Inputs, kind: &str) -> Result<Value, Failure> {
    use ds_command_kernel::style_history::Command as History;
    let reference = inputs.require("ref")?.into();
    let command = match kind {
        "list" => History::List { reference },
        "read" => History::Read {
            reference,
            revision: inputs.require("revision")?.into(),
        },
        "compare" => History::Compare {
            reference,
            revision: inputs.require("revision")?.into(),
        },
        _ => History::Restore {
            reference,
            revision: inputs.require("revision")?.into(),
            expected_head: inputs.require("expected-head")?.into(),
        },
    };
    command.validate().map_err(|message| {
        Failure::invalid("style_refused", message).remedy(
            "copy one exact _print ref and revision/head ids from style print versions list",
        )
    })?;
    ds_cli_auth::style_history(
        inputs.require("lane")?,
        inputs.require("project")?,
        &command,
    )
    .map_err(crate::native::named)
}
macro_rules! command{
    ($module:ident,$summary:literal,$effect:ident,$args:expr,$output:literal,$refusals:expr)=>{pub mod $module{use super::*;
        pub static COMMAND:Command=Command{id:concat!("style.print.versions.",stringify!($module)),path:&["style","print","versions",stringify!($module)],contract:1,summary:$summary,purpose:"Governed print documents retain immutable revisions with author, time and scope. Read or compare exact saved content, or restore a known revision under its current head fence. Restore appends a revision and retains all history. The explicitly named project authorizes the call; no Desktop or saved selection is read.",chapter:Chapter::MapPresentation,effect:Effect::$effect,authority:Authority::HeadlessProject,execution:Execution::Sync,args:$args,output:$output,examples:&[],refusals:$refusals,reference:Some("docs/reference/style.md"),search:&[],requires:Requires::Server,availability:ds_cli_auth::native_availability};
        pub fn run(inputs:&Inputs,_:&Context)->Result<Value,Failure>{super::run(inputs,stringify!($module))}
        pub fn render(data:&Value)->String{format!("{} · {}\n",data["style_ref"].as_str().unwrap_or("?"),data["head_revision"].as_str().or(data["revision_id"].as_str()).unwrap_or("exact revision"))}
    }}
}
command!(
    list,
    "List immutable print style revisions and the current head.",
    LocalAuthState,
    &[PROJECT_ARG, REF_ARG, LANE_ARG],
    "style_ref, head_revision and up to 200 revisions, with more indicating older history, with ordinal, content digest, author, created_at and scope.",
    crate::native::REFUSALS
);
command!(
    read,
    "Read one exact immutable print style revision.",
    LocalAuthState,
    &[PROJECT_ARG, REF_ARG, REVISION, LANE_ARG],
    "Exact style_data and immutable revision metadata.",
    crate::native::REFUSALS
);
command!(
    compare,
    "Compare an exact print style revision with the current head.",
    LocalAuthState,
    &[PROJECT_ARG, REF_ARG, REVISION, LANE_ARG],
    "Current revision, selected revision, equal and JSON-pointer changes with before/after values.",
    crate::native::REFUSALS
);
command!(
    restore,
    "Restore exact print style content under a current head fence.",
    GlobalWrite,
    &[PROJECT_ARG, REF_ARG, REVISION, HEAD, LANE_ARG],
    "Exact restored style_data, restored_from and newly appended head_revision; prior history remains available.",
    crate::native::PUBLISH_REFUSALS
);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restore_declares_the_head_fence() {
        assert!(
            restore::COMMAND
                .args
                .iter()
                .any(|arg| arg.name == "expected-head" && arg.required)
        );
        assert_eq!(restore::COMMAND.effect, Effect::GlobalWrite);
        assert_eq!(read::COMMAND.effect, Effect::LocalAuthState);
    }
}
