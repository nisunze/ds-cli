//! The Kernel admits reader requests and stream steps into unit effects and
//! decodes the unit's replies; the readers themselves are tested beside the
//! Network unit (`ds-network-wasm` `grid::page_data`).
use super::*;
use ds_network_contracts::grid_unit::{encode_reply, GridRefusal};
use serde_json::json;

const IMPORT: GridOperation = GridOperation::Model(GridModelMethod::LayerImport);

fn geojson() -> LayerImportRequest {
    LayerImportRequest::GeojsonLayers {
        name: "p.geojson".into(),
        crs: None,
        center: None,
    }
}

fn arrow(tail: &[u8]) -> Vec<u8> {
    [b"ARROW1".as_slice(), tail, b"ARROW1"].concat()
}

fn layers_reply(chunks: Vec<Vec<u8>>, counts: &[usize]) -> Vec<u8> {
    let text = serde_json::to_string(&json!({"kind": "ipc_layers", "result": [{
        "name": "p", "featureCount": counts.iter().sum::<usize>(), "geometryType": "Point",
        "columns": ["a"], "chunkFeatureCounts": counts}]}))
    .unwrap();
    encode_reply(
        IMPORT,
        &Ok(GridResult::Parts {
            text,
            blobs: chunks,
        }),
    )
}

#[test]
fn reader_requests_become_unit_effects_after_admission() {
    let input = encode_request(&geojson()).unwrap();
    let effect = request_effect(&input, &[b"{}"]).unwrap();
    assert_eq!(effect.unit, "network");
    assert_eq!(effect.entry, IMPORT.entry());
    let sent = GridRequest::decode(&effect.payload).unwrap();
    assert_eq!(sent.operation, IMPORT);
    assert_eq!(
        sent.args,
        [
            GridArg::Bytes(input.clone()),
            GridArg::Bytes(b"{}".to_vec())
        ]
    );
    // Admission refuses before any effect: wrong part count, blank name.
    let parts = request_effect(&input, &[b"a", b"b"]).unwrap_err();
    assert_eq!(
        parts.to_string(),
        "layer_import_request_invalid: expected 1 byte part(s), received 2"
    );
    let blank = request_effect(br#"{"operation":"kml_layers","name":" "}"#, &[b"x"]).unwrap_err();
    assert!(matches!(blank, LayerImportError::InvalidRequest(_)));
    let shapefile = encode_request(&LayerImportRequest::Shapefile { name: "s".into() }).unwrap();
    assert!(request_effect(&shapefile, &[b"shp", b"", b""]).is_ok());
}

#[test]
fn unit_replies_are_validated_and_refusals_keep_their_message() {
    let request = geojson();
    let reply = decode_unit_reply(&request, &layers_reply(vec![arrow(b"x")], &[1])).unwrap();
    assert_eq!(reply.chunks, [arrow(b"x")]);
    let LayerImportResponse::IpcLayers(layers) = reply.response else {
        panic!("layers")
    };
    assert_eq!(layers[0].chunk_feature_counts, [1]);
    // A chunk count or magic that disagrees with the answer is refused.
    for bad in [
        layers_reply(vec![arrow(b"x")], &[1, 1]),
        layers_reply(vec![b"not arrow".to_vec()], &[1]),
    ] {
        let error = decode_unit_reply(&request, &bad).unwrap_err();
        assert!(
            matches!(error, LayerImportError::InvalidResponse(_)),
            "{error}"
        );
    }
    // The reader's refusal crosses with its own message.
    let refused = encode_reply(
        IMPORT,
        &Err(GridRefusal::new(
            "layer_import_reader_refused",
            "kml: no Placemark",
        )),
    );
    assert_eq!(
        decode_unit_reply(&request, &refused)
            .unwrap_err()
            .to_string(),
        "kml: no Placemark"
    );
    // A crossed or malformed reply is a response refusal.
    let crossed = encode_reply(
        GridOperation::Model(GridModelMethod::LayerExport),
        &Ok(GridResult::Bytes(vec![1])),
    );
    for bad in [
        crossed,
        b"DSG1".to_vec(),
        encode_reply(IMPORT, &Ok(GridResult::Text("{}".into()))),
    ] {
        assert!(matches!(
            decode_unit_reply(&request, &bad),
            Err(LayerImportError::InvalidResponse(_))
        ));
    }
}

#[test]
fn stream_steps_are_admitted_and_decoded() {
    assert!(matches!(
        stream_effect(&StreamStep::Open(" ")),
        Err(LayerImportError::InvalidRequest(_))
    ));
    for (step, method) in [
        (
            StreamStep::Open("s.geojson"),
            GridStreamMethod::GeoJsonStreamOpen,
        ),
        (StreamStep::Push(b"{"), GridStreamMethod::GeoJsonStreamPush),
        (StreamStep::Finish, GridStreamMethod::GeoJsonStreamFinish),
    ] {
        let effect = stream_effect(&step).unwrap();
        assert_eq!(effect.entry, GridOperation::Stream(method).entry());
        assert_eq!(
            GridRequest::decode(&effect.payload).unwrap().operation,
            GridOperation::Stream(method)
        );
    }
    let open = GridOperation::Stream(GridStreamMethod::GeoJsonStreamOpen);
    let opened = encode_reply(open, &Ok(GridResult::Opened));
    assert_eq!(
        decode_stream_reply(GridStreamMethod::GeoJsonStreamOpen, &opened).unwrap(),
        None
    );
    let push = GridOperation::Stream(GridStreamMethod::GeoJsonStreamPush);
    let update = |emitted: &[usize]| {
        serde_json::to_string(
            &json!({"name": "s", "featureCount": 2, "geometryType": null,
            "columns": ["a"], "done": false, "chunkFeatureCounts": emitted}),
        )
        .unwrap()
    };
    let step = encode_reply(
        push,
        &Ok(GridResult::Parts {
            text: update(&[2]),
            blobs: vec![arrow(b"y")],
        }),
    );
    let (decoded, chunks) = decode_stream_reply(GridStreamMethod::GeoJsonStreamPush, &step)
        .unwrap()
        .unwrap();
    assert_eq!(decoded.chunk_feature_counts, [2]);
    assert_eq!(chunks, [arrow(b"y")]);
    // More features emitted than read is refused.
    let over = encode_reply(
        push,
        &Ok(GridResult::Parts {
            text: update(&[3]),
            blobs: vec![arrow(b"y")],
        }),
    );
    assert!(decode_stream_reply(GridStreamMethod::GeoJsonStreamPush, &over).is_err());
}

#[test]
fn temporary_upload_groups_match_the_shared_plan_and_validate_members() {
    let wire = json!({"mode":"layer","sources":[
        {"fileName":"one.zip","layers":[{"name":"a/roads.shp","geometryType":"Point","featureCount":1,"features":[{"properties":{"_ds_source_file":"original"}}],"columns":["id"]}]},
        {"fileName":"two.zip","layers":[{"name":"roads.shp","geometryType":"LineString","featureCount":2,"features":[],"columns":[],"chunkSources":[{"fileName":"two.zip","parsedLayer":"roads"}],"sourceLayers":[{"sourceFileName":"two.zip","sourceLayerName":"roads.shp","featureCount":2,"chunkKey":"k"}]}]}
    ]});
    let request: UploadGroupsRequest = serde_json::from_value(wire).unwrap();
    let groups = temporary_upload_groups(&request).unwrap();
    let expected = ds_upload_rules::upload_groups::plan(
        serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap(),
    );
    assert_eq!(serde_json::to_value(&groups).unwrap(), expected);
    assert_eq!(groups[0].name, "roads_Point");
    assert_eq!(groups[1].name, "roads_LineString");
    assert_eq!(
        groups[1].source_layers[0].reference.extra["chunkKey"],
        json!("k")
    );
    assert_eq!(groups[1].source_layers[0].member, [1, 0, 0]);
    assert_eq!(groups[1].chunk_sources[0].parsed_layer, "roads");

    let mut forged = groups.clone();
    forged[0].members.push([9, 0]);
    assert!(matches!(
        validate_upload_groups(&request, forged),
        Err(LayerImportError::InvalidResponse(_))
    ));
    let mut shape = groups.clone();
    shape[0].geometry_type = "Circle".into();
    assert!(validate_upload_groups(&request, shape).is_err());
    let mut empty = groups;
    empty[0].members.clear();
    assert!(validate_upload_groups(&request, empty).is_err());
    assert!(
        serde_json::from_value::<UploadGroupsRequest>(json!({"mode":"merge","sources":[]}))
            .is_err()
    );
}
