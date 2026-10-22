use crate::panel_model::Action as PanelAction;
use crate::{model::*, panel_model::*};
use leptos::prelude::*;
use std::{rc::Rc, sync::Arc};
use wasm_bindgen::JsCast;
#[derive(Clone, Default, PartialEq)]
pub struct Facts {
    pub edit_mode: bool,
    pub selected: Vec<String>,
    pub primary: Option<String>,
}
#[derive(Clone)]
pub enum Event {
    Intent(Intent),
    Fit,
    Zoom(f64),
    ZoomWindow,
    GoTo(String),
    GoOpen,
    FocusCanvas,
    Dismiss,
    Clear,
}
pub type Send = send_wrapper::SendWrapper<Rc<dyn Fn(Event)>>;
fn button(label: String, action: PanelAction, disabled: bool, send: Send) -> AnyView {
    view!{<button type="button" role="menuitem" disabled=disabled on:click=move|_|send(Event::Intent(Intent::PanelAction{action:action.clone()}))>{label}</button>}.into_any()
}
fn action_button(label: &str, action: PanelAction, disabled: bool, send: Send) -> AnyView {
    button(label.into(), action, disabled, send)
}
fn menu(title: &str, children: Vec<AnyView>) -> AnyView {
    view!{<details class="ds-profile-menu"><summary>{title.to_string()}" ▾"</summary><div role="menu" class="ds-profile-dropdown">{children}</div></details>}.into_any()
}
pub fn toolbar(
    model: ArcRwSignal<Option<Arc<PanelModel>>>,
    scene: ArcRwSignal<Arc<Scene>>,
    facts: ArcRwSignal<Facts>,
    scale: ArcRwSignal<f64>,
    send: Send,
) -> impl IntoView {
    let resize_model = model.clone();
    let s1 = send.clone();
    let error_model = model.clone();
    let loading_model = model.clone();
    let readiness = crate::readiness::view(model.clone(), send.clone());
    let display = crate::display::view(model.clone(), scene.clone(), send.clone());
    let title = model.clone();
    let header_ref = NodeRef::<leptos::html::Header>::new();
    view! {
        <style>{format!("{}{}",CSS,BADGE_CSS)}</style>
        <div data-profile-resize role="separator" aria-label="Resize Profile" style=move||if resize_model.get().as_ref().is_some_and(|p|p.enabled&&!p.popout_mode){"height:6px;background:#0f172a;cursor:ns-resize;touch-action:none;flex-shrink:0"}else{"display:none"}></div>
        <div style="display:flex;align-items:center;flex-shrink:0;background:#0f172a">{readiness}<header node_ref=header_ref tabindex="-1" data-profile-header role="toolbar" aria-label=move||title.get().map(|panel|panel.title.clone()).unwrap_or_default() style="flex:1;min-width:0" on:contextmenu=move|e:web_sys::MouseEvent|{e.prevent_default();open_header_actions(header_ref);} on:keydown=move|e:web_sys::KeyboardEvent|{if e.key()=="ContextMenu"||e.key()=="F10"&&e.shift_key(){e.prevent_default();open_header_actions(header_ref);}}>{display}
        {move||{let Some(p)=model.get() else{return ().into_any();};if !p.enabled{return ().into_any();}let s=scene.get();let f=facts.get();let generic=p.generic;let scale=scale.clone();
            let mut model_items=vec![];
            for (label,action) in [("Model tables",PanelAction::Tables),("Staking register",PanelAction::Staking),("Data factory",PanelAction::Data),("Structure library",PanelAction::Library),("Feature codes",PanelAction::FeatureCodes)] {model_items.push(action_button(label,action,false,s1.clone()));}
            if p.dev {model_items.push(action_button("Spotting progress",PanelAction::SpottingProgress,false,s1.clone()));}

            let mut analysis=vec![action_button("Results",PanelAction::Results,false,s1.clone()),action_button("Validate model",PanelAction::Validate,false,s1.clone()),action_button("Span check",PanelAction::SpanCheck,false,s1.clone()),action_button("Calculate all",PanelAction::CalculateAll,p.read_only,s1.clone()),action_button("Automatic spotting and stringing",PanelAction::AutoStringing,p.read_only,s1.clone()),action_button("Process MV model",PanelAction::Process,p.read_only,s1.clone())];
            let analyze=s1.clone();analysis.insert(0,view!{<button type="button" role="menuitem" disabled=!p.scene_present on:click=move|_|analyze(Event::Intent(Intent::Display{patch:DisplayPatch::default(),analyze:true,intent:None}))>"Analyze"</button>}.into_any());
            let legend=crate::legend::view(&s);
            let edit=s1.clone();let issues=s1.clone();let fit=s1.clone();let zoomout=s1.clone();let zoomin=s1.clone();let boxzoom=s1.clone();let go=s1.clone();let filter=s1.clone();let context=context(&s,&f,&p,s1.clone());
            let header_filter=s1.clone();let close=s1.clone();let popup=s1.clone();let rebuild=s1.clone();let undo=s1.clone();let redo=s1.clone();let create=create_controls(&p,s1.clone());
            view!{                {(!generic).then(||menu("Model",model_items))}{(!generic).then(||menu("Analysis",analysis))}
                {(!generic).then(||view!{<button type="button" on:click=move|_|issues(Event::Intent(Intent::PanelAction{action:PanelAction::Issues}))>"Issues"</button>})}
                {(!generic&&!p.popout_mode).then(||view!{<button type="button" aria-label=if f.edit_mode{"Done editing model"}else{"Edit model"} aria-pressed=f.edit_mode.to_string() disabled=!f.edit_mode&&!p.can_edit_model on:click=move|_|edit(Event::Intent(Intent::PanelAction{action:PanelAction::EditToggle}))>"✎"</button>})}
                {f.edit_mode.then(||view!{<button type="button" aria-label="Undo DS Grid edit" disabled=!p.undo_available on:click=move|_|undo(Event::Intent(Intent::PanelAction{action:PanelAction::Undo}))>"↶"</button><button type="button" aria-label="Redo DS Grid edit" disabled=!p.redo_available on:click=move|_|redo(Event::Intent(Intent::PanelAction{action:PanelAction::Redo}))>"↷"</button>})}
                {context}{legend}<span class="ds-profile-spacer"></span>{create}
                <button type="button" aria-label="Go to a structure" title="Go to a structure (G)" on:click=move|_|go(Event::GoOpen)>"⌕"</button>
                <button type="button" aria-label="Selection filter" disabled=!p.can_filter on:click=move|_|filter(Event::Intent(Intent::PanelAction{action:PanelAction::Filter}))>"☷"</button>
                {(!generic&&!p.popout_mode).then(||view!{<button type="button" aria-label="Rebuild Profile" disabled=!p.preparation.can_rebuild on:click=move|_|rebuild(Event::Intent(Intent::PanelAction{action:PanelAction::Rebuild}))>"↻"</button>})}
                {p.show_fit.then(||view!{<button type="button" on:click=move|_|fit(Event::Fit)>"Fit"</button>})}
                {p.show_zoom.then(||view!{<button type="button" aria-label="Zoom window" on:click=move|_|boxzoom(Event::ZoomWindow)>"▣"</button><button type="button" aria-label="Zoom out" on:click=move|_|zoomout(Event::Zoom(1./1.2))>"−"</button><span class="ds-profile-scale">{move||format!("{:.1}%",scale.get()*100.)}</span><button type="button" aria-label="Zoom in" on:click=move|_|zoomin(Event::Zoom(1.2))>"+"</button>})}
                <details class="ds-profile-menu" data-profile-header-actions><summary aria-label="Profile view actions">"⋮"</summary><div class="ds-profile-dropdown ds-profile-right" role="menu"><button type="button" role="menuitem" on:click=move|_|close(Event::Intent(Intent::PanelAction{action:PanelAction::Close}))>"Close"</button>{p.can_popout.then(||view!{<button type="button" role="menuitem" on:click=move|_|popup(Event::Intent(Intent::PanelAction{action:PanelAction::Popout}))>"Popup"</button>})}<button type="button" role="menuitem" disabled=!p.can_filter on:click=move|_|header_filter(Event::Intent(Intent::PanelAction{action:PanelAction::Filter}))>"Selection filter…"</button></div></details>
            }.into_any()
        }}
        </header></div>
        {move||error_model.get().and_then(|p|p.error.as_ref().filter(|s|!s.is_empty()).map(|e|view!{<p class="ds-profile-error" role="alert">{e.clone()}</p>}))}
        {move||loading_model.get().filter(|p|p.loading).map(|_|view!{<span role="status" class="ds-profile-loading">"Updating Profile…"</span>})}

    }
}
fn open_header_actions(header: NodeRef<leptos::html::Header>) {
    let Some(header) = header.get_untracked() else {
        return;
    };
    let Ok(Some(menu)) = header.query_selector("[data-profile-header-actions]") else {
        return;
    };
    let _ = menu.set_attribute("open", "");
    let _ = menu.set_attribute("data-profile-menu-focus", "header");
    if let Some(first) = menu
        .query_selector("button:not([disabled])")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = first.focus();
    }
}

fn context(scene: &Scene, f: &Facts, p: &PanelModel, send: Send) -> AnyView {
    if f.selected.is_empty() {
        return ().into_any();
    }
    let mut buttons = vec![];
    if f.primary.is_some() {
        let info = send.clone();
        buttons.push(view! {<button type="button" aria-label="Properties" title="Inspect selection" on:click=move |_| info(Event::Intent(Intent::PanelAction { action: PanelAction::Properties }))>"i"</button>}.into_any());
    }
    let structures = f
        .selected
        .iter()
        .all(|id| scene.selection_order.structures.contains(id));
    let sections = f
        .selected
        .iter()
        .all(|id| scene.selection_order.tension_sections.contains(id));
    let alignments = f
        .selected
        .iter()
        .all(|id| scene.bands.iter().any(|b| &b.alignment_id == id));
    let attachments = f.selected.iter().all(|id| {
        scene
            .selection_order
            .attachments
            .iter()
            .any(|a| &a.key() == id)
    });
    if f.edit_mode && structures && !p.popout_mode {
        let label = (f.selected.len() == 1)
            .then_some(f)
            .and_then(|f| {
                f.primary
                    .as_ref()
                    .and_then(|id| {
                        scene
                            .bands
                            .iter()
                            .flat_map(|b| &b.structures)
                            .find(|s| &s.entity_id == id)
                    })
                    .map(|s| s.engineering_name.as_str())
            })
            .unwrap_or("Type");
        buttons.push(action_button(
            &format!("{label} ▾"),
            PanelAction::Retype,
            false,
            send.clone(),
        ));
        let delete = send.clone();
        buttons.push(view! {<button type="button" aria-label="Delete" title="Delete selected structures and their connected stringing" on:click=move |_| delete(Event::Intent(Intent::PanelAction { action: PanelAction::Delete }))><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7"/></svg></button>}.into_any());
    }
    if f.edit_mode && alignments {
        buttons.push(action_button(
            "Spot + string",
            PanelAction::SelectedAutoStringing,
            false,
            send.clone(),
        ));
    }
    if sections {
        buttons.push(action_button(
            "Section criteria",
            PanelAction::SectionCriteria,
            false,
            send.clone(),
        ));
    }
    if attachments {
        buttons.push(action_button(
            "Select pole",
            PanelAction::ParentStructure,
            false,
            send,
        ));
    }
    view! {<div data-profile-context-bar data-edit-mode=f.edit_mode>{buttons}</div>}.into_any()
}
fn create_controls(panel: &PanelModel, send: Send) -> AnyView {
    if !panel.edit_context_active || panel.popout_mode {
        return ().into_any();
    }
    let mut views = vec![];
    for (mode, label, path) in [
        (
            "structure",
            "Add structure",
            "M7 21V5m-3 4 3-4 3 4M3 21h8M17 3v8m-4-4h8",
        ),
        (
            "tension_section",
            "Add tension section",
            "M3 13v8m18-8v8M3 15c4-8 14-8 18 0M12 2v7M8.5 5.5h7",
        ),
        (
            "ground_point",
            "Add interpolated ground point",
            "M2 19l6-5 6 3 8-6M16 3v6m-3-3h6M6.5 14h3",
        ),
    ] {
        let send = send.clone();
        let mode = mode.to_string();
        views.push(view!{<button type="button" aria-label=label title=label disabled=!panel.can_create||panel.create.busy on:click=move|_|send(Event::Intent(Intent::CreateArm{mode:mode.clone()}))><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d=path/></svg></button>}.into_any());
    }
    if let (Some(mode), Some(options)) = (&panel.create.mode, &panel.create.options) {
        let choices = match mode.as_str() {
            "structure" => &options.structure_types,
            "ground_point" => &options.ground_features,
            _ => &options.section_templates,
        };
        let send_choice = send.clone();
        let options=choices.iter().map(|c|view!{<option value=c.id.clone() selected=c.id==panel.create.choice>{c.label.clone()}</option>}).collect_view();
        views.push(view!{<select aria-label="Geometry creation choice" disabled=panel.create.busy on:change=move|e|send_choice(Event::Intent(Intent::CreateChoice{choice:event_target_value(&e)}))>{options}</select>}.into_any());
        if mode == "structure" {
            views.push(action_button(
                "Preview type…",
                PanelAction::PreviewType,
                panel.create.busy,
                send.clone(),
            ));
        }
        if panel.create.pending {
            views.push(action_button(
                "Place structure",
                PanelAction::CreatePlace,
                panel.create.busy,
                send.clone(),
            ));
        }
        views.push(action_button(
            "Cancel",
            PanelAction::CreateCancel,
            false,
            send,
        ));
    }
    view! {<div class="ds-profile-create">{views}</div>}.into_any()
}
pub use crate::weather::view as weather;
#[derive(Clone, Copy)]
struct Drafts {
    sag: RwSignal<Option<usize>>,
}
pub fn dialogs(
    model: ArcRwSignal<Option<Arc<PanelModel>>>,
    scene: ArcRwSignal<Arc<Scene>>,
    facts: ArcRwSignal<Facts>,
    send: Send,
) -> impl IntoView {
    let drafts = Drafts {
        sag: RwSignal::new(None),
    };
    let previous = RwSignal::new(String::new());
    let observed = model.clone();
    Effect::new(move |_| {
        let p = observed.get();
        let key = p
            .as_ref()
            .and_then(|p| p.dialogs.sag.as_ref())
            .map(|d| format!("{}:{}:{:?}", d.model_id, d.expected_revision, d.section_ids))
            .unwrap_or_default();
        if previous.get_untracked() != key {
            previous.set(key);
            drafts.sag.set(None);
        }
    });
    let retype = crate::retype::view(model.clone(), scene.clone(), send.clone());
    let filter = crate::filter::view(model.clone(), facts, send.clone());
    view! {<div class="ds-profile-dialogs">{retype}{filter}{move||model.get().map(|panel|dialog_views(&panel,drafts,send.clone()))}</div>}
}
fn modal(title: &str, body: Vec<AnyView>, kind: DialogKind, send: Send) -> AnyView {
    view!{<div role="dialog" aria-modal="true" aria-label=title.to_string() class="ds-profile-dialog"><header><strong>{title.to_string()}</strong><button type="button" aria-label="Close dialog" on:click=move|_|send(Event::Intent(Intent::DialogClose{dialog:kind.clone()}))>"×"</button></header>{body}</div>}.into_any()
}
fn dialog_views(panel: &PanelModel, d: Drafts, send: Send) -> Vec<AnyView> {
    let mut dialogs = vec![];
    if let Some(delete) = &panel.dialogs.delete {
        let revision = delete.revision.clone();
        let ids = delete.structure_ids.clone();
        let dispatch = send.clone();
        dialogs.push(modal("Delete selected structures",vec![view!{<p>{format!("Delete {} structures and their connected stringing?",ids.len())}</p><p role="alert">{delete.error.clone()}</p><button type="button" disabled=delete.busy on:click=move|_|dispatch(Event::Intent(Intent::DeleteApply{expected_revision:revision.clone(),commands:vec![HeadCommand{command_id:command_id(),command:ModelCommand::DeleteProfileEntities{ids:ids.clone()}}]}))>"Delete"</button>}.into_any()],DialogKind::Delete,send.clone()));
    }
    if let Some(sag) = &panel.dialogs.sag {
        let choices = sag.choices.clone().unwrap_or_default();
        let summary = format!(
            "{} selected sections · {} physical sections · {} cable types",
            choices.selected_section_ids.len(),
            choices.physical_section_ids.len(),
            choices.cable_ids.len()
        );
        let options = choices.options.clone();
        let choices_ui=options.iter().enumerate().map(|(i,o)|view!{<label style="display:block;padding:6px"><input type="radio" name="profile-sag" checked=move||d.sag.get()==Some(i) disabled=o.availability!="available" on:change=move|_|d.sag.set(Some(i)) />{format!("{} · {} · {}",o.criterion_set_label,o.condition.as_deref().unwrap_or("unspecified").replace('_'," "),o.weather_state_label)}{o.failures.iter().map(|f|view!{<p>{f.message.clone()}</p>}).collect_view()}</label>}).collect_view();
        let ids = choices.selected_section_ids;
        let revision = sag.expected_revision.clone();
        let dispatch = send.clone();
        let busy = sag.busy || sag.loading;
        dialogs.push(modal("Selected section criteria",vec![view!{<p>{summary}</p><div role="radiogroup" aria-label="Sag criterion choices">{choices_ui}</div><p role="alert">{sag.error.clone()}</p><button type="button" disabled=move||busy||d.sag.get().is_none() on:click=move|_|{if let Some(o)=d.sag.get_untracked().and_then(|i|options.get(i)){if o.availability!="available"{return;}dispatch(Event::Intent(Intent::SagApply{expected_revision:revision.clone(),commands:vec![HeadCommand{command_id:command_id(),command:ModelCommand::SetTensionSectionsSagMode{ids:ids.clone(),sag:SagMode::FromCriteria,sag_condition:o.condition.clone(),sag_weather_state_id:o.weather_state_id.clone(),criterion_set_id:o.criterion_set_id.clone(),ruling_span_m:None}}]}));}} >"Apply to selected sections"</button>}.into_any()],DialogKind::Sag,send.clone()));
    }
    dialogs
}
fn command_id() -> String {
    web_sys::window().unwrap().crypto().unwrap().random_uuid()
}
const CSS: &str = r#"
[data-profile-root]{height:100%;min-height:0;display:flex;flex-direction:column;position:relative;color:#cbd5e1;background:#020617;font:11px system-ui,sans-serif}
[data-profile-root] header{display:flex;align-items:center;gap:3px;min-height:30px;flex-shrink:0;background:#0f172a;border-bottom:1px solid #334155;padding:2px 8px}
[data-profile-root] button,[data-profile-root] input,[data-profile-root] select,[data-profile-root] textarea{font:inherit;color:inherit;box-sizing:border-box}[data-profile-root] button{appearance:none;background:transparent;border:1px solid transparent}[data-profile-root] button:focus-visible,[data-profile-root] summary:focus-visible,[data-profile-root] input:focus-visible,[data-profile-root] select:focus-visible{outline:2px solid #67e8f9;outline-offset:1px}[data-profile-root] button,[data-profile-root] summary{border-radius:3px;padding:4px 6px;cursor:pointer;text-align:left;white-space:nowrap}
[data-profile-root] button:hover,[data-profile-root] summary:hover{background:#1e293b}[data-profile-root] button:disabled{opacity:.4;cursor:default}
.ds-profile-filter-row{display:flex;align-items:center;gap:4px;flex-wrap:wrap;margin:6px 0}.ds-profile-filter-row input{min-width:50px;max-width:130px}.ds-profile-spacer{flex:1}.ds-profile-scale{font:10px monospace;min-width:40px;text-align:center}.ds-profile-menu{position:relative;flex-shrink:0}.ds-profile-menu summary{list-style:none}
.ds-profile-dropdown{position:absolute;left:0;top:100%;z-index:80;background:#020617;border:1px solid #475569;box-shadow:0 8px 24px #0008;min-width:220px;max-height:65vh;overflow:auto;padding:6px}.ds-profile-dropdown button{display:block;width:100%}.ds-profile-dropdown label{display:flex;align-items:center;gap:8px;padding:4px 2px}.ds-profile-dropdown input[type=number]{width:80px;margin-left:auto}.ds-profile-dropdown input,.ds-profile-dialog input,.ds-profile-dialog select,.ds-profile-create select,.ds-profile-dropdown textarea,.ds-profile-dialog textarea{color:#cbd5e1;background:#0f172a;border:1px solid #475569;border-radius:3px;padding:3px}.ds-profile-dropdown textarea,.ds-profile-dialog textarea{width:100%;min-height:100px;font:11px monospace}.ds-profile-dropdown pre,.ds-profile-dialog pre{white-space:pre-wrap;max-height:200px;overflow:auto;font:10px monospace}.ds-profile-right{left:auto;right:0}.ds-profile-above{top:auto;bottom:100%;left:auto;right:0}.ds-profile-weather{position:absolute;right:8px;bottom:8px;z-index:30;background:#020617cc}.ds-profile-create{display:flex;gap:3px;align-items:center}.ds-profile-error{padding:4px 8px;color:#fda4af}.ds-profile-loading{position:absolute;right:8px;top:40px;z-index:30;pointer-events:none}.ds-profile-dialog{position:absolute;right:10px;top:40px;z-index:100;max-width:420px;max-height:75%;overflow:auto;padding:12px;border:1px solid #475569;border-radius:4px;box-shadow:0 8px 30px #000b;background:#020617}.ds-profile-dialog header{justify-content:space-between;margin:-12px -12px 8px}.ds-profile-dialog p{margin:8px 0}[data-profile-context-bar]{display:flex;gap:3px}
"#;
pub fn go_to(
    open: ArcRwSignal<bool>,
    scene: ArcRwSignal<Arc<Scene>>,
    panel: ArcRwSignal<Option<Arc<PanelModel>>>,
    send: Send,
) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let active = RwSignal::new(0usize);
    let entries = Memo::new(move |_| {
        let numbers = panel
            .get()
            .map(|p| p.structure_numbers.clone())
            .unwrap_or_default();
        crate::navigation::search(&scene.get(), &numbers, &query.get())
    });
    move || {
        if !open.get() {
            return ().into_any();
        }
        let close = open.clone();
        let close_send = send.clone();
        let keyboard = send.clone();
        let choose = send.clone();
        view!{<div role="dialog" aria-label="Go to a structure" class="ds-profile-dialog">
            <header><strong>"Go to a structure"</strong><button type="button" on:click=move|_|{close.set(false);close_send(Event::FocusCanvas);}>"×"</button></header>
            <input type="search" aria-label="Find structure" autofocus on:input=move|e|{query.set(event_target_value(&e));active.set(0);} on:keydown=move|e: web_sys::KeyboardEvent|{let rows=entries.get_untracked();match e.key().as_str(){"ArrowDown"=>{e.prevent_default();active.set((active.get_untracked()+1).min(rows.len().saturating_sub(1)));},"ArrowUp"=>{e.prevent_default();active.set(active.get_untracked().saturating_sub(1));},"Enter"=>{e.prevent_default();if let Some(entry)=rows.get(active.get_untracked()){keyboard(Event::GoTo(entry.id.clone()));}},"Escape"=>{e.prevent_default();keyboard(Event::FocusCanvas);},_=>{}}}/>
            <div role="listbox" aria-label="Matching structures">{move||entries.get().into_iter().take(80).enumerate().map(|(i,entry)|{let id=entry.id.clone();let send=choose.clone();view!{<button type="button" role="option" aria-selected=move||(active.get()==i).to_string() style="display:block;width:100%" on:click=move|_|send(Event::GoTo(id.clone()))>{format!("{} · {}",entry.number.as_deref().unwrap_or("—"),entry.name)}</button>}}).collect_view()}</div>
        </div>}.into_any()
    }
}
pub fn context_menu(
    open: ArcRwSignal<Option<Screen>>,
    scene: ArcRwSignal<Arc<Scene>>,
    facts: ArcRwSignal<Facts>,
    state: ArcRwSignal<ViewState>,
    send: Send,
) -> impl IntoView {
    move || {
        let Some(point) = open.get() else {
            return ().into_any();
        };
        let scene = scene.get();
        let f = facts.get();
        let state = state.get();
        let p = state.panel.clone().unwrap_or_default();
        let copy = send.clone();
        let clear = send.clone();
        let fit = send.clone();
        let close = send.clone();
        let ids = f.selected.clone();
        let references = format!(
            "DS Grid selection\nmodel_id: {}\nrevision: {}\n{}",
            state.model_id.as_deref().unwrap_or(""),
            scene.model_revision,
            ids.iter()
                .map(|id| format!("entity_id: {id}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let selected = context(&scene, &f, &p, send.clone());
        view!{<div role="menu" aria-label="Profile selection actions" class="ds-profile-dropdown" style=format!("position:fixed;left:{}px;top:{}px",point.x,point.y)>
            {selected}<button type="button" role="menuitem" on:click=move|_|copy(Event::Intent(Intent::Clipboard{text:references.clone()}))>"Copy DS Grid references"</button>
            <button type="button" role="menuitem" on:click=move|_|clear(Event::Clear)>"Clear selection"</button>{action_button("Edit model",PanelAction::EditToggle,!p.can_edit_model&&!f.edit_mode,send.clone())}{action_button("Issues for selection",PanelAction::SelectedIssues,ids.is_empty(),send.clone())}
            <button type="button" role="menuitem" on:click=move|_|fit(Event::Fit)>"Fit"</button><button type="button" role="menuitem" on:click=move|_|close(Event::Dismiss)>"Close"</button>
        </div>}.into_any()
    }
}
pub fn badges(
    model: ArcRwSignal<Option<Arc<PanelModel>>>,
    scene: ArcRwSignal<Arc<Scene>>,
    state: ArcRwSignal<ViewState>,
    send: Send,
) -> impl IntoView {
    move || {
        let Some(p) = model.get() else {
            return ().into_any();
        };
        let scene = scene.get();
        let state = state.get();
        let results = send.clone();
        let count = p.unavailable_curve_section_count;
        view!{
            {(p.desktop&&!p.generic&&p.scene_present&&scene.display.is_none()).then(||view!{<div role="status" style="position:absolute;inset:0;display:grid;place-items:center;background:#020617;z-index:20">"Native Profile display is unavailable"</div>})}
            <span style="position:absolute;left:8px;top:8px;pointer-events:none;font:10px system-ui;color:#64748b">{p.model_name.clone()}</span>
            {(p.generic&&!scene.conductor_spans.is_empty()&&!state.visibility.show_wire).then(||view!{<span role="status" class="ds-profile-notice">"Conductors hidden"</span>})}
            {(p.generic&&scene.conductor_spans.is_empty()&&p.generic_ui_model.as_ref().is_some_and(|u|u.domain=="lv_distribution")).then(||view!{<span role="status" class="ds-profile-notice">"No stringing"</span>})}
            {p.calculation_required.then(||view!{<span role="status" class="ds-profile-notice">{if p.reconciling{"Reconciling calculation…"}else{"Calculation due"}}</span>})}
            {p.moving_structure.then(||view!{<span role="status" class="ds-profile-notice">"Checking move along alignment…"</span>})}
            {p.provisional.then(||view!{<span role="status" class="ds-profile-notice">"Provisional move preview · not saved"</span>})}
            {(count>0).then(||view!{<button type="button" class="ds-profile-curve-warning" data-profile-curve-warning title=format!("{count} sections have unavailable curves · Analysis results") aria-label=format!("{count} sections have unavailable curves · Analysis results") on:click=move|_|results(Event::Intent(Intent::PanelAction{action:PanelAction::Results}))><span aria-hidden="true">"⚠"</span>{count}</button>})}
        }.into_any()
    }
}
const BADGE_CSS: &str = r#".ds-profile-notice{position:absolute;left:8px;top:26px;pointer-events:none;padding:4px 6px;color:#fde68a;background:#020617dd;border:1px solid #92400e;border-radius:3px;z-index:25}.ds-profile-curve-warning{position:absolute;right:8px;top:8px;z-index:25;color:#fde68a;background:#020617ee;border:1px solid #92400e}.ds-profile-weather-error{position:absolute;right:8px;bottom:40px;padding:10px;max-width:300px;background:#020617;border:1px solid #92400e;z-index:80}"#;
