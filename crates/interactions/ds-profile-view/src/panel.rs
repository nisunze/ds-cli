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
    view! {
        <style>{format!("{}{}",CSS,BADGE_CSS)}</style>
        <div data-profile-resize role="separator" aria-label="Resize Profile" style=move||if resize_model.get().as_ref().is_some_and(|p|p.enabled&&!p.popout_mode){"height:6px;background:#0f172a;cursor:ns-resize;touch-action:none;flex-shrink:0"}else{"display:none"}></div>
        <div style="display:flex;align-items:center;flex-shrink:0;background:#0f172a">{readiness}
        {move||{let Some(p)=model.get() else{return ().into_any();};if !p.enabled{return ().into_any();}let s=scene.get();let f=facts.get();let generic=p.generic;let scale=scale.clone();
            let mut model_items=vec![];
            for (label,action) in [("Model tables",PanelAction::Tables),("Staking register",PanelAction::Staking),("Data factory",PanelAction::Data),("Structure library",PanelAction::Library),("Feature codes",PanelAction::FeatureCodes)] {model_items.push(action_button(label,action,false,s1.clone()));}
            if p.dev {model_items.push(action_button("Spotting progress",PanelAction::SpottingProgress,false,s1.clone()));}

            let mut analysis=vec![action_button("Results",PanelAction::Results,false,s1.clone()),action_button("Validate model",PanelAction::Validate,false,s1.clone()),action_button("Span check",PanelAction::SpanCheck,false,s1.clone()),action_button("Calculate all",PanelAction::CalculateAll,p.read_only,s1.clone()),action_button("Automatic spotting and stringing",PanelAction::AutoStringing,p.read_only,s1.clone()),action_button("Process MV model",PanelAction::Process,p.read_only,s1.clone())];
            let analyze=s1.clone();analysis.insert(0,view!{<button type="button" role="menuitem" disabled=!p.scene_present on:click=move|_|analyze(Event::Intent(Intent::Display{patch:DisplayPatch::default(),analyze:true,intent:None}))>"Analyze"</button>}.into_any());
            let display=display_menu(&p,&s,s1.clone());let legend=legend(&s);
            let edit=s1.clone();let issues=s1.clone();let fit=s1.clone();let zoomout=s1.clone();let zoomin=s1.clone();let boxzoom=s1.clone();let go=s1.clone();let filter=s1.clone();let context=context(&s,&f,&p,s1.clone());
            let header_ref=NodeRef::<leptos::html::Header>::new();let header_filter=s1.clone();let close=s1.clone();let popup=s1.clone();let rebuild=s1.clone();let undo=s1.clone();let redo=s1.clone();let create=create_controls(&p,s1.clone());
            view!{<header node_ref=header_ref tabindex="-1" data-profile-header role="toolbar" aria-label=p.title.clone() style="flex:1;min-width:0" on:contextmenu=move|e:web_sys::MouseEvent|{e.prevent_default();open_header_actions(header_ref);} on:keydown=move|e:web_sys::KeyboardEvent|{if e.key()=="ContextMenu"||e.key()=="F10"&&e.shift_key(){e.prevent_default();open_header_actions(header_ref);}}>
                {display}{(!generic).then(||menu("Model",model_items))}{(!generic).then(||menu("Analysis",analysis))}
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
            </header>}.into_any()
        }}
        </div>
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
fn display_menu(panel: &PanelModel, scene: &Scene, send: Send) -> AnyView {
    let mut children = vec![];
    if let Some(display) = &scene.display {
        for group in &display.groups {
            let mut rows = vec![];
            for row in &group.rows {
                let option = row.option.clone();
                let dispatch = send.clone();
                let label = panel.label(&row.label_key, &row.label_key);
                let line = format!(
                    "display:inline-block;width:24px;border-top:{}px solid {}",
                    row.encoding.width_px, row.encoding.color
                );
                rows.push(view!{<label><input type="checkbox" checked=row.checked on:change=move|e|{let checked=event_target_checked(&e);dispatch(Event::Intent(Intent::Display{patch:DisplayPatch{visibility:Some(BTreeMap::from([(option.clone(),checked)])),..DisplayPatch::default()},analyze:false,intent:None}));}/><span style=line></span>{label}</label>}.into_any());
            }
            for number in &group.numbers {
                let control = number.clone();
                let dispatch = send.clone();
                let label = panel.label(&number.label_key, &number.label_key);
                rows.push(view!{<label>{label}<input type="number" step="any" min=number.minimum max=number.maximum value=number.value on:change=move|e|{let value=event_target_value(&e).parse::<f64>().unwrap_or(f64::NAN);match control.patch(value) {Ok(patch)=>dispatch(Event::Intent(Intent::Display{patch,analyze:false,intent:None})),Err(message)=>dispatch(Event::Intent(Intent::Error{message}))}}/></label>}.into_any());
            }
            for cable in &group.cables {
                let name = cable.cable_name.clone();
                let reset_name = name.clone();
                let dispatch = send.clone();
                let reset = send.clone();
                rows.push(view!{<label>{cable.cable_name.clone()}<input type="color" value=cable.encoding.color.clone() on:change=move|e|dispatch(Event::Intent(Intent::Display{patch:DisplayPatch{cable_colors:Some(BTreeMap::from([(name.clone(),Some(event_target_value(&e)))])),..DisplayPatch::default()},analyze:false,intent:None})) />{cable.can_reset.then(||view!{<button type="button" on:click=move|_|reset(Event::Intent(Intent::Display{patch:DisplayPatch{cable_colors:Some(BTreeMap::from([(reset_name.clone(),None)])),..DisplayPatch::default()},analyze:false,intent:None}))>"Reset"</button>})}</label>}.into_any());
            }
            children.push(view!{<details open=group.initially_open data-profile-display-group=group.id.clone()><summary>{panel.label(&group.label_key,&group.id)}</summary><div>{rows}</div></details>}.into_any());
        }
        if let Some(styles) = &display.style_controls {
            let draft =
                RwSignal::new(serde_json::to_string_pretty(&styles.request).unwrap_or_default());
            let dispatch = send.clone();
            let schema = serde_json::to_string_pretty(&styles.schema).unwrap_or_default();
            children.push(view!{<details data-profile-display-styles><summary>"Styles"</summary><textarea aria-label="Profile style request" prop:value=move||draft.get() on:input=move|e|draft.set(event_target_value(&e))></textarea><button type="button" on:click=move|_|{match serde_json::from_str::<DisplayPatch>(&draft.get_untracked()){Ok(patch)=>dispatch(Event::Intent(Intent::Display{patch,analyze:false,intent:None})),Err(e)=>dispatch(Event::Intent(Intent::Error{message:e.to_string()}))}}>"Apply"</button><details><summary>"Schema"</summary><pre>{schema}</pre></details></details>}.into_any());
        }
    } else {
        children.push(view!{<p role="status">{panel.label("gprof_display_native_unavailable","Native display controls are unavailable")}</p>}.into_any());
        if panel.generic
            && panel
                .generic_ui_model
                .as_ref()
                .is_some_and(|u| u.capabilities.add_alignments)
            && !panel.popout_mode
        {
            children.push(action_button(
                "Add alignment…",
                PanelAction::AddAlignment,
                false,
                send,
            ));
        }
    }
    menu("Display", children)
}
use std::collections::BTreeMap;
fn create_controls(panel: &PanelModel, send: Send) -> AnyView {
    if !panel.edit_context_active || panel.popout_mode {
        return ().into_any();
    }
    let mut views = vec![];
    for (mode, label) in [
        ("structure", "Add structure"),
        ("tension_section", "Add tension section"),
        ("ground_point", "Add interpolated ground point"),
    ] {
        let send = send.clone();
        let mode = mode.to_string();
        views.push(view!{<button type="button" aria-label=label disabled=!panel.can_create||panel.create.busy on:click=move|_|send(Event::Intent(Intent::CreateArm{mode:mode.clone()}))>{label}</button>}.into_any());
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
pub fn weather(
    model: ArcRwSignal<Option<Arc<PanelModel>>>,
    scene: ArcRwSignal<Arc<Scene>>,
    send: Send,
) -> impl IntoView {
    view! {<div class="ds-profile-weather" data-profile-weather-position="lower-right">{move||{let Some(panel)=model.get() else{return ().into_any();};let scene=scene.get();let Some(engineering)=&scene.engineering else{return ().into_any();};let selected=engineering.cases.get(engineering.selected_case_index).map(|c|c.label.clone()).unwrap_or_else(||"Weather cases loading…".into());let options=engineering.cases.iter().map(|c|{let value=c.value.clone();let calculate_value=value.clone();let choose=send.clone();let calculate=send.clone();view!{<div><button type="button" role="option" disabled=c.disabled||!panel.preparation.interactive on:click=move|_|choose(Event::Intent(Intent::Display{patch:DisplayPatch{display_case:Some(value.clone()),..DisplayPatch::default()},analyze:false,intent:None}))>{c.label.clone()}</button><button type="button" disabled=!panel.preparation.can_rebuild on:click=move|_|calculate(Event::Intent(Intent::Display{patch:DisplayPatch{display_case:Some(calculate_value.clone()),..DisplayPatch::default()},analyze:false,intent:Some("rebuild".into())}))>"Calculate and choose"</button></div>}}).collect_view();view!{<details class="ds-profile-menu" data-profile-engineering><summary aria-label="Weather case">{selected}" ▾"</summary><div role="listbox" aria-label="Weather cases" class="ds-profile-dropdown ds-profile-above">{options}</div></details>}.into_any()}}</div>}
}
#[derive(Clone, Copy)]
struct Drafts {
    retype: RwSignal<String>,
    type_query: RwSignal<String>,
    sag: RwSignal<Option<usize>>,
    table: RwSignal<String>,
    filters: RwSignal<Vec<Filter>>,
    mode: RwSignal<String>,
    dirty: RwSignal<bool>,
}
pub fn dialogs(
    model: ArcRwSignal<Option<Arc<PanelModel>>>,
    scene: ArcRwSignal<Arc<Scene>>,
    facts: ArcRwSignal<Facts>,
    send: Send,
) -> impl IntoView {
    let drafts = Drafts {
        retype: RwSignal::new(String::new()),
        type_query: RwSignal::new(String::new()),
        sag: RwSignal::new(None),
        table: RwSignal::new("structures".into()),
        filters: RwSignal::new(vec![]),
        mode: RwSignal::new("replace".into()),
        dirty: RwSignal::new(true),
    };
    let previous = RwSignal::new(String::new());
    let observed = model.clone();
    Effect::new(move |_| {
        let p = observed.get();
        let key = p
            .as_ref()
            .map(|p| {
                format!(
                    "{:?}:{:?}:{:?}",
                    p.dialogs.retype.as_ref().map(|d| (
                        &d.model_id,
                        &d.expected_revision,
                        &d.structure_ids
                    )),
                    p.dialogs.sag.as_ref().map(|d| (
                        &d.model_id,
                        &d.expected_revision,
                        &d.section_ids
                    )),
                    p.dialogs
                        .selection
                        .as_ref()
                        .map(|d| (&d.model_id, &d.expected_revision))
                )
            })
            .unwrap_or_default();
        if previous.get_untracked() != key {
            previous.set(key);
            drafts.retype.set(String::new());
            drafts.type_query.set(String::new());
            drafts.sag.set(None);
            drafts.filters.set(vec![]);
            drafts.dirty.set(true);
        }
    });
    let auto_model = model.clone();
    let apply = send.clone();
    let applied_key = RwSignal::new(String::new());
    Effect::new(move |_| {
        let Some(model) = auto_model.get() else {
            return;
        };
        let Some(dialog) = &model.dialogs.retype else {
            return;
        };
        let Some(preview) = &dialog.preview else {
            return;
        };
        let choice = drafts.retype.get();
        if choice.is_empty()
            || preview.r#type.id != choice
            || preview.refusal_on_write.is_some()
            || dialog.busy
            || dialog.loading
            || dialog.applied.is_some()
        {
            return;
        }
        let key = format!(
            "{}:{}:{:?}",
            dialog.expected_revision, choice, dialog.structure_ids
        );
        if applied_key.get_untracked() == key {
            return;
        }
        applied_key.set(key);
        apply(Event::Intent(Intent::RetypeApply {
            request: RetypeRequest {
                expected_revision: dialog.expected_revision.clone(),
                structure_ids: dialog.structure_ids.clone(),
                structure_type_id: choice,
            },
        }));
    });
    view! {<div class="ds-profile-dialogs">{move||model.get().map(|panel|dialog_views(&panel,&scene.get(),&facts.get(),drafts,send.clone()))}</div>}
}
fn modal(title: &str, body: Vec<AnyView>, kind: DialogKind, send: Send) -> AnyView {
    view!{<div role="dialog" aria-modal="true" aria-label=title.to_string() class="ds-profile-dialog"><header><strong>{title.to_string()}</strong><button type="button" aria-label="Close dialog" on:click=move|_|send(Event::Intent(Intent::DialogClose{dialog:kind.clone()}))>"×"</button></header>{body}</div>}.into_any()
}
fn dialog_views(
    panel: &PanelModel,
    scene: &Scene,
    facts: &Facts,
    d: Drafts,
    send: Send,
) -> Vec<AnyView> {
    let mut dialogs = vec![];
    if let Some(delete) = &panel.dialogs.delete {
        let revision = delete.revision.clone();
        let ids = delete.structure_ids.clone();
        let dispatch = send.clone();
        dialogs.push(modal("Delete selected structures",vec![view!{<p>{format!("Delete {} structures and their connected stringing?",ids.len())}</p><p role="alert">{delete.error.clone()}</p><button type="button" disabled=delete.busy on:click=move|_|dispatch(Event::Intent(Intent::DeleteApply{expected_revision:revision.clone(),commands:vec![HeadCommand{command_id:command_id(),command:ModelCommand::DeleteProfileEntities{ids:ids.clone()}}]}))>"Delete"</button>}.into_any()],DialogKind::Delete,send.clone()));
    }
    if let Some(retype) = &panel.dialogs.retype {
        let request_ids = retype.structure_ids.clone();
        let revision = retype.expected_revision.clone();
        let apply_ids = request_ids.clone();
        let apply_rev = revision.clone();
        let preview = send.clone();
        let apply = send.clone();
        let definitions = retype.definitions.clone();
        let choose_ids = request_ids.clone();
        let choose_revision = revision.clone();
        let choose = send.clone();
        let selected_types: Vec<_> = scene
            .bands
            .iter()
            .flat_map(|b| &b.structures)
            .filter(|s| retype.structure_ids.contains(&s.entity_id))
            .map(|s| s.structure_type_id.clone())
            .collect();
        let same_type = selected_types.len() == retype.structure_ids.len()
            && selected_types
                .first()
                .is_some_and(|first| selected_types.iter().all(|id| id == first));
        let current_type = if same_type {
            selected_types.first().cloned()
        } else {
            None
        };
        let options = move || {
            let query = d.type_query.get().to_lowercase();
            definitions.iter().filter(|x|x.table=="structure_types"&&query.split_whitespace().all(|q|x.label.to_lowercase().contains(q))).take(40).filter_map(|x|x.structure_type_id.as_ref().map(|id|{let id=id.clone();let ids=choose_ids.clone();let revision=choose_revision.clone();let send=choose.clone();let disabled=current_type.as_ref()==Some(&id);view!{<button type="button" role="option" disabled=disabled on:click=move|_|{d.retype.set(id.clone());send(Event::Intent(Intent::RetypePreview{request:RetypeRequest{expected_revision:revision.clone(),structure_ids:ids.clone(),structure_type_id:id.clone()}}));}>{x.label.clone()}{disabled.then_some(" · current")}</button>}})).collect_view()
        };
        let findings=retype.preview.as_ref().map(|p|view!{<p>{format!("{} cleared · {} remaining · {} new findings",p.findings.cleared.len(),p.findings.remaining.len(),p.findings.created.len())}</p>{p.structures.iter().map(|s|view!{<p>{format!("{}: {} → {}",s.number,s.type_before,s.type_after)}</p>}).collect_view()}{p.findings.remaining.iter().chain(&p.findings.created).map(|f|view!{<p>{format!("{} · {} · {}",f.structure_id,f.rule_id,f.finding)}</p>}).collect_view()}<p>{p.refusal_on_write.clone()}</p>});
        let can_apply = retype
            .preview
            .as_ref()
            .is_some_and(|p| p.refusal_on_write.is_some());
        let busy = retype.busy || retype.loading;
        dialogs.push(modal("Choose structure type",vec![view!{<input type="search" aria-label="Replacement type" placeholder="Search assembly letters or name" on:input=move|e|d.type_query.set(event_target_value(&e)) /><div role="listbox" aria-label="Replacement structure types">{options}</div><button type="button" disabled=move||busy||d.retype.get().is_empty() on:click=move|_|preview(Event::Intent(Intent::RetypePreview{request:RetypeRequest{expected_revision:revision.clone(),structure_ids:request_ids.clone(),structure_type_id:d.retype.get_untracked()}}))>"Preview"</button>{findings}<p role="alert">{retype.error.clone()}</p><button type="button" disabled=move||busy||!can_apply||d.retype.get().is_empty() on:click=move|_|apply(Event::Intent(Intent::RetypeApply{request:RetypeRequest{expected_revision:apply_rev.clone(),structure_ids:apply_ids.clone(),structure_type_id:d.retype.get_untracked()}}))>"Replace anyway"</button>}.into_any()],DialogKind::Retype,send.clone()));
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
    if let Some(selection) = &panel.dialogs.selection {
        dialogs.push(selection_dialog(selection, facts, d, send));
    }
    dialogs
}
fn selection_dialog(selection: &Selection, facts: &Facts, d: Drafts, send: Send) -> AnyView {
    let result = selection.result.clone().unwrap_or_default();
    let columns = result.columns.clone();
    let add_columns = columns.clone();
    let family = send.clone();
    let query = send.clone();
    let apply = send.clone();
    let existing = facts.selected.clone();
    let matched = result.entity_ids.clone();
    let busy = selection.busy || selection.loading;
    let fields = move || {
        d.filters.get().into_iter().enumerate().map(|(i,row)|{let columns=columns.clone();let column_options=columns.iter().map(|c|view!{<option value=c.name.clone() selected=c.name==row.column>{c.name.clone()}</option>}).collect_view();let ty=columns.iter().find(|c|c.name==row.column).map(|c|c.data_type.as_str()).unwrap_or("string");let operators=operators(ty).into_iter().map(|(op,label)|{let name=serde_json::to_value(&op).unwrap().as_str().unwrap().to_string();view!{<option value=name selected=op==row.op>{label}</option>}}).collect_view();view!{<div style="display:flex;gap:4px;margin:6px 0"><select aria-label="Filter column" on:change=move|e|{d.filters.update(|rows|{if let Some(r)=rows.get_mut(i){r.column=event_target_value(&e);r.op=FilterOp::Equals;r.value.clear();r.value2=None;}});d.dirty.set(true);}>{column_options}</select><select aria-label="Filter operator" on:change=move|e|{if let Ok(op)=serde_json::from_value(serde_json::Value::String(event_target_value(&e))){d.filters.update(|rows|{if let Some(r)=rows.get_mut(i){r.op=op;}});d.dirty.set(true);}}>{operators}</select><input aria-label="Filter value" value=row.value on:input=move|e|{d.filters.update(|rows|{if let Some(r)=rows.get_mut(i){r.value=event_target_value(&e);}});d.dirty.set(true);}/>{matches!(row.op,FilterOp::Between).then(||view!{<input aria-label="Upper filter value" value=row.value2.clone().unwrap_or_default() on:input=move|e|{d.filters.update(|rows|{if let Some(r)=rows.get_mut(i){r.value2=Some(event_target_value(&e));}});d.dirty.set(true);}/>})}<button type="button" aria-label="Remove filter" on:click=move|_|{d.filters.update(|rows|{if i<rows.len(){rows.remove(i);}});d.dirty.set(true);}>"×"</button></div>}}).collect_view()
    };
    modal("Profile selection filter",vec![view!{<select aria-label="Selection family" on:change=move|e|{let table=event_target_value(&e);d.table.set(table.clone());d.filters.set(vec![]);d.dirty.set(true);family(Event::Intent(Intent::SelectionQuery{table,query:FilterQuery::default()}));}><option value="structures">"Structures"</option><option value="tension_sections">"Tension sections"</option><option value="terrain_points">"Ground points"</option><option value="alignments">"Alignments"</option></select>{fields}<button type="button" on:click=move|_|{if let Some(c)=add_columns.iter().find(|c|c.name!="id").or_else(||add_columns.first()){d.filters.update(|rows|rows.push(Filter{column:c.name.clone(),op:if c.data_type=="string"{FilterOp::Contains}else{FilterOp::Equals},value:String::new(),value2:None}));d.dirty.set(true);}}>"Add condition"</button><button type="button" disabled=busy on:click=move|_|{d.dirty.set(false);query(Event::Intent(Intent::SelectionQuery{table:d.table.get_untracked(),query:FilterQuery{filters:d.filters.get_untracked(),..FilterQuery::default()}}));}>"Find matches"</button><p role="status">{format!("{} of {} match",result.matched_count,result.total_rows)}</p><select aria-label="Selection operation" on:change=move|e|d.mode.set(event_target_value(&e))><option value="replace">"Replace"</option><option value="add">"Add"</option><option value="remove">"Remove"</option><option value="intersect">"Intersect"</option></select><p role="alert">{selection.error.clone()}</p><button type="button" disabled=move||busy||d.dirty.get() on:click=move|_|{let ids=compose(&d.mode.get_untracked(),&existing,&matched);apply(Event::Intent(Intent::SelectionApply{primary_id:ids.last().cloned(),entity_ids:ids}));}>"Apply selection"</button>}.into_any()],DialogKind::Selection,send)
}
fn operators(ty: &str) -> Vec<(FilterOp, &'static str)> {
    let mut o = vec![
        (FilterOp::Equals, "equals"),
        (FilterOp::NotEquals, "does not equal"),
        (FilterOp::IsEmpty, "is empty"),
        (FilterOp::NotEmpty, "is not empty"),
    ];
    match ty {
        "string" => o.extend([
            (FilterOp::Contains, "contains"),
            (FilterOp::StartsWith, "starts with"),
            (FilterOp::EndsWith, "ends with"),
        ]),
        "boolean" => {}
        _ => o.extend([
            (FilterOp::Gt, "greater than"),
            (FilterOp::Gte, "at least"),
            (FilterOp::Lt, "less than"),
            (FilterOp::Lte, "at most"),
            (FilterOp::Between, "between"),
        ]),
    };
    o
}
fn compose(mode: &str, current: &[String], matched: &[String]) -> Vec<String> {
    match mode {
        "add" => {
            let mut ids = current.to_vec();
            for id in matched {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
            ids
        }
        "remove" => current
            .iter()
            .filter(|id| !matched.contains(id))
            .cloned()
            .collect(),
        "intersect" => current
            .iter()
            .filter(|id| matched.contains(id))
            .cloned()
            .collect(),
        _ => matched.to_vec(),
    }
}
fn command_id() -> String {
    web_sys::window().unwrap().crypto().unwrap().random_uuid()
}
const CSS: &str = r#"
[data-profile-root]{height:100%;min-height:0;display:flex;flex-direction:column;position:relative;color:#cbd5e1;background:#020617;font:11px system-ui,sans-serif}
[data-profile-root] header{display:flex;align-items:center;gap:3px;min-height:30px;flex-shrink:0;background:#0f172a;border-bottom:1px solid #334155;padding:2px 8px}
[data-profile-root] button,[data-profile-root] input,[data-profile-root] select,[data-profile-root] textarea{font:inherit;color:inherit;box-sizing:border-box}[data-profile-root] button{appearance:none;background:transparent;border:1px solid transparent}[data-profile-root] button:focus-visible,[data-profile-root] summary:focus-visible,[data-profile-root] input:focus-visible,[data-profile-root] select:focus-visible{outline:2px solid #67e8f9;outline-offset:1px}[data-profile-root] button,[data-profile-root] summary{border-radius:3px;padding:4px 6px;cursor:pointer;text-align:left;white-space:nowrap}
[data-profile-root] button:hover,[data-profile-root] summary:hover{background:#1e293b}[data-profile-root] button:disabled{opacity:.4;cursor:default}
.ds-profile-spacer{flex:1}.ds-profile-scale{font:10px monospace;min-width:40px;text-align:center}.ds-profile-menu{position:relative;flex-shrink:0}.ds-profile-menu summary{list-style:none}
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
fn legend(scene: &Scene) -> AnyView {
    let Some(display) = &scene.display else {
        return ().into_any();
    };
    let entries=display.legend.iter().map(|l|view!{<p style=format!("color:{}",l.encoding.color)>{format!("{} {}",l.glyph,l.label)}</p>}).collect_view();
    view!{<details class="ds-profile-menu"><summary aria-label="Profile legend">"☰"</summary><div class="ds-profile-dropdown">{entries}</div></details>}.into_any()
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
        let feedback = p.display_feedback.clone();
        let retry = send.clone();
        let requested = feedback.patch.clone();
        view!{
            {(p.desktop&&!p.generic&&p.scene_present&&scene.display.is_none()).then(||view!{<div role="status" style="position:absolute;inset:0;display:grid;place-items:center;background:#020617;z-index:20">"Native Profile display is unavailable"</div>})}
            <span style="position:absolute;left:8px;top:8px;pointer-events:none;font:10px system-ui;color:#64748b">{p.model_name.clone()}</span>
            {(p.generic&&!scene.conductor_spans.is_empty()&&!state.visibility.show_wire).then(||view!{<span role="status" class="ds-profile-notice">"Conductors hidden"</span>})}
            {(p.generic&&scene.conductor_spans.is_empty()&&p.generic_ui_model.as_ref().is_some_and(|u|u.domain=="lv_distribution")).then(||view!{<span role="status" class="ds-profile-notice">"No stringing"</span>})}
            {p.calculation_required.then(||view!{<span role="status" class="ds-profile-notice">{if p.reconciling{"Reconciling calculation…"}else{"Calculation due"}}</span>})}
            {p.moving_structure.then(||view!{<span role="status" class="ds-profile-notice">"Checking move along alignment…"</span>})}
            {p.provisional.then(||view!{<span role="status" class="ds-profile-notice">"Provisional move preview · not saved"</span>})}
            {(count>0).then(||view!{<button type="button" class="ds-profile-curve-warning" data-profile-curve-warning title=format!("{count} sections have unavailable curves · Analysis results") aria-label=format!("{count} sections have unavailable curves · Analysis results") on:click=move|_|results(Event::Intent(Intent::PanelAction{action:PanelAction::Results}))><span aria-hidden="true">"⚠"</span>{count}</button>})}
            {feedback.error.map(|e|{let recoverable=["profile_calculation_required","profile_case_base_unavailable","profile_replay_stale"].contains(&e.code.as_str());view!{<div role="alert" class="ds-profile-weather-error"><p>{e.message}</p>{recoverable.then(||view!{<button type="button" disabled=!p.preparation.can_rebuild on:click=move|_|{if let Some(patch)=&requested{retry(Event::Intent(Intent::Display{patch:patch.clone(),analyze:false,intent:Some("rebuild".into())}));}}>"Calculate and choose"</button>})}</div>}})}
        }.into_any()
    }
}
const BADGE_CSS: &str = r#".ds-profile-notice{position:absolute;left:8px;top:26px;pointer-events:none;padding:4px 6px;color:#fde68a;background:#020617dd;border:1px solid #92400e;border-radius:3px;z-index:25}.ds-profile-curve-warning{position:absolute;right:8px;top:8px;z-index:25;color:#fde68a;background:#020617ee;border:1px solid #92400e}.ds-profile-weather-error{position:absolute;right:8px;bottom:40px;padding:10px;max-width:300px;background:#020617;border:1px solid #92400e;z-index:80}"#;
