use crate::{camera::Camera, model::*};
use std::collections::{BTreeMap, HashSet};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d as Context, HtmlCanvasElement};

pub fn canvas() -> Result<(HtmlCanvasElement, Context), JsValue> {
    let canvas: HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")?
        .dyn_into()?;
    let ctx = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("profile_canvas_context_unavailable"))?
        .dyn_into()?;
    Ok((canvas, ctx))
}
pub fn size(canvas: &HtmlCanvasElement, ctx: &Context, w: f64, h: f64, dpr: f64) {
    let pw = (w * dpr).round().max(1.) as u32;
    let ph = (h * dpr).round().max(1.) as u32;
    if canvas.width() != pw {
        canvas.set_width(pw);
    }
    if canvas.height() != ph {
        canvas.set_height(ph);
    }
    let _ = canvas.style().set_property("width", &format!("{w}px"));
    let _ = canvas.style().set_property("height", &format!("{h}px"));
    let _ = ctx.set_transform(dpr, 0., 0., dpr, 0., 0.);
}
fn color(ctx: &Context, fill: &str, stroke: &str) {
    ctx.set_fill_style_str(fill);
    ctx.set_stroke_style_str(stroke);
}
fn dash(ctx: &Context, values: &[f64]) {
    let values: js_sys::Array = values.iter().map(|v| JsValue::from_f64(*v)).collect();
    let _ = ctx.set_line_dash(&values);
}
fn text(ctx: &Context, value: &str, p: Screen) {
    let _ = ctx.fill_text(value, p.x, p.y);
}
fn source_color(source: &str) -> &'static str {
    match source {
        "surveyed" => "#4ade80",
        "dem_survey_corrected" => "#38bdf8",
        "dem_raw" => "#fbbf24",
        _ => "#94a3b8",
    }
}
fn pen<'a>(scene: &'a Scene, option: &str) -> Option<&'a Encoding> {
    scene
        .display
        .as_ref()?
        .rows
        .iter()
        .find(|r| r.option == option)
        .map(|r| &r.encoding)
}
fn line(ctx: &Context, camera: &Camera, points: &[Point], color: &str, width: f64, dashes: &[f64]) {
    if points.len() < 2 {
        return;
    }
    let proj = camera.projection();
    let left = (-32. - proj.offset_x) / proj.scale;
    let right = (camera.width + 32. - proj.offset_x) / proj.scale;
    // Most native samples are sorted. Keep the segment crossing either edge.
    let increasing = points[0].path_order == 1;
    let (first, end) = if dashes.is_empty() && increasing {
        (
            points
                .partition_point(|p| p.scene_x < left)
                .saturating_sub(1),
            (points.partition_point(|p| p.scene_x <= right) + 1).min(points.len()),
        )
    } else {
        (0, points.len())
    };
    if first >= end || end - first < 2 {
        return;
    }
    ctx.begin_path();
    for (i, p) in points[first..end].iter().enumerate() {
        let p = camera.project(*p);
        if i == 0 {
            ctx.move_to(p.x, p.y);
        } else {
            ctx.line_to(p.x, p.y);
        }
    }
    ctx.set_stroke_style_str(color);
    ctx.set_line_width(width);
    dash(ctx, dashes);
    ctx.stroke();
    dash(ctx, &[]);
}
fn dot(ctx: &Context, p: Screen, r: f64, fill: bool) {
    ctx.begin_path();
    let _ = ctx.arc(p.x, p.y, r, 0., std::f64::consts::TAU);
    if fill {
        ctx.fill();
    } else {
        ctx.stroke();
    }
}
fn diamond(ctx: &Context, p: Screen, r: f64) {
    ctx.begin_path();
    ctx.move_to(p.x, p.y - r);
    ctx.line_to(p.x + r, p.y);
    ctx.line_to(p.x, p.y + r);
    ctx.line_to(p.x - r, p.y);
    ctx.close_path();
    ctx.stroke();
}
fn visible(camera: &Camera, p: Point, margin: f64) -> bool {
    let p = camera.project(p);
    p.x >= -margin
        && p.x <= camera.width + margin
        && p.y >= -margin
        && p.y <= camera.height + margin
}
fn plot_line(ctx: &Context, camera: &Camera, a: Point, b: Point, stroke: &Stroke) {
    line(
        ctx,
        camera,
        &[a, b],
        if stroke.color.is_empty() {
            "#334155"
        } else {
            &stroke.color
        },
        stroke.width_px.max(0.5),
        &stroke.dash_px,
    );
}
fn grid(ctx: &Context, scene: &Scene, camera: &Camera) {
    let s = camera.projection().scale;
    let target = scene.grid.target_screen_spacing_px.max(32.);
    let bounds = &scene.plot_bounds;
    ctx.set_font("9px monospace");
    ctx.set_text_align("center");
    ctx.set_text_baseline("top");
    ctx.set_fill_style_str("#64748b");
    let stations = scene
        .grid
        .station_levels
        .iter()
        .find(|l| l.step_m * scene.scale.horizontal_scene_units_per_m * s >= target)
        .or_else(|| scene.grid.station_levels.last());
    let mut last_label = f64::NEG_INFINITY;
    if let Some(level) = stations {
        for t in &level.ticks {
            let p = camera.project(Point::xy(t.scene_x, bounds.max_y));
            if p.x < 0. || p.x > camera.width {
                continue;
            }
            plot_line(
                ctx,
                camera,
                Point::xy(t.scene_x, bounds.min_y),
                Point::xy(t.scene_x, bounds.max_y),
                if t.boundary {
                    &scene.foundation.alignment_boundary_grid
                } else {
                    &scene.foundation.station_grid
                },
            );
            if p.x - last_label > 65. {
                let km = (t.value_m / 1000.).floor();
                let m = t.value_m - km * 1000.;
                text(
                    ctx,
                    &format!("{km:.0}+{m:06.2}"),
                    Screen {
                        x: p.x,
                        y: p.y + 4.,
                    },
                );
                last_label = p.x;
            }
        }
    }
    let elevations = scene
        .grid
        .elevation_levels
        .iter()
        .find(|l| l.step_m * scene.scale.vertical_scene_units_per_m * s >= target)
        .or_else(|| scene.grid.elevation_levels.last());
    ctx.set_text_align("right");
    ctx.set_text_baseline("middle");
    if let Some(level) = elevations {
        for t in &level.ticks {
            plot_line(
                ctx,
                camera,
                Point::xy(bounds.min_x, t.scene_y),
                Point::xy(bounds.max_x, t.scene_y),
                &scene.foundation.elevation_grid,
            );
            let p = camera.project(Point::xy(bounds.min_x, t.scene_y));
            text(
                ctx,
                &format!("{:.0}", t.value_m),
                Screen {
                    x: p.x - 4.,
                    y: p.y,
                },
            );
        }
    }
    let b = bounds;
    line(
        ctx,
        camera,
        &[
            Point::xy(b.min_x, b.min_y),
            Point::xy(b.max_x, b.min_y),
            Point::xy(b.max_x, b.max_y),
            Point::xy(b.min_x, b.max_y),
            Point::xy(b.min_x, b.min_y),
        ],
        &scene.foundation.plot_boundary.color,
        scene.foundation.plot_boundary.width_px.max(0.5),
        &[],
    );
}

pub fn base(
    ctx: &Context,
    scene: &Scene,
    state: &ViewState,
    camera: &Camera,
    _markers: &mut MarkerCache,
    lowest_wire_segments: &[Vec<Point>],
) {
    ctx.set_fill_style_str("#020617");
    ctx.fill_rect(0., 0., camera.width, camera.height);
    if state.show_grid {
        grid(ctx, scene, camera);
    }
    let v = &state.visibility;
    for band in &scene.bands {
        if v.show_ground {
            for ground in &band.ground {
                let pen = pen(scene, "ground");
                line(
                    ctx,
                    camera,
                    &ground.points,
                    pen.map(|p| p.color.as_str())
                        .unwrap_or(source_color(&ground.source_class)),
                    pen.map(|p| p.width_px).unwrap_or(1.4),
                    pen.map(|p| p.dash_px.as_slice()).unwrap_or(&[]),
                );
            }
        }
        if scene
            .display
            .as_ref()
            .is_none_or(|d| d.state.visibility.get("side_profiles") != Some(&false))
        {
            for side in &band.side_profiles {
                for segment in &side.segments {
                    line(
                        ctx,
                        camera,
                        &segment.points,
                        &side.style.color,
                        side.style.width_px,
                        &side.style.dash_px,
                    );
                }
            }
        }
        if state.show_grid {
            for route in &band.route {
                if let Some(style) = route
                    .ordinate_kind
                    .as_ref()
                    .and_then(|k| scene.foundation.ordinates.get(k))
                {
                    plot_line(
                        ctx,
                        camera,
                        Point::xy(route.point.scene_x, scene.plot_bounds.min_y),
                        Point::xy(route.point.scene_x, scene.plot_bounds.max_y),
                        style,
                    );
                }
            }
        }
        if v.show_terrain_points && scene.display.is_none() {
            for p in &band.terrain_points {
                if !visible(camera, p.point, 24.) {
                    continue;
                }
                let screen = camera.project(p.point);
                let c = p
                    .feature_clearance
                    .as_ref()
                    .map(|f| f.point_color.as_str())
                    .filter(|c| !c.is_empty())
                    .unwrap_or(&state.terrain_point_color);
                color(ctx, c, c);
                dot(ctx, screen, state.terrain_point_width_px.max(1.), true);
            }
        }
        if v.show_structures {
            for structure in &band.structures {
                let top = structure.top.unwrap_or(structure.ground);
                if !visible(camera, top, camera.height + 24.) {
                    continue;
                }
                let stroke = pen(scene, "structures");
                let transformer = structure.network_role.as_deref() == Some("transformer");
                let role_color = match structure.network_role.as_deref() {
                    Some("transformer") => "#c084fc",
                    Some("tapping") => "#fb7185",
                    Some("substation") => "#60a5fa",
                    Some("plant") => "#34d399",
                    _ => "#e2e8f0",
                };
                line(
                    ctx,
                    camera,
                    &[structure.ground, top],
                    stroke.map(|p| p.color.as_str()).unwrap_or(role_color),
                    stroke
                        .map(|p| p.width_px)
                        .unwrap_or(if transformer { 3. } else { 1.3 }),
                    &[],
                );
                if structure.embedment_m > 0.
                    && scene
                        .display
                        .as_ref()
                        .is_none_or(|d| d.state.visibility.get("embedded") == Some(&true))
                {
                    let p = pen(scene, "embedded");
                    line(
                        ctx,
                        camera,
                        &[structure.base, structure.ground],
                        p.map(|p| p.color.as_str()).unwrap_or("#64748b"),
                        p.map(|p| p.width_px).unwrap_or(2.),
                        p.map(|p| p.dash_px.as_slice()).unwrap_or(&[3., 3.]),
                    );
                }
                let p = camera.project(top);
                color(ctx, "#a7f3d0", "#a7f3d0");
                if structure.junction {
                    ctx.set_stroke_style_str("#fbbf24");
                    diamond(ctx, p, 7.);
                }
                if scene.display.is_none() {
                    color(ctx, role_color, role_color);
                    dot(ctx, p, if transformer { 4.5 } else { 3. }, true);
                    if transformer && structure.outgoing_feeder_count.is_some_and(|n| n > 1) {
                        ctx.set_font("11px monospace");
                        ctx.set_text_align("left");
                        ctx.set_text_baseline("bottom");
                        text(
                            ctx,
                            &format!("{} feeders", structure.outgoing_feeder_count.unwrap()),
                            Screen {
                                x: p.x + 8.,
                                y: p.y + 4.,
                            },
                        );
                    }
                    for attachment in &structure.attachment_points {
                        let ink = if attachment.connected_section_ids.is_empty() {
                            "#64748b"
                        } else {
                            "#a7f3d0"
                        };
                        color(ctx, ink, ink);
                        ctx.set_line_width(1.2);
                        let p = camera.project(attachment.point);
                        match attachment.dead_end {
                            Some(true) => ctx.fill_rect(p.x - 3., p.y - 3., 6., 6.),
                            Some(false) => dot(ctx, p, 3.5, false),
                            None => diamond(ctx, p, 3.5),
                        }
                    }
                }
            }
        }
    }
    feature_clearances(ctx, scene, state, camera, false);
    if state.show_clearance_guides && scene.current() {
        for guide in &scene.clearance_guides {
            if guide.kind == "lowest_wire_drop" && !scene.current() {
                continue;
            }
            for segment in &guide.segments {
                let p = pen(scene, "clearance_line");
                line(
                    ctx,
                    camera,
                    &segment.points,
                    p.map(|p| p.color.as_str()).unwrap_or("#f59e0b"),
                    p.map(|p| p.width_px).unwrap_or(1.),
                    &[5., 4.],
                );
            }
        }
    }
    if v.show_wire && scene.current() {
        ctx.set_global_alpha(state.wire_opacity.clamp(0., 1.));
        for span in &scene.conductor_spans {
            let encoding = scene
                .display
                .as_ref()
                .and_then(|d| {
                    d.cables
                        .iter()
                        .find(|c| c.cable_name == span.cable_engineering_name)
                        .map(|c| &c.encoding)
                })
                .or_else(|| pen(scene, "wire"));
            let c = encoding
                .map(|p| p.color.as_str())
                .or(span.display_color.as_deref())
                .unwrap_or("#67e8f9");
            if !span.cross_alignment {
                line(
                    ctx,
                    camera,
                    &span.points,
                    c,
                    encoding.map(|p| p.width_px).unwrap_or(1.3),
                    encoding.map(|p| p.dash_px.as_slice()).unwrap_or(&[]),
                );
            }
            if state.show_span_labels && scene.display.is_none() {
                if let Some(label) = &span.span_label {
                    if visible(camera, label.point, 80.) {
                        ctx.set_font("10px monospace");
                        ctx.set_text_align("center");
                        ctx.set_text_baseline("bottom");
                        ctx.set_fill_style_str(c);
                        let mut p = camera.project(label.point);
                        p.y -= 5.;
                        text(ctx, &label.text, p);
                    }
                }
            }
            if let Some(h) = &span.junction_hint {
                let delta = Point::xy(
                    h.direction_arrow.shaft_end.scene_x - h.direction_arrow.shaft_start.scene_x,
                    h.direction_arrow.shaft_end.scene_y - h.direction_arrow.shaft_start.scene_y,
                );
                for start in crate::draw_geometry::junction_scene_starts(scene, h) {
                    if !visible(camera, start, 16.) {
                        continue;
                    }
                    let arrow = crate::draw_geometry::junction_screen_arrow(
                        camera.project(start),
                        camera.project(Point::xy(
                            start.scene_x + delta.scene_x,
                            start.scene_y + delta.scene_y,
                        )),
                    );
                    color(ctx, c, c);
                    ctx.set_line_width(1.1);
                    ctx.begin_path();
                    ctx.move_to(arrow.start.x, arrow.start.y);
                    ctx.line_to(arrow.end.x, arrow.end.y);
                    ctx.stroke();
                    ctx.begin_path();
                    ctx.move_to(arrow.head[0].x, arrow.head[0].y);
                    ctx.line_to(arrow.head[1].x, arrow.head[1].y);
                    ctx.line_to(arrow.head[2].x, arrow.head[2].y);
                    ctx.close_path();
                    ctx.fill();
                }
            }
        }
        if state.show_clearance_guides {
            for points in lowest_wire_segments {
                line(ctx, camera, points, "#f59e0b", 1., &[5., 4.]);
            }
        }
        ctx.set_global_alpha(1.);
    }
    if !scene.current() && !scene.provisional_geometry && v.show_wire {
        for c in &scene.authored_connections {
            line(ctx, camera, &c.points, "#94a3b8", 1., &[4., 4.]);
        }
    }
    if let Some(display) = &scene.display {
        for primitive in display.primitives.iter().chain(
            display
                .authored_structures
                .values()
                .flat_map(|g| &g.primitives),
        ) {
            if !scene.current()
                && ["span_distances", "section_labels"].contains(&primitive.option.as_str())
            {
                continue;
            }
            let e = &primitive.encoding;
            line(
                ctx,
                camera,
                &primitive.points,
                &e.color,
                e.width_px,
                &e.dash_px,
            );
            for label in &primitive.labels {
                if !visible(camera, label.point, camera.width) {
                    continue;
                }
                ctx.save();
                ctx.set_font(&e.font);
                ctx.set_fill_style_str(&e.label_color);
                ctx.set_text_align("left");
                ctx.set_text_baseline("bottom");
                let p = camera.project(label.point);
                let _ = ctx.translate(p.x + label.offset_px[0], p.y + label.offset_px[1]);
                let _ = ctx.rotate(label.rotation_radians);
                text(ctx, &label.text, Screen { x: 0., y: 0. });
                ctx.restore();
            }
        }
        for symbol in display.symbols.iter().chain(
            display
                .authored_structures
                .values()
                .flat_map(|g| &g.symbols),
        ) {
            if !visible(camera, symbol.point, 24.) {
                continue;
            }
            ctx.set_font(&symbol.encoding.font);
            ctx.set_fill_style_str(&symbol.encoding.color);
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            text(ctx, &symbol.glyph, camera.project(symbol.point));
        }
        if scene.current() && display.state.visibility.get("review") != Some(&false) {
            // Review glyphs are composited after the selection layer.
        }
    }
    if scene.bands.is_empty() {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_style_str("#94a3b8");
        ctx.set_text_align("center");
        text(
            ctx,
            "No profile geometry",
            Screen {
                x: camera.width / 2.,
                y: camera.height / 2.,
            },
        );
    }
}

pub fn generic_labels(ctx: &Context, scene: &Scene, state: &ViewState, camera: &Camera) {
    if scene.display.is_some() || !state.show_structure_labels || !state.visibility.show_structures
    {
        return;
    }
    use crate::labels::{self, Align, Baseline, LabelRequest};
    let orientation =
        labels::orientation(&state.label_orientation).unwrap_or(labels::Orientation::Auto);
    let requests: Vec<_> = scene
        .bands
        .iter()
        .flat_map(|b| &b.structures)
        .filter_map(|s| {
            let selected = state.selected_entity_ids.contains(&s.entity_id)
                || state.selected_entity_id.as_ref() == Some(&s.entity_id);
            Some(LabelRequest {
                id: s.entity_id.clone(),
                label: s.display_label.clone()?,
                anchor: camera.project(s.top.unwrap_or(s.ground)),
                orientation,
                priority: if selected { 100. } else { 0. },
                required: selected,
            })
        })
        .collect();
    ctx.set_font("11px monospace");
    let locales = js_sys::Array::new();
    let locale_options = js_sys::Object::new();
    let placed = labels::layout_labels(
        &requests,
        labels::options(camera.width, camera.height),
        |_, line| ctx.measure_text(line).map(|m| m.width()).unwrap_or(0.),
        |a, b| {
            js_sys::JsString::from(a)
                .locale_compare(b, &locales, &locale_options)
                .cmp(&0)
        },
    );
    for label in placed {
        let request = &requests[label.input_index];
        let p = label.placement;
        ctx.save();
        let _ = ctx.translate(p.x, p.y);
        let _ = ctx.rotate(p.rotation_rad);
        ctx.set_text_align(match p.align {
            Align::Left => "left",
            Align::Right => "right",
            Align::Center => "center",
        });
        ctx.set_text_baseline(match p.baseline {
            Baseline::Bottom => "bottom",
            Baseline::Top => "top",
        });
        ctx.set_fill_style_str(if request.required {
            "#67e8f9"
        } else {
            "#cbd5e1"
        });
        ctx.set_stroke_style_str("rgba(2,6,23,0.92)");
        ctx.set_line_width(3.);
        for (i, line) in label.lines.iter().enumerate() {
            let y = if p.baseline == Baseline::Bottom {
                (i as f64 + 1. - label.lines.len() as f64) * 13.
            } else {
                i as f64 * 13.
            };
            let _ = ctx.stroke_text(line, 0., y);
            let _ = ctx.fill_text(line, 0., y);
        }
        ctx.restore();
    }
}

pub fn selection(ctx: &Context, scene: &Scene, state: &ViewState, camera: &Camera) {
    let ids: HashSet<_> = state
        .selected_entity_ids
        .iter()
        .map(String::as_str)
        .chain(state.selected_entity_id.iter().map(String::as_str))
        .collect();
    if ids.is_empty() {
        return;
    }
    let selected = "#67e8f9";
    color(ctx, selected, selected);
    for band in &scene.bands {
        if ids.contains(band.alignment_id.as_str()) && state.visibility.show_ground {
            for g in &band.ground {
                line(ctx, camera, &g.points, selected, 3., &[]);
            }
            for side in &band.side_profiles {
                for g in &side.segments {
                    line(ctx, camera, &g.points, selected, 3., &side.style.dash_px);
                }
            }
        }
        if state.visibility.show_structures {
            for s in &band.structures {
                if ids.contains(s.entity_id.as_str()) {
                    line(
                        ctx,
                        camera,
                        &[s.base, s.top.unwrap_or(s.ground)],
                        selected,
                        4.,
                        &[],
                    );
                }
                for a in &s.attachment_points {
                    if ids.contains(
                        AttachmentRef {
                            structure_id: s.entity_id.clone(),
                            attachment_point_id: a.attachment_point_id.clone(),
                        }
                        .key()
                        .as_str(),
                    ) {
                        color(ctx, selected, selected);
                        ctx.set_line_width(2.5);
                        dot(ctx, camera.project(a.point), 5., false);
                    }
                }
            }
        }
        if state.visibility.show_terrain_points {
            for p in &band.terrain_points {
                if ids.contains(p.entity_id.as_str()) {
                    color(ctx, selected, selected);
                    ctx.set_line_width(2.);
                    dot(ctx, camera.project(p.point), 6., false);
                }
            }
        }
    }
    if state.visibility.show_wire && scene.current() {
        for s in &scene.conductor_spans {
            if !s.cross_alignment
                && (ids.contains(s.section_id.as_str())
                    || s.section_set_member_ids
                        .iter()
                        .any(|id| ids.contains(id.as_str())))
            {
                line(ctx, camera, &s.points, selected, 3., &[]);
            }
        }
    }
    if let Some(display) = &scene.display {
        for s in display.symbols.iter().chain(
            display
                .authored_structures
                .values()
                .flat_map(|g| &g.symbols),
        ) {
            if ids.contains(s.entity_id.as_str())
                || s.parent_id
                    .as_ref()
                    .is_some_and(|id| ids.contains(id.as_str()))
            {
                ctx.set_font(&s.encoding.font);
                ctx.set_fill_style_str(selected);
                ctx.set_text_align("center");
                ctx.set_text_baseline("middle");
                text(ctx, &s.glyph, camera.project(s.point));
            }
        }
    }
}

struct Sprite {
    canvas: HtmlCanvasElement,
    rect: [f64; 4],
}
#[derive(Default)]
pub struct MarkerCache {
    sprites: BTreeMap<usize, Sprite>,
    bytes: usize,
    key: String,
}
impl MarkerCache {
    pub fn clear(&mut self) {
        for s in self.sprites.values() {
            s.canvas.set_width(0);
            s.canvas.set_height(0);
        }
        self.sprites.clear();
        self.bytes = 0;
    }
    pub fn paint(&mut self, ctx: &Context, scene: &Scene, camera: &Camera) {
        let Some(engineering) = &scene.engineering else {
            return;
        };
        let Some(display) = &scene.display else {
            return;
        };
        let dpr = web_sys::window().unwrap().device_pixel_ratio();
        let key = format!(
            "{}:{:?}:{}",
            dpr, display.review_status_colors, display.review_box_ink
        );
        if key != self.key {
            self.clear();
            self.key = key;
        }
        for (i, m) in engineering.markers.iter().enumerate() {
            if display.state.visibility.get(&m.kind) == Some(&false)
                || !visible(camera, m.point, camera.width)
            {
                continue;
            }
            if !self.sprites.contains_key(&i) {
                let mut rect = [0f64; 4];
                let mut right = 0f64;
                let mut bottom = 0f64;
                if let Some(r) = m.box_rect_px {
                    rect[0] = r[0];
                    rect[1] = r[1];
                    right = r[0] + r[2];
                    bottom = r[1] + r[3];
                }
                for label in &m.labels {
                    let r = label.hit_rect_px;
                    rect[0] = rect[0].min(r[0]);
                    rect[1] = rect[1].min(r[1]);
                    right = right.max(r[0] + r[2]);
                    bottom = bottom.max(r[1] + r[3]);
                    ctx.set_font(&label.font);
                    ctx.set_text_align(&label.align);
                    ctx.set_text_baseline(&label.baseline);
                    if let Ok(metrics) = ctx.measure_text(&label.text) {
                        let cos = label.rotation_radians.cos();
                        let sin = label.rotation_radians.sin();
                        for x in [
                            -metrics.actual_bounding_box_left(),
                            metrics.actual_bounding_box_right(),
                        ] {
                            for y in [
                                -metrics.actual_bounding_box_ascent(),
                                metrics.actual_bounding_box_descent(),
                            ] {
                                let px = label.offset_px[0] + x * cos - y * sin;
                                let py = label.offset_px[1] + x * sin + y * cos;
                                rect[0] = rect[0].min(px);
                                rect[1] = rect[1].min(py);
                                right = right.max(px);
                                bottom = bottom.max(py);
                            }
                        }
                    }
                }
                rect = [
                    rect[0].floor() - 2.,
                    rect[1].floor() - 2.,
                    (right - rect[0]).ceil() + 4.,
                    (bottom - rect[1]).ceil() + 4.,
                ];
                let w = (rect[2] * dpr).ceil().max(1.) as u32;
                let h = (rect[3] * dpr).ceil().max(1.) as u32;
                let bytes = w as usize * h as usize * 4;
                if self.bytes + bytes <= 32 * 1024 * 1024 {
                    if let Ok((surface, c)) = canvas() {
                        surface.set_width(w);
                        surface.set_height(h);
                        let _ = c.set_transform(dpr, 0., 0., dpr, -rect[0] * dpr, -rect[1] * dpr);
                        marker(&c, m, display);
                        self.sprites.insert(
                            i,
                            Sprite {
                                canvas: surface,
                                rect,
                            },
                        );
                        self.bytes += bytes;
                    }
                }
            }
            let p = camera.project(m.point);
            if let Some(sprite) = self.sprites.get(&i) {
                let r = sprite.rect;
                let _ = ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
                    &sprite.canvas,
                    p.x + r[0],
                    p.y + r[1],
                    r[2],
                    r[3],
                );
            } else {
                ctx.save();
                let _ = ctx.translate(p.x, p.y);
                marker(ctx, m, display);
                ctx.restore();
            }
        }
    }
}
impl Drop for MarkerCache {
    fn drop(&mut self) {
        self.clear();
    }
}
fn review_color<'a>(display: &'a Display, role: &str) -> &'a str {
    let key = match role {
        "good" => "pass",
        "bad" => "fail",
        other => other,
    };
    display
        .review_status_colors
        .get(key)
        .map(String::as_str)
        .unwrap_or("#94a3b8")
}
fn marker(ctx: &Context, m: &Marker, display: &Display) {
    if let Some(r) = m.box_rect_px {
        ctx.set_fill_style_str(review_color(
            display,
            m.labels
                .first()
                .map(|l| l.colour_role.as_str())
                .unwrap_or("neutral"),
        ));
        ctx.fill_rect(r[0], r[1], r[2], r[3]);
    }
    for l in &m.labels {
        ctx.save();
        let _ = ctx.translate(l.offset_px[0], l.offset_px[1]);
        let _ = ctx.rotate(l.rotation_radians);
        ctx.set_font(&l.font);
        ctx.set_text_align(&l.align);
        ctx.set_text_baseline(&l.baseline);
        ctx.set_fill_style_str(if m.box_rect_px.is_some() {
            &display.review_box_ink
        } else {
            review_color(display, &l.colour_role)
        });
        text(ctx, &l.text, Screen { x: 0., y: 0. });
        ctx.restore();
    }
}
pub fn review(ctx: &Context, scene: &Scene, camera: &Camera, index: (usize, usize, usize)) {
    let Some(display) = &scene.display else {
        return;
    };
    let Some(marker) = scene
        .engineering
        .as_ref()
        .and_then(|e| e.markers.get(index.0))
    else {
        return;
    };
    let Some(label) = marker.labels.get(index.1) else {
        return;
    };
    let Some(root) = &label.summary else {
        return;
    };
    let summary = if index.2 == 0 {
        root
    } else {
        root.members.get(index.2 - 1).unwrap_or(root)
    };
    let anchor = review_anchor(camera, marker, summary);
    let r = summary.rect_px;
    ctx.save();
    let _ = ctx.translate(anchor.x, anchor.y);
    color(ctx, "#0f172a", review_color(display, &label.colour_role));
    ctx.fill_rect(r[0], r[1], r[2], r[3]);
    ctx.stroke_rect(r[0], r[1], r[2], r[3]);
    ctx.set_font("12px monospace");
    ctx.set_text_align("left");
    ctx.set_text_baseline("top");
    ctx.set_fill_style_str("#e2e8f0");
    for (i, line) in summary.lines.iter().enumerate() {
        text(
            ctx,
            line,
            Screen {
                x: r[0] + 10.,
                y: r[1] + 10. + i as f64 * 16.,
            },
        );
    }
    ctx.set_fill_style_str("#67e8f9");
    text(
        ctx,
        &summary.issues_label,
        Screen {
            x: summary.issues_rect_px[0],
            y: summary.issues_rect_px[1],
        },
    );
    text(
        ctx,
        &summary.details_label,
        Screen {
            x: summary.details_rect_px[0],
            y: summary.details_rect_px[1],
        },
    );
    ctx.restore();
}
pub fn review_anchor(camera: &Camera, marker: &Marker, summary: &Summary) -> Screen {
    let p = camera.project(marker.point);
    let r = summary.rect_px;
    Screen {
        x: p.x.min(camera.width - 4. - r[0] - r[2]).max(4. - r[0]),
        y: p.y.min(camera.height - 4. - r[1] - r[3]).max(4. - r[1]),
    }
}

pub fn feature_clearances(
    ctx: &Context,
    scene: &Scene,
    state: &ViewState,
    camera: &Camera,
    selected_only: bool,
) {
    if !scene.current() || !state.show_clearance_guides {
        return;
    }
    for band in &scene.bands {
        for p in &band.terrain_points {
            let Some(f) = &p.feature_clearance else {
                continue;
            };
            let selected = state.selected_entity_ids.contains(&p.entity_id)
                || state.selected_entity_id.as_ref() == Some(&p.entity_id);
            if selected_only && !selected {
                continue;
            }
            if !visible(camera, p.point, camera.height) {
                continue;
            }
            let baseline = scene
                .feature_baselines
                .get(&(p.entity_id.clone(), p.point.scene_x.to_bits()))
                .copied()
                .unwrap_or(p.point);
            let extra = baseline.scene_y - f.threshold.scene_y
                > 1e-6 * scene.scale.vertical_scene_units_per_m;
            if !extra && !selected {
                continue;
            }
            if extra {
                line(
                    ctx,
                    camera,
                    &[baseline, f.threshold],
                    &f.point_color,
                    1.5,
                    &[4., 3.],
                );
            }
            let tip = camera.project(f.threshold);
            color(ctx, &f.point_color, &f.point_color);
            dot(ctx, tip, 3., true);
            if selected_only {
                if let Some(wire) = f.lowest_display_wire_elevation_m {
                    let point = Point {
                        scene_y: f.threshold.scene_y
                            + (f.threshold.elevation_m - wire)
                                * scene.scale.vertical_scene_units_per_m,
                        ..f.threshold
                    };
                    line(ctx, camera, &[f.threshold, point], "#94a3b8", 1., &[2., 3.]);
                }
                ctx.set_font("11px monospace");
                ctx.set_text_align("left");
                ctx.set_text_baseline("bottom");
                text(
                    ctx,
                    &format!(
                        "{} ({}) · {:.1} m{}",
                        f.feature_name,
                        f.feature_code,
                        f.required_vertical_m,
                        if f.height_default_used {
                            " · default height"
                        } else {
                            ""
                        }
                    ),
                    Screen {
                        x: tip.x + 6.,
                        y: tip.y - 4.,
                    },
                );
                if let Some(m) = f.display_margin_m {
                    ctx.set_fill_style_str("#cbd5e1");
                    text(
                        ctx,
                        &format!("display Δ {m:+.1} m"),
                        Screen {
                            x: tip.x + 6.,
                            y: tip.y + 9.,
                        },
                    );
                }
            }
        }
    }
}
