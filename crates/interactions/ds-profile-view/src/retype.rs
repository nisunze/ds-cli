//! Display the native retype preview and committed repair receipt.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize)]
pub struct Outcome {
    pub outcomes: Vec<Applied>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Applied {
    pub delta: Delta,
}
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub unresolved_retype_attachments: Vec<Repair>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Repair {
    pub section_id: String,
    pub sequence: u64,
    pub structure_id: String,
    pub previous_attachment_point_id: String,
    pub set_label: String,
    pub slot: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub message: String,
    pub tone: &'static str,
    pub repair_hint: String,
    pub lines: Vec<String>,
}
fn words(
    translations: &BTreeMap<String, String>,
    key: &str,
    fallback: &str,
    params: &[(&str, String)],
) -> String {
    let mut text = translations
        .get(key)
        .cloned()
        .unwrap_or_else(|| fallback.into());
    for (key, value) in params {
        text = text.replace(&format!("{{{key}}}"), value);
    }
    text
}
pub fn summary(outcome: &Outcome, translations: &BTreeMap<String, String>) -> Summary {
    let repairs: Vec<_> = outcome
        .outcomes
        .iter()
        .flat_map(|o| &o.delta.unresolved_retype_attachments)
        .collect();
    let sections = repairs
        .iter()
        .map(|repair| &repair.section_id)
        .collect::<BTreeSet<_>>()
        .len();
    Summary {
        message: if repairs.is_empty() { words(translations, "gprof_retype_applied", "Selected structures retyped.", &[]) }
            else { words(translations, "gprof_retype_draft_applied", "Draft type applied: {attachments} attachments need repair in {sections} sections.", &[("attachments", repairs.len().to_string()), ("sections", sections.to_string())]) },
        tone: if repairs.is_empty() { "success" } else { "warning" },
        repair_hint: if repairs.is_empty() { String::new() } else { words(translations, "gprof_retype_repair_hint", "Sections preserved. Explicitly repair the named supports below before export or publication.", &[]) },
        lines: repairs.iter().map(|repair| words(translations, "gprof_retype_attachment", "{section} · support {sequence} · structure {structure} · {set}, slot {slot} · previous attachment {previous}", &[
            ("section", repair.section_id.clone()), ("sequence", repair.sequence.to_string()), ("structure", repair.structure_id.clone()),
            ("set", repair.set_label.clone()), ("slot", repair.slot.to_string()), ("previous", repair.previous_attachment_point_id.clone()),
        ])).collect(),
    }
}

#[cfg(target_arch = "wasm32")]
mod ui {
    use super::*;
    use crate::{
        model::{Intent, Scene},
        panel::{Event, Send},
        panel_model::{Definition, DialogKind, PanelModel, Retype, RetypeRequest},
        retype_search,
    };
    use leptos::{leptos_dom::helpers::window_event_listener, prelude::*};
    use std::{collections::HashSet, sync::Arc};
    use wasm_bindgen::JsCast;

    #[derive(Clone, Copy)]
    struct Draft {
        query: RwSignal<String>,
        choice: RwSignal<String>,
        active: RwSignal<usize>,
        applied: RwSignal<String>,
    }
    fn choose(
        model: &ArcRwSignal<Option<Arc<PanelModel>>>,
        scene: &ArcRwSignal<Arc<Scene>>,
        draft: Draft,
        id: &str,
        send: &Send,
    ) {
        let Some(panel) = model.get_untracked() else {
            return;
        };
        let Some(dialog) = &panel.dialogs.retype else {
            return;
        };
        if !dialog.current
            || panel.read_only
            || dialog.busy
            || dialog.loading
            || dialog.applied.is_some()
            || id.is_empty()
        {
            return;
        }
        let scene = scene.get_untracked();
        let current = current_types(&scene, dialog, &dialog.definitions);
        if scene.model_revision == dialog.expected_revision
            && current.len() == 1
            && current[0].2 == dialog.structure_ids.len()
            && current[0].0 == id
        {
            return;
        }
        draft.choice.set(id.into());
        draft.applied.set(String::new());
        send(Event::Intent(Intent::RetypePreview {
            request: RetypeRequest {
                expected_revision: dialog.expected_revision.clone(),
                structure_ids: dialog.structure_ids.clone(),
                structure_type_id: id.into(),
            },
        }));
    }
    fn current_types(
        scene: &Scene,
        dialog: &Retype,
        definitions: &[Definition],
    ) -> Vec<(String, String, usize)> {
        let wanted: HashSet<_> = dialog.structure_ids.iter().collect();
        let mut seen = HashSet::new();
        let mut types: Vec<(String, String, usize)> = vec![];
        for structure in scene.bands.iter().flat_map(|band| &band.structures) {
            if !wanted.contains(&structure.entity_id) || !seen.insert(&structure.entity_id) {
                continue;
            }
            if let Some(current) = types
                .iter_mut()
                .find(|current| current.0 == structure.structure_type_id)
            {
                current.2 += 1;
            } else {
                let label = definitions
                    .iter()
                    .find(|definition| {
                        definition.structure_type_id.as_ref() == Some(&structure.structure_type_id)
                    })
                    .map(|definition| definition.label.clone())
                    .unwrap_or_else(|| {
                        if structure.engineering_name.is_empty() {
                            structure.structure_type_id.clone()
                        } else {
                            structure.engineering_name.clone()
                        }
                    });
                types.push((structure.structure_type_id.clone(), label, 1));
            }
        }
        types
    }
    pub fn view(
        model: ArcRwSignal<Option<Arc<PanelModel>>>,
        scene: ArcRwSignal<Arc<Scene>>,
        send: Send,
    ) -> impl IntoView {
        let draft = Draft {
            query: RwSignal::new(String::new()),
            choice: RwSignal::new(String::new()),
            active: RwSignal::new(0),
            applied: RwSignal::new(String::new()),
        };
        let previous = StoredValue::new(String::new());
        let reset = model.clone();
        Effect::new(move |_| {
            let key = reset
                .get()
                .and_then(|p| p.dialogs.retype.clone())
                .map(|dialog| {
                    format!(
                        "{}:{}:{:?}",
                        dialog.model_id, dialog.expected_revision, dialog.structure_ids
                    )
                })
                .unwrap_or_default();
            if key != previous.get_value() {
                previous.set_value(key);
                draft.query.set(String::new());
                draft.choice.set(String::new());
                draft.active.set(0);
                draft.applied.set(String::new());
            }
        });
        let auto_model = model.clone();
        let auto = send.clone();
        Effect::new(move |_| {
            let Some(panel) = auto_model.get() else {
                return;
            };
            let Some(dialog) = &panel.dialogs.retype else {
                return;
            };
            let Some(preview) = &dialog.preview else {
                return;
            };
            let choice = draft.choice.get();
            if choice.is_empty()
                || !dialog.current
                || panel.read_only
                || dialog.busy
                || dialog.loading
                || dialog.applied.is_some()
                || preview.revision != dialog.expected_revision
                || preview.r#type.id != choice
                || preview
                    .refusal_on_write
                    .as_ref()
                    .is_some_and(|code| !code.is_empty())
            {
                return;
            }
            let key = format!(
                "{}:{}:{:?}",
                dialog.expected_revision, choice, dialog.structure_ids
            );
            if draft.applied.get_untracked() == key {
                return;
            }
            draft.applied.set(key);
            auto(Event::Intent(Intent::RetypeApply {
                request: RetypeRequest {
                    expected_revision: dialog.expected_revision.clone(),
                    structure_ids: dialog.structure_ids.clone(),
                    structure_type_id: choice,
                },
            }));
        });
        let root = NodeRef::<leptos::html::Div>::new();
        let input = NodeRef::<leptos::html::Input>::new();
        let close_model = model.clone();
        let outside = send.clone();
        let pointer = window_event_listener(leptos::ev::pointerdown, move |event| {
            if !close_model
                .get_untracked()
                .is_some_and(|p| p.dialogs.retype.is_some())
            {
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
                outside(Event::Intent(Intent::DialogClose {
                    dialog: DialogKind::Retype,
                }));
            }
        });
        let close_model = model.clone();
        let escape = send.clone();
        let keyboard = window_event_listener(leptos::ev::keydown, move |event| {
            if event.key() == "Escape"
                && close_model
                    .get_untracked()
                    .is_some_and(|p| p.dialogs.retype.is_some())
            {
                event.prevent_default();
                escape(Event::Intent(Intent::DialogClose {
                    dialog: DialogKind::Retype,
                }));
            }
        });
        on_cleanup(move || {
            pointer.remove();
            keyboard.remove();
        });
        let focused = StoredValue::new(String::new());
        let focus_model = model.clone();
        Effect::new(move |_| {
            let Some(panel) = focus_model.get() else {
                return;
            };
            let Some(dialog) = &panel.dialogs.retype else {
                return;
            };
            let key = format!("{}:{}", dialog.model_id, dialog.expected_revision);
            if focused.get_value() != key {
                if let Some(input) = input.get() {
                    let _ = input.focus();
                    focused.set_value(key);
                }
            }
        });
        view! {<div node_ref=root class="ds-profile-retype-host">{move || {
            let Some(panel) = model.get() else { return ().into_any(); }; let Some(dialog) = panel.dialogs.retype.clone() else { return ().into_any(); };
            let displayed = scene.get(); let choice = draft.choice.get(); let types = current_types(&displayed,&dialog,&dialog.definitions); let same = displayed.model_revision == dialog.expected_revision && types.len()==1 && types[0].2==dialog.structure_ids.len();
            let current = format!("{}: {}",if displayed.model_revision==dialog.expected_revision {"Current"} else {"Displayed"},if types.is_empty(){"unavailable".into()}else{types.iter().map(|(_,name,count)|if same{name.clone()}else{format!("{name} ({count})")}).collect::<Vec<_>>().join(" · ")});
            let close = send.clone();
            if let Some(applied) = &dialog.applied { let receipt = serde_json::from_value::<Outcome>(applied.clone()).ok().map(|outcome|summary(&outcome,&panel.translations));
                return view! {<div role="group" aria-label="Structure type" class="ds-profile-dialog"><header><strong>"Structure type"</strong><button type="button" aria-label="Close retype picker" on:click=move |_|close(Event::Intent(Intent::DialogClose{dialog:DialogKind::Retype}))>"×"</button></header>{receipt.map(|receipt|view! {<p role="status">{receipt.message}</p><p>{receipt.repair_hint}</p><ul>{receipt.lines.into_iter().map(|line|view!{<li>{line}</li>}).collect_view()}</ul>})}</div>}.into_any();
            }
            let mut definitions:Vec<_>=dialog.definitions.iter().filter(|entry|entry.table=="structure_types"&&entry.structure_type_id.is_some()).cloned().collect();
            let locales=js_sys::Array::new();let options=js_sys::Object::new();definitions.sort_by(|a,b|js_sys::JsString::from(a.label.as_str()).locale_compare(&b.label,&locales,&options).cmp(&0));
            let definitions=Arc::new(definitions);let rows=definitions.clone();let key_rows=definitions.clone();
            let query_model=model.clone();let query_scene=scene.clone();let key_model=model.clone();let key_scene=scene.clone();
            let choose_send=send.clone();let key_send=send.clone();let replace=send.clone();let replace_dialog=dialog.clone();
            let current_id=if same {types.first().map(|row|row.0.clone())}else{None};let disabled=dialog.busy||dialog.loading||!dialog.current||panel.read_only;
            let findings=dialog.preview.as_ref().filter(|preview|preview.revision==dialog.expected_revision && preview.r#type.id == choice && !choice.is_empty()).map(|preview|{
                let refusal=preview.refusal_on_write.as_ref().filter(|code|!code.is_empty()).cloned();
                let warning=refusal.as_ref().map(|code|words(&panel.translations,"gprof_retype_rule_warning","This replacement breaks a design rule: {code}. Replacing anyway keeps it; the structure stays flagged.",&[("code",code.replace('_'," "))]));
                view!{<div><p>{format!("{} → {}",dialog.structure_ids.len(),preview.r#type.name)}</p><p>{format!("Rule findings: {} cleared · {} remaining · {} new",preview.findings.cleared.len(),preview.findings.remaining.len(),preview.findings.created.len())}</p>
                    {warning.map(|warning|view!{<p role="alert">{warning}</p>})}
                    {refusal.map(|_|view!{<button type="button" disabled=disabled on:click=move |_|{let choice=draft.choice.get_untracked();if !choice.is_empty(){replace(Event::Intent(Intent::RetypeApply{request:RetypeRequest{expected_revision:replace_dialog.expected_revision.clone(),structure_ids:replace_dialog.structure_ids.clone(),structure_type_id:choice}}));}}>{panel.label("gprof_retype_replace_anyway","Replace anyway")}</button>})}
                    <ul>{preview.findings.remaining.iter().chain(&preview.findings.created).map(|finding|view!{<li>{format!("{}: {} · {}",finding.structure_id,finding.finding.replace('_'," "),finding.source_clause)}</li>}).collect_view()}</ul></div>}
            });
            view! {<div role="group" aria-label="Structure type" class="ds-profile-dialog" on:contextmenu=move|event:web_sys::MouseEvent|event.prevent_default()>
                <header><div><strong>{if dialog.structure_ids.len()==1{"Structure type".into()}else{format!("Type for {} structures",dialog.structure_ids.len())}}</strong><p>{current}</p></div><button type="button" aria-label="Close retype picker" on:click=move |_|close(Event::Intent(Intent::DialogClose{dialog:DialogKind::Retype}))>"×"</button></header>
                <label>"Replacement type"<input node_ref=input type="search" aria-label="Replacement type" placeholder="Search assembly letters or name" prop:value=move||draft.query.get() disabled=dialog.busy on:input=move|event|{draft.query.set(event_target_value(&event));draft.active.set(0);draft.choice.set(String::new());} on:keydown=move|event:web_sys::KeyboardEvent|{let matches=retype_search::search(&key_rows,&draft.query.get_untracked());match event.key().as_str(){"ArrowDown"=>{event.prevent_default();draft.active.set((draft.active.get_untracked()+1).min(matches.len().min(40).saturating_sub(1)));},"ArrowUp"=>{event.prevent_default();draft.active.set(draft.active.get_untracked().saturating_sub(1));},"Enter"=>{event.prevent_default();if let Some(id)=matches.get(draft.active.get_untracked()).and_then(|entry|entry.structure_type_id.as_deref()){choose(&key_model,&key_scene,draft,id,&key_send);}},_=>{}}}/></label>
                {dialog.loading.then(||view!{<p role="status">"Loading model library…"</p>})}
                <div role="listbox" aria-label="Replacement structure types">{move||{let matches=retype_search::search(&rows,&draft.query.get());let count=matches.len();let rendered=matches.into_iter().take(40).enumerate().map(|(index,entry)|{let id=entry.structure_type_id.clone().unwrap();let send=choose_send.clone();let model=query_model.clone();let scene=query_scene.clone();let current=current_id.as_ref()==Some(&id);view!{<button type="button" role="option" aria-selected=current.to_string() style=move||if draft.active.get()==index{"background:#083344"}else{""} disabled=disabled on:click=move |_|choose(&model,&scene,draft,&id,&send)>{entry.label.clone()}{current.then_some(" · current")}</button>}}).collect_view();view!{{rendered}{(count==0).then(||view!{<p>"No matching type."</p>})}{(count>40).then(||view!{<p>{format!("Showing 40 of {count}. Type more to narrow the list.")}</p>})}}}}</div>
                {dialog.busy.then(||view!{<p role="status">"Checking selected structures…"</p>})}{findings}
                {(!dialog.current).then(||view!{<p role="alert">"The model changed. Select the structures again."</p>})}
                {(!dialog.error.is_empty()).then(||view!{<p role="alert">{dialog.error.clone()}</p>})}
            </div>}.into_any()
        }}</div>}
    }
}
#[cfg(target_arch = "wasm32")]
pub use ui::view;
