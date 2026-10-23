//! The Kernel admits staged-chunk requests into unit effects and validates
//! the unit's replies, putting each reply's chunk back beside its answer.
use super::*;
use ds_network_contracts::grid_unit::{encode_reply, GridRefusal};
use serde_json::json;

fn annotate() -> UploadBatchRequest {
    serde_json::from_value(
        json!({"operation": "annotate", "input": {"sourceFile": "f.xml", "sourceLayer": "l"}}),
    )
    .unwrap()
}

fn chunk() -> Vec<u8> {
    [b"ARROW1".as_slice(), b"rows", b"ARROW1"].concat()
}

fn parts(value: serde_json::Value, blobs: Vec<Vec<u8>>) -> Vec<u8> {
    encode_reply(
        OPERATION,
        &Ok(GridResult::Parts {
            text: value.to_string(),
            blobs,
        }),
    )
}

#[test]
fn chunk_requests_become_unit_effects_with_the_chunk_as_bytes() {
    let effect = request_effect(&chunk(), &annotate()).unwrap();
    let sent = GridRequest::decode(&effect.payload).unwrap();
    assert_eq!(sent.operation, OPERATION);
    let [GridArg::Text(text), GridArg::Bytes(ipc)] = sent.args.as_slice() else {
        panic!("request text then chunk")
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(text).unwrap(),
        serde_json::to_value(annotate()).unwrap()
    );
    assert_eq!(ipc, &chunk());
}

#[test]
fn unit_replies_carry_their_chunk_beside_the_answer() {
    let request = annotate();
    let reply = parts(
        json!({"operation": "annotate", "result": {"ipc": [], "featureCount": 2}}),
        vec![chunk()],
    );
    let UploadBatchResponse::Annotate(batch) = decode_unit_reply(&request, &reply).unwrap() else {
        panic!("annotate")
    };
    assert_eq!(batch.ipc, chunk());
    assert_eq!(batch.feature_count, 2);
    // A chunk the answer has no place for, or a missing one, is refused.
    for bad in [
        parts(
            json!({"operation": "annotate", "result": {"ipc": [], "featureCount": 2}}),
            vec![],
        ),
        parts(
            json!({"operation": "annotate", "result": {"ipc": [], "featureCount": 2}}),
            vec![chunk(), chunk()],
        ),
        parts(
            json!({"operation": "annotate", "result": {"ipc": [], "featureCount": 2}}),
            vec![b"x".to_vec()],
        ),
    ] {
        let error = decode_unit_reply(&request, &bad).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("upload_batch_response_invalid: "),
            "{error}"
        );
    }
    let refused = encode_reply(
        OPERATION,
        &Err(GridRefusal::new(
            "upload_batch_request_invalid",
            "upload_batch_request_invalid: the chunk is not Arrow IPC",
        )),
    );
    assert_eq!(
        decode_unit_reply(&request, &refused)
            .unwrap_err()
            .to_string(),
        "upload_batch_request_invalid: the chunk is not Arrow IPC"
    );
}
