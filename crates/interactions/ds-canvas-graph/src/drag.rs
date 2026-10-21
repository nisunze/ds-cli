//! Bounded drag intent.
//!
//! The host reports a pointer delta. It does not decide where the node lands:
//! it asks, and geometry answers with the authoritative cell and the exact
//! parts of the scene that changed. A drag therefore cannot drift, because the
//! result is recomputed from the authored grid rather than accumulated from
//! pointer deltas.
//!
//! Bounded in three ways: a non-finite delta is refused, a delta larger than
//! `metrics.max_drag_world` is refused, and a destination outside the authored
//! grid is clamped back into it and reported as `clamped`. Clamping is the
//! authority's decision and is stated in the outcome; it is not a silent
//! correction the host has to detect.
//!
//! ## Preview and apply run the same code
//!
//! [`plan_drag`] is pure. `preview` returns its outcome and throws the new
//! scene away; `apply` keeps it. There is no second validator that could
//! disagree with the preview the operator was shown.

use serde::Serialize;

use crate::error::{GeometryError, finite};
use crate::input::{GraphInput, NodeId, Orientation};
use crate::layout::build_scene;
use crate::scene::{Bounds, EdgeRoute, NodeRect, Scene, TetherRoute};

/// One drag request, in world units.
#[derive(Clone, Debug, PartialEq)]
pub struct DragIntent {
    pub node: NodeId,
    pub world_dx: f64,
    pub world_dy: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchedNode {
    pub index: usize,
    #[serde(flatten)]
    pub node: NodeRect,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchedEdge {
    pub index: usize,
    #[serde(flatten)]
    pub route: EdgeRoute,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchedTether {
    pub index: usize,
    #[serde(flatten)]
    pub route: TetherRoute,
}

/// Exactly what changed. Node, edge and tether indices are stable across a
/// drag — the graph's membership did not change, only its geometry — which is
/// why a patch can address them by index and stay a few dozen numbers instead
/// of a whole scene.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenePatch {
    pub revision: u32,
    pub nodes: Vec<PatchedNode>,
    pub edges: Vec<PatchedEdge>,
    pub tethers: Vec<PatchedTether>,
    /// Always present: any move can change the scene extent, and a stale
    /// extent silently changes what "fit" means on the next frame.
    pub bounds: Bounds,
}

/// The authoritative answer to one drag intent.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DragOutcome {
    pub node: NodeId,
    pub node_index: usize,
    pub from_column: i32,
    pub from_row: i32,
    pub to_column: i32,
    pub to_row: i32,
    /// False when the delta resolved to the cell the node already occupies.
    pub moved: bool,
    /// True when the requested cell was outside the authored grid.
    pub clamped: bool,
    pub patch: ScenePatch,
}

/// The result of planning a drag: the outcome plus the state that would be
/// committed. `apply` keeps both; `preview` keeps only the outcome.
pub struct DragPlan {
    pub outcome: DragOutcome,
    pub input: GraphInput,
    pub scene: Scene,
}

/// Resolve a drag against the current input and scene. Pure: nothing is
/// mutated, and the caller decides whether to commit. `revision` is the
/// session's current revision; the patch is stamped with the revision the
/// scene would carry once the drag is committed.
pub fn plan_drag(
    input: &GraphInput,
    scene: &Scene,
    intent: &DragIntent,
    revision: u32,
) -> Result<DragPlan, GeometryError> {
    let dx = finite("drag.worldDx", intent.world_dx)?;
    let dy = finite("drag.worldDy", intent.world_dy)?;
    let max = input.metrics.max_drag_world;
    if dx.abs() > max || dy.abs() > max {
        return Err(GeometryError::DragTooLarge {
            dx: format!("{dx}"),
            dy: format!("{dy}"),
            max: format!("{max}"),
        });
    }

    let node_index =
        scene
            .node_index_of(&intent.node)
            .ok_or_else(|| GeometryError::UnknownNode {
                reference: "drag.node".to_string(),
                node: intent.node.clone(),
            })?;

    let metrics = input.metrics;
    let flow_step = metrics.flow_step(input.orientation);
    let cross_step = metrics.cross_step(input.orientation);
    let (flow_delta, cross_delta) = match input.orientation {
        Orientation::Horizontal => (dx, dy),
        Orientation::Vertical => (dy, dx),
    };

    let from_column = scene.nodes[node_index].column;
    let from_row = scene.nodes[node_index].row;
    let wanted_column = i64::from(from_column) + steps(flow_delta, flow_step);
    let wanted_row = i64::from(from_row) + steps(cross_delta, cross_step);
    let to_column = wanted_column.clamp(0, i64::from(metrics.max_column));
    let to_row = wanted_row.clamp(0, i64::from(metrics.max_row));
    let clamped = to_column != wanted_column || to_row != wanted_row;

    let mut next_input = input.clone();
    let target = next_input
        .nodes
        .iter_mut()
        .find(|node| node.id == intent.node)
        .ok_or_else(|| GeometryError::UnknownNode {
            reference: "drag.node".to_string(),
            node: intent.node.clone(),
        })?;
    // The clamp above already proved both fit in i32.
    target.column = to_column as i32;
    target.row = to_row as i32;

    let next_scene = build_scene(&next_input)?;
    let moved = to_column != i64::from(from_column) || to_row != i64::from(from_row);
    // A preview reports the revision the scene *would* carry, so the host can
    // discard a patch that a concurrent apply has already superseded.
    let patch = diff_scene(
        scene,
        &next_scene,
        revision.saturating_add(u32::from(moved)),
    );
    let outcome = DragOutcome {
        node: intent.node.clone(),
        node_index,
        from_column,
        from_row,
        to_column: to_column as i32,
        to_row: to_row as i32,
        moved,
        clamped,
        patch,
    };
    Ok(DragPlan {
        outcome,
        input: next_input,
        scene: next_scene,
    })
}

/// How many authored cells one world delta crosses. Rounded, so a drag settles
/// on the nearest cell rather than the one it has fully entered.
fn steps(delta: f64, step: f64) -> i64 {
    if step <= 0.0 {
        return 0;
    }
    let raw = (delta / step).round();
    // `max_drag_world` bounds `delta`, but a caller may set a very small step.
    raw.clamp(-1.0e15, 1.0e15) as i64
}

/// Emit only what differs. Exact `f64` equality is the right test: both scenes
/// come from the same deterministic code path, so an unchanged node produces
/// bit-identical coordinates or the layout is not deterministic at all.
fn diff_scene(previous: &Scene, next: &Scene, revision: u32) -> ScenePatch {
    ScenePatch {
        revision,
        nodes: next
            .nodes
            .iter()
            .enumerate()
            .filter(|(index, node)| previous.nodes.get(*index) != Some(*node))
            .map(|(index, node)| PatchedNode {
                index,
                node: node.clone(),
            })
            .collect(),
        edges: next
            .edges
            .iter()
            .enumerate()
            .filter(|(index, route)| previous.edges.get(*index) != Some(*route))
            .map(|(index, route)| PatchedEdge {
                index,
                route: route.clone(),
            })
            .collect(),
        tethers: next
            .tethers
            .iter()
            .enumerate()
            .filter(|(index, route)| previous.tethers.get(*index) != Some(*route))
            .map(|(index, route)| PatchedTether {
                index,
                route: route.clone(),
            })
            .collect(),
        bounds: next.bounds,
    }
}
