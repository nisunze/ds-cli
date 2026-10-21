#![cfg(target_arch = "wasm32")]
//! WASM boundary and parity checks for the Canvas2D geometry authority.
//!
//! The native suite proves the geometry. This proves the wire: that the packed
//! buffers survive the JS numeric representation unchanged, that a typed
//! refusal arrives as a JS `Error` carrying its machine code, and that the wasm
//! build answers exactly what the native core answers for the same input.

use ds_canvas_graph::GeometrySession;
use ds_interactions_wasm::{CanvasGraphSession, canvas_graph_abi};
use serde_json::Value;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

fn chapter_json() -> String {
    serde_json::json!({
        "orientation": "horizontal",
        "nodes": [
            { "id": "a", "column": 0, "row": 0,
              "ports": [{ "name": "out", "side": "right" }] },
            { "id": "b", "column": 1, "row": 1 },
            { "id": "c", "column": 0, "row": 1, "parent": "a" }
        ],
        "edges": [
            { "id": "a-b", "from": { "node": "a", "port": "out" }, "to": { "node": "b" } },
            { "id": "a-c", "from": { "node": "a" }, "to": { "node": "c" }, "shape": "cubic" }
        ]
    })
    .to_string()
}

fn session() -> CanvasGraphSession {
    let mut session = CanvasGraphSession::open_canvas_graph(&chapter_json()).unwrap();
    session.set_host_surface(800.0, 600.0, 2.0).unwrap();
    session
}

#[wasm_bindgen_test]
fn the_wasm_scene_buffer_is_the_native_scene_buffer() {
    let mut native = GeometrySession::open_json(&chapter_json()).unwrap();
    native.set_host(800.0, 600.0, 2.0).unwrap();

    let session = session();
    assert_eq!(session.scene_buffer(), native.scene_buffer());
    assert_eq!(session.projection_buffer(), native.projection_buffer());
    assert_eq!(
        session.scene_index_json().unwrap(),
        native.scene_index_json().unwrap()
    );
}

#[wasm_bindgen_test]
fn packed_geometry_crosses_as_a_float64array_without_losing_a_value() {
    let session = session();
    let buffer = session.scene_buffer();
    let typed = js_sys::Float64Array::from(buffer.as_slice());

    assert_eq!(typed.length() as usize, buffer.len());
    let round_tripped = typed.to_vec();
    assert_eq!(round_tripped, buffer, "f64 survives the JS number boundary");

    // Header: version, revision, orientation, then the four counts.
    assert_eq!(round_tripped[0], 1.0);
    assert_eq!(round_tripped[1], 1.0);
    assert_eq!(round_tripped[2], 0.0);
    assert_eq!(round_tripped[3], 3.0, "three nodes");
    assert_eq!(round_tripped[4], 1.0, "one port");
    assert_eq!(round_tripped[5], 2.0, "two edges");
    assert_eq!(round_tripped[6], 1.0, "one tether");
}

#[wasm_bindgen_test]
fn the_projection_buffer_is_seven_finite_numbers_per_frame() {
    let mut session = session();
    session.set_viewport(2.0, 12.0, -8.0).unwrap();
    let buffer = session.projection_buffer();

    assert_eq!(buffer.len(), 7);
    assert!(buffer.iter().all(|value| value.is_finite()));
    assert_eq!(buffer[3], 2.0, "device pixel ratio");
    assert_eq!(buffer[4], 2.0, "zoom");
    assert_eq!(buffer[5], 12.0, "panX");
    assert_eq!(buffer[6], -8.0, "panY");
}

#[wasm_bindgen_test]
fn a_pointer_move_answers_with_one_packed_number() {
    let session = session();
    let world = session.world_from_screen(400.0, 300.0).unwrap();
    assert_eq!(world.len(), 2);

    let code = session.hit_test_world(world[0], world[1]).unwrap();
    // Whatever it hit, the code decodes and re-resolves consistently.
    let resolved: Value = serde_json::from_str(&session.hit_target_json(code).unwrap()).unwrap();
    assert!(resolved["kind"].is_string());

    let miss = session.hit_test_world(-100_000.0, -100_000.0).unwrap();
    assert_eq!(miss, 0);
    let resolved: Value = serde_json::from_str(&session.hit_target_json(miss).unwrap()).unwrap();
    assert_eq!(resolved["kind"], "none");
}

#[wasm_bindgen_test]
fn a_drag_returns_a_packed_patch_and_the_new_revision() {
    let mut session = session();
    let scale = session.projection_buffer()[0];
    let patch = session.apply_drag("b", 304.0 * scale, 0.0).unwrap();

    assert_eq!(patch[0], 1.0, "buffer version");
    assert_eq!(patch[1], 2.0, "revision after the move");
    assert_eq!(patch[2], 1.0, "moved");
    assert_eq!(patch[3], 0.0, "not clamped");
    assert_eq!(patch[5], 2.0, "resolved column");
    assert_eq!(patch[6], 1.0, "resolved row");
    assert_eq!(session.scene_revision(), 2);
    assert!(patch.iter().all(|value| value.is_finite()));
}

#[wasm_bindgen_test]
fn a_typed_refusal_arrives_as_a_js_error_carrying_its_machine_code() {
    let error = CanvasGraphSession::open_canvas_graph(
        r#"{"nodes":[{"id":"a","column":0,"row":0}],"edges":[
            {"id":"e","from":{"node":"a"},"to":{"node":"ghost"}}]}"#,
    )
    .unwrap_err();
    assert_eq!(error.code, "canvas_geometry_unknown_node");
    assert_eq!(error.details["node"], "ghost");

    let thrown: JsValue = error.into();
    let js_error: js_sys::Error = thrown.dyn_into().expect("thrown value is a JS Error");
    let envelope: Value = serde_json::from_str(&String::from(js_error.message())).unwrap();
    assert_eq!(envelope["code"], "canvas_geometry_unknown_node");
    assert_eq!(envelope["details"]["node"], "ghost");
}

#[wasm_bindgen_test]
fn non_finite_host_facts_are_refused_on_wasm_too() {
    let mut session = session();
    for (width, height, dpr) in [
        (f64::NAN, 600.0, 1.0),
        (800.0, f64::INFINITY, 1.0),
        (800.0, 600.0, f64::NAN),
    ] {
        let error = session.set_host_surface(width, height, dpr).unwrap_err();
        assert_eq!(error.code, "canvas_geometry_non_finite_input");
    }
    assert_eq!(
        session.apply_drag("a", f64::NAN, 0.0).unwrap_err().code,
        "canvas_geometry_non_finite_input"
    );
}

#[wasm_bindgen_test]
fn both_orientations_produce_their_own_scene_on_wasm() {
    let descriptor: Value = serde_json::from_str(&canvas_graph_abi().unwrap()).unwrap();
    let node_stride = descriptor["nodeStride"].as_u64().unwrap() as usize;

    for (orientation, code) in [("horizontal", 0.0), ("vertical", 1.0)] {
        let input = serde_json::json!({
            "orientation": orientation,
            "nodes": [
                { "id": "a", "column": 0, "row": 0 },
                { "id": "b", "column": 1, "row": 0 }
            ],
            "edges": [{ "id": "a-b", "from": { "node": "a" }, "to": { "node": "b" } }]
        })
        .to_string();
        let session = CanvasGraphSession::open_canvas_graph(&input).unwrap();
        let buffer = session.scene_buffer();
        assert_eq!(buffer[2], code);

        // The second node's origin moves along the flow axis for the
        // orientation in play, and nowhere else.
        let header = 11;
        let second_start = header + node_stride;
        let second = &buffer[second_start..second_start + node_stride];
        if orientation == "horizontal" {
            assert_eq!(second[0], 304.0);
            assert_eq!(second[1], 0.0);
        } else {
            assert_eq!(second[0], 0.0);
            assert_eq!(second[1], 156.0);
        }
    }
}

#[wasm_bindgen_test]
fn the_abi_descriptor_is_exported_and_matches_the_buffers() {
    let descriptor: Value = serde_json::from_str(&canvas_graph_abi().unwrap()).unwrap();
    let session = session();

    assert_eq!(descriptor["version"], session.scene_buffer()[0]);
    assert_eq!(descriptor["sceneHeaderLen"], 11);
    assert_eq!(descriptor["segmentStride"], 7);
    assert_eq!(descriptor["curveOps"]["cubic"], 3);
    assert_eq!(descriptor["hitKinds"]["port"], 2);
    assert_eq!(descriptor["noPort"], -1.0);
}
