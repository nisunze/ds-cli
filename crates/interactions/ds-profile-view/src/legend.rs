//! Read-only symbols and pens projected by the native Profile owner.
use crate::model::Scene;
use leptos::{leptos_dom::helpers::window_event_listener, prelude::*};
use wasm_bindgen::JsCast;

pub fn view(scene: &Scene) -> AnyView {
    let Some(display) = &scene.display else {
        return ().into_any();
    };
    let rows = display.legend.clone();
    let inks: Vec<_> = rows
        .iter()
        .take(3)
        .map(|row| row.encoding.color.clone())
        .collect();
    let open = RwSignal::new(false);
    let root = NodeRef::<leptos::html::Div>::new();
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
        if open.get_untracked() && event.key() == "Escape" {
            event.prevent_default();
            open.set(false);
        }
    });
    on_cleanup(move || {
        outside.remove();
        escape.remove();
    });
    view! {<style>{CSS}</style><div node_ref=root class="ds-profile-legend" data-profile-legend-position="toolbar">
        <button type="button" aria-label="Legend" title="Show the symbols and colours" aria-haspopup="dialog"
            aria-expanded=move || open.get().to_string() on:click=move |_| open.update(|value| *value = !*value)>
            <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">{inks.into_iter().enumerate().map(|(index, ink)| view! {
                <rect x="1" y=(2 + index * 5).to_string() width="4" height="3" fill=ink.clone()/>
                <line x1="8" y1=(3 + index * 5).to_string() x2="17" y2=(3 + index * 5).to_string() stroke=ink stroke-width="2"/>
            }).collect_view()}</svg>
        </button>
        {move || open.get().then(|| view! {<div role="dialog" aria-label="Profile legend" class="ds-profile-legend-dialog">
            {rows.iter().map(|row| {
                let sample = if row.line {
                    view! {<svg width="28" height="14" aria-hidden="true"><line x1="1" y1="7" x2="27" y2="7"
                        stroke=row.encoding.color.clone() stroke-width=row.encoding.width_px.to_string()
                        stroke-dasharray={row.encoding.dash_px.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")}/></svg>}.into_any()
                } else {
                    view! {<span style=format!("font:{};color:{}", row.encoding.font, row.encoding.color)>{row.glyph.clone()}</span>}.into_any()
                };
                view! {<div class="ds-profile-legend-row"><span class="ds-profile-legend-sample">{sample}</span><span title=row.label.clone()>{row.label.clone()}</span></div>}
            }).collect_view()}
        </div>})}
    </div>}.into_any()
}

const CSS: &str = ".ds-profile-legend{position:relative;flex-shrink:0}.ds-profile-legend>button{display:flex;align-items:center;justify-content:center;width:28px;height:28px}.ds-profile-legend-dialog{position:absolute;left:0;top:100%;z-index:90;width:max-content;max-width:calc(100vw - 2rem);max-height:60vh;overflow:auto;margin-top:4px;padding:8px;border:1px solid #475569;border-radius:4px;background:#020617;box-shadow:0 12px 24px #0008;color:#cbd5e1}.ds-profile-legend-row{display:flex;align-items:center;gap:8px;white-space:nowrap;padding:3px 0}.ds-profile-legend-sample{display:inline-flex;align-items:center;justify-content:center;width:28px;flex-shrink:0}";
