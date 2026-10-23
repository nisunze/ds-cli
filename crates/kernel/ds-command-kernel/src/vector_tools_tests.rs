//! The page's vector tool host half: the effect carries the operation and its
//! layer bytes to the Network unit's `vectorTool`, and only a reply in the
//! declared shape for that operation is admitted. The runner and its byte
//! parity are the unit's tests (ds-network-wasm vector_tools).
use super::*;
use ds_network_contracts::grid_unit::{encode_reply, GridOperation, GridRefusal, GridRequest};

fn ipc(tag: u8) -> Bytes {
    let mut bytes = b"ARROW1".to_vec();
    bytes.extend([tag; 4]);
    bytes.extend(b"ARROW1");
    Bytes(bytes)
}

fn merge() -> VectorToolOperation {
    VectorToolOperation::MergeLayers(VectorLayerChunks {
        chunks: vec![ipc(1), ipc(2)],
    })
}

fn operation_of(effect: &GridEffect) -> (GridOperation, VectorToolOperation) {
    let request = GridRequest::decode(&effect.payload).unwrap();
    let [GridArg::Text(text), blobs @ ..] = &request.args[..] else {
        panic!("vectorTool takes its lifted text first");
    };
    let blobs = blobs
        .iter()
        .map(|blob| match blob {
            GridArg::Bytes(bytes) => bytes.clone(),
            _ => panic!("then only blobs"),
        })
        .collect();
    (request.operation, frame::lower(text, blobs).unwrap())
}

fn answered(operation: GridOperation, response: &VectorToolResponse) -> Vec<u8> {
    let (text, blobs) = frame::lift(response).unwrap();
    encode_reply(
        operation,
        &Ok(GridResult::Artifact {
            bytes: frame::pack(&blobs),
            receipt_json: text,
        }),
    )
}

#[test]
fn the_effect_carries_the_operation_and_its_bytes_as_blobs() {
    let effect = effect(&merge()).unwrap();
    assert_eq!(effect.unit, "network");
    let (operation, carried) = operation_of(&effect);
    assert_eq!(operation.name(), UNIT_METHOD);
    assert_eq!(carried, merge());
    let text = String::from_utf8_lossy(&effect.payload);
    assert!(
        text.contains(r#"{"$blob":0}"#),
        "bytes travel beside the JSON"
    );
}

#[test]
fn a_reply_is_admitted_only_in_the_shape_of_its_operation() {
    let operation = merge();
    let (grid, _) = operation_of(&effect(&operation).unwrap());
    let merged = VectorToolResponse::MergeLayers(VectorLayerIpc { layer: ipc(3) });
    assert_eq!(reply(&operation, &answered(grid, &merged)).unwrap(), merged);
    let crossed = VectorToolResponse::PointCoordinates(VectorPointCoordinates {
        xy: vec![30.0, -2.0],
    });
    assert!(reply(&operation, &answered(grid, &crossed))
        .unwrap_err()
        .to_string()
        .contains("vector_response_invalid"));
    let not_ipc = VectorToolResponse::MergeLayers(VectorLayerIpc {
        layer: Bytes(vec![1, 2]),
    });
    assert!(reply(&operation, &answered(grid, &not_ipc)).is_err());
    let text = encode_reply(grid, &Ok(GridResult::Text("{}".into())));
    assert!(
        reply(&operation, &text).is_err(),
        "the reply carries its blobs"
    );
}

#[test]
fn the_tool_refusal_reaches_the_page_with_its_remedy() {
    let operation = merge();
    let (grid, _) = operation_of(&effect(&operation).unwrap());
    let refused = encode_reply(
        grid,
        &Err(GridRefusal {
            code: "vector_data_bound_exceeded".into(),
            message: "IPC input exceeds 32 MiB; split the layer".into(),
            detail: Some(r#"{"remedy":"Split the source."}"#.into()),
        }),
    );
    let fault = reply(&operation, &refused).unwrap_err().0;
    assert_eq!(
        fault,
        json!({"code":"vector_data_bound_exceeded","message":"IPC input exceeds 32 MiB; split the layer","remedy":"Split the source."})
    );
}

#[test]
fn the_catalogue_and_closed_tool_set_are_the_contracts() {
    let catalogue = catalogue().unwrap();
    assert!(catalogue.tools.len() > VectorToolId::ALL.len());
    assert_eq!(tool_id("measure").unwrap(), VectorToolId::Measure);
    assert_eq!(
        tool_id("unknown-tool").unwrap_err().code,
        "vector_tool_unknown"
    );
}
