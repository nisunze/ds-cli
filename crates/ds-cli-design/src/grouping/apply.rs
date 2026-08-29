use crate::{CONSUMER_GROUPING_APPLY, DESCRIPTOR_ARG};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{json, Value};
pub static COMMAND: Command = Command { id:"design.consumer-grouping.apply",path:&["design","consumer-grouping","apply"],contract:1,summary:"Apply a previewed typed Solar report grouping.",purpose:"Echoes the preview plan digest; stale definitions, assignments or Solar inventory require re-preview.",chapter:Chapter::Design,effect:Effect::GlobalWrite,authority:Authority::Project,execution:Execution::Sync,args:&[crate::group::PROJECTION_TRANSFORMERS_ARG,crate::group::PROJECTION_DEFINITION_IDS_ARG,crate::group::DIGEST_ARG,DESCRIPTOR_ARG],output:"The applied solar_report grouping plan.",examples:&[],refusals:&[crate::NOT_PAIRED,crate::REFUSED],reference:Some("docs/reference/design.md"),availability:crate::paired_availability };
pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(&descriptor,&CONSUMER_GROUPING_APPLY,json!({"transformers":crate::group::projection_transformers(inputs)?,"definition-ids":crate::group::projection_definition_ids(inputs)?,"bindings":inputs.value("bindings").unwrap_or("[]"),"digest":inputs.require("digest")?}),crate::READ_TIMEOUT).map_err(crate::classify_design_failure)
}
pub fn render(data: &Value) -> String {
    format!(
        "consumer grouping applied {}",
        data["plan_digest"].as_str().unwrap_or("?")
    )
}
