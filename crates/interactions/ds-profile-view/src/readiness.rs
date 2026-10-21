//! Inspection of already requested work. Rendering never requests a native job.
use crate::{
    model::Intent,
    panel::{Event, Send},
    panel_model::{Action, PanelModel},
};
use leptos::{leptos_dom::helpers::window_event_listener, prelude::*};
use std::sync::Arc;
use wasm_bindgen::JsCast;

fn action(label: &str, kind: Action, open: RwSignal<bool>, send: Send) -> AnyView {
    view! {<button type="button" on:click=move |_| {
        open.set(false);
        send(Event::Intent(Intent::PanelAction { action: kind.clone() }));
    }>{label.to_string()}</button>}
    .into_any()
}

pub fn view(model: ArcRwSignal<Option<Arc<PanelModel>>>, send: Send) -> impl IntoView {
    let open = RwSignal::new(false);
    let container = NodeRef::<leptos::html::Div>::new();
    let owner = StoredValue::new(String::new());
    let ownership_model = model.clone();
    Effect::new(move |_| {
        let current = ownership_model
            .get()
            .map(|p| {
                format!(
                    "{:?}|{:?}|{}|{}|{}",
                    p.readiness.as_ref().and_then(|r| r.get("context")),
                    p.readiness.as_ref().and_then(|r| r.get("clientScope")),
                    p.model_name,
                    p.enabled,
                    p.show_readiness
                )
            })
            .unwrap_or_default();
        if current != owner.get_value() {
            owner.set_value(current);
            open.set(false);
        }
    });
    let dismiss = window_event_listener(leptos::ev::pointerdown, move |event| {
        if !open.get_untracked() {
            return;
        }
        let inside = event
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
            .is_some_and(|target| {
                container
                    .get_untracked()
                    .is_some_and(|node| node.contains(Some(&target)))
            });
        if !inside {
            open.set(false);
        }
    });
    let escape = window_event_listener(leptos::ev::keydown, move |event| {
        if open.get_untracked() && event.key() == "Escape" {
            event.prevent_default();
            open.set(false);
        }
    });
    on_cleanup(move || {
        dismiss.remove();
        escape.remove();
    });
    view! {<style>{CSS}</style><div node_ref=container class="ds-profile-status-container">{move || {
        let Some(p) = model.get().filter(|p| p.enabled && p.show_readiness) else { return ().into_any(); };
        let q = &p.preparation;
        let icon = if q.busy {
            view! {<svg class="animate-spin motion-reduce:animate-none" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M20 12a8 8 0 1 0-3 6" stroke-linecap="round"/></svg>}.into_any()
        } else if q.tone == "success" {
            view! {<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7" stroke-linecap="round" stroke-linejoin="round"/></svg>}.into_any()
        } else {
            view! {<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M12 3 2 21h20L12 3Z" stroke-linejoin="round"/><path d="M12 9v5m0 3h.01" stroke-linecap="round"/></svg>}.into_any()
        };
        let color = match q.tone.as_str() { "error" => "text-rose-300", "success" => "text-emerald-300", _ => "text-amber-300" };
        let indicator = view! {<button type="button" data-profile-readiness data-profile-reconciliation data-phase=q.phase.clone()
            class=format!("ds-profile-status {color}") aria-expanded=move || open.get().to_string() aria-haspopup="dialog"
            aria-busy=q.busy.to_string() aria-label=format!("Profile status: {}", q.label) title=format!("{}. Click for readiness and available actions.", q.label)
            on:click=move |_| open.update(|value| *value = !*value)>{icon}</button>
            <span role="status" aria-live="polite" class="ds-profile-status-live">{q.label.clone()}</span>};
        let panel = p.clone(); let send = send.clone();
        view! {{indicator}{move || {
            if !open.get() { return ().into_any(); }
            let q = &panel.preparation;
            let job = panel.readiness.as_ref().and_then(|r| r.get("job")).filter(|j| !j.is_null());
            let admitted = panel.readiness.as_ref().and_then(|r| r.get("admittedScene")).and_then(|r| r.as_bool()).unwrap_or(false);
            let readonly = panel.read_only || panel.popout_mode;
            let mut actions = vec![];
            if !readonly {
                if q.can_admit { actions.push(action("Admit result", Action::JobAdmit, open, send.clone())); }
                if q.can_discard { actions.push(action("Discard result", Action::JobCancel, open, send.clone())); }
                if job.is_some() { actions.push(action("Read calculation status", Action::JobStatus, open, send.clone())); }
                if q.can_retry { actions.push(action("Retry Profile", Action::Rebuild, open, send.clone())); }
                if q.can_reconcile { actions.push(action(if panel.reconciliation.as_ref().is_some_and(|r| r.phase == "complete") { "Inspect findings" } else { "Reconcile and inspect" }, Action::Reconcile, open, send.clone())); }
            }
            actions.push(action("Close view", Action::Close, open, send.clone()));
            let notice = if q.initial_uninterruptible {
                "Initial preparation cannot be interrupted. Closing the view leaves model edits in RAM and prevents this result being shown.".to_string()
            } else if let Some(job) = job {
                format!("Native job: {}. Discarding a result prevents admission; the solver's current phase may continue.", job.get("status").and_then(|s| s.as_str()).unwrap_or("unknown"))
            } else { String::new() };
            let reconciliation = if !q.busy && admitted { panel.reconciliation.as_ref().map(|r| r.detail.clone()) } else { None };
            view! {<div role="dialog" aria-label="Profile readiness" class="ds-profile-status-dialog">
                <p class="ds-profile-status-title">{q.label.clone()}</p><p>{q.detail.clone()}</p>
                {(!notice.is_empty()).then(|| view! {<p>{notice}</p>})}
                {reconciliation.map(|detail| view! {<p>{detail}</p>})}
                <div class="ds-profile-status-actions">{actions}</div>
            </div>}.into_any()
        }}}.into_any()
    }}</div>}
}

const CSS: &str = ".ds-profile-status-container{position:relative;flex-shrink:0}.ds-profile-status{display:inline-flex;align-items:center;justify-content:center;width:28px;height:28px}.ds-profile-status svg{width:16px;height:16px}.ds-profile-status.text-rose-300{color:#fda4af}.ds-profile-status.text-emerald-300{color:#6ee7b7}.ds-profile-status.text-amber-300{color:#fcd34d}.ds-profile-status-live{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}.ds-profile-status-dialog{position:absolute;left:0;top:100%;z-index:90;width:288px;max-width:calc(100vw - 2rem);margin-top:4px;padding:12px;border:1px solid #475569;border-radius:4px;background:#020617;color:#e2e8f0;box-shadow:0 12px 24px #0008;font-size:11px}.ds-profile-status-title{font-weight:600}.ds-profile-status-dialog p{margin:4px 0;overflow-wrap:anywhere}.ds-profile-status-actions{display:flex;flex-wrap:wrap;gap:8px;margin-top:12px}.ds-profile-status-actions button{padding:4px 8px;border:1px solid #475569;border-radius:4px}.ds-profile-status .animate-spin{animation:ds-profile-status-spin 1s linear infinite}@keyframes ds-profile-status-spin{to{transform:rotate(360deg)}}@media(prefers-reduced-motion:reduce){.ds-profile-status .animate-spin{animation:none}}";
