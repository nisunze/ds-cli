//! The actual existing engine handler through the application's request/result
//! owners. This is in the CLI's test graph, where Network is already a dev-only
//! dependency; no engineering implementation is linked into the Kernel.
use super::*;
use ds_command_kernel::{design_lv, envelope};
use ds_network_contracts::lv_processing::NativeLvResultV1;

fn page_input() -> ds_network_contracts::lv::LvProcessBatchInput {
    let source: Value = serde_json::from_slice(include_bytes!(
        "../../../../kernel/ds-client-core/tests/fixtures/native-lv-save-request.json"
    ))
    .unwrap();
    let mut first = source["jobs"][0].clone();
    first["ds_project"] = json!("project_a");
    let mut failed = first.clone();
    failed["transformer_name"] = json!("T_missing_source");
    failed["customer_sources"] = json!([{"address":"idb://entries/project_a/not_retained","endpoint":"customers_from_table"}]);
    // This is the same page payload constructor shape, using the owner's
    // existing immutable engineering fixture and an actual missing-table job.
    serde_json::from_value(json!({"jobs":[first,failed],"local_tables":{}})).unwrap()
}

#[test]
fn page_constructor_serialized_input_real_handler_response_constructor_and_decoder_agree() {
    let capture = design_lv::capture("project_a", page_input()).unwrap();
    let native_request = capture.native_request().unwrap();
    let before = native_request.clone();
    let bytes = ds_network::application::run_lv_bytes(&native_request).unwrap();
    assert_eq!(native_request, before);
    let native: NativeLvResultV1 = serde_json::from_slice(&bytes).unwrap();
    let output = capture.response_from_native(native).unwrap();
    assert!(output.jobs[0].ok, "{:?}", output.jobs[0].error);
    let produced = output.jobs[0].output.as_ref().unwrap();
    assert!(produced.voltage_drop.is_some());
    assert!(output.jobs[0].preservation.is_some());
    assert!(!output.jobs[1].ok);
    assert!(output.jobs[1]
        .error
        .as_deref()
        .unwrap()
        .contains("not mounted"));
    let wire = serde_json::to_vec(&output).unwrap();
    let admitted = capture.decode_response(&wire).unwrap();
    assert_eq!(admitted.jobs[0].ds_project, "project_a");
    assert_eq!(admitted.jobs[0].transformer_name, "T1");
    assert_eq!(admitted.jobs[1].transformer_name, "T_missing_source");
    let mut core = envelope::success(design_lv::COMMAND, COMMAND.contract, json!({"jobs":2}));
    core["files"] = json!({"out":output});
    let admitted = capture.decode_core_response(core).unwrap();
    assert_eq!(serde_json::to_vec(&admitted).unwrap(), wire);
}
