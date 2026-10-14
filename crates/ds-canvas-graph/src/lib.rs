//! ds-canvas-graph — the Canvas2D graph geometry authority.
//!
//! Rust owns every position a graph canvas draws: node rectangles, named port
//! anchors, edge routes and their control points, containment tethers, scene
//! bounds, the world-to-window projection, what is under the pointer, and where
//! a dragged node lands. The host supplies window facts and user intent —
//! canvas size, device-pixel ratio, pointer events, the requested operation —
//! and renders the answer. It does not recompute, correct or persist geometry.
//!
//! This is the boundary the DS Grid Profile surface already proves: Rust/WASM
//! computes, Canvas2D presents. The contract that makes it the rule for every
//! canvas surface is
//! `ds-web/docs/specs/RUST_WASM_CANVAS_GEOMETRY_MIGRATION_CONTRACT.md`; the ABI
//! is documented in `docs/contracts/canvas-geometry-authority.md`.
//!
//! ## Domain-neutral on purpose
//!
//! There is no lane, kind, confidence, evidence, delivery state, task or
//! milestone in this crate. Two surfaces sharing one geometry authority must
//! not be able to reach each other's business objects through it — they share
//! the geometry, not the domain. A consumer keeps its own model and speaks to
//! this crate in identities, authored cells and named ports.
//!
//! ## The three things that cross the boundary
//!
//! | | shape | when |
//! |---|---|---|
//! | scene | packed `f64` buffer + id tables | once per revision |
//! | projection | 7 `f64` | per frame |
//! | hit / drag | one `u32` / a packed patch | per pointer event |
//!
//! JSON is available for durable interchange and debugging. It is not on the
//! interaction path.
//!
//! ## One core, two hosts
//!
//! Everything here is `no_std`-shaped pure computation with no wasm
//! dependency: native UI hosts use [`GeometrySession`] directly, and
//! `ds-interactions-wasm` wraps that same type for the browser. There is no second
//! implementation to drift.

pub mod abi;
pub mod arrange;
pub mod composition;
pub mod connector;
pub mod curve;
pub mod drag;
pub mod error;
pub mod free_hit;
pub mod hit;
pub mod input;
pub mod json;
pub mod layout;
pub mod scene;
pub mod session;
pub mod viewport;

pub use abi::{
    AbiDescriptor, SCENE_BUFFER_VERSION, SceneIndex, abi_descriptor, decode_drag, decode_scene,
    encode_drag, encode_scene,
};
pub use curve::{CURVE_OP_CUBIC, CURVE_OP_LINE, CURVE_OP_QUADRATIC, Curve, CurveSegment, Point};
pub use drag::{DragIntent, DragOutcome, ScenePatch, plan_drag};
pub use error::GeometryError;
pub use hit::{HitTarget, HitTolerance, hit_test};
pub use input::{
    EdgeInput, EdgeShape, EndpointInput, GraphInput, LayoutMetrics, NodeInput, Orientation,
    PortSide, PortSpec,
};
pub use json::{error_to_json, input_from_json, scene_to_json};
pub use layout::build_scene;
pub use scene::{Bounds, EdgeRoute, NodeRect, PortAnchor, Rect, Scene, TetherRoute};
pub use session::GeometrySession;
pub use viewport::{HostSurface, Projection, Viewport};

/// Bumped when the geometry this crate produces changes in a way a pinned
/// fixture would notice.
pub const CANVAS_GEOMETRY_VERSION: &str = "1";
