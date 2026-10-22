//! Stable native display controls. Encoding and requests come from the scene.
use crate::{
    model::{Intent, Scene},
    panel::{Event, Send},
    panel_model::{DisplayPatch, PanelModel},
};
use leptos::prelude::*;
use std::sync::Arc;
#[derive(Clone, Copy)]
struct Local {
    draft: RwSignal<String>,
    error: RwSignal<String>,
    pending: RwSignal<bool>,
    sequence: StoredValue<u64>,
}
fn dispatch(
    local: Local,
    model: &ArcRwSignal<Option<Arc<PanelModel>>>,
    send: &Send,
    patch: DisplayPatch,
) {
    if local.pending.get_untracked()
        || model
            .get_untracked()
            .is_some_and(|panel| panel.display_feedback.busy)
    {
        return;
    }
    local.error.set(String::new());
    local.pending.set(true);
    local.sequence.set_value(
        model
            .get_untracked()
            .map(|panel| panel.display_feedback.sequence)
            .unwrap_or_default(),
    );
    send(Event::Intent(Intent::Display {
        patch,
        analyze: false,
        intent: None,
    }));
}
pub fn view(
    model: ArcRwSignal<Option<Arc<PanelModel>>>,
    scene: ArcRwSignal<Arc<Scene>>,
    send: Send,
) -> impl IntoView {
    let local = Local {
        draft: RwSignal::new(String::new()),
        error: RwSignal::new(String::new()),
        pending: RwSignal::new(false),
        sequence: StoredValue::new(0),
    };
    let previous = StoredValue::new(0usize);
    let source = scene.clone();
    Effect::new(move |_| {
        let scene = source.get();
        let key = Arc::as_ptr(&scene) as usize;
        if key != previous.get_value() {
            previous.set_value(key);
            local.draft.set(
                scene
                    .display
                    .as_ref()
                    .and_then(|display| display.style_controls.as_ref())
                    .map(|style| serde_json::to_string_pretty(&style.request).unwrap_or_default())
                    .unwrap_or_default(),
            );
            local.error.set(String::new());
            local.pending.set(false);
        }
    });
    let feedback = model.clone();
    Effect::new(move |_| {
        let Some(panel) = feedback.get() else {
            return;
        };
        if panel.display_feedback.patch.is_none()
            || (!panel.display_feedback.busy
                && panel.display_feedback.sequence != local.sequence.get_value())
        {
            local.pending.set(false);
        }
    });
    let groups_model = model.clone();
    let groups_scene = scene.clone();
    let groups_send = send.clone();
    let styles_scene = scene.clone();
    let styles_model = model.clone();
    let styles_send = send.clone();
    let error_model = model.clone();
    view! {<details class="ds-profile-menu" data-native-profile-display><summary>"Display ▾"</summary><div role="menu" class="ds-profile-dropdown">
        {move||{let scene=groups_scene.get();let panel=groups_model.get_untracked().unwrap_or_default();let Some(display)=&scene.display else{let add=groups_send.clone();return view!{<p role="status">{panel.label("gprof_display_native_unavailable","Native display controls are unavailable")}</p>{(panel.generic&&!panel.popout_mode&&panel.generic_ui_model.as_ref().is_some_and(|model|model.capabilities.add_alignments)).then(||view!{<button type="button" role="menuitem" on:click=move |_|add(Event::Intent(Intent::PanelAction{action:crate::panel_model::Action::AddAlignment}))>"Add alignment…"</button>})}}.into_any();};
            let groups=display.groups.iter().map(|group|{let mut rows=vec![];
                for row in &group.rows{let option=row.option.clone();let dispatch_model=groups_model.clone();let busy=groups_model.clone();let send=groups_send.clone();let label=panel.label(&row.label_key,&row.label_key);let dash=row.encoding.dash_px.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");
                    rows.push(view!{<label><input type="checkbox" checked=row.checked disabled=move||local.pending.get()||busy.get().is_some_and(|panel|panel.display_feedback.busy) on:change=move|event|dispatch(local,&dispatch_model,&send,DisplayPatch{visibility:Some(std::collections::BTreeMap::from([(option.clone(),event_target_checked(&event))])),..DisplayPatch::default()})/><svg width="28" height="14" aria-hidden="true"><line x1="1" y1="7" x2="27" y2="7" stroke=row.encoding.color.clone() stroke-width=row.encoding.width_px stroke-dasharray=dash/></svg><span style=format!("color:{}",row.encoding.label_color)>{label}</span></label>}.into_any());
                }
                for number in &group.numbers{let number=number.clone();let label=panel.label(&number.label_key,&number.label_key);let busy=groups_model.clone();let model=groups_model.clone();let send=groups_send.clone();
                    rows.push(view!{<label>{label}<input type="number" step="any" min=number.minimum max=number.maximum value=number.value disabled=move||local.pending.get()||busy.get().is_some_and(|panel|panel.display_feedback.busy) on:change=move|event|{let value=event_target_value(&event).parse::<f64>().unwrap_or(f64::NAN);match number.patch(value){Ok(patch)=>dispatch(local,&model,&send,patch),Err(error)=>local.error.set(error)}}/></label>}.into_any());
                }
                for cable in &group.cables{let name=cable.cable_name.clone();let reset_name=name.clone();let busy=groups_model.clone();let reset_busy=groups_model.clone();let model=groups_model.clone();let reset_model=groups_model.clone();let send=groups_send.clone();let reset_send=groups_send.clone();
                    rows.push(view!{<label>{cable.cable_name.clone()}<input type="color" value=cable.encoding.color.clone() disabled=move||local.pending.get()||busy.get().is_some_and(|panel|panel.display_feedback.busy) on:change=move|event|dispatch(local,&model,&send,DisplayPatch{cable_colors:Some(std::collections::BTreeMap::from([(name.clone(),Some(event_target_value(&event)))])),..DisplayPatch::default()})/>{cable.can_reset.then(||view!{<button type="button" disabled=move||local.pending.get()||reset_busy.get().is_some_and(|panel|panel.display_feedback.busy) on:click=move |_|dispatch(local,&reset_model,&reset_send,DisplayPatch{cable_colors:Some(std::collections::BTreeMap::from([(reset_name.clone(),None)])),..DisplayPatch::default()})>"Reset"</button>})}</label>}.into_any());
                }
                view!{<details open=group.initially_open data-profile-display-group=group.id.clone()><summary>{panel.label(&group.label_key,&group.id)}</summary><div>{rows}</div></details>}
            }).collect_view();view!{{groups}}.into_any()
        }}
        {move||{let scene=styles_scene.get();let Some(styles)=scene.display.as_ref().and_then(|display|display.style_controls.as_ref()) else{return ().into_any();};let schema=serde_json::to_string_pretty(&styles.schema).unwrap_or_default();let busy=styles_model.clone();let apply_busy=styles_model.clone();let model=styles_model.clone();let send=styles_send.clone();
            view!{<details data-profile-display-styles><summary>"Styles"</summary><textarea aria-label="Profile style request" prop:value=move||local.draft.get() disabled=move||local.pending.get()||busy.get().is_some_and(|panel|panel.display_feedback.busy) on:input=move|event|local.draft.set(event_target_value(&event))></textarea><button type="button" disabled=move||local.pending.get()||apply_busy.get().is_some_and(|panel|panel.display_feedback.busy) on:click=move |_|{match serde_json::from_str::<DisplayPatch>(&local.draft.get_untracked()){Ok(patch)=>dispatch(local,&model,&send,patch),Err(error)=>local.error.set(error.to_string())}}>"Apply"</button><details><summary>"Schema"</summary><pre>{schema}</pre></details></details>}.into_any()
        }}
        {move||{let native=error_model.get().and_then(|panel|panel.display_feedback.error.clone().filter(|_|panel.display_feedback.patch.as_ref().is_some_and(|patch|patch.display_case.is_none()))).map(|error|error.message).unwrap_or_default();let own=local.error.get();let error=if own.is_empty(){native}else{own};(!error.is_empty()).then(||view!{<p role="alert">{error}</p>})}}
    </div></details>}
}
