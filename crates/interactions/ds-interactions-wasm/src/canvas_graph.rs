//! Canvas2D graph geometry — the browser shell over `ds-canvas-graph`.
//!
//! This module owns transport and nothing else. Every position, route, control
//! point, projection, hit answer and drag result comes from
//! `ds_canvas_graph::GeometrySession`, which native callers use directly. There
//! is no layout, no camera math and no hit rule in this file, because a second
//! implementation here is exactly the thing the migration removes.
//!
//! ## Shape of the boundary
//!
//! JSON crosses once, when the graph is opened, and again only for durable or
//! debug reads. The interaction loop speaks numbers:
//!
//! - `sceneBuffer()` → one `Float64Array`, re-read when `sceneRevision()` changes;
//! - `sceneIndexJson()` → the id tables for that revision;
//! - `projectionBuffer()` → 7 numbers, cheap enough for every frame;
//! - `hitTestScreen()` → one packed `u32`;
//! - `previewDrag()` / `applyDrag()` → a packed patch of only what moved.
//!
//! The packed layouts are published by `canvasGraphAbi()` and documented in
//! `docs/contracts/canvas-geometry-authority.md`.

use std::error::Error;
use std::fmt;

use ds_canvas_graph::{GeometryError, GeometrySession, HitTarget};
use serde::Serialize;
use serde_json::Value;
use wasm_bindgen::prelude::*;

/// Route free placed rectangles through the same graph geometry as cell scenes.
#[wasm_bindgen(js_name = canvasGraphRoute)]
pub fn canvas_graph_route(input: &[f64]) -> Result<Vec<f64>, CanvasGeometryBoundaryError> {
    ds_canvas_graph::connector::route_buffer(input).map_err(Into::into)
}

#[wasm_bindgen(js_name = canvasGraphCurveMarkers)]
pub fn canvas_graph_curve_markers(input: &[f64]) -> Result<Vec<f64>, CanvasGeometryBoundaryError> {
    ds_canvas_graph::connector::markers_buffer(input).map_err(Into::into)
}

#[wasm_bindgen(js_name = canvasGraphCurveDistance)]
pub fn canvas_graph_curve_distance(
    input: &[f64],
    x: f64,
    y: f64,
) -> Result<f64, CanvasGeometryBoundaryError> {
    ds_canvas_graph::connector::distance_buffer(input, x, y).map_err(Into::into)
}

/// One explicit arrange/model-initialization intent; JSON never crosses a
/// pointer or animation loop. Results are packed in the supplied node order.
#[wasm_bindgen(js_name = canvasGraphArrange)]
pub fn canvas_graph_arrange(input_json: &str) -> Result<Vec<f64>, CanvasGeometryBoundaryError> {
    if input_json.len() > MAX_CANVAS_GRAPH_INPUT_BYTES {
        return Err(GeometryError::InputTooLarge {
            field: "arrangeJsonBytes".into(),
            count: input_json.len(),
            max: MAX_CANVAS_GRAPH_INPUT_BYTES,
        }
        .into());
    }
    let input: ds_canvas_graph::arrange::ArrangeInput =
        serde_json::from_str(input_json).map_err(|error| GeometryError::InvalidJson {
            detail: error.to_string(),
        })?;
    ds_canvas_graph::arrange::arrange(&input).map_err(Into::into)
}

/// Hard ceiling on the one-shot authored-graph payload. The kernel enforces
/// its own node, edge and port bounds after deserialization.
pub const MAX_CANVAS_GRAPH_INPUT_BYTES: usize = 8 * 1024 * 1024;

/// Stable native/WASM boundary error. On WASM this is thrown as a JS `Error`
/// whose message is this exact JSON object, so a host branches on `code` and
/// never on prose.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasGeometryBoundaryError {
    pub code: String,
    pub message: String,
    pub details: Value,
}

impl CanvasGeometryBoundaryError {
    fn new(code: impl Into<String>, message: impl Into<String>, details: Value) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details,
        }
    }

    /// Canonical error envelope placed in the JavaScript `Error.message`.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            "{\"code\":\"serialize_failed\",\"message\":\"failed to serialize canvas-geometry error\",\"details\":{}}".to_string()
        })
    }

    fn input_too_large(actual: usize) -> Self {
        Self::new(
            "canvas_geometry_input_too_large",
            "canvas graph input exceeds the one-shot JSON byte bound",
            serde_json::json!({
                "payloadBytes": actual,
                "maxPayloadBytes": MAX_CANVAS_GRAPH_INPUT_BYTES,
            }),
        )
    }

    /// Carry the kernel's own operands through unchanged. The typed variant
    /// already serializes to a machine object; re-describing it here would be
    /// a second, driftable copy of the refusal vocabulary.
    fn kernel(error: GeometryError) -> Self {
        let details = serde_json::to_value(&error).unwrap_or_else(|_| serde_json::json!({}));
        Self::new(error.code(), error.to_string(), details)
    }

    fn serialize(error: serde_json::Error) -> Self {
        Self::new(
            "canvas_geometry_serialize_failed",
            "canvas geometry result could not be serialized",
            serde_json::json!({ "category": format!("{:?}", error.classify()).to_lowercase() }),
        )
    }
}

impl fmt::Display for CanvasGeometryBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "[{}] {}", self.code, self.message)
    }
}

impl Error for CanvasGeometryBoundaryError {}

impl From<CanvasGeometryBoundaryError> for JsValue {
    fn from(error: CanvasGeometryBoundaryError) -> Self {
        JsError::new(&error.to_json()).into()
    }
}

impl From<GeometryError> for CanvasGeometryBoundaryError {
    fn from(error: GeometryError) -> Self {
        Self::kernel(error)
    }
}

type BoundaryResult<T> = Result<T, CanvasGeometryBoundaryError>;

/// The packed-buffer layout, opcodes and hit-kind codes this build writes.
/// A host reads it once and derives its reader from it; the native test
/// `abi_descriptor_matches_the_encoder` fails if the two ever disagree.
#[wasm_bindgen(js_name = "canvasGraphAbi")]
pub fn canvas_graph_abi() -> BoundaryResult<String> {
    GeometrySession::abi_descriptor_json().map_err(CanvasGeometryBoundaryError::kernel)
}

/// One live Canvas2D graph surface.
#[derive(Debug)]
#[wasm_bindgen]
pub struct CanvasGraphSession {
    inner: GeometrySession,
}

#[wasm_bindgen]
impl CanvasGraphSession {
    /// Open a session from the authored graph. See
    /// `ds_canvas_graph::GraphInput` for the exact JSON shape; unknown fields
    /// are refused so a misspelled key cannot become a silently missing node.
    #[wasm_bindgen(js_name = "openCanvasGraph")]
    pub fn open_canvas_graph(input_json: &str) -> BoundaryResult<CanvasGraphSession> {
        if input_json.len() > MAX_CANVAS_GRAPH_INPUT_BYTES {
            return Err(CanvasGeometryBoundaryError::input_too_large(
                input_json.len(),
            ));
        }
        let inner = GeometrySession::open_json(input_json)?;
        crate::log_info(&format!(
            "openCanvasGraph: {} nodes, {} ports, {} edges, {} tethers",
            inner.scene().nodes.len(),
            inner.scene().ports.len(),
            inner.scene().edges.len(),
            inner.scene().tethers.len(),
        ));
        Ok(CanvasGraphSession { inner })
    }

    /// Bumps only when scene geometry changes. Camera and window changes do
    /// not bump it, so a host re-reads the scene buffer only when it must.
    #[wasm_bindgen(js_name = "sceneRevision")]
    pub fn scene_revision(&self) -> u32 {
        self.inner.revision()
    }

    #[wasm_bindgen(js_name = "arrangeGraph")]
    pub fn arrange_graph(&mut self) -> BoundaryResult<bool> {
        self.inner.arrange_cells().map_err(Into::into)
    }

    /// Canvas size in CSS pixels plus the device-pixel ratio.
    #[wasm_bindgen(js_name = "setHostSurface")]
    pub fn set_host_surface(
        &mut self,
        width: f64,
        height: f64,
        device_pixel_ratio: f64,
    ) -> BoundaryResult<()> {
        Ok(self.inner.set_host(width, height, device_pixel_ratio)?)
    }

    #[wasm_bindgen(js_name = "setViewport")]
    pub fn set_viewport(&mut self, zoom: f64, pan_x: f64, pan_y: f64) -> BoundaryResult<()> {
        Ok(self.inner.set_viewport(zoom, pan_x, pan_y)?)
    }

    #[wasm_bindgen(js_name = "fitViewport")]
    pub fn fit_viewport(&mut self) {
        self.inner.fit();
    }

    #[wasm_bindgen(js_name = "panViewport")]
    pub fn pan_viewport(&mut self, dx: f64, dy: f64) -> BoundaryResult<()> {
        Ok(self.inner.pan(dx, dy)?)
    }

    #[wasm_bindgen(js_name = "zoomViewportCentered")]
    pub fn zoom_viewport_centered(&mut self, factor: f64) -> BoundaryResult<()> {
        Ok(self.inner.zoom_centered(factor)?)
    }

    /// The wheel gesture: zoom while the world point under the cursor stays put.
    #[wasm_bindgen(js_name = "zoomViewportAt")]
    pub fn zoom_viewport_at(
        &mut self,
        anchor_x: f64,
        anchor_y: f64,
        factor: f64,
    ) -> BoundaryResult<()> {
        Ok(self.inner.zoom_at(anchor_x, anchor_y, factor)?)
    }

    #[wasm_bindgen(js_name = "centerViewportOn")]
    pub fn center_viewport_on(&mut self, world_x: f64, world_y: f64) -> BoundaryResult<()> {
        Ok(self.inner.center_on(world_x, world_y)?)
    }

    /// `[scale, offsetX, offsetY, devicePixelRatio, zoom, panX, panY]`.
    /// A renderer applies it as
    /// `setTransform(scale, 0, 0, scale, offsetX, offsetY)`.
    #[wasm_bindgen(js_name = "projectionBuffer")]
    pub fn projection_buffer(&self) -> Vec<f64> {
        self.inner.projection_buffer()
    }

    /// The whole scene, packed. Re-read when `sceneRevision()` changes.
    #[wasm_bindgen(js_name = "sceneBuffer")]
    pub fn scene_buffer(&self) -> Vec<f64> {
        self.inner.scene_buffer()
    }

    /// `[x, y]` in world coordinates.
    #[wasm_bindgen(js_name = "worldFromScreen")]
    pub fn world_from_screen(&self, x: f64, y: f64) -> BoundaryResult<Vec<f64>> {
        let point = self.inner.world_from_screen(x, y)?;
        Ok(vec![point.x, point.y])
    }

    /// `[x, y]` in CSS pixels.
    #[wasm_bindgen(js_name = "screenFromWorld")]
    pub fn screen_from_world(&self, x: f64, y: f64) -> BoundaryResult<Vec<f64>> {
        let point = self.inner.screen_from_world(x, y)?;
        Ok(vec![point.x, point.y])
    }

    /// The pointer-move entry point. Returns one packed code: kind in the top
    /// four bits, dense index in the low 28. Nothing is allocated per event.
    #[wasm_bindgen(js_name = "hitTestScreen")]
    pub fn hit_test_screen(&self, x: f64, y: f64) -> BoundaryResult<u32> {
        Ok(self.inner.hit_test_screen(x, y)?.code())
    }

    #[wasm_bindgen(js_name = "hitTestWorld")]
    pub fn hit_test_world(&self, x: f64, y: f64) -> BoundaryResult<u32> {
        Ok(self.inner.hit_test_world(x, y)?.code())
    }

    /// Resolve a packed code to `{ "kind": …, "index": … }` plus the identity.
    /// Called only when the hovered target changes.
    #[wasm_bindgen(js_name = "hitTargetJson")]
    pub fn hit_target_json(&self, code: u32) -> BoundaryResult<String> {
        let target = self.inner.hit_target(code)?;
        let scene = self.inner.scene();
        let identity = match target {
            HitTarget::None => serde_json::json!({}),
            HitTarget::Node { index } => serde_json::json!({ "node": scene.nodes[index].id }),
            HitTarget::Port { index } => serde_json::json!({
                "node": scene.ports[index].node_id,
                "port": scene.ports[index].name,
            }),
            HitTarget::Edge { index } => serde_json::json!({ "edge": scene.edges[index].id }),
            HitTarget::Tether { index } => serde_json::json!({
                "child": scene.nodes[scene.tethers[index].child_node_index].id,
                "parent": scene.nodes[scene.tethers[index].parent_node_index].id,
            }),
        };
        let mut value =
            serde_json::to_value(target).map_err(CanvasGeometryBoundaryError::serialize)?;
        if let (Some(object), Some(extra)) = (value.as_object_mut(), identity.as_object()) {
            object.extend(extra.clone());
        }
        serde_json::to_string(&value).map_err(CanvasGeometryBoundaryError::serialize)
    }

    /// What a drag would do, without committing it. Screen deltas; the
    /// authority divides by the current scale.
    #[wasm_bindgen(js_name = "previewDrag")]
    pub fn preview_drag(
        &self,
        node_id: &str,
        screen_dx: f64,
        screen_dy: f64,
    ) -> BoundaryResult<Vec<f64>> {
        let outcome = self
            .inner
            .preview_drag_screen(node_id, screen_dx, screen_dy)?;
        Ok(GeometrySession::drag_buffer(&outcome))
    }

    /// Commit a drag and return the packed patch of exactly what changed.
    #[wasm_bindgen(js_name = "applyDrag")]
    pub fn apply_drag(
        &mut self,
        node_id: &str,
        screen_dx: f64,
        screen_dy: f64,
    ) -> BoundaryResult<Vec<f64>> {
        let outcome = self
            .inner
            .apply_drag_screen(node_id, screen_dx, screen_dy)?;
        Ok(GeometrySession::drag_buffer(&outcome))
    }

    /// Id tables for the current revision — the only place strings cross.
    #[wasm_bindgen(js_name = "sceneIndexJson")]
    pub fn scene_index_json(&self) -> BoundaryResult<String> {
        Ok(self.inner.scene_index_json()?)
    }

    /// Durable/debug interchange only. Never call this per frame.
    #[wasm_bindgen(js_name = "sceneJson")]
    pub fn scene_json(&self) -> BoundaryResult<String> {
        Ok(self.inner.scene_json()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter_json() -> String {
        serde_json::json!({
            "orientation": "horizontal",
            "nodes": [
                { "id": "a", "column": 0, "row": 0,
                  "ports": [{ "name": "out", "side": "right" }] },
                { "id": "b", "column": 1, "row": 0 },
                { "id": "c", "column": 0, "row": 1, "parent": "a" }
            ],
            "edges": [
                { "id": "a-b", "from": { "node": "a", "port": "out" }, "to": { "node": "b" } },
                { "id": "a-c", "from": { "node": "a" }, "to": { "node": "c" } }
            ]
        })
        .to_string()
    }

    fn session() -> CanvasGraphSession {
        let mut session = CanvasGraphSession::open_canvas_graph(&chapter_json()).unwrap();
        session.set_host_surface(800.0, 600.0, 2.0).unwrap();
        session
    }

    #[test]
    fn the_shell_returns_exactly_what_the_core_computes() {
        let session = session();
        let core = {
            let mut core = GeometrySession::open_json(&chapter_json()).unwrap();
            core.set_host(800.0, 600.0, 2.0).unwrap();
            core
        };

        assert_eq!(session.scene_buffer(), core.scene_buffer());
        assert_eq!(session.projection_buffer(), core.projection_buffer());
        assert_eq!(session.scene_revision(), core.revision());
        assert_eq!(
            session.scene_index_json().unwrap(),
            core.scene_index_json().unwrap()
        );
        assert_eq!(session.scene_json().unwrap(), core.scene_json().unwrap());
    }

    #[test]
    fn the_hot_loop_answers_with_numbers_not_json() {
        let session = session();
        let index = session.inner.scene().node_index_of("b").unwrap();
        let centre = session.inner.scene().nodes[index].rect.center();
        let screen = session.screen_from_world(centre.x, centre.y).unwrap();

        let code = session.hit_test_screen(screen[0], screen[1]).unwrap();
        assert_eq!(code >> 28, 1, "node kind");
        assert_eq!(code & 0x0fff_ffff, index as u32);

        let resolved: Value =
            serde_json::from_str(&session.hit_target_json(code).unwrap()).unwrap();
        assert_eq!(resolved["kind"], "node");
        assert_eq!(resolved["index"], index);
        assert_eq!(resolved["node"], "b");
    }

    #[test]
    fn a_port_hit_resolves_to_its_node_and_name() {
        let session = session();
        let anchor = session.inner.scene().ports[0].point;
        let code = session.hit_test_world(anchor.x, anchor.y).unwrap();
        let resolved: Value =
            serde_json::from_str(&session.hit_target_json(code).unwrap()).unwrap();

        assert_eq!(resolved["kind"], "port");
        assert_eq!(resolved["node"], "a");
        assert_eq!(resolved["port"], "out");
    }

    #[test]
    fn a_tether_hit_names_both_ends_of_the_containment() {
        let session = session();
        let rail = session.inner.scene().tethers[0].polyline[1];
        let code = session.hit_test_world(rail.x, rail.y + 20.0).unwrap();
        let resolved: Value =
            serde_json::from_str(&session.hit_target_json(code).unwrap()).unwrap();

        assert_eq!(resolved["kind"], "tether");
        assert_eq!(resolved["child"], "c");
        assert_eq!(resolved["parent"], "a");
    }

    #[test]
    fn a_drag_patch_crosses_as_packed_numbers_and_bumps_the_revision() {
        let mut session = session();
        let scale = session.projection_buffer()[0];
        let before = session.scene_revision();

        let empty = session.preview_drag("b", 1.0, 1.0).unwrap();
        assert_eq!(
            session.scene_revision(),
            before,
            "a preview commits nothing"
        );

        let patch = session.apply_drag("b", 304.0 * scale, 0.0).unwrap();
        assert_ne!(patch, empty);
        assert_eq!(patch[0], 1.0, "buffer version");
        assert_eq!(patch[2], 1.0, "moved");
        assert_eq!(patch[5], 2.0, "resolved column");
        assert_eq!(session.scene_revision(), before + 1);
    }

    #[test]
    fn refusals_keep_the_kernel_code_and_operands_at_the_boundary() {
        let error = CanvasGraphSession::open_canvas_graph(
            r#"{"nodes":[{"id":"a","column":0,"row":0},{"id":"a","column":1,"row":0}]}"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "canvas_geometry_duplicate_node");
        assert_eq!(error.details["id"], "a");
        assert_eq!(error.details["code"], "canvas_geometry_duplicate_node");

        let envelope: Value = serde_json::from_str(&error.to_json()).unwrap();
        assert_eq!(envelope["code"], "canvas_geometry_duplicate_node");
        assert!(envelope["message"].as_str().unwrap().contains("duplicate"));

        let mut session = session();
        assert_eq!(
            session
                .set_host_surface(f64::NAN, 600.0, 1.0)
                .unwrap_err()
                .code,
            "canvas_geometry_non_finite_input"
        );
        assert_eq!(
            session.apply_drag("ghost", 1.0, 1.0).unwrap_err().code,
            "canvas_geometry_unknown_node"
        );
        assert_eq!(
            session
                .hit_test_screen(f64::INFINITY, 0.0)
                .unwrap_err()
                .code,
            "canvas_geometry_non_finite_input"
        );
        assert_eq!(
            session.hit_target_json(9_999_999).unwrap_err().code,
            "canvas_geometry_invalid_hit_code"
        );
    }

    #[test]
    fn the_one_shot_payload_bound_is_checked_before_json_parsing() {
        let input = " ".repeat(MAX_CANVAS_GRAPH_INPUT_BYTES + 1);
        let error = CanvasGraphSession::open_canvas_graph(&input).unwrap_err();
        assert_eq!(error.code, "canvas_geometry_input_too_large");
        assert_eq!(
            error.details,
            serde_json::json!({
                "payloadBytes": MAX_CANVAS_GRAPH_INPUT_BYTES + 1,
                "maxPayloadBytes": MAX_CANVAS_GRAPH_INPUT_BYTES,
            })
        );
    }

    #[test]
    fn the_published_abi_descriptor_is_the_one_the_buffers_use() {
        let descriptor: Value = serde_json::from_str(&canvas_graph_abi().unwrap()).unwrap();
        let session = session();
        let buffer = session.scene_buffer();

        assert_eq!(descriptor["version"], buffer[0]);
        let header = descriptor["sceneHeaderLen"].as_u64().unwrap() as usize;
        let node_stride = descriptor["nodeStride"].as_u64().unwrap() as usize;
        // The first node's rectangle sits immediately after the header.
        let first = &buffer[header..header + node_stride];
        assert_eq!(first[2], 212.0, "default node width");
        assert_eq!(first[3], 64.0, "default node height");
        assert_eq!(descriptor["hitKinds"]["tether"], 4);
    }

    #[test]
    fn both_orientations_cross_the_boundary() {
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
            assert_eq!(session.scene_buffer()[2], code);

            let index: Value = serde_json::from_str(&session.scene_index_json().unwrap()).unwrap();
            assert_eq!(index["orientation"], orientation);
        }
    }
}
