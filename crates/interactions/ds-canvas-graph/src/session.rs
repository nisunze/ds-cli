//! The interaction handle.
//!
//! One session owns the graph, the scene computed from it, the host's window
//! facts and the camera. The hot loop calls methods on it and reads packed
//! numbers back; it does not rebuild the graph, and it never re-parses JSON to
//! answer a pointer move.
//!
//! Native callers use exactly this type. The wasm wrapper adds a `#[wasm_bindgen]`
//! shell and nothing else — no second layout, no second projection, no second
//! opinion about what is under the cursor.
//!
//! ```
//! use ds_canvas_graph::{GeometrySession, GraphInput, NodeInput};
//!
//! let input = GraphInput {
//!     nodes: vec![NodeInput {
//!         id: "a".into(),
//!         column: 0,
//!         row: 0,
//!         width: None,
//!         height: None,
//!         parent: None,
//!         ports: vec![],
//!     }],
//!     ..GraphInput::default()
//! };
//! let mut session = GeometrySession::open(input).unwrap();
//! session.set_host(800.0, 600.0, 2.0).unwrap();
//! assert_eq!(session.scene().nodes.len(), 1);
//! ```

use crate::abi::{
    DecodedScene, SceneIndex, abi_descriptor, encode_drag, encode_scene, scene_index,
};
use crate::curve::Point;
use crate::drag::{DragIntent, DragOutcome};
use crate::error::{GeometryError, finite};
use crate::hit::{HitTarget, HitTolerance};
use crate::input::{GraphInput, NodeId};
use crate::json;
use crate::layout::build_scene;
use crate::scene::Scene;
use crate::viewport::{HostSurface, Projection, Viewport};
use crate::{drag, viewport};

/// One live canvas surface.
#[derive(Clone, Debug)]
pub struct GeometrySession {
    input: GraphInput,
    scene: Scene,
    host: HostSurface,
    viewport: Viewport,
    revision: u32,
}

impl GeometrySession {
    /// Build a session. Refuses the same inputs [`build_scene`] refuses; a
    /// session that exists always holds an exact scene.
    pub fn open(input: GraphInput) -> Result<Self, GeometryError> {
        let scene = build_scene(&input)?;
        Ok(Self {
            input,
            scene,
            host: HostSurface::default(),
            viewport: Viewport::fitted(),
            revision: 1,
        })
    }

    /// Durable/debug entry point. JSON is the interchange format, never the
    /// per-frame one.
    pub fn open_json(input_json: &str) -> Result<Self, GeometryError> {
        Self::open(json::input_from_json(input_json)?)
    }

    /// Bumps whenever the scene geometry changes. The host re-reads the scene
    /// buffer and the id tables only when this changes.
    pub fn revision(&self) -> u32 {
        self.revision
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn input(&self) -> &GraphInput {
        &self.input
    }

    /// Explicit ephemeral arrangement of authored-cell scenes. The same free
    /// arrangement owner supplies the proposal; this native adapter resolves
    /// its positions onto the session's grid and commits the scene atomically.
    /// Domain manifests and node/edge identities are never rewritten.
    pub fn arrange_cells(&mut self) -> Result<bool, GeometryError> {
        use crate::arrange::{ArrangeEdge, ArrangeInput, ArrangeNode, arrange};
        let request = ArrangeInput {
            nodes: self
                .scene
                .nodes
                .iter()
                .map(|node| ArrangeNode {
                    id: node.id.clone(),
                    width: node.rect.width,
                    height: node.rect.height,
                    minimum_flow: 0.0,
                    day: None,
                })
                .collect(),
            edges: self
                .input
                .edges
                .iter()
                .map(|edge| ArrangeEdge {
                    from: edge.from.node.clone(),
                    to: edge.to.node.clone(),
                })
                .collect(),
            orientation: self.input.orientation,
            gap_x: self.input.metrics.column_gap,
            gap_y: self.input.metrics.row_gap,
            origin: None,
            day_width: 0.0,
        };
        let proposal = arrange(&request)?;
        let mut input = self.input.clone();
        let flow_step = input.metrics.flow_step(input.orientation);
        let cross_step = input.metrics.cross_step(input.orientation);
        let mut changed = false;
        for (index, node) in input.nodes.iter_mut().enumerate() {
            let x = proposal[2 + index * 3];
            let y = proposal[3 + index * 3];
            let (flow, cross) = match input.orientation {
                crate::Orientation::Horizontal => (x, y),
                crate::Orientation::Vertical => (y, x),
            };
            let column = (flow / flow_step).round() as i32;
            let row = (cross / cross_step).round() as i32;
            changed |= node.column != column || node.row != row;
            node.column = column;
            node.row = row;
        }
        if !changed {
            return Ok(false);
        }
        let scene = build_scene(&input)?;
        self.input = input;
        self.scene = scene;
        self.revision = self.revision.saturating_add(1);
        Ok(true)
    }

    pub fn host(&self) -> HostSurface {
        self.host
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Host facts only: canvas size in CSS pixels and the device-pixel ratio.
    /// None of this is graph state, so it does not bump the revision.
    pub fn set_host(
        &mut self,
        width: f64,
        height: f64,
        device_pixel_ratio: f64,
    ) -> Result<(), GeometryError> {
        self.host = HostSurface::new(width, height, device_pixel_ratio)?;
        Ok(())
    }

    pub fn set_viewport(&mut self, zoom: f64, pan_x: f64, pan_y: f64) -> Result<(), GeometryError> {
        self.viewport = Viewport::new(zoom, pan_x, pan_y)?;
        Ok(())
    }

    pub fn fit(&mut self) {
        self.viewport = Viewport::fitted();
    }

    pub fn pan(&mut self, dx: f64, dy: f64) -> Result<(), GeometryError> {
        self.viewport = viewport::pan(&self.viewport, dx, dy)?;
        Ok(())
    }

    pub fn zoom_centered(&mut self, factor: f64) -> Result<(), GeometryError> {
        self.viewport = viewport::zoom_centered(&self.viewport, factor)?;
        Ok(())
    }

    pub fn zoom_at(
        &mut self,
        anchor_x: f64,
        anchor_y: f64,
        factor: f64,
    ) -> Result<(), GeometryError> {
        self.viewport = viewport::zoom_at(
            &self.scene.bounds,
            &self.host,
            &self.viewport,
            anchor_x,
            anchor_y,
            factor,
        )?;
        Ok(())
    }

    pub fn center_on(&mut self, world_x: f64, world_y: f64) -> Result<(), GeometryError> {
        self.viewport = viewport::center_on(
            &self.scene.bounds,
            &self.host,
            &self.viewport,
            world_x,
            world_y,
        )?;
        Ok(())
    }

    pub fn projection(&self) -> Projection {
        viewport::projection(&self.scene.bounds, &self.host, &self.viewport)
    }

    pub fn world_from_screen(&self, x: f64, y: f64) -> Result<Point, GeometryError> {
        let point = Point::new(finite("screen.x", x)?, finite("screen.y", y)?);
        Ok(self.projection().unproject(point))
    }

    pub fn screen_from_world(&self, x: f64, y: f64) -> Result<Point, GeometryError> {
        let point = Point::new(finite("world.x", x)?, finite("world.y", y)?);
        Ok(self.projection().project(point))
    }

    /// Hit tolerances in world units for the current scale, so a port keeps
    /// the same physical target size at every zoom.
    fn tolerance(&self, scale: f64) -> HitTolerance {
        HitTolerance {
            port_radius: self.input.metrics.port_hit_radius_px / scale,
            edge_tolerance: self.input.metrics.edge_hit_tolerance_px / scale,
        }
    }

    pub fn hit_test_world(&self, x: f64, y: f64) -> Result<HitTarget, GeometryError> {
        let point = Point::new(finite("hit.worldX", x)?, finite("hit.worldY", y)?);
        let scale = self.projection().scale;
        Ok(crate::hit::hit_test(
            &self.scene,
            point,
            self.tolerance(scale),
        ))
    }

    /// The pointer-move entry point. One unproject, one index walk, one packed
    /// answer.
    pub fn hit_test_screen(&self, x: f64, y: f64) -> Result<HitTarget, GeometryError> {
        let point = Point::new(finite("hit.screenX", x)?, finite("hit.screenY", y)?);
        let projection = self.projection();
        Ok(crate::hit::hit_test(
            &self.scene,
            projection.unproject(point),
            self.tolerance(projection.scale),
        ))
    }

    /// Resolve a packed hit code back to its identity. Called only when the
    /// hovered target changes, which is the whole point of the packed code.
    pub fn hit_target(&self, code: u32) -> Result<HitTarget, GeometryError> {
        HitTarget::from_code(code, &self.scene)
    }

    /// What would happen, without committing it.
    pub fn preview_drag_world(
        &self,
        node: &str,
        world_dx: f64,
        world_dy: f64,
    ) -> Result<DragOutcome, GeometryError> {
        Ok(self.plan(node, world_dx, world_dy)?.outcome)
    }

    /// Commit a drag. The scene becomes the returned outcome's scene, and the
    /// revision bumps only when the node actually changed cell.
    pub fn apply_drag_world(
        &mut self,
        node: &str,
        world_dx: f64,
        world_dy: f64,
    ) -> Result<DragOutcome, GeometryError> {
        let plan = self.plan(node, world_dx, world_dy)?;
        if plan.outcome.moved {
            self.revision = self.revision.saturating_add(1);
            self.input = plan.input;
            self.scene = plan.scene;
        }
        Ok(plan.outcome)
    }

    /// Screen deltas are divided by the current scale before they mean
    /// anything: a pixel is not a world unit, and only the projection knows
    /// the difference.
    pub fn preview_drag_screen(
        &self,
        node: &str,
        screen_dx: f64,
        screen_dy: f64,
    ) -> Result<DragOutcome, GeometryError> {
        let (dx, dy) = self.world_delta(screen_dx, screen_dy)?;
        self.preview_drag_world(node, dx, dy)
    }

    pub fn apply_drag_screen(
        &mut self,
        node: &str,
        screen_dx: f64,
        screen_dy: f64,
    ) -> Result<DragOutcome, GeometryError> {
        let (dx, dy) = self.world_delta(screen_dx, screen_dy)?;
        self.apply_drag_world(node, dx, dy)
    }

    fn world_delta(&self, screen_dx: f64, screen_dy: f64) -> Result<(f64, f64), GeometryError> {
        let dx = finite("drag.screenDx", screen_dx)?;
        let dy = finite("drag.screenDy", screen_dy)?;
        let scale = self.projection().scale;
        Ok((dx / scale, dy / scale))
    }

    fn plan(
        &self,
        node: &str,
        world_dx: f64,
        world_dy: f64,
    ) -> Result<drag::DragPlan, GeometryError> {
        drag::plan_drag(
            &self.input,
            &self.scene,
            &DragIntent {
                node: NodeId::from(node),
                world_dx,
                world_dy,
            },
            self.revision,
        )
    }

    /// The whole scene, packed. Re-read when [`Self::revision`] changes.
    pub fn scene_buffer(&self) -> Vec<f64> {
        encode_scene(&self.scene, self.revision)
    }

    /// `[scale, offsetX, offsetY, devicePixelRatio, zoom, panX, panY]`. Cheap
    /// enough to read every frame — it is what pan and zoom actually change.
    pub fn projection_buffer(&self) -> Vec<f64> {
        let projection = self.projection();
        vec![
            projection.scale,
            projection.offset_x,
            projection.offset_y,
            projection.device_pixel_ratio,
            self.viewport.zoom,
            self.viewport.pan_x,
            self.viewport.pan_y,
        ]
    }

    /// A drag answer, packed.
    pub fn drag_buffer(outcome: &DragOutcome) -> Vec<f64> {
        encode_drag(outcome)
    }

    pub fn scene_index(&self) -> SceneIndex {
        scene_index(&self.scene, self.revision)
    }

    /// Durable/debug interchange only.
    pub fn scene_json(&self) -> Result<String, GeometryError> {
        json::scene_to_json(&self.scene, self.revision)
    }

    pub fn scene_index_json(&self) -> Result<String, GeometryError> {
        json::to_json(&self.scene_index())
    }

    pub fn abi_descriptor_json() -> Result<String, GeometryError> {
        json::to_json(&abi_descriptor())
    }

    /// Decode this session's own scene buffer. Exists so a native caller — and
    /// the tests — exercise exactly the reader a browser host must write.
    pub fn decoded_scene_buffer(&self) -> Result<DecodedScene, GeometryError> {
        crate::abi::decode_scene(&self.scene_buffer())
    }
}
