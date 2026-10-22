//! Browser adapter: Leptos owns this mount; all pointer work and paint stay here.
use crate::{
    camera::Camera,
    hit::{self, HitIndex},
    model::*,
    paint, panel,
    selection::{self, Modifiers},
};
use leptos::{mount::mount_to, prelude::*};
use reactive_graph::signal::ArcRwSignal;
use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::{Rc, Weak},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{
    CanvasRenderingContext2d as Context, HtmlCanvasElement, HtmlElement, KeyboardEvent, MouseEvent,
    PointerEvent, WheelEvent,
};

#[derive(Clone, Default)]
pub(crate) struct Handles {
    pub station: Option<Screen>,
    pub ground: Option<Screen>,
    pub ground_label: String,
}
#[derive(Clone)]
struct Gesture {
    id: i32,
    capture: Option<web_sys::Element>,
    start: Screen,
    last: Screen,
    viewport: Viewport,
    moved: bool,
    button: i16,
    mode: Mode,
    revision: String,
    axis: String,
    selection: Option<String>,
    selection_epoch: u64,
    additive: bool,
}
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Pan,
    Station,
    Ground,
    ZoomWindow,
    Lasso,
}
#[derive(Clone)]
struct Pending {
    serial: u64,
    generation: u64,
    screen: Screen,
    client: Screen,
    modifiers: Modifiers,
    context: bool,
    hover: bool,
    candidates: Vec<Candidate>,
    picked: Option<String>,
}
#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    started_at: f64,
    finished_at: f64,
    sequence: u64,
    scene_generation: u64,
    width: f64,
    height: f64,
    scale: f64,
    viewport: Viewport,
    visibility: Visibility,
    context: String,
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PaintSample {
    sequence: u64,
    scene_generation: u64,
    started_at: f64,
    finished_at: f64,
}
struct Listener {
    target: web_sys::EventTarget,
    name: &'static str,
    callback: Closure<dyn FnMut(web_sys::Event)>,
}
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.name, self.callback.as_ref().unchecked_ref());
    }
}
pub struct Mounted {
    runtime: Rc<RefCell<Runtime>>,
    mount: Option<Box<dyn Any>>,
    listeners: Vec<Listener>,
    observer: Option<web_sys::ResizeObserver>,
    _resize: Closure<dyn FnMut(js_sys::Array, web_sys::ResizeObserver)>,
}
struct ResizeGesture {
    pointer: i32,
    y: f64,
    height: f64,
}
struct Runtime {
    root: HtmlElement,
    lowest_wire_segments: Vec<Vec<Point>>,
    scene: Arc<Scene>,
    source_scene: Arc<Scene>,
    scene_signal: ArcRwSignal<Arc<Scene>>,
    panel: ArcRwSignal<Option<Arc<crate::panel_model::PanelModel>>>,
    facts: ArcRwSignal<panel::Facts>,
    scale: ArcRwSignal<f64>,
    goto_open: ArcRwSignal<bool>,
    context_menu: ArcRwSignal<Option<Screen>>,
    state: ArcRwSignal<ViewState>,
    handles: ArcRwSignal<Handles>,
    canvas: HtmlCanvasElement,
    ctx: Context,
    base: HtmlCanvasElement,
    base_ctx: Context,
    review_canvas: HtmlCanvasElement,
    review_ctx: Context,
    callback: js_sys::Function,
    outbox: Vec<Intent>,
    index: HitIndex,
    width: f64,
    height: f64,
    dpr: f64,
    dirty: bool,
    base_dirty: bool,
    request: Option<i32>,
    raf: Option<Closure<dyn FnMut(f64)>>,
    dead: bool,
    generation: u64,
    scene_generation: u64,
    serial: u64,
    pending: Option<Pending>,
    receipt: Option<Receipt>,
    paint_samples: VecDeque<PaintSample>,
    resize_drag: Option<ResizeGesture>,
    tool: Option<Mode>,
    lasso_points: Vec<Screen>,
    drag: Option<Gesture>,
    touches: BTreeMap<i32, Screen>,
    pinch: Option<(f64, Screen)>,
    last_cycle: Option<(Screen, String)>,
    markers: paint::MarkerCache,
    review: Option<(usize, usize, usize)>,
    last_hover: f64,
    last_focus_epoch: u64,
}
fn now() -> f64 {
    web_sys::window().unwrap().performance().unwrap().now()
}
fn parse<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, JsValue> {
    serde_json::from_str(json).map_err(|e| failure(&format!("profile_view_invalid_input: {e}")))
}
fn failure(s: &str) -> JsValue {
    let (candidate, message) = s.split_once(':').unwrap_or((s, s));
    let code = if candidate.starts_with("profile_") {
        candidate
    } else {
        "profile_view_error"
    };
    JsValue::from_str(&serde_json::json!({"code":code,"message":message.trim()}).to_string())
}
fn flush(runtime: &Rc<RefCell<Runtime>>) {
    let (callback, intents) = {
        let mut r = runtime.borrow_mut();
        (r.callback.clone(), std::mem::take(&mut r.outbox))
    };
    for intent in intents {
        if let Ok(json) = serde_json::to_string(&intent) {
            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&json));
        }
    }
}
fn transact(runtime: &Rc<RefCell<Runtime>>, f: impl FnOnce(&mut Runtime)) {
    if runtime.borrow().dead {
        return;
    }
    f(&mut runtime.borrow_mut());
    flush(runtime);
}
fn handle_style(p: Option<Screen>) -> String {
    match p {
        Some(p) => format!(
            "position:absolute;left:{}px;top:{}px;transform:translate(-50%,-50%);z-index:20;touch-action:none;color:#a5f3fc;background:#020617;border:1px solid #67e8f9;border-radius:4px;width:28px;height:28px;cursor:move",
            p.x, p.y
        ),
        None => "display:none".into(),
    }
}

pub fn mount(
    parent: HtmlElement,
    scene_json: &str,
    state_json: &str,
    callback: js_sys::Function,
) -> Result<Mounted, JsValue> {
    let mut scene: Scene = parse(scene_json)?;
    scene.prepare_render();
    scene.validate().map_err(|s| failure(&s))?;
    let state: ViewState = parse(state_json)?;
    let panel = ArcRwSignal::new(state.panel.clone());
    let facts = ArcRwSignal::new(panel::Facts {
        edit_mode: state.edit_mode,
        selected: state.selected_entity_ids.clone(),
        primary: state.selected_entity_id.clone(),
    });
    let scale = ArcRwSignal::new(1.);
    let goto_open = ArcRwSignal::new(false);
    let context_menu = ArcRwSignal::new(None);
    let source_scene = Arc::new(scene);
    let scene = crate::preview::project(&source_scene, state.geometry_preview.as_ref())
        .map(Arc::new)
        .unwrap_or_else(|| source_scene.clone());
    let scene_signal = ArcRwSignal::new(scene.clone());
    let lowest_wire_segments =
        crate::draw_geometry::lowest_wire_drop(&scene, state.lowest_wire_drop_m);
    let state = ArcRwSignal::new(state);
    let handles = ArcRwSignal::new(Handles::default());
    let station = handles.clone();
    let ground = handles.clone();
    let label = handles.clone();
    let target: Rc<RefCell<Weak<RefCell<Runtime>>>> = Rc::new(RefCell::new(Weak::new()));
    let event_target = target.clone();
    let send: panel::Send = send_wrapper::SendWrapper::new(Rc::new(move |event| {
        if let Some(r) = event_target.borrow().upgrade() {
            transact(&r, |r| r.ui_event(event));
        }
    }) as Rc<dyn Fn(panel::Event)>);
    let view_panel = panel.clone();
    let view_scene = scene_signal.clone();
    let view_facts = facts.clone();
    let view_scale = scale.clone();
    let view_go = goto_open.clone();
    let view_context = context_menu.clone();
    let view_state = state.clone();
    let mounted = mount_to(parent.clone(), move || {
        let toolbar = panel::toolbar(
            view_panel.clone(),
            view_scene.clone(),
            view_facts.clone(),
            view_scale.clone(),
            send.clone(),
        );
        let weather = panel::weather(view_panel.clone(), view_scene.clone(), send.clone());
        let badges = panel::badges(
            view_panel.clone(),
            view_scene.clone(),
            view_state.clone(),
            send.clone(),
        );
        let dialogs = panel::dialogs(
            view_panel.clone(),
            view_scene.clone(),
            view_facts.clone(),
            send.clone(),
        );
        let go = panel::go_to(
            view_go.clone(),
            view_scene.clone(),
            view_panel.clone(),
            send.clone(),
        );
        let context = panel::context_menu(
            view_context.clone(),
            view_scene.clone(),
            view_facts.clone(),
            view_state.clone(),
            send,
        );
        view! {
            <div data-profile-root>
            {toolbar}
            <div data-profile-canvas-host style="position:relative;flex:1;min-height:0;overflow:hidden">
            <canvas class="block h-full w-full touch-none select-none focus:outline-none" tabindex="0" style="touch-action:none;display:block;outline:none" aria-label="DS Grid profile. Click to select, Alt-click to cycle overlapping items, Enter to inspect, S for section criteria, wheel to zoom, drag to pan." />
            <canvas data-profile-review="true" aria-hidden="true" style="position:absolute;inset:0;pointer-events:none" />
            <button type="button" data-profile-station="true" aria-label="Drag selected structure along its alignment" style=move||handle_style(station.get().station)>"↔"</button>
            <button type="button" data-profile-ground="true" aria-label="Drag selected ground point vertically" style=move||handle_style(ground.get().ground)>"↕"</button>
            <span role="status" style="position:absolute;right:8px;top:8px;pointer-events:none;color:#a7f3d0;font:11px monospace">{move||label.get().ground_label}</span>
            {weather}{badges}
            </div>
            {dialogs}{go}{context}
            </div>
        }
    });
    let canvas: HtmlCanvasElement = parent
        .query_selector("canvas")?
        .ok_or_else(|| failure("profile_canvas_mount_failed"))?
        .dyn_into()?;
    static NEXT_MOUNT: AtomicU64 = AtomicU64::new(1);
    canvas.set_id(&format!(
        "ds-profile-{}",
        NEXT_MOUNT.fetch_add(1, Ordering::Relaxed)
    ));
    let ctx: Context = canvas
        .get_context("2d")?
        .ok_or_else(|| failure("profile_canvas_context_unavailable"))?
        .dyn_into()?;
    let review_canvas: HtmlCanvasElement = parent
        .query_selector("[data-profile-review]")?
        .unwrap()
        .dyn_into()?;
    let review_ctx: Context = review_canvas
        .get_context("2d")?
        .ok_or_else(|| failure("profile_canvas_context_unavailable"))?
        .dyn_into()?;
    let (base, base_ctx) = paint::canvas()?;
    let index = HitIndex::new(&scene);
    let runtime = Rc::new(RefCell::new(Runtime {
        root: parent.clone(),
        lowest_wire_segments,
        scene,
        source_scene,
        scene_signal,
        panel,
        facts,
        scale,
        goto_open,
        context_menu,
        state,
        handles,
        canvas: canvas.clone(),
        ctx,
        base,
        base_ctx,
        review_canvas,
        review_ctx,
        callback,
        outbox: vec![],
        index,
        width: 1.,
        height: 1.,
        dpr: 1.,
        dirty: true,
        base_dirty: true,
        request: None,
        raf: None,
        dead: false,
        generation: 1,
        scene_generation: 1,
        serial: 0,
        pending: None,
        receipt: None,
        paint_samples: VecDeque::with_capacity(512),
        resize_drag: None,
        tool: None,
        lasso_points: vec![],
        drag: None,
        touches: BTreeMap::new(),
        pinch: None,
        last_cycle: None,
        markers: paint::MarkerCache::default(),
        review: None,
        last_hover: 0.,
        last_focus_epoch: 0,
    }));
    *target.borrow_mut() = Rc::downgrade(&runtime);
    let weak = Rc::downgrade(&runtime);
    runtime.borrow_mut().raf = Some(Closure::new(move |_: f64| {
        if let Some(runtime) = weak.upgrade() {
            transact(&runtime, |r| {
                r.request = None;
                r.paint();
            });
        }
    }));
    let mut listeners = Vec::new();
    if let Some(separator) = parent.query_selector("[data-profile-resize]")? {
        let target: web_sys::EventTarget = separator.into();
        for name in [
            "pointerdown",
            "pointermove",
            "pointerup",
            "pointercancel",
            "lostpointercapture",
        ] {
            listen(&mut listeners, &target, name, &runtime, move |r, event| {
                if let Some(e) = event.dyn_ref::<PointerEvent>() {
                    r.resize_pointer(name, e);
                }
            })?;
        }
    }
    for name in [
        "pointerdown",
        "pointermove",
        "pointerup",
        "pointercancel",
        "lostpointercapture",
        "pointerleave",
    ] {
        listen(
            &mut listeners,
            &canvas.clone().into(),
            name,
            &runtime,
            move |r, e| {
                if let Some(e) = e.dyn_ref::<PointerEvent>() {
                    r.pointer(name, e, Mode::Pan);
                }
            },
        )?;
    }
    for (selector, mode) in [
        ("[data-profile-station]", Mode::Station),
        ("[data-profile-ground]", Mode::Ground),
    ] {
        let button: web_sys::EventTarget = parent.query_selector(selector)?.unwrap().into();
        for name in [
            "pointerdown",
            "pointermove",
            "pointerup",
            "pointercancel",
            "lostpointercapture",
        ] {
            listen(&mut listeners, &button, name, &runtime, move |r, e| {
                if let Some(e) = e.dyn_ref::<PointerEvent>() {
                    r.pointer(name, e, mode);
                }
            })?;
        }
    }
    listen(
        &mut listeners,
        &canvas.clone().into(),
        "wheel",
        &runtime,
        |r, e| {
            if let Some(e) = e.dyn_ref::<WheelEvent>() {
                r.wheel(e);
            }
        },
    )?;
    listen(
        &mut listeners,
        &canvas.clone().into(),
        "dblclick",
        &runtime,
        |r, e| {
            if let Some(e) = e.dyn_ref::<MouseEvent>() {
                e.prevent_default();
                let p = r.screen(e.client_x() as f64, e.client_y() as f64);
                let v = r.camera().zoom(p, 2.);
                r.camera_intent(v);
            }
        },
    )?;
    listen(
        &mut listeners,
        &canvas.clone().into(),
        "contextmenu",
        &runtime,
        |r, e| {
            if let Some(e) = e.dyn_ref::<MouseEvent>() {
                e.prevent_default();
                let p = r.screen(e.client_x() as f64, e.client_y() as f64);
                r.pick(
                    p,
                    Screen {
                        x: e.client_x() as f64,
                        y: e.client_y() as f64,
                    },
                    Modifiers::default(),
                    true,
                    false,
                    12.,
                );
            }
        },
    )?;
    listen(
        &mut listeners,
        &canvas.clone().into(),
        "keydown",
        &runtime,
        |r, e| {
            if let Some(e) = e.dyn_ref::<KeyboardEvent>() {
                r.key(e);
            }
        },
    )?;
    let window: web_sys::EventTarget = web_sys::window().unwrap().into();
    listen(&mut listeners, &window, "pointerdown", &runtime, |r, e| {
        let target = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok());
        if target.as_ref().is_none_or(|t| !r.root.contains(Some(t))) {
            r.close_menus();
            r.context_menu.set(None);
        }
    })?;
    listen(&mut listeners, &window, "keydown", &runtime, |r, e| {
        if let Some(e) = e.dyn_ref::<KeyboardEvent>() {
            if r.menu_key(e) {
                return;
            }
            let editing = e
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .is_some_and(|t| ["INPUT", "SELECT", "TEXTAREA"].contains(&t.tag_name().as_str()));
            if e.key() == "Escape" {
                let lasso = r.tool == Some(Mode::Lasso);
                r.cancel();
                r.tool = None;
                r.goto_open.set(false);
                r.context_menu.set(None);
                r.close_menus();
                if lasso {
                    r.outbox.push(Intent::LassoCancel);
                }
                r.schedule(false);
            } else if !editing
                && e.key().eq_ignore_ascii_case("g")
                && !e.ctrl_key()
                && !e.meta_key()
                && !e.alt_key()
            {
                e.prevent_default();
                r.goto_open.set(true);
            }
        }
    })?;
    let weak = Rc::downgrade(&runtime);
    let resize = Closure::new(move |entries: js_sys::Array, _: web_sys::ResizeObserver| {
        if let Some(runtime) = weak.upgrade() {
            if let Ok(entry) = entries.get(0).dyn_into::<web_sys::ResizeObserverEntry>() {
                let rect = entry.content_rect();
                transact(&runtime, |r| r.resize(rect.width(), rect.height()));
            }
        }
    });
    let observer = web_sys::ResizeObserver::new(resize.as_ref().unchecked_ref())?;
    let canvas_host = parent
        .query_selector("[data-profile-canvas-host]")?
        .unwrap();
    observer.observe(&canvas_host);
    let rect = canvas_host.get_bounding_client_rect();
    transact(&runtime, |r| r.resize(rect.width(), rect.height()));
    Ok(Mounted {
        runtime,
        mount: Some(Box::new(mounted)),
        listeners,
        observer: Some(observer),
        _resize: resize,
    })
}
fn listen(
    list: &mut Vec<Listener>,
    target: &web_sys::EventTarget,
    name: &'static str,
    runtime: &Rc<RefCell<Runtime>>,
    f: impl Fn(&mut Runtime, web_sys::Event) + 'static,
) -> Result<(), JsValue> {
    let weak: Weak<RefCell<Runtime>> = Rc::downgrade(runtime);
    let callback = Closure::new(move |event: web_sys::Event| {
        if let Some(r) = weak.upgrade() {
            transact(&r, |r| f(r, event));
        }
    });
    target.add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())?;
    list.push(Listener {
        target: target.clone(),
        name,
        callback,
    });
    Ok(())
}
impl Runtime {
    fn menu_key(&mut self, event: &KeyboardEvent) -> bool {
        let Some(target) = event
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return false;
        };
        if !self.root.contains(Some(&target)) {
            return false;
        }
        let menu = target.closest("[role=menu]").ok().flatten();
        let Some(menu) = menu else {
            return false;
        };
        if event.key() == "Escape" {
            event.prevent_default();
            if let Some(details) = menu.closest("details").ok().flatten() {
                let _ = details.remove_attribute("open");
                if details.get_attribute("data-profile-menu-focus").as_deref() == Some("header") {
                    let _ = details.remove_attribute("data-profile-menu-focus");
                    if let Some(header) = details
                        .closest("header")
                        .ok()
                        .flatten()
                        .and_then(|h| h.dyn_into::<HtmlElement>().ok())
                    {
                        let _ = header.focus();
                    }
                    return true;
                }
                if let Some(summary) = details
                    .query_selector("summary")
                    .ok()
                    .flatten()
                    .and_then(|s| s.dyn_into::<HtmlElement>().ok())
                {
                    let _ = summary.focus();
                }
            } else {
                self.context_menu.set(None);
                let _ = self.canvas.focus();
            }
            return true;
        }
        if !["ArrowDown", "ArrowUp", "Home", "End"].contains(&event.key().as_str()) {
            return false;
        }
        let Ok(nodes) = menu.query_selector_all(
            "button:not([disabled]), input:not([disabled]), select:not([disabled]), summary",
        ) else {
            return false;
        };
        let items: Vec<HtmlElement> = (0..nodes.length())
            .filter_map(|i| nodes.item(i)?.dyn_into().ok())
            .collect();
        if items.is_empty() {
            return false;
        }
        let current = items
            .iter()
            .position(|e| <HtmlElement as AsRef<web_sys::Element>>::as_ref(e) == &target)
            .unwrap_or(0);
        let index = match event.key().as_str() {
            "Home" => 0,
            "End" => items.len() - 1,
            "ArrowDown" => (current + 1) % items.len(),
            _ => (current + items.len() - 1) % items.len(),
        };
        event.prevent_default();
        let _ = items[index].focus();
        true
    }
    fn close_menus(&self) {
        if let Ok(nodes) = self.root.query_selector_all("details[open]") {
            for i in 0..nodes.length() {
                if let Some(e) = nodes
                    .item(i)
                    .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
                {
                    let _ = e.remove_attribute("open");
                }
            }
        }
    }
    fn resize_pointer(&mut self, name: &str, e: &PointerEvent) {
        if name == "pointerdown" && e.button() == 0 {
            e.prevent_default();
            if let Some(target) = e
                .current_target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            {
                let _ = target.set_pointer_capture(e.pointer_id());
            }
            let height = self
                .canvas
                .closest("[data-profile-root]")
                .ok()
                .flatten()
                .map(|e| e.get_bounding_client_rect().height())
                .unwrap_or(self.height + 36.);
            self.resize_drag = Some(ResizeGesture {
                pointer: e.pointer_id(),
                y: e.client_y() as f64,
                height,
            });
        } else if self
            .resize_drag
            .as_ref()
            .is_some_and(|g| g.pointer == e.pointer_id())
        {
            e.prevent_default();
            if name == "pointermove" {
                let g = self.resize_drag.as_ref().unwrap();
                self.outbox.push(Intent::Resize {
                    height_px: (g.height + g.y - e.client_y() as f64).max(80.),
                });
            } else {
                self.resize_drag = None;
            }
        }
    }
    fn ui_event(&mut self, event: panel::Event) {
        self.close_menus();
        match event {
            panel::Event::Intent(intent) => {
                self.context_menu.set(None);
                self.outbox.push(intent);
            }
            panel::Event::Dismiss => self.context_menu.set(None),
            panel::Event::Clear => {
                self.context_menu.set(None);
                self.select(vec![], None);
            }
            panel::Event::Fit => self.camera_intent(Viewport::default()),
            panel::Event::Zoom(factor) => {
                let v = self.camera().zoom(
                    Screen {
                        x: self.width / 2.,
                        y: self.height / 2.,
                    },
                    factor,
                );
                self.camera_intent(v);
            }
            panel::Event::GoOpen => self.goto_open.set(true),
            panel::Event::FocusCanvas => {
                self.goto_open.set(false);
                let _ = self.canvas.focus();
            }
            panel::Event::GoTo(id) => {
                self.goto_open.set(false);
                self.focus(&id);
                self.outbox.push(Intent::StructureGoto { entity_id: id });
            }
            panel::Event::ZoomWindow => {
                self.tool = if self.tool == Some(Mode::ZoomWindow) {
                    None
                } else {
                    Some(Mode::ZoomWindow)
                };
                self.cancel();
            }
        }
    }
    fn focus_requested(&mut self) {
        let request = self.state.get_untracked().focus_request;
        if let Some(request) = request {
            if request.epoch != self.last_focus_epoch
                && request.model_revision == self.scene.model_revision
                && self.width > 1.
                && self.height > 1.
                && self.scene.point(&request.entity_id).is_some()
            {
                self.last_focus_epoch = request.epoch;
                self.focus(&request.entity_id);
            }
        }
    }
    fn focus(&mut self, id: &str) {
        let Some(point) = self.scene.point(id) else {
            return;
        };
        let mut positions: Vec<_> = self
            .scene
            .bands
            .iter()
            .flat_map(|b| b.structures.iter().map(|s| s.ground.scene_x))
            .collect();
        positions.push(point.scene_x);
        positions.sort_by(f64::total_cmp);
        positions.dedup();
        let index = positions.partition_point(|x| *x < point.scene_x);
        let start = index
            .saturating_sub(1)
            .min(positions.len().saturating_sub(3));
        let width = (positions[(start + 2).min(positions.len() - 1)] - positions[start]).max(1.);
        let mut fitted = self.camera();
        fitted.viewport = Viewport::default();
        let desired = ((self.width - 48.).max(1.) * 0.72
            / (width * fitted.projection().scale).max(0.01))
        .clamp(0.2, 1e6);
        let mut camera = self.camera();
        camera.viewport.zoom = camera.viewport.zoom.max(desired);
        self.camera_intent(camera.center(point));
        let _ = self.canvas.focus();
    }
    fn camera(&self) -> Camera {
        Camera {
            bounds: self.scene.bounds,
            width: self.width,
            height: self.height,
            viewport: self.state.get_untracked().viewport,
        }
    }
    fn invalidate(&mut self) {
        self.context_menu.set(None);
        self.generation += 1;
        self.pending = None;
    }
    fn schedule(&mut self, base: bool) {
        self.dirty = true;
        self.base_dirty |= base;
        if self.request.is_none() && !self.dead {
            if let Some(callback) = &self.raf {
                self.request = web_sys::window()
                    .unwrap()
                    .request_animation_frame(callback.as_ref().unchecked_ref())
                    .ok();
            }
        }
    }
    fn immediate(&mut self) {
        if let Some(id) = self.request.take() {
            let _ = web_sys::window().unwrap().cancel_animation_frame(id);
        }
        self.paint();
    }
    fn resize(&mut self, w: f64, h: f64) {
        let w = w.round().max(1.);
        let h = h.round().max(1.);
        let dpr = web_sys::window().unwrap().device_pixel_ratio().max(1.);
        if w != self.width || h != self.height || dpr != self.dpr {
            self.cancel();
            self.width = w;
            self.height = h;
            self.dpr = dpr;
            self.invalidate();
            self.base_dirty = true;
            let target = self
                .state
                .get_untracked()
                .selected_entity_id
                .as_ref()
                .and_then(|id| self.scene.point(id));
            let v = self.camera().rebase(target);
            if v != self.state.get_untracked().viewport {
                self.camera_intent(v);
            }
            self.focus_requested();
            self.immediate();
        }
    }
    fn paint(&mut self) {
        if self.dead || self.width <= 1. || self.height <= 1. {
            return;
        }
        let start = now();
        let state = self.state.get_untracked();
        let camera = self.camera();
        paint::size(&self.canvas, &self.ctx, self.width, self.height, self.dpr);
        paint::size(
            &self.base,
            &self.base_ctx,
            self.width,
            self.height,
            self.dpr,
        );
        paint::size(
            &self.review_canvas,
            &self.review_ctx,
            self.width,
            self.height,
            self.dpr,
        );
        if self.base_dirty {
            paint::base(
                &self.base_ctx,
                &self.scene,
                &state,
                &camera,
                &mut self.markers,
                &self.lowest_wire_segments,
            );
            self.base_dirty = false;
        }
        let _ = self.ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
            &self.base,
            0.,
            0.,
            self.width,
            self.height,
        );
        paint::selection(&self.ctx, &self.scene, &state, &camera);
        paint::feature_clearances(&self.ctx, &self.scene, &state, &camera, true);
        if self.scene.current()
            && self
                .scene
                .display
                .as_ref()
                .is_some_and(|d| d.state.visibility.get("review") != Some(&false))
        {
            self.markers.paint(&self.ctx, &self.scene, &camera);
        }
        paint::generic_labels(&self.ctx, &self.scene, &state, &camera);
        self.review_ctx.clear_rect(0., 0., self.width, self.height);
        if let Some(g) = &self.drag {
            if g.mode == Mode::ZoomWindow || g.mode == Mode::Lasso {
                let c = &self.review_ctx;
                c.set_stroke_style_str("#67e8f9");
                c.set_line_width(1.);
                if g.mode == Mode::ZoomWindow {
                    c.stroke_rect(
                        g.start.x,
                        g.start.y,
                        g.last.x - g.start.x,
                        g.last.y - g.start.y,
                    );
                } else {
                    c.begin_path();
                    for (i, p) in self.lasso_points.iter().enumerate() {
                        if i == 0 {
                            c.move_to(p.x, p.y);
                        } else {
                            c.line_to(p.x, p.y);
                        }
                    }
                    c.stroke();
                }
            }
        }
        if let Some(review) = self.review {
            paint::review(&self.review_ctx, &self.scene, &camera, review);
        }
        let sequence = self.receipt.as_ref().map(|r| r.sequence + 1).unwrap_or(1);
        let finished = now();
        if self.paint_samples.len() == 512 {
            self.paint_samples.pop_front();
        }
        self.paint_samples.push_back(PaintSample {
            sequence,
            scene_generation: self.scene_generation,
            started_at: start,
            finished_at: finished,
        });
        self.receipt = Some(Receipt {
            started_at: start,
            finished_at: finished,
            sequence,
            scene_generation: self.scene_generation,
            width: self.width,
            height: self.height,
            scale: camera.projection().scale,
            viewport: state.viewport,
            visibility: state.visibility,
            context: self.context(),
        });
        self.dirty = false;
        self.scale.set(camera.projection().scale);
        self.update_handles();
        self.outbox.push(Intent::Surface {
            width: self.width,
            height: self.height,
            scale: camera.projection().scale,
        });
    }
    fn context(&self) -> String {
        let s = self.state.get_untracked();
        if !s.enabled
            || !s.pickable
            || self.width <= 1.
            || self.height <= 1.
            || s.history_key.is_empty()
            || self.scene.axis_digest().is_empty()
        {
            return String::new();
        }
        format!(
            "{}:{}:{}",
            self.canvas.id(),
            self.scene_generation,
            self.generation
        )
    }
    fn camera_intent(&mut self, viewport: Viewport) {
        let mut s = self.state.get_untracked();
        if s.viewport == viewport {
            return;
        }
        s.viewport = viewport.clone();
        self.state.set(s);
        self.invalidate();
        self.schedule(true);
        self.outbox.push(Intent::Viewport { viewport });
        self.update_handles();
    }
    fn screen(&self, x: f64, y: f64) -> Screen {
        let r = self.canvas.get_bounding_client_rect();
        Screen {
            x: x - r.left(),
            y: y - r.top(),
        }
    }
    fn update_handles(&self) {
        let s = self.state.get_untracked();
        let mut handles = Handles::default();
        if s.edit_mode
            && s.model_id.is_some()
            && !s.create_active
            && s.selected_entity_ids.len() == 1
        {
            if let Some(id) = &s.selected_entity_id {
                if s.station_move_enabled {
                    if let Some(structure) = self
                        .scene
                        .bands
                        .iter()
                        .flat_map(|b| &b.structures)
                        .find(|p| &p.entity_id == id)
                    {
                        let p = structure
                            .top
                            .map(|top| {
                                Point::xy(
                                    structure.ground.scene_x,
                                    (structure.ground.scene_y + top.scene_y) / 2.,
                                )
                            })
                            .unwrap_or(structure.ground);
                        let mut p = self.camera().project(p);
                        p.y = p.y.clamp(18., (self.height - 18.).max(18.));
                        if let Some(g) = &self.drag {
                            if g.mode == Mode::Station {
                                p.x += g.last.x - g.start.x;
                            }
                        }
                        handles.station = Some(p);
                    }
                }
                if s.ground_move_enabled && !s.ground_move_busy && s.visibility.show_terrain_points
                {
                    if let Some(p) = self
                        .scene
                        .bands
                        .iter()
                        .flat_map(|b| &b.terrain_points)
                        .find(|p| &p.entity_id == id)
                    {
                        let mut point = p.point;
                        if let (Some(g), Some(axis)) = (&self.drag, &self.scene.axis_pin) {
                            if g.mode == Mode::Ground {
                                if let Some((e, clamped)) = selection::ground_candidate(
                                    p.point.elevation_m,
                                    g.last.y - g.start.y,
                                    self.camera().projection().scale,
                                    axis,
                                ) {
                                    point.scene_y +=
                                        (p.point.elevation_m - e) * axis.vertical_px_per_m;
                                    handles.ground_label = format!(
                                        "{e:.2} m{}",
                                        if clamped { " · limit" } else { "" }
                                    );
                                }
                            }
                        }
                        handles.ground = Some(self.camera().project(point));
                    }
                }
            }
        }
        self.handles.set(handles);
    }
    fn cancel(&mut self) {
        if let Some(g) = self.drag.take() {
            if g.mode == Mode::Lasso {
                self.outbox.push(Intent::LassoAbort);
            }
            if let Some(target) = g.capture {
                let _ = target.release_pointer_capture(g.id);
            }
        }
        self.touches.clear();
        self.pinch = None;
        self.lasso_points.clear();
        self.update_handles();
    }
    fn pinch_geometry(&self) -> Option<(f64, Screen)> {
        let mut i = self.touches.values();
        let a = i.next()?;
        let b = i.next()?;
        Some((
            (b.x - a.x).hypot(b.y - a.y),
            Screen {
                x: (a.x + b.x) / 2.,
                y: (a.y + b.y) / 2.,
            },
        ))
    }
    fn sample_lasso(&mut self, point: Screen) {
        if self
            .lasso_points
            .last()
            .is_some_and(|p| (p.x - point.x).hypot(p.y - point.y) < 3.)
        {
            return;
        }
        if self.lasso_points.len() >= 255 {
            let mut index = 0usize;
            self.lasso_points.retain(|_| {
                let keep = index.is_multiple_of(2);
                index += 1;
                keep
            });
        }
        self.lasso_points.push(point);
    }
    fn pointer(&mut self, name: &str, e: &PointerEvent, mode: Mode) {
        let s = self.state.get_untracked();
        if !s.enabled {
            return;
        }
        let p = self.screen(e.client_x() as f64, e.client_y() as f64);
        if name == "pointerleave" {
            let _ = web_sys::HtmlElement::style(&self.canvas).set_property("cursor", "crosshair");
            return;
        }
        let mode = if mode == Mode::Pan {
            self.tool.unwrap_or(mode)
        } else {
            mode
        };
        if name == "pointerdown" {
            if e.button() != 0 && e.button() != 1 {
                return;
            }
            if mode == Mode::Lasso
                && !s
                    .panel
                    .as_ref()
                    .is_some_and(|p| p.can_lasso && !p.lasso_busy)
            {
                return;
            }
            if mode == Mode::Station && !s.station_move_enabled
                || mode == Mode::Ground && (!s.ground_move_enabled || s.ground_move_busy)
            {
                return;
            }
            e.prevent_default();
            e.stop_propagation();
            let _ = self.canvas.focus();
            let target = e
                .current_target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok());
            if let Some(target) = &target {
                let _ = target.set_pointer_capture(e.pointer_id());
            }
            if e.pointer_type() == "touch" && mode == Mode::Pan {
                self.touches.insert(e.pointer_id(), p);
                if self.touches.len() >= 2 {
                    self.drag = None;
                    self.pinch = self.pinch_geometry();
                    return;
                }
            }
            if mode == Mode::Lasso {
                self.outbox.push(Intent::LassoBegin);
                self.lasso_points = vec![p];
            }
            self.drag = Some(Gesture {
                id: e.pointer_id(),
                capture: target,
                start: p,
                last: p,
                viewport: s.viewport,
                moved: false,
                button: e.button(),
                mode,
                revision: self.scene.model_revision.clone(),
                axis: self.scene.axis_digest().into(),
                selection: s.selected_entity_id,
                selection_epoch: s.selection_epoch,
                additive: e.shift_key() || e.ctrl_key() || e.meta_key(),
            });
            return;
        }
        if name == "pointermove" {
            if e.pointer_type() == "touch" && self.touches.contains_key(&e.pointer_id()) {
                self.touches.insert(e.pointer_id(), p);
                if let Some((dist, mid)) = self.pinch {
                    if let Some((next_dist, next_mid)) = self.pinch_geometry() {
                        if dist > 0. {
                            let mut v = self.camera().zoom(next_mid, next_dist / dist);
                            v.pan_x += next_mid.x - mid.x;
                            v.pan_y += next_mid.y - mid.y;
                            self.camera_intent(v);
                        }
                        self.pinch = Some((next_dist, next_mid));
                    }
                    return;
                }
            }
            if let Some(mut drag) = self.drag.clone().filter(|g| g.id == e.pointer_id()) {
                if e.pointer_type() != "touch" && e.buttons() == 0 {
                    self.cancel();
                    return;
                }
                e.prevent_default();
                e.stop_propagation();
                drag.last = p;
                drag.moved |= (p.x - drag.start.x).hypot(p.y - drag.start.y)
                    >= if e.pointer_type() == "touch" { 10. } else { 3. };
                self.drag = Some(drag.clone());
                if drag.moved && mode == Mode::Pan {
                    self.camera_intent(Viewport {
                        zoom: s.viewport.zoom,
                        pan_x: drag.viewport.pan_x + p.x - drag.start.x,
                        pan_y: drag.viewport.pan_y + p.y - drag.start.y,
                    });
                }
                if mode == Mode::ZoomWindow || mode == Mode::Lasso {
                    if mode == Mode::Lasso {
                        self.sample_lasso(p);
                    }
                    self.schedule(false);
                }
                self.update_handles();
                return;
            }
            if mode == Mode::Pan && e.pointer_type() != "touch" && now() - self.last_hover > 70. {
                self.last_hover = now();
                self.pick(p, p, Modifiers::default(), false, true, 12.);
            }
            return;
        }
        if e.pointer_type() == "touch" {
            self.touches.remove(&e.pointer_id());
            if self.pinch.is_some() {
                if self.touches.len() < 2 {
                    self.pinch = None;
                    self.drag = None;
                }
                return;
            }
        }
        if self.drag.as_ref().is_none_or(|g| g.id != e.pointer_id()) {
            return;
        }
        let drag = self.drag.take().unwrap();
        e.prevent_default();
        e.stop_propagation();
        self.update_handles();
        let cancelled = name != "pointerup";
        if let Some(target) = e
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        {
            if target.has_pointer_capture(e.pointer_id()) {
                let _ = target.release_pointer_capture(e.pointer_id());
            }
        }
        if cancelled {
            if drag.mode == Mode::Lasso {
                self.outbox.push(Intent::LassoAbort);
            }
            return;
        }
        if drag.mode == Mode::ZoomWindow {
            self.tool = None;
            if drag.moved {
                if let Ok(v) = self.camera().window(drag.start, p) {
                    self.camera_intent(v);
                }
            }
            self.schedule(false);
            return;
        }
        if drag.mode == Mode::Lasso {
            self.tool = None;
            self.sample_lasso(p);
            if drag.moved && self.lasso_points.len() >= 3 {
                let polygon = self
                    .lasso_points
                    .iter()
                    .map(|p| self.camera().unproject(*p))
                    .collect();
                self.outbox.push(Intent::LassoFinish {
                    polygon,
                    selection_epoch: drag.selection_epoch,
                    mode: if drag.additive {
                        Some("add".into())
                    } else {
                        None
                    },
                });
            } else {
                self.outbox.push(Intent::LassoAbort);
            }
            self.schedule(false);
            return;
        }
        if drag.mode != Mode::Pan {
            if drag.revision != self.scene.model_revision
                || drag.axis != self.scene.axis_digest()
                || drag.selection != s.selected_entity_id
                || s.selected_entity_ids.len() != 1
                || !s.edit_mode
            {
                return;
            }
            let Some(id) = drag.selection else {
                return;
            };
            let scale = self.camera().projection().scale;
            if drag.mode == Mode::Station && (p.x - drag.start.x).abs() >= 3. {
                if let Some((structure, axis)) = self.scene.bands.iter().find_map(|b| {
                    b.structures
                        .iter()
                        .find(|s| s.entity_id == id)
                        .map(|s| (s, &b.station_axis))
                }) {
                    if let Some(station_m) = selection::station_candidate(
                        structure.ground.station_m,
                        p.x - drag.start.x,
                        scale,
                        axis,
                    ) {
                        self.outbox.push(Intent::MoveStation {
                            entity_id: id,
                            station_m,
                        });
                    }
                }
            } else if drag.mode == Mode::Ground && (p.y - drag.start.y).abs() >= 3. {
                if let (Some(point), Some(axis)) = (
                    self.scene
                        .bands
                        .iter()
                        .flat_map(|b| &b.terrain_points)
                        .find(|p| p.entity_id == id),
                    &self.scene.axis_pin,
                ) {
                    if let Some((elevation_m, _)) = selection::ground_candidate(
                        point.point.elevation_m,
                        p.y - drag.start.y,
                        scale,
                        axis,
                    ) {
                        if elevation_m != point.point.elevation_m {
                            self.outbox.push(Intent::MoveGround {
                                entity_id: id,
                                elevation_m,
                                revision: self.scene.model_revision.clone(),
                            });
                        }
                    }
                }
            }
            return;
        }
        if drag.moved
            || drag.button != 0
            || (p.x - drag.start.x).hypot(p.y - drag.start.y)
                >= if e.pointer_type() == "touch" { 10. } else { 3. }
        {
            return;
        }
        let tolerance = if e.pointer_type() == "touch" {
            18.
        } else {
            12.
        };
        if s.create_active {
            if !s.create_busy && s.pickable && s.edit_mode && !self.scene.axis_digest().is_empty() {
                let mut pointer = self.pick_request(p, tolerance);
                pointer.tolerance_scene_units = pointer.tolerance_scene_units.min(32.);
                self.outbox.push(Intent::Create { pointer });
            }
            return;
        }
        if self.review_click(p) {
            return;
        }
        self.pick(
            p,
            Screen {
                x: e.client_x() as f64,
                y: e.client_y() as f64,
            },
            Modifiers {
                toggle: e.ctrl_key() || e.meta_key(),
                range: e.shift_key(),
                cycle: e.alt_key(),
            },
            false,
            false,
            tolerance,
        );
    }
    fn wheel(&mut self, e: &WheelEvent) {
        e.prevent_default();
        let v = if e.shift_key() {
            self.camera().pan(
                -if e.delta_x() != 0. {
                    e.delta_x()
                } else {
                    e.delta_y()
                },
                0.,
            )
        } else {
            self.camera().zoom(
                self.screen(e.client_x() as f64, e.client_y() as f64),
                if e.delta_y() < 0. { 1.12 } else { 0.89 },
            )
        };
        self.camera_intent(v);
    }
    fn key(&mut self, e: &KeyboardEvent) {
        let s = self.state.get_untracked();
        if e.key().eq_ignore_ascii_case("g") && !e.ctrl_key() && !e.meta_key() && !e.alt_key() {
            self.goto_open.set(true);
            e.prevent_default();
            return;
        }
        let step = if e.shift_key() { 120. } else { 48. };
        let camera = self.camera();
        let v = match e.key().as_str() {
            "Home" | "0" => Some(Viewport::default()),
            "ArrowLeft" => Some(camera.pan(step, 0.)),
            "ArrowRight" => Some(camera.pan(-step, 0.)),
            "ArrowUp" => Some(camera.pan(0., step)),
            "ArrowDown" => Some(camera.pan(0., -step)),
            "+" | "=" => Some(camera.zoom(
                Screen {
                    x: self.width / 2.,
                    y: self.height / 2.,
                },
                1.25,
            )),
            "-" => Some(camera.zoom(
                Screen {
                    x: self.width / 2.,
                    y: self.height / 2.,
                },
                0.8,
            )),
            _ => None,
        };
        if let Some(v) = v {
            e.prevent_default();
            self.camera_intent(v);
            return;
        }
        match e.key().as_str() {
            "Escape" => {
                e.prevent_default();
                if self.tool == Some(Mode::Lasso) {
                    self.outbox.push(Intent::LassoCancel);
                }
                self.cancel();
                self.tool = None;
                self.goto_open.set(false);
                self.review = None;
                self.schedule(false);
                if s.create_active {
                    self.outbox.push(Intent::CreateCancel);
                }
            }
            "Enter" => {
                if let Some(entity_id) = s.selected_entity_id {
                    e.prevent_default();
                    self.outbox.push(Intent::Inspect { entity_id });
                }
            }
            "s" | "S"
                if !e.ctrl_key()
                    && !e.meta_key()
                    && !e.alt_key()
                    && s.selected_entity_ids
                        .iter()
                        .any(|id| self.scene.selection_order.tension_sections.contains(id)) =>
            {
                e.prevent_default();
                self.outbox.push(Intent::SectionCriteria);
            }
            _ => {}
        }
    }
    fn pick_request(&self, screen: Screen, tolerance: f64) -> PickRequest {
        let p = self.camera().unproject(screen);
        let scale = self.camera().projection().scale;
        PickRequest {
            model_revision: self.scene.model_revision.clone(),
            axis_pin_digest: self.scene.axis_digest().into(),
            scene_x: p.scene_x,
            scene_y: p.scene_y,
            tolerance_scene_units: tolerance / scale,
            scene_units_per_px: 1. / scale,
            visibility: self.state.get_untracked().visibility,
        }
    }
    fn pick(
        &mut self,
        screen: Screen,
        client: Screen,
        modifiers: Modifiers,
        context: bool,
        hover: bool,
        tolerance: f64,
    ) {
        let state = self.state.get_untracked();
        if !state.pickable || !state.enabled {
            return;
        }
        if hover && self.pending.is_some() {
            return;
        }
        self.serial += 1;
        let pending = Pending {
            serial: self.serial,
            generation: self.generation,
            screen,
            client,
            modifiers,
            context,
            hover,
            candidates: vec![],
            picked: None,
        };
        if state.model_id.is_some() {
            if self.context().is_empty() || !self.painted_current() {
                return;
            }
            let request = self.pick_request(screen, tolerance);
            self.pending = Some(pending);
            self.outbox.push(Intent::Pick {
                request_id: self.serial,
                request,
            });
        } else {
            let candidates = self
                .index
                .pick(&self.camera(), screen, &state.visibility, tolerance);
            self.accept_pick(pending, candidates);
        }
    }
    fn painted_current(&self) -> bool {
        self.receipt.as_ref().is_some_and(|r| {
            r.scene_generation == self.scene_generation
                && r.width == self.width
                && r.height == self.height
                && r.viewport == self.state.get_untracked().viewport
                && r.visibility == self.state.get_untracked().visibility
        })
    }
    fn accept_pick(&mut self, mut pending: Pending, candidates: Vec<Candidate>) {
        if pending.generation != self.generation {
            return;
        }
        if pending.hover {
            let _ = web_sys::HtmlElement::style(&self.canvas).set_property(
                "cursor",
                if candidates.is_empty() {
                    "crosshair"
                } else {
                    "pointer"
                },
            );
            return;
        }
        let state = self.state.get_untracked();
        if candidates.is_empty()
            && (pending.modifiers.toggle || pending.modifiers.range || pending.modifiers.cycle)
            && !pending.context
        {
            return;
        }
        let mut picked = candidates.first();
        if pending.context {
            picked = candidates
                .iter()
                .find(|c| state.selected_entity_ids.contains(&c.entity_id))
                .or(picked);
        }
        if pending.modifiers.cycle {
            if let Some((point, id)) = &self.last_cycle {
                if (point.x - pending.screen.x).hypot(point.y - pending.screen.y) <= 12. {
                    if let Some(i) = candidates.iter().position(|c| &c.entity_id == id) {
                        picked = candidates.get((i + 1) % candidates.len().max(1));
                    }
                }
            }
        }
        let id = picked.map(|c| c.entity_id.clone());
        pending.picked = id.clone();
        self.last_cycle = id.as_ref().map(|id| (pending.screen, id.clone()));
        if pending.context {
            let (ids, primary) = if id.is_none()
                || id
                    .as_ref()
                    .is_some_and(|id| state.selected_entity_ids.contains(id))
            {
                (
                    state.selected_entity_ids.clone(),
                    id.clone().or_else(|| state.selected_entity_id.clone()),
                )
            } else {
                (id.iter().cloned().collect(), id)
            };
            self.select(ids.clone(), primary.clone());
            self.context_menu.set(Some(pending.client));
            return;
        }
        if pending.modifiers.range {
            if let (Some(from), Some(to)) = (&state.selected_entity_id, &id) {
                if state.model_id.is_some() {
                    let request = RangeRequest {
                        model_revision: self.scene.model_revision.clone(),
                        axis_pin_digest: self.scene.axis_digest().into(),
                        from_entity_id: from.clone(),
                        to_entity_id: to.clone(),
                    };
                    pending.candidates = candidates;
                    self.pending = Some(pending.clone());
                    self.outbox.push(Intent::Range {
                        request_id: pending.serial,
                        request,
                    });
                    return;
                }
                let range = selection::range(&self.scene, from, to);
                let (ids, primary) = selection::choose(
                    &state.selected_entity_ids,
                    id.as_deref(),
                    Some(&range),
                    pending.modifiers,
                );
                self.select(ids, primary);
                return;
            }
        }
        let (ids, primary) = selection::choose(
            &state.selected_entity_ids,
            id.as_deref(),
            None,
            pending.modifiers,
        );
        self.select(ids, primary);
    }
    fn select(&mut self, entity_ids: Vec<String>, primary_id: Option<String>) {
        let mut state = self.state.get_untracked();
        state.selected_entity_ids = entity_ids.clone();
        state.selected_entity_id = primary_id.clone();
        self.state.set(state);
        self.facts.set(panel::Facts {
            edit_mode: self.state.get_untracked().edit_mode,
            selected: entity_ids.clone(),
            primary: primary_id.clone(),
        });
        self.update_handles();
        self.schedule(false);
        self.outbox.push(Intent::Select {
            entity_ids,
            primary_id,
        });
    }
    fn review_click(&mut self, p: Screen) -> bool {
        if !self.scene.current() {
            return false;
        }
        let Some(display) = &self.scene.display else {
            return false;
        };
        if display.state.visibility.get("review") == Some(&false) {
            return false;
        }
        let Some(engineering) = &self.scene.engineering else {
            return false;
        };
        let hit = engineering.markers.iter().enumerate().find_map(|(i, m)| {
            if display.state.visibility.get(&m.kind) == Some(&false) {
                return None;
            }
            m.labels
                .iter()
                .enumerate()
                .find(|(_, l)| {
                    l.summary.is_some()
                        && hit::contains(l.hit_rect_px, self.camera().project(m.point), p)
                })
                .map(|(j, _)| (i, j, 0))
        });
        let hit_id = hit.map(|h| engineering.markers[h.0].entity_id.clone());
        if let Some(open) = self.review {
            let marker = &engineering.markers[open.0];
            let root = marker.labels[open.1].summary.as_ref().unwrap();
            let summary = if open.2 == 0 {
                root
            } else {
                &root.members[open.2 - 1]
            };
            let anchor = paint::review_anchor(&self.camera(), marker, summary);
            if hit::contains(summary.details_rect_px, anchor, p)
                || hit.is_some_and(|h| h.0 == open.0 && !root.members.is_empty())
            {
                self.review = Some((open.0, open.1, (open.2 + 1) % (root.members.len() + 1)));
                self.schedule(false);
                return true;
            }
            if hit::contains(summary.issues_rect_px, anchor, p) {
                self.outbox.push(Intent::ReviewIssues {
                    entity_id: marker.entity_id.clone(),
                });
                self.review = None;
                self.schedule(false);
                return true;
            }
            if hit::contains(summary.rect_px, anchor, p) {
                return true;
            }
            self.review = None;
            self.schedule(false);
        }
        if let Some(hit) = hit {
            let id = hit_id.unwrap();
            self.review = Some(hit);
            self.select(vec![id.clone()], Some(id));
            return true;
        }
        false
    }
}
impl Mounted {
    pub fn update(&mut self, scene_json: Option<&str>, state_json: &str) -> Result<(), JsValue> {
        let mut state: ViewState = parse(state_json)?;
        let mut scene = scene_json.map(parse::<Scene>).transpose()?;
        if let Some(scene) = &mut scene {
            scene.prepare_render();
        }
        if let Some(scene) = &scene {
            scene.validate().map_err(|s| failure(&s))?;
        }
        transact(&self.runtime, |r| {
            let old = r.state.get_untracked();
            let geometry = scene.is_some()
                || old.geometry_preview != state.geometry_preview
                || old.viewport != state.viewport
                || old.visibility != state.visibility
                || old.scope != state.scope
                || old.history_key != state.history_key
                || old.history_generation != state.history_generation
                || old.model_id != state.model_id
                || old.enabled != state.enabled
                || old.pickable != state.pickable;
            // Selection is an overlay update. Display option changes repaint base.
            let base = scene.is_some()
                || old.geometry_preview != state.geometry_preview
                || old.viewport != state.viewport
                || old.visibility != state.visibility
                || old.show_grid != state.show_grid
                || old.show_clearance_guides != state.show_clearance_guides
                || old.show_span_labels != state.show_span_labels
                || old.show_structure_labels != state.show_structure_labels
                || old.label_orientation != state.label_orientation
                || old.wire_opacity != state.wire_opacity
                || old.lowest_wire_drop_m != state.lowest_wire_drop_m
                || old.terrain_point_color != state.terrain_point_color
                || old.terrain_point_width_px != state.terrain_point_width_px;
            if geometry {
                r.invalidate();
            }
            let new_source = scene.is_some();
            if let Some(scene) = scene {
                r.source_scene = Arc::new(scene);
            }
            if new_source || old.geometry_preview != state.geometry_preview {
                r.close_menus();
                r.cancel();
                let next =
                    crate::preview::project(&r.source_scene, state.geometry_preview.as_ref())
                        .map(Arc::new)
                        .unwrap_or_else(|| r.source_scene.clone());
                r.index = HitIndex::new(&next);
                r.scene = next;
                r.scene_signal.set(r.scene.clone());
                r.scene_generation += 1;
                r.markers.clear();
                r.review = None;
                let target = state
                    .selected_entity_id
                    .as_ref()
                    .and_then(|id| r.scene.point(id));
                let mut camera = r.camera();
                camera.viewport = state.viewport.clone();
                let rebased = camera.rebase(target);
                if rebased != state.viewport {
                    state.viewport = rebased.clone();
                    r.outbox.push(Intent::Viewport { viewport: rebased });
                }
            }
            if (geometry || old.selection_epoch != state.selection_epoch)
                && r.drag
                    .as_ref()
                    .is_some_and(|g| g.mode == Mode::Lasso || g.mode == Mode::ZoomWindow)
            {
                r.cancel();
                r.tool = None;
            }
            if r.drag.as_ref().is_some_and(|g| {
                g.mode != Mode::Pan
                    && (geometry
                        || old.selected_entity_ids != state.selected_entity_ids
                        || g.selection != state.selected_entity_id
                        || g.revision != r.scene.model_revision
                        || g.axis != r.scene.axis_digest()
                        || !state.edit_mode)
            }) {
                r.cancel();
            }
            let facts = panel::Facts {
                edit_mode: state.edit_mode,
                selected: state.selected_entity_ids.clone(),
                primary: state.selected_entity_id.clone(),
            };
            if r.facts.get_untracked() != facts {
                r.facts.set(facts);
            }
            if new_source
                || old.geometry_preview != state.geometry_preview
                || old.lowest_wire_drop_m != state.lowest_wire_drop_m
            {
                r.lowest_wire_segments =
                    crate::draw_geometry::lowest_wire_drop(&r.scene, state.lowest_wire_drop_m);
            }
            if old.panel != state.panel {
                r.panel.set(state.panel.clone());
            }
            r.tool = if state.panel.as_ref().is_some_and(|p| p.lasso_active) {
                Some(Mode::Lasso)
            } else if r.tool == Some(Mode::Lasso) {
                None
            } else {
                r.tool
            };
            r.state.set(state);
            r.focus_requested();
            r.schedule(base);
            r.update_handles();
            if scene_json.is_some() {
                r.immediate();
            }
        });
        Ok(())
    }
    pub fn invalidate(&mut self) {
        transact(&self.runtime, |r| {
            r.invalidate();
            r.schedule(false);
        });
    }
    pub fn reply(&mut self, id: u64, json: &str) -> Result<(), JsValue> {
        let result: serde_json::Value = parse(json)?;
        transact(&self.runtime, |r| {
            let Some(pending) = r.pending.take() else {
                return;
            };
            if pending.serial != id {
                r.pending = Some(pending);
                return;
            }
            if pending.generation != r.generation {
                return;
            }
            if result.get("error").is_some() {
                if !pending.hover {
                    r.outbox.push(Intent::Error {
                        message: result["error"]
                            .as_str()
                            .unwrap_or("Profile selection failed")
                            .into(),
                    });
                }
                return;
            }
            if result.get("candidates").is_some() {
                if let Ok(pick) = serde_json::from_value::<PickResult>(result) {
                    if pick.model_revision == r.scene.model_revision
                        && pick.axis_pin_digest == r.scene.axis_digest()
                    {
                        r.accept_pick(pending, pick.candidates);
                    }
                }
            } else if let Ok(range) = serde_json::from_value::<RangeResult>(result) {
                if range.model_revision == r.scene.model_revision
                    && range.axis_pin_digest == r.scene.axis_digest()
                {
                    let state = r.state.get_untracked();
                    let id = pending.picked.as_deref();
                    let (ids, primary) = selection::choose(
                        &state.selected_entity_ids,
                        id,
                        Some(&range.entity_ids),
                        pending.modifiers,
                    );
                    r.select(ids, primary);
                }
            }
        });
        Ok(())
    }
    pub fn command(&mut self, json: &str) -> Result<(), JsValue> {
        #[derive(serde::Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum Command {
            ZoomWindow {
                from: Screen,
                to: Screen,
            },
            Focus {
                entity_id: String,
            },
            Wheel {
                client_x: f64,
                client_y: f64,
                delta_x: f64,
                delta_y: f64,
                shift: bool,
            },
            Key {
                key: String,
                shift: bool,
            },
            Pick {
                screen: Screen,
                request_id: u64,
                context: String,
            },
        }
        let command: Command = parse(json)?;
        let mut error = None;
        transact(&self.runtime, |r| match command {
            Command::ZoomWindow { from, to } => match r.camera().window(from, to) {
                Ok(v) => r.camera_intent(v),
                Err(e) => error = Some(e),
            },
            Command::Focus { entity_id } => r.focus(&entity_id),
            Command::Wheel {
                client_x,
                client_y,
                delta_x,
                delta_y,
                shift,
            } => {
                let v = if shift {
                    r.camera()
                        .pan(-if delta_x != 0. { delta_x } else { delta_y }, 0.)
                } else {
                    r.camera().zoom(
                        r.screen(client_x, client_y),
                        if delta_y < 0. { 1.12 } else { 0.89 },
                    )
                };
                r.camera_intent(v);
            }
            Command::Key { key, shift } => {
                let init = web_sys::KeyboardEventInit::new();
                init.set_key(&key);
                init.set_shift_key(shift);
                if let Ok(e) = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init) {
                    r.key(&e);
                }
            }
            Command::Pick {
                screen,
                request_id,
                context,
            } => {
                if context != r.context() || !r.painted_current() {
                    error = Some("profile_selection_stale".into());
                    return;
                }
                let request = r.pick_request(screen, 12.);
                r.outbox.push(Intent::Pick {
                    request_id,
                    request,
                });
            }
        });
        error.map_or(Ok(()), |e| Err(failure(&e)))
    }
    pub fn read(&self) -> Result<String, JsValue> {
        let r = self.runtime.borrow();
        let camera = r.camera();
        serde_json::to_string(&serde_json::json!({"receipt":r.receipt,"paintSamples":r.paint_samples,"context":if r.painted_current(){r.context()}else{String::new()},"surface":{"width":r.width,"height":r.height,"scale":camera.projection().scale},"sceneGeneration":r.scene_generation})).map_err(|e|failure(&e.to_string()))
    }
    pub fn unmount(&mut self) {
        if self.mount.is_none() {
            return;
        }
        if let Some(observer) = self.observer.take() {
            observer.disconnect();
        }
        self.listeners.clear();
        {
            let mut r = self.runtime.borrow_mut();
            r.dead = true;
            r.cancel();
            r.invalidate();
            if let Some(id) = r.request.take() {
                let _ = web_sys::window().unwrap().cancel_animation_frame(id);
            }
            r.raf = None;
            r.markers.clear();
            r.base.set_width(0);
            r.base.set_height(0);
            r.receipt = None;
            r.paint_samples.clear();
        }
        self.mount.take();
    }
}
impl Drop for Mounted {
    fn drop(&mut self) {
        self.unmount();
    }
}
