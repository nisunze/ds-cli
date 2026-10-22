//! Weather selection remains a native display request. Calculation is explicit.
use crate::{
    model::{Intent, Scene},
    panel::{Event, Send},
    panel_model::{DisplayCase, DisplayPatch, FeedbackError, PanelModel},
};
use leptos::{leptos_dom::helpers::window_event_listener, prelude::*};
use std::sync::Arc;
use wasm_bindgen::JsCast;

pub fn view(
    model: ArcRwSignal<Option<Arc<PanelModel>>>,
    scene: ArcRwSignal<Arc<Scene>>,
    send: Send,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let requested = RwSignal::new(None::<DisplayCase>);
    let eligible = RwSignal::new(false);
    let pending = RwSignal::new(false);
    let failure = RwSignal::new(None::<FeedbackError>);
    let start_sequence = StoredValue::new(0_u64);
    let root = NodeRef::<leptos::html::Div>::new();
    let observed = model.clone();
    Effect::new(move |_| {
        let Some(panel) = observed.get() else {
            return;
        };
        let feedback = &panel.display_feedback;
        let case = feedback
            .patch
            .as_ref()
            .and_then(|patch| patch.display_case.clone());
        if case.is_none() {
            requested.set(None);
            failure.set(None);
            eligible.set(false);
            pending.set(false);
            open.set(false);
        } else {
            requested.set(case);
            if let Some(error) = &feedback.error {
                failure.set(Some(error.clone()));
            }
            if feedback.error.as_ref().is_some_and(|error| {
                [
                    "profile_calculation_required",
                    "profile_case_base_unavailable",
                    "profile_replay_stale",
                ]
                .contains(&error.code.as_str())
            }) {
                eligible.set(true);
            }
            if !feedback.busy && feedback.sequence != start_sequence.get_value() {
                pending.set(false);
                if feedback.error.is_none() {
                    requested.set(None);
                    failure.set(None);
                    eligible.set(false);
                }
            }
        }
    });
    let outside = window_event_listener(leptos::ev::pointerdown, move |event| {
        if !open.get_untracked() {
            return;
        }
        let inside = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Node>().ok())
            .is_some_and(|target| {
                root.get_untracked()
                    .is_some_and(|root| root.contains(Some(&target)))
            });
        if !inside {
            open.set(false);
        }
    });
    let escape = window_event_listener(leptos::ev::keydown, move |event| {
        if event.key() == "Escape" && open.get_untracked() {
            event.prevent_default();
            open.set(false);
        }
    });
    on_cleanup(move || {
        outside.remove();
        escape.remove();
    });
    view! {<div node_ref=root class="ds-profile-weather" data-profile-weather-position="lower-right">{move||{
        let Some(panel)=model.get() else{return ().into_any();};let displayed=scene.get();let Some(engineering)=&displayed.engineering else{return ().into_any();};
        let selected=engineering.cases.get(engineering.selected_case_index).map(|case|case.label.clone()).unwrap_or_else(||"Weather cases loading…".into());
        let busy=pending.get()||panel.display_feedback.busy;
        let case=requested.get();let label=case.as_ref().and_then(|case|engineering.cases.iter().find(|choice|&choice.value==case)).map(|choice|choice.label.clone()).unwrap_or_else(||"weather case".into());
        let options=engineering.cases.iter().map(|choice|{let value=choice.value.clone();let choose=send.clone();let sequence=panel.display_feedback.sequence;
            view!{<button type="button" role="option" aria-selected=(engineering.cases.get(engineering.selected_case_index).is_some_and(|selected|selected.value==choice.value)).to_string() disabled=choice.disabled||!panel.preparation.interactive||busy on:click=move |_|{if pending.get_untracked(){return;}open.set(false);eligible.set(false);failure.set(None);requested.set(Some(value.clone()));pending.set(true);start_sequence.set_value(sequence);choose(Event::Intent(Intent::Display{patch:DisplayPatch{display_case:Some(value.clone()),..DisplayPatch::default()},analyze:false,intent:None}));}>{choice.label.clone()}</button>}
        }).collect_view();
        let calculate=send.clone();let sequence=panel.display_feedback.sequence;
        let error=failure.get();
        let retry=eligible.get()&&case.is_some();let disabled=busy||!panel.preparation.can_rebuild;
        view!{<div class="ds-profile-menu" data-profile-engineering><button type="button" aria-label="Weather case" aria-expanded=open.get().to_string() on:click=move |_|open.update(|value|*value=!*value)>{selected}" ▾"</button>
            {open.get().then(||view!{<div role="listbox" aria-label="Weather cases" class="ds-profile-dropdown ds-profile-above">{options}</div>})}
            {error.map(|error|view!{<div role="alert" class="ds-profile-weather-error"><p>{error.message}</p><p>{format!("Requested: {label}")}</p>
                {retry.then(||view!{<button type="button" disabled=disabled on:click=move |_|{if pending.get_untracked(){return;}let Some(case)=requested.get_untracked() else{return;};pending.set(true);start_sequence.set_value(sequence);calculate(Event::Intent(Intent::Display{patch:DisplayPatch{display_case:Some(case),..DisplayPatch::default()},analyze:false,intent:Some("rebuild".into())}));}>"Calculate and choose"</button>})}
            </div>})}
        </div>}.into_any()
    }}</div>}
}
