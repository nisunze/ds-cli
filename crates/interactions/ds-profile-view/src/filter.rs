//! Local filter controls. The native model evaluates every condition and facet.
use crate::panel_model::{Filter, FilterColumn, FilterOp};
use std::collections::HashSet;

pub fn compose(mode: &str, current: &[String], matched: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let current: Vec<_> = current
        .iter()
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect();
    seen.clear();
    let matched: Vec<_> = matched
        .iter()
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect();
    match mode {
        "add" => {
            let mut ids = current;
            for id in matched {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            ids
        }
        "remove" => current
            .into_iter()
            .filter(|id| !matched.contains(id))
            .collect(),
        "intersect" => current
            .into_iter()
            .filter(|id| matched.contains(id))
            .collect(),
        _ => matched,
    }
}
pub fn operators(kind: &str) -> Vec<(FilterOp, &'static str)> {
    let mut result = vec![
        (FilterOp::Equals, "equals"),
        (FilterOp::NotEquals, "does not equal"),
        (FilterOp::IsEmpty, "is empty"),
        (FilterOp::NotEmpty, "is not empty"),
    ];
    match kind {
        "string" => result.extend([
            (FilterOp::Contains, "contains"),
            (FilterOp::StartsWith, "starts with"),
            (FilterOp::EndsWith, "ends with"),
        ]),
        "boolean" => {}
        _ => result.extend([
            (FilterOp::Gt, "greater than"),
            (FilterOp::Gte, "at least"),
            (FilterOp::Lt, "less than"),
            (FilterOp::Lte, "at most"),
            (FilterOp::Between, "between"),
        ]),
    }
    result
}

/// Validate only control completeness; native filtering owns predicate evaluation.
pub fn validate(filters: &[Filter], columns: &[FilterColumn]) -> Result<(), String> {
    for filter in filters {
        if !columns.iter().any(|column| column.name == filter.column) {
            return Err(format!("Unknown field: {}", filter.column));
        }
        if !matches!(filter.op, FilterOp::IsEmpty | FilterOp::NotEmpty)
            && filter.value.trim().is_empty()
        {
            return Err("Enter a value for every condition.".into());
        }
        if matches!(filter.op, FilterOp::Between)
            && filter
                .value2
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err("Enter both bounds for Between.".into());
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
mod ui {
    use super::*;
    use crate::{
        model::Intent,
        panel::{Event, Facts, Send},
        panel_model::{DialogKind, FilterQuery, PanelModel},
    };
    use leptos::{leptos_dom::helpers::window_event_listener, prelude::*};
    use std::sync::Arc;
    #[derive(Clone)]
    struct Row {
        key: u64,
        // Draft rows survive the model-driven replacement of the dialog DOM.
        value: ArcRwSignal<Filter>,
    }
    #[derive(Clone, Copy)]
    struct Draft {
        table: RwSignal<String>,
        filters: RwSignal<Vec<Row>>,
        next: StoredValue<u64>,
        mode: RwSignal<String>,
        dirty: RwSignal<bool>,
        pending: RwSignal<bool>,
        error: RwSignal<String>,
    }
    impl Draft {
        fn values(self) -> Vec<Filter> {
            self.filters
                .get_untracked()
                .into_iter()
                .map(|row| row.value.get_untracked())
                .collect()
        }
    }
    fn submit(draft: Draft, send: &Send, stats: Option<Vec<String>>, matches: bool) {
        if draft.pending.get_untracked() {
            return;
        }
        if matches {
            draft.dirty.set(false);
        }
        draft.pending.set(true);
        send(Event::Intent(Intent::SelectionQuery {
            table: draft.table.get_untracked(),
            query: FilterQuery {
                filters: if stats.is_some() {
                    vec![]
                } else {
                    draft.values()
                },
                stats_columns: stats,
                candidate_ids: None,
            },
        }));
    }
    fn facet(draft: Draft, column: String, send: &Send) {
        draft.dirty.set(true);
        submit(draft, send, Some(vec![column]), false);
    }
    fn name(table: &str) -> &'static str {
        match table {
            "terrain_points" => "ground points",
            "tension_sections" => "tension sections",
            "alignments" => "alignments",
            _ => "structures",
        }
    }
    fn default_field(columns: &[FilterColumn]) -> Option<&FilterColumn> {
        columns
            .iter()
            .find(|column| column.name == "engineering_name")
            .or_else(|| columns.iter().find(|column| column.name != "id"))
            .or_else(|| columns.first())
    }
    pub fn view(
        model: ArcRwSignal<Option<Arc<PanelModel>>>,
        facts: ArcRwSignal<Facts>,
        send: Send,
    ) -> impl IntoView {
        let draft = Draft {
            table: RwSignal::new("structures".into()),
            filters: RwSignal::new(vec![]),
            next: StoredValue::new(0),
            mode: RwSignal::new("replace".into()),
            dirty: RwSignal::new(true),
            pending: RwSignal::new(false),
            error: RwSignal::new(String::new()),
        };
        let previous = StoredValue::new(String::new());
        let reset = model.clone();
        Effect::new(move |_| {
            let selection = reset
                .get()
                .and_then(|panel| panel.dialogs.selection.clone());
            let key = selection
                .as_ref()
                .map(|selection| format!("{}:{}", selection.model_id, selection.expected_revision))
                .unwrap_or_default();
            if key != previous.get_value() {
                previous.set_value(key);
                draft.table.set("structures".into());
                draft.filters.set(vec![]);
                draft.mode.set("replace".into());
                draft.dirty.set(true);
                draft.pending.set(false);
                draft.error.set(String::new());
            }
            if selection
                .as_ref()
                .is_none_or(|selection| !selection.loading && !selection.busy)
            {
                draft.pending.set(false);
            }
        });
        let close_model = model.clone();
        let close = send.clone();
        let escape = window_event_listener(leptos::ev::keydown, move |event| {
            if event.key() == "Escape"
                && close_model
                    .get_untracked()
                    .is_some_and(|panel| panel.dialogs.selection.is_some())
            {
                event.prevent_default();
                close(Event::Intent(Intent::DialogClose {
                    dialog: DialogKind::Selection,
                }));
            }
        });
        on_cleanup(move || escape.remove());
        view! {<div class="ds-profile-filter-host">{move||{
            let Some(panel)=model.get() else{return ().into_any();};let Some(selection)=panel.dialogs.selection.clone() else{return ().into_any();};
            let table=draft.table.get();let result=selection.result.clone().filter(|result|result.table_kind==table&&result.revision==selection.expected_revision);
            let columns=result.as_ref().map(|result|result.columns.clone()).unwrap_or_default();let stats=result.as_ref().map(|result|result.value_stats.clone()).unwrap_or_default();let matched=result.as_ref().map(|result|result.entity_ids.clone()).unwrap_or_default();
            let available=selection.current&&!selection.busy&&!selection.loading;
            let family=send.clone();let add=send.clone();let query=send.clone();let apply=send.clone();let close=send.clone();let header_close=send.clone();
            let no_columns=columns.is_empty();let add_columns=columns.clone();let all_columns=columns.clone();let query_columns=columns.clone();let fields_send=send.clone();
            let selected=facts.get().selected;
            let status=result.as_ref().map(|result|format!("{} of {} {} match this revision.",result.matched_count,result.total_rows,name(&table))).unwrap_or_else(||"Loading filterable fields…".into());
            let can_apply=available&&result.is_some()&&selection.error.is_empty();
            view! {<div role="dialog" aria-modal="true" aria-label="Selection" class="ds-profile-dialog">
                <header><strong>"Selection"</strong><button type="button" aria-label="Close selection" on:click=move |_|header_close(Event::Intent(Intent::DialogClose{dialog:DialogKind::Selection}))>"×"</button></header>
                <label>"Select from"<select aria-label="Selection family" prop:value=move||draft.table.get() disabled=!selection.current on:change=move|event|{let table=event_target_value(&event);draft.table.set(table.clone());draft.filters.set(vec![]);draft.dirty.set(true);draft.pending.set(false);submit(draft,&family,None,false);}><option value="structures">"Structures"</option><option value="tension_sections">"Tension sections"</option><option value="terrain_points">"Ground points"</option><option value="alignments">"Alignments"</option></select></label>
                <label>"Selection operation"<select aria-label="Selection operation" prop:value=move||draft.mode.get() on:change=move|event|draft.mode.set(event_target_value(&event))><option value="replace">"Replace"</option><option value="add">"Add"</option><option value="remove">"Remove"</option><option value="intersect">"Intersect"</option></select></label>
                <For each=move||draft.filters.get() key=|row|row.key children=move|entry|{
                    let field=RwSignal::from(entry.value);let row=field.get_untracked();let columns=all_columns.clone();let operator_columns=columns.clone();
                    let options=columns.iter().map(|column|view!{<option value=column.name.clone() selected=column.name==row.column>{column.name.replace('_'," ")}</option>}).collect_view();
                    let change=fields_send.clone();let requested=stats.iter().find(|stat|stat.column==row.column).cloned();let datalist=format!("profile-filter-values-{}",entry.key);let list=datalist.clone();
                    let between=Memo::new(move |_|matches!(field.get().op,FilterOp::Between));
                    view! {<div class="ds-profile-filter-row"><select aria-label="Field" on:change=move|event|{let column=event_target_value(&event);field.update(|row|{row.column=column.clone();row.op=if columns.iter().find(|value|value.name==column).is_some_and(|value|value.data_type=="string"){FilterOp::Contains}else{FilterOp::Equals};row.value.clear();row.value2=None;});facet(draft,column,&change);}>{options}</select>
                        <select aria-label="Condition" on:change=move|event|{if let Ok(op)=serde_json::from_value(serde_json::Value::String(event_target_value(&event))){field.update(|row|row.op=op);draft.dirty.set(true);}}>{move||{let row=field.get();let kind=operator_columns.iter().find(|column|column.name==row.column).map(|column|column.data_type.as_str()).unwrap_or("string");operators(kind).into_iter().map(|(op,label)|view!{<option value=serde_json::to_value(&op).unwrap().as_str().unwrap().to_string() selected=op==row.op>{label}</option>}).collect_view()}}</select>
                        <input aria-label="Value" prop:value=move||field.get().value list=datalist on:input=move|event|{field.update(|row|row.value=event_target_value(&event));draft.dirty.set(true);}/>
                        {move||between.get().then(||view!{<input aria-label="Upper bound" prop:value=move||field.get().value2.unwrap_or_default() on:input=move|event|{field.update(|row|row.value2=Some(event_target_value(&event)));draft.dirty.set(true);}/>})}
                        <button type="button" aria-label="Remove condition" on:click=move |_|{draft.filters.update(|rows|rows.retain(|row|row.key!=entry.key));draft.dirty.set(true);}>"×"</button>
                        {requested.map(|stat|view!{<datalist id=list>{stat.values.into_iter().map(|value|view!{<option value=value/>}).collect_view()}</datalist>{stat.numeric_min.zip(stat.numeric_max).map(|(min,max)|view!{<span>{format!("Native range: {min}–{max}")}</span>})}})}
                    </div>}
                }/>
                {move||(draft.filters.get().is_empty()).then(||view!{<p>{format!("No conditions selects every {}.",name(&table))}</p>})}
                <button type="button" disabled=move||!available||draft.pending.get()||no_columns on:click=move |_|{if let Some(column)=default_field(&add_columns){let field=column.name.clone();draft.filters.update(|rows|{let key=draft.next.get_value()+1;draft.next.set_value(key);rows.push(Row{key,value:ArcRwSignal::new(Filter{column:field.clone(),op:if column.data_type=="string"{FilterOp::Contains}else{FilterOp::Equals},value:String::new(),value2:None})});});facet(draft,field,&add);}}>"+ Add condition"</button>
                <button type="button" disabled=move||!available||draft.pending.get() on:click=move |_|{match validate(&draft.values(),&query_columns){Ok(())=>{draft.error.set(String::new());submit(draft,&query,None,true);},Err(error)=>draft.error.set(error)}}>"Find matches"</button>
                <p role="status">{status}</p>
                {(!selection.current).then(||view!{<p role="alert">"The model changed. Reopen Selection to query the current revision."</p>})}
                {(!selection.error.is_empty()).then(||view!{<p role="alert">{selection.error.clone()}</p>})}
                {move||(!draft.error.get().is_empty()).then(||view!{<p role="alert">{draft.error.get()}</p>})}
                <button type="button" disabled=move||!can_apply||draft.dirty.get()||draft.pending.get() on:click=move |_|{let ids=compose(&draft.mode.get_untracked(),&selected,&matched);apply(Event::Intent(Intent::SelectionApply{primary_id:matched.last().cloned().or_else(||ids.last().cloned()),entity_ids:ids}));}>"Select matches"</button>
                <button type="button" on:click=move |_|close(Event::Intent(Intent::DialogClose{dialog:DialogKind::Selection}))>"Close"</button>
            </div>}.into_any()
        }}</div>}
    }
}
#[cfg(target_arch = "wasm32")]
pub use ui::view;
