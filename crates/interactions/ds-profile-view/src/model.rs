use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Point {
    pub chain_station_m: f64,
    #[serde(skip)]
    pub path_order: i8,
    pub scene_x: f64,
    pub scene_y: f64,
    pub station_m: f64,
    pub elevation_m: f64,
}
impl Point {
    pub fn xy(x: f64, y: f64) -> Self {
        Self {
            scene_x: x,
            scene_y: y,
            ..Self::default()
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
impl From<Bounds> for ds_canvas::Bounds {
    fn from(b: Bounds) -> Self {
        Self {
            min_x: b.min_x,
            min_y: b.min_y,
            max_x: b.max_x,
            max_y: b.max_y,
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Scene {
    #[serde(skip)]
    pub feature_baselines: BTreeMap<(String, u64), Point>,
    pub schema_version: u32,
    pub model_revision: String,
    pub computed_revision: Option<String>,
    pub provisional_geometry: bool,
    pub bounds: Bounds,
    pub plot_bounds: Bounds,
    pub axis_pin: Option<Axis>,
    pub scale: SceneScale,
    pub bands: Vec<Band>,
    pub conductor_spans: Vec<Span>,
    pub clearance_guides: Vec<Guide>,
    pub authored_connections: Vec<Connection>,
    pub selection_order: SelectionOrder,
    pub display: Option<Display>,
    pub engineering: Option<Engineering>,
    pub grid: Grid,
    pub foundation: Foundation,
}
impl Scene {
    pub fn current(&self) -> bool {
        !self.provisional_geometry
            && self
                .computed_revision
                .as_ref()
                .is_none_or(|r| r == &self.model_revision)
    }
    pub fn point(&self, id: &str) -> Option<Point> {
        self.bands.iter().find_map(|b| {
            b.structures
                .iter()
                .find(|s| s.entity_id == id)
                .map(|s| s.top.unwrap_or(s.ground))
                .or_else(|| {
                    b.terrain_points
                        .iter()
                        .find(|p| p.entity_id == id)
                        .map(|p| p.point)
                })
        })
    }
    pub fn axis_digest(&self) -> &str {
        self.axis_pin
            .as_ref()
            .map(|a| a.digest.as_str())
            .unwrap_or("")
    }
    pub fn validate(&self) -> Result<(), String> {
        let b = &self.bounds;
        if ![b.min_x, b.min_y, b.max_x, b.max_y]
            .iter()
            .all(|x| x.is_finite())
            || b.min_x > b.max_x
            || b.min_y > b.max_y
        {
            return Err("profile_view_invalid_bounds".into());
        }
        let mut count = 0usize;
        let mut admit = |p: &Point| {
            count += 1;
            p.scene_x.is_finite() && p.scene_y.is_finite() && count <= 2_000_000
        };
        for b in &self.bands {
            for s in &b.structures {
                if !admit(&s.ground) || !admit(&s.base) || s.top.as_ref().is_some_and(|p| !admit(p))
                {
                    return Err("profile_view_invalid_geometry".into());
                }
            }
            for p in &b.terrain_points {
                if !admit(&p.point) {
                    return Err("profile_view_invalid_geometry".into());
                }
            }
            for g in &b.ground {
                for p in &g.points {
                    if !admit(p) {
                        return Err("profile_view_invalid_geometry".into());
                    }
                }
            }
        }
        for s in &self.conductor_spans {
            for p in &s.points {
                if !admit(p) {
                    return Err("profile_view_invalid_geometry".into());
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SceneScale {
    pub horizontal_scene_units_per_m: f64,
    pub vertical_scene_units_per_m: f64,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Axis {
    pub digest: String,
    pub vertical_px_per_m: f64,
    pub domain_min_m: f64,
    pub domain_max_m: f64,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Band {
    pub alignment_id: String,
    pub label: String,
    pub station_axis: StationAxis,
    pub route: Vec<RoutePoint>,
    pub ground: Vec<Ground>,
    pub side_profiles: Vec<SideProfile>,
    pub terrain_points: Vec<TerrainPoint>,
    pub structures: Vec<Structure>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct StationAxis {
    pub minimum_chain_station_m: f64,
    pub maximum_chain_station_m: f64,
    pub minimum_scene_x: f64,
    pub maximum_scene_x: f64,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct RoutePoint {
    pub entity_id: String,
    pub point: Point,
    pub ordinate_kind: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Ground {
    pub source_class: String,
    pub points: Vec<Point>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SideProfile {
    pub style: Stroke,
    pub segments: Vec<Ground>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct TerrainPoint {
    pub entity_id: String,
    pub point: Point,
    pub source_class: String,
    pub feature_clearance: Option<FeatureClearance>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct FeatureClearance {
    pub feature_name: String,
    pub feature_code: i32,
    pub required_vertical_m: f64,
    pub height_default_used: bool,
    pub baseline: Point,
    pub threshold: Point,
    pub point_color: String,
    pub display_margin_m: Option<f64>,
    pub lowest_display_wire_elevation_m: Option<f64>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Structure {
    pub network_role: Option<String>,
    pub outgoing_feeder_count: Option<usize>,
    pub engineering_number: Option<String>,
    pub structure_type_id: String,
    pub entity_id: String,
    pub engineering_name: String,
    pub display_label: Option<String>,
    pub ground: Point,
    pub base: Point,
    pub top: Option<Point>,
    pub attachment_points: Vec<Attachment>,
    pub junction: bool,
    pub embedment_m: f64,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Attachment {
    pub connected_section_ids: Vec<String>,
    pub attachment_point_id: String,
    pub dead_end: Option<bool>,
    pub point: Point,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Span {
    pub section_id: String,
    pub section_set_member_ids: Vec<String>,
    pub from_alignment_id: String,
    pub to_alignment_id: String,
    pub from_structure_id: String,
    pub to_structure_id: String,
    pub points: Vec<Point>,
    pub display_color: Option<String>,
    pub cable_engineering_name: String,
    pub cross_alignment: bool,
    pub junction_hint: Option<Junction>,
    pub span_label: Option<SpanLabel>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Junction {
    pub child_alignment_id: String,
    pub junction_structure_id: String,
    pub direction_arrow: Arrow,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Arrow {
    pub shaft_start: Point,
    pub shaft_end: Point,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SpanLabel {
    pub text: String,
    pub point: Point,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Guide {
    pub label: Option<String>,
    pub kind: String,
    pub offset_m: f64,
    pub segments: Vec<Ground>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Connection {
    pub section_id: String,
    pub points: Vec<Point>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SelectionOrder {
    pub structures: Vec<String>,
    pub terrain_points: Vec<String>,
    pub tension_sections: Vec<String>,
    pub attachments: Vec<AttachmentRef>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct AttachmentRef {
    pub structure_id: String,
    pub attachment_point_id: String,
}
impl AttachmentRef {
    pub fn key(&self) -> String {
        format!("{}\0{}", self.structure_id, self.attachment_point_id)
    }
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Display {
    pub rows: Vec<DisplayRow>,
    pub cables: Vec<Cable>,
    pub primitives: Vec<Primitive>,
    pub symbols: Vec<Symbol>,
    pub authored_structures: BTreeMap<String, AuthoredDisplay>,
    pub state: DisplayState,
    pub review_status_colors: BTreeMap<String, String>,
    pub review_box_ink: String,
    pub groups: Vec<crate::panel_model::ControlGroup>,
    pub legend: Vec<crate::panel_model::Legend>,
    pub style_controls: Option<crate::panel_model::StyleControls>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct DisplayState {
    pub visibility: BTreeMap<String, bool>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct AuthoredDisplay {
    pub primitives: Vec<Primitive>,
    pub symbols: Vec<Symbol>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct DisplayRow {
    pub option: String,
    pub encoding: Encoding,
    pub label_key: String,
    pub checked: bool,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Cable {
    pub cable_name: String,
    pub encoding: Encoding,
    pub can_reset: bool,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct Encoding {
    pub color: String,
    pub label_color: String,
    pub width_px: f64,
    pub dash_px: Vec<f64>,
    pub font: String,
}
impl Default for Encoding {
    fn default() -> Self {
        Self {
            color: "#94a3b8".into(),
            label_color: "#94a3b8".into(),
            width_px: 1.,
            dash_px: vec![],
            font: "10px monospace".into(),
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Primitive {
    pub option: String,
    pub encoding: Encoding,
    pub points: Vec<Point>,
    pub labels: Vec<Label>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Label {
    pub point: Point,
    pub text: String,
    pub rotation_radians: f64,
    pub offset_px: [f64; 2],
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Symbol {
    pub entity_id: String,
    pub parent_id: Option<String>,
    pub point: Point,
    pub glyph: String,
    pub encoding: Encoding,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Engineering {
    pub markers: Vec<Marker>,
    pub cases: Vec<crate::panel_model::WeatherChoice>,
    pub selected_case_index: usize,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Marker {
    pub entity_id: String,
    pub kind: String,
    pub point: Point,
    pub box_rect_px: Option<[f64; 4]>,
    pub labels: Vec<ReviewLabel>,
    pub status: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ReviewLabel {
    pub colour_role: String,
    pub hit_rect_px: [f64; 4],
    pub summary: Option<Summary>,
    pub text: String,
    pub offset_px: [f64; 2],
    pub rotation_radians: f64,
    pub font: String,
    pub align: String,
    pub baseline: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Summary {
    pub members: Vec<Summary>,
    pub lines: Vec<String>,
    pub rect_px: [f64; 4],
    pub issues_rect_px: [f64; 4],
    pub details_rect_px: [f64; 4],
    pub issues_label: String,
    pub details_label: String,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Stroke {
    pub color: String,
    pub width_px: f64,
    pub dash_px: Vec<f64>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Foundation {
    pub station_grid: Stroke,
    pub alignment_boundary_grid: Stroke,
    pub elevation_grid: Stroke,
    pub plot_boundary: Stroke,
    pub ordinates: BTreeMap<String, Stroke>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Grid {
    pub target_screen_spacing_px: f64,
    pub station_levels: Vec<StationLevel>,
    pub elevation_levels: Vec<ElevationLevel>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct StationLevel {
    pub step_m: f64,
    pub ticks: Vec<StationTick>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct StationTick {
    pub value_m: f64,
    pub scene_x: f64,
    pub boundary: bool,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ElevationLevel {
    pub step_m: f64,
    pub ticks: Vec<ElevationTick>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ElevationTick {
    pub value_m: f64,
    pub scene_y: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Viewport {
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
}
impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.,
            pan_x: 0.,
            pan_y: 0.,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Visibility {
    pub show_ground: bool,
    pub show_terrain_points: bool,
    pub show_structures: bool,
    pub show_wire: bool,
    pub show_attachments: bool,
}
impl Default for Visibility {
    fn default() -> Self {
        Self {
            show_ground: true,
            show_terrain_points: true,
            show_structures: true,
            show_wire: true,
            show_attachments: true,
        }
    }
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ViewState {
    pub focus_request: Option<FocusRequest>,
    pub geometry_preview: Option<crate::preview::GeometryPreview>,
    pub panel: Option<std::sync::Arc<crate::panel_model::PanelModel>>,
    pub viewport: Viewport,
    pub visibility: Visibility,
    pub selected_entity_id: Option<String>,
    pub selected_entity_ids: Vec<String>,
    pub edit_mode: bool,
    pub model_id: Option<String>,
    pub pickable: bool,
    pub show_grid: bool,
    pub show_clearance_guides: bool,
    pub show_span_labels: bool,
    pub show_structure_labels: bool,
    pub label_orientation: String,
    pub wire_opacity: f64,
    pub lowest_wire_drop_m: f64,
    pub terrain_point_color: String,
    pub terrain_point_width_px: f64,
    pub create_active: bool,
    pub create_busy: bool,
    pub ground_move_busy: bool,
    pub station_move_enabled: bool,
    pub ground_move_enabled: bool,
    pub scope: String,
    pub history_key: String,
    pub history_generation: u64,
    pub selection_epoch: u64,
    pub enabled: bool,
}
impl Default for ViewState {
    fn default() -> Self {
        Self {
            focus_request: None,
            geometry_preview: None,
            panel: None,
            viewport: Viewport::default(),
            visibility: Visibility::default(),
            selected_entity_id: None,
            selected_entity_ids: vec![],
            edit_mode: false,
            model_id: None,
            pickable: true,
            show_grid: true,
            show_clearance_guides: true,
            show_span_labels: true,
            show_structure_labels: true,
            label_orientation: "auto".into(),
            wire_opacity: 1.,
            lowest_wire_drop_m: 0.,
            terrain_point_color: "#94a3b8".into(),
            terrain_point_width_px: 4.,
            create_active: false,
            create_busy: false,
            ground_move_busy: false,
            station_move_enabled: false,
            ground_move_enabled: false,
            scope: String::new(),
            history_key: String::new(),
            history_generation: 0,
            selection_epoch: 0,
            enabled: true,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PickRequest {
    pub model_revision: String,
    pub axis_pin_digest: String,
    pub scene_x: f64,
    pub scene_y: f64,
    pub tolerance_scene_units: f64,
    pub scene_units_per_px: f64,
    pub visibility: Visibility,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct PickResult {
    pub model_revision: String,
    pub axis_pin_digest: String,
    pub indexed_segment_count: usize,
    pub candidates: Vec<Candidate>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Candidate {
    pub entity_id: String,
    pub kind: String,
    pub distance_scene_units: f64,
    pub distance_px: f64,
    pub attachment_ref: Option<AttachmentRef>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct RangeRequest {
    pub model_revision: String,
    pub axis_pin_digest: String,
    pub from_entity_id: String,
    pub to_entity_id: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RangeResult {
    pub model_revision: String,
    pub axis_pin_digest: String,
    pub entity_ids: Vec<String>,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Screen {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Intent {
    StructureGoto {
        entity_id: String,
    },
    Clipboard {
        text: String,
    },
    PanelAction {
        action: crate::panel_model::Action,
    },
    Display {
        patch: crate::panel_model::DisplayPatch,
        analyze: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        intent: Option<String>,
    },
    CreateArm {
        mode: String,
    },
    CreateChoice {
        choice: String,
    },
    Resize {
        height_px: f64,
    },
    DialogClose {
        dialog: crate::panel_model::DialogKind,
    },
    RetypePreview {
        request: crate::panel_model::RetypeRequest,
    },
    RetypeApply {
        request: crate::panel_model::RetypeRequest,
    },
    SagApply {
        expected_revision: String,
        commands: Vec<crate::panel_model::HeadCommand>,
    },
    SelectionQuery {
        table: String,
        query: crate::panel_model::FilterQuery,
    },
    SelectionApply {
        entity_ids: Vec<String>,
        primary_id: Option<String>,
    },
    DeleteApply {
        expected_revision: String,
        commands: Vec<crate::panel_model::HeadCommand>,
    },
    LassoBegin,
    LassoAbort,
    LassoCancel,
    LassoFinish {
        polygon: Vec<Point>,
        selection_epoch: u64,
        mode: Option<String>,
    },
    Viewport {
        viewport: Viewport,
    },
    Select {
        entity_ids: Vec<String>,
        primary_id: Option<String>,
    },
    Pick {
        request_id: u64,
        request: PickRequest,
    },
    Range {
        request_id: u64,
        request: RangeRequest,
    },
    MoveStation {
        entity_id: String,
        station_m: f64,
    },
    MoveGround {
        entity_id: String,
        elevation_m: f64,
        revision: String,
    },
    Create {
        pointer: PickRequest,
    },
    CreateCancel,
    Inspect {
        entity_id: String,
    },
    SectionCriteria,
    Context {
        client_x: f64,
        client_y: f64,
        entity_ids: Vec<String>,
        primary_id: Option<String>,
    },
    ReviewIssues {
        entity_id: String,
    },
    Surface {
        width: f64,
        height: f64,
        scale: f64,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct FocusRequest {
    pub entity_id: String,
    pub epoch: u64,
    pub model_revision: String,
}

impl Scene {
    pub fn prepare_render(&mut self) {
        fn path(points: &mut [Point]) {
            let increasing = points.windows(2).all(|p| p[0].scene_x <= p[1].scene_x);
            let decreasing = points.windows(2).all(|p| p[0].scene_x >= p[1].scene_x);
            if let Some(first) = points.first_mut() {
                first.path_order = if increasing {
                    1
                } else if decreasing {
                    -1
                } else {
                    0
                };
            }
        }
        for band in &mut self.bands {
            for ground in &mut band.ground {
                path(&mut ground.points);
            }
            for side in &mut band.side_profiles {
                for segment in &mut side.segments {
                    path(&mut segment.points);
                }
            }
        }
        for span in &mut self.conductor_spans {
            path(&mut span.points);
        }
        for guide in &mut self.clearance_guides {
            for segment in &mut guide.segments {
                path(&mut segment.points);
            }
        }
        for connection in &mut self.authored_connections {
            path(&mut connection.points);
        }
        if let Some(display) = &mut self.display {
            for p in &mut display.primitives {
                path(&mut p.points);
            }
            for group in display.authored_structures.values_mut() {
                for p in &mut group.primitives {
                    path(&mut p.points);
                }
            }
        }
        let mut edges: Vec<_> = self
            .clearance_guides
            .iter()
            .filter(|g| g.kind == "ground_offset" && g.offset_m != 0.)
            .flat_map(|g| &g.segments)
            .flat_map(|s| s.points.windows(2))
            .map(|p| {
                (
                    p[0].scene_x.min(p[1].scene_x),
                    p[0].scene_x.max(p[1].scene_x),
                    p[0],
                    p[1],
                )
            })
            .collect();
        edges.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut max = f64::NEG_INFINITY;
        let prefix: Vec<_> = edges
            .iter()
            .map(|e| {
                max = max.max(e.1);
                max
            })
            .collect();
        self.feature_baselines.clear();
        for band in &self.bands {
            for p in &band.terrain_points {
                if p.feature_clearance.is_none() {
                    continue;
                }
                let x = p.point.scene_x;
                let end = edges.partition_point(|e| e.0 <= x);
                let first = prefix[..end].partition_point(|v| *v < x);
                let mut baseline = p.point;
                if let Some(e) = edges[first..end].iter().find(|e| e.1 >= x) {
                    let width = e.3.scene_x - e.2.scene_x;
                    let t = if width.abs() < 1e-12 {
                        0.5
                    } else {
                        (x - e.2.scene_x) / width
                    };
                    baseline.scene_y = e.2.scene_y + t * (e.3.scene_y - e.2.scene_y);
                }
                self.feature_baselines
                    .insert((p.entity_id.clone(), x.to_bits()), baseline);
            }
        }
    }
}
