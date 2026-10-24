//! Kernel printing operations over the shared print-layout contract
//! (ds-report-contracts): sessions, previews, inventory and the evaluator of
//! the Kernel's own printing operations. The documents and their admission
//! live in the contract; a layout edit (pens, paper, furniture, frames,
//! placement, tables, context layers, adjacent circuits, style resolution) is
//! the Reporter's `edit_print_layout` task, which Core's layout commands call.
pub use ds_report_contracts::printing::*;
pub mod custom;
pub mod held_context;
pub mod inventory;
pub mod lifecycle;
pub mod map;
pub mod mv_models;
pub mod mv_preview;
pub mod preview;
pub mod project_sources;
pub mod retained_preview;
pub mod retained_reads;
pub mod session;
pub mod sheet_status;
pub mod standard_read;
pub mod style_migration;
pub mod template_read;
pub mod view;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Plan the explicit migration of one stored layout's legacy style_refs
    /// to exact style_keys against the governed API resolution table (see
    /// `style_migration`). The reply names every finding; a migrated layout
    /// is returned only when all refs map exactly. Nothing is saved.
    MigrateStyleKeys {
        layout: Layout,
        resolution: crate::style_resolution::Snapshot,
        /// Owner choices, layer id → exact governed key, for named findings.
        #[serde(default)]
        choices: BTreeMap<String, crate::style_resolution::Key>,
    },
    StandardRegistry,
    ConsumeResolvedStyles {
        capture: Value,
        keys: BTreeMap<String, Value>,
        project: String,
    },
    StandardLayout {
        templates: standards::Templates,
        kind: standards::Kind,
        role: standards::PageRole,
        identity: standards::Identity,
        #[serde(default)]
        overrides: Option<standards::Overrides>,
    },
    ProjectSettings {
        sheets: serde_json::Value,
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        mode: Option<crate::report_formats::ReadinessMode>,
    },
    MvResolve {
        sheets: Value,
        project_id: String,
        #[serde(default)]
        model_fields: BTreeMap<mv::ModelField, String>,
    },
    MvSelectionPatch {
        current: Vec<Value>,
        selection: mv::Selection,
    },
    MvBookletPlan {
        setup: Box<mv::Resolved>,
        model_revision: String,
        pages: Vec<mv_booklet::Page>,
    },
    MvStandardize {
        layout: Layout,
    },
    MvBookletPreview {
        plan: mv_booklet::Plan,
        previews: Vec<mv_booklet::Preview>,
    },
    OutputSelection {
        #[serde(default)]
        selection: Option<crate::report_formats::DesignOutputSelection>,
        #[serde(default)]
        legacy_formats: Option<Vec<String>>,
        /// Whatever the project has actually stored, in any shape it has ever
        /// been stored in. A host passes the row value through and never
        /// decides which of the other two inputs it resembles.
        #[serde(default, deserialize_with = "deserialize_present")]
        stored: Option<serde_json::Value>,
        #[serde(default)]
        execution: Option<BTreeMap<String, Vec<crate::report_formats::Placement>>>,
        #[serde(default)]
        required_layout: Option<String>,
    },
    /// Patch a project's settings sheet with the outputs it should produce and
    /// the per-transformer print views one layout was authored with. The
    /// export row is resolved through every alias the planner reads, and
    /// created when the sheet has none.
    ProjectOutputPatch {
        current: Vec<serde_json::Value>,
        selection: crate::report_formats::DesignOutputSelection,
        #[serde(default)]
        layout_id: Option<String>,
        #[serde(default)]
        views: BTreeMap<String, BTreeMap<String, GeographicView>>,
    },
    /// What a host may do to a named setup right now — read live or from a
    /// held copy, create or update, copy, delete, or refuse — decided from
    /// the facts the host reports (see `lifecycle`).
    LifecyclePlan {
        facts: lifecycle::Facts,
    },
    /// Which transformers may be printed, in what order, and which of them
    /// this device holds (see `inventory`).
    PrintableTransformers {
        rows: Vec<serde_json::Value>,
        #[serde(default)]
        rooms: Option<Vec<inventory::Room>>,
        #[serde(default)]
        current_user: String,
        limit: usize,
    },
    /// May a PDF preview of this layout be produced right now, and from what:
    /// every bound print style must be available and there must be map data
    /// to draw. The host assembles the render request from the answer.
    RenderPlan {
        layout: Layout,
        #[serde(default)]
        available_style_refs: Vec<String>,
        #[serde(default)]
        layer_count: usize,
        #[serde(default)]
        text: serde_json::Value,
        #[serde(default)]
        date: String,
    },
    /// May a live transformer preview be produced right now, and as what
    /// (see `preview`). Rendering remains a host effect.
    PreviewPlan {
        facts: preview::Facts,
    },
    /// The DPI and paper a local map print may ask for, with the defaults.
    MapRequestChoices {},
    /// The print editor's zero state and its history cap (see `session`).
    SessionDefaults {},
    /// One event against the editor's workflow state: the next state and the
    /// operations the host performs on the documents it holds.
    SessionStep {
        state: session::State,
        event: session::Event,
    },
    /// Read-only selection/page consumption of an already held printing view.
    ViewPlan {
        request: view::Request,
        facts: view::Facts,
    },
    TransformerViews {
        views: BTreeMap<String, BTreeMap<String, GeographicView>>,
    },
    New {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        paper: Paper,
        #[serde(default)]
        orientation: Orientation,
    },
    TemplateCatalogue {
        rows: Vec<catalogue::Row>,
        #[serde(default)]
        project_id: Option<String>,
        #[serde(default)]
        governed: Option<Value>,
    },
    ReadGovernedTemplate {
        location: catalogue::Location,
        response: Value,
    },
    /// Admit one exact immutable standard page for read-only consumption.
    ReadGovernedStandard {
        location: catalogue::Location,
        response: Value,
    },
    /// Capture a static, pinned governed page with retained renderer policy.
    CaptureGovernedStandardPreview {
        location: catalogue::Location,
        document: standards::TemplateDocument,
        resolution: crate::style_resolution::Snapshot,
        #[serde(default)]
        text: Value,
        #[serde(default)]
        date: String,
    },
    /// Capture a static saved project page from its authenticated get receipt.
    CaptureProjectLayoutPreview {
        project: String,
        response: Value,
        resolution: crate::style_resolution::Snapshot,
        #[serde(default)]
        text: Value,
        #[serde(default)]
        date: String,
    },
    Validate {
        layout: Layout,
    },
    /// Map space -> printable border -> independent container -> local table.
    SheetStatus {
        assessments: Vec<Value>,
        warnings: Vec<table_rows::PrintWarning>,
    },
    ContextStatus {
        omitted: Vec<sheet_status::ContextOmission>,
    },
}
/// Tell "the key is absent" apart from "the key is present and null". Serde
/// collapses both onto `None`, and for a stored settings value that difference
/// is the whole question: a project's export row may hold `null`, and `null` is
/// something it stored — an empty selection — not a caller that passed no
/// argument at all.
fn deserialize_present<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

/// The per-transformer print view map, bounded and validated once. The native
/// reporter reads this exact nesting at export time, so the rule lives beside
/// the document grammar rather than in whichever host writes it.
fn validate_transformer_views(
    views: &BTreeMap<String, BTreeMap<String, GeographicView>>,
) -> Result<(), String> {
    if views.len() > 10_000 {
        return Err("transformer print views exceed 10,000 transformers".into());
    }
    for (transformer, slots) in views {
        let bytes = transformer.as_bytes();
        if bytes.is_empty()
            || bytes.len() > 121
            || !((bytes[0].is_ascii_lowercase())
                || (bytes[0] == b'_' && bytes.get(1).is_some_and(u8::is_ascii_digit)))
            || !bytes[1..]
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
            || slots.len() > 64
        {
            return Err("invalid transformer print view map".into());
        }
        for (slot, view) in slots {
            if slot.is_empty()
                || slot.len() > 80
                || !slot
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                return Err("invalid print viewport slot".into());
            }
            view.validate()?;
        }
    }
    Ok(())
}

/// The whole project-settings patch a printing preparation performs: the
/// export selection into its aliased row, and one layout's authored views into
/// the `printing_views` row the native reporter reads
/// (transformer → layout id → slot → view, bounded 10 000 / 32 / 8).
fn project_output_patch(
    current: Vec<serde_json::Value>,
    selection: &crate::report_formats::DesignOutputSelection,
    layout_id: Option<&str>,
    views: &BTreeMap<String, BTreeMap<String, GeographicView>>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut rows = current;
    selection.require_deliverable()?;
    crate::report_formats::apply_output_selection(&mut rows, selection)?;
    if views.is_empty() {
        return Ok(rows);
    }
    let layout_id = layout_id.ok_or("print views require the layout they were authored on")?;
    if !identifier(layout_id) {
        return Err("invalid printing setup id".into());
    }
    validate_transformer_views(views)?;
    let index = rows
        .iter()
        .position(|row| {
            crate::report_formats::setting_parameter_key(row)
                .is_some_and(|key| key == PRINTING_VIEWS_SETTING)
        })
        .unwrap_or_else(|| {
            rows.push(json!({"parameter":PRINTING_VIEWS_SETTING,"value":{}}));
            rows.len() - 1
        });
    let mut current = match rows[index]["value"].as_object() {
        Some(existing) => existing.clone(),
        None => serde_json::Map::new(),
    };
    for (transformer, slots) in views {
        let layouts = current
            .entry(transformer.clone())
            .or_insert_with(|| json!({}));
        if !layouts.is_object() {
            *layouts = json!({});
        }
        layouts[layout_id] = json!(slots);
    }
    if current.len() > 10_000 {
        return Err("transformer print views exceed 10,000 transformers".into());
    }
    for layouts in current.values() {
        let layouts = layouts
            .as_object()
            .ok_or("invalid transformer print views")?;
        if layouts.len() > 32 {
            return Err("a transformer carries print views for more than 32 layouts".into());
        }
        for slots in layouts.values() {
            if slots.as_object().is_none_or(|slots| slots.len() > 8) {
                return Err("a print layout carries more than 8 viewport slots".into());
            }
        }
    }
    rows[index]["value"] = Value::Object(current);
    Ok(rows)
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("print input exceeds 32 MiB".into());
    }
    let command: Command =
        serde_json::from_slice(input).map_err(|e| format!("invalid printing request: {e}"))?;
    let value = match command {
        Command::MigrateStyleKeys {
            layout,
            resolution,
            choices,
        } => {
            crate::style_resolution::validate_snapshot(&resolution).map_err(|e| e.to_string())?;
            let catalogue = style_migration::Catalogue::of(&resolution);
            json!(style_migration::answer(style_migration::Request {
                layout,
                catalogue,
                choices,
            })?)
        }
        Command::StandardRegistry => standards::registry(),
        Command::ConsumeResolvedStyles {
            capture,
            keys,
            project,
        } => resolved_styles::consume(&capture, &keys, &project)?,
        Command::StandardLayout {
            templates,
            kind,
            role,
            identity,
            overrides,
        } => {
            json!({"layout":standards::layout(&templates, kind, role, &identity, overrides.as_ref())?})
        }
        Command::ProjectSettings {
            sheets,
            mode,
            project_id,
        } => {
            let mut result = crate::report_formats::inspect_project_settings(&sheets, mode)?;
            result["mv"] = mv::inspect(&sheets, project_id.as_deref());
            result["transformer_instructions"] = sheets["project_settings"]
                .as_array()
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row["parameter"] == "printing_overrides")
                })
                .map(|row| row["value"].clone())
                .unwrap_or_else(|| json!({}));
            result
        }
        Command::MvResolve {
            sheets,
            project_id,
            model_fields,
        } => match mv::resolve(&sheets, &project_id, model_fields) {
            Ok(resolved) => json!({"resolved":resolved,"refusal":Value::Null}),
            Err(refusal) => json!({"resolved":Value::Null,"refusal":refusal}),
        },
        Command::MvSelectionPatch { current, selection } => {
            match mv::patch_selection(&current, &selection) {
                Ok(rows) => json!({"project_settings":rows,"refusal":Value::Null}),
                Err(refusal) => json!({"project_settings":Value::Null,"refusal":refusal}),
            }
        }
        Command::MvBookletPlan {
            setup,
            model_revision,
            pages,
        } => serde_json::to_value(mv_booklet::plan(&setup, &model_revision, pages)?)
            .map_err(|e| e.to_string())?,
        Command::MvStandardize { layout } => {
            json!({"standard":mv::STANDARD,"layout":mv::standardize(layout)?})
        }
        Command::MvBookletPreview { plan, previews } => {
            mv_booklet::preview_document(&plan, &previews)?
        }
        Command::OutputSelection {
            selection,
            legacy_formats,
            stored,
            execution,
            required_layout,
        } => {
            let selection = match stored {
                Some(stored) => {
                    if selection.is_some() || legacy_formats.is_some() || execution.is_some() {
                        return Err(
                            "pass a stored settings value alone, without a selection, legacy formats or placements".into(),
                        );
                    }
                    crate::report_formats::stored_output_selection(&stored)?
                }
                None => crate::report_formats::normalize_output_selection(
                    selection,
                    legacy_formats,
                    execution,
                )?,
            };
            if let Some(layout) = required_layout {
                if !identifier(&layout)
                    || !selection.prints.iter().any(|print| {
                        print.enabled && print.layout_id == layout && !print.formats.is_empty()
                    })
                {
                    return Err(
                        "the selected outputs must include the required named layout".into(),
                    );
                }
            }
            let placements = selection.placement_map()?;
            // The selection omits the default layout; the reply states it, so
            // a host renders the choice without re-stating `extended`.
            let voltage_drop_report = selection.voltage_drop_report_format();
            json!({"selection":selection,"placements":placements,
                "voltage_drop_report":voltage_drop_report})
        }
        Command::ProjectOutputPatch {
            current,
            selection,
            layout_id,
            views,
        } => {
            json!({"project_settings":project_output_patch(current, &selection, layout_id.as_deref(), &views)?})
        }
        Command::LifecyclePlan { facts } => {
            serde_json::to_value(lifecycle::plan(&facts)?).map_err(|e| e.to_string())?
        }
        Command::PrintableTransformers {
            rows,
            rooms,
            current_user,
            limit,
        } => {
            let mut result = serde_json::to_value(inventory::printable(
                &rows,
                rooms.as_deref().unwrap_or_default(),
                &current_user,
                limit,
            )?)
            .map_err(|e| e.to_string())?;
            // Absence of a host room inventory is unknown, not proof of no cache.
            result["local_rooms_known"] = json!(rooms.is_some());
            if rooms.is_none() {
                for row in result["transformers"]
                    .as_array_mut()
                    .expect("inventory rows")
                {
                    row["cached"] = serde_json::Value::Null;
                    row["dirty"] = serde_json::Value::Null;
                }
            }
            result
        }
        Command::RenderPlan {
            layout,
            available_style_refs,
            layer_count,
            text,
            date,
        } => {
            validate(&layout)?;
            let mut refusals = Vec::new();
            let mut refs: Vec<&String> = Vec::new();
            for reference in layout.style_refs.values() {
                if !refs.contains(&reference) {
                    refs.push(reference);
                }
                if !available_style_refs.contains(reference) {
                    refusals.push(json!({
                        "code": "print_style_unavailable",
                        "message_key": "printing_style_unavailable",
                        "params": {"reference": reference},
                    }));
                }
            }
            if layer_count == 0
                && preview::expected_subject(&layout) != preview::SubjectKind::Layout
            {
                refusals.push(json!({
                    "code": "print_preview_needs_layers",
                    "message_key": "printing_preview_needs_layers",
                    "params": {},
                }));
            }
            let mut composed = text.as_object().cloned().unwrap_or_default();
            composed.insert("date".into(), serde_json::Value::String(date));
            let mut names: BTreeMap<String, String> = composed
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_owned())))
                .collect();
            names::plan_text(&mut names);
            for (key, value) in names {
                composed.insert(key, json!(value));
            }
            json!({
                "ready": refusals.is_empty(),
                "refusals": refusals,
                "print_style_refs": refs,
                "text": composed,
            })
        }
        Command::PreviewPlan { facts } => preview::evaluate(preview::Facts {
            layout: upgrade(facts.layout)?,
            ..facts
        })?,
        Command::MapRequestChoices {} => map::request_choices(),
        Command::SessionDefaults {} => json!({
            "state": session::State::default(),
            "history_cap": session::HISTORY_CAP,
        }),
        Command::SessionStep { state, event } => {
            serde_json::to_value(session::step(&state, &event)?).map_err(|e| e.to_string())?
        }
        Command::ViewPlan { request, facts } => {
            serde_json::to_value(view::plan(&request, &facts)?).map_err(|e| e.to_string())?
        }
        Command::TransformerViews { views } => {
            validate_transformer_views(&views)?;
            json!({"views":views})
        }
        Command::New {
            id,
            name,
            paper,
            orientation,
        } => json!({"layout":named_layout(id,name,paper,orientation)?}),
        Command::TemplateCatalogue {
            rows,
            project_id,
            governed,
        } => serde_json::to_value(catalogue::project(rows, project_id.as_deref(), governed)?)
            .map_err(|e| e.to_string())?,
        Command::ReadGovernedTemplate { location, response } => {
            serde_json::to_value(catalogue::read_governed(location, response)?)
                .map_err(|e| e.to_string())?
        }
        Command::ReadGovernedStandard { location, response } => {
            serde_json::to_value(standard_read::admit(&location, &response)?)
                .map_err(|e| e.to_string())?
        }
        Command::CaptureGovernedStandardPreview {
            location,
            document,
            resolution,
            text,
            date,
        } => standard_read::capture_preview(location, document, &resolution, text, date)?,
        Command::CaptureProjectLayoutPreview {
            project,
            response,
            resolution,
            text,
            date,
        } => standard_read::capture_project_preview(project, response, &resolution, text, date)?,
        Command::Validate { layout } => {
            let layout = upgrade(layout)?;
            validate(&layout)?;
            json!({"layout":layout,"valid":true})
        }
        Command::ContextStatus { omitted } => json!(sheet_status::context_status(omitted)?),
        Command::SheetStatus {
            assessments,
            warnings,
        } => serde_json::to_value(sheet_status::summarize(&assessments, &warnings)?)
            .map_err(|e| e.to_string())?,
    };
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

pub fn command_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Command)).expect("command schema serializes")
}

/// Whether this evaluator owns an operation, read from its own grammar. A
/// layout edit it does not own is the Reporter's `edit_print_layout` task
/// (pens, paper, furniture, frames, placement, tables, context layers,
/// adjacent circuits, style resolution); Core routes the edit there.
pub fn owns_op(op: &str) -> bool {
    static OPS: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    OPS.get_or_init(|| {
        command_schema()["oneOf"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|variant| variant["properties"]["op"]["const"].as_str())
            .map(str::to_owned)
            .collect()
    })
    .contains(op)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_evaluator_names_its_own_operations_and_leaves_layout_edits_to_the_reporter() {
        for op in [
            "new",
            "validate",
            "output_selection",
            "session_step",
            "render_plan",
        ] {
            assert!(owns_op(op), "{op}");
        }
        for op in [
            "set_paper",
            "print_pens",
            "context_toggle",
            "placement_areas",
            "no_such_op",
        ] {
            assert!(!owns_op(op), "{op}");
            assert!(evaluate(format!(r#"{{"op":"{op}"}}"#).as_bytes())
                .unwrap_err()
                .contains("unknown variant"));
        }
    }

    #[test]
    fn graticule_label_ink_is_optional_and_legacy_documents_round_trip_unchanged() {
        let legacy = json!({
            "crs":"tm_rwanda", "interval":500., "lines":true, "labels":true,
            "color":"#6B7280", "width_mm":0.12, "font_pt":6.
        });
        let graticule: Graticule = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(graticule.label_color, None);
        assert_eq!(serde_json::to_value(graticule).unwrap(), legacy);
        assert!(serde_json::to_value(Graticule::default())
            .unwrap()
            .get("label_color")
            .is_none());
    }

    #[test]
    fn legend_category_fields_are_explicit_bounded_and_legacy_omitted() {
        let mut layout = default_layout();
        let index = layout
            .elements
            .iter()
            .position(|e| e.kind == ElementKind::Legend)
            .unwrap();
        let legacy = serde_json::to_value(layout.elements[index].legend.as_ref().unwrap()).unwrap();
        assert!(legacy.get("category_fields").is_none());
        let legend = layout.elements[index].legend.as_mut().unwrap();
        legend.layers = vec!["lv_poles".into()];
        legend.category_fields.insert(
            "lv_poles".into(),
            vec![
                "struct_type".into(),
                "material".into(),
                "existing_status".into(),
            ],
        );
        validate(&layout).unwrap();
        for fields in [
            vec![],
            vec!["struct_type", "struct_type"],
            vec![" struct_type"],
            vec!["struct_type", "material", "existing_status", "other"],
        ] {
            let mut invalid = layout.clone();
            invalid.elements[index]
                .legend
                .as_mut()
                .unwrap()
                .category_fields
                .insert(
                    "lv_poles".into(),
                    fields.into_iter().map(str::to_owned).collect(),
                );
            assert!(validate(&invalid).is_err());
        }
        let mut unrelated = layout.clone();
        unrelated.elements[index]
            .legend
            .as_mut()
            .unwrap()
            .category_fields
            .insert("other".into(), vec!["struct_type".into()]);
        assert!(validate(&unrelated).is_err());
        let mut disabled = layout;
        disabled.elements[index]
            .legend
            .as_mut()
            .unwrap()
            .categorical = false;
        assert!(validate(&disabled).is_err());
    }

    #[test]
    fn migrate_style_keys_is_one_evaluated_print_intent() {
        let mut layout = default_layout();
        layout
            .style_refs
            .insert("lv_lines".into(), "master/lv_lines_print".into());
        let document = json!({"type":"line","paint":{"line-width":0.6,"line-color":"#64748B"}});
        let resolution = json!({
            "schema": crate::style_resolution::SCHEMA, "project_id": "fixture",
            "revision_id": "b".repeat(64),
            "bindings": [{"key": context::style_key("lv_lines", "saved_design", "project"),
                "style_ref": "master/lv_lines_print", "revision_id": "a".repeat(64),
                "content_sha256": crate::style_plan::style_digest(&document),
                "scope": "global", "document": document}]
        });
        let input = json!({"op":"migrate_style_keys","layout":layout,"resolution":resolution});
        let reply: Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&input).unwrap()).unwrap()).unwrap();
        assert_eq!(reply["plan"]["status"], "migrate");
        assert_eq!(reply["layout"]["style_keys"]["lv_lines"]["role"], "project");
        assert_eq!(reply["layout"]["style_refs"], json!({}));
    }
    #[test]
    fn a_label_may_print_one_named_feature_property() {
        let mut layout = default_layout();
        layout
            .style_refs
            .insert("tr".into(), "master/tr_print".into());
        let mut change = PrintStyleOverride {
            label: Some(PrintLabelOverride {
                field: Some("key_number".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        layout.style_overrides.insert("tr".into(), change.clone());
        validate(&layout).unwrap();
        change.label.as_mut().unwrap().field = Some("key number".into());
        layout.style_overrides.insert("tr".into(), change);
        assert!(validate(&layout).is_err());
    }
    #[test]
    fn table_numbering_is_declared_on_a_bound_layer_with_identifier_keys() {
        let mut layout = default_layout();
        let mut index = elements::new_element(ElementKind::Table, "index".into(), "");
        index.rect = [12., 40., 80., 40.];
        index.table.as_mut().unwrap().layer = "tr".into();
        index.table.as_mut().unwrap().numbering = Some(FeatureNumbering {
            property: "key_number".into(),
            order_by: "name".into(),
        });
        layout.elements.push(index);
        validate(&layout).unwrap();
        let held = serde_json::to_value(&layout).unwrap();
        assert_eq!(
            held["elements"].as_array().unwrap().last().unwrap()["table"]["numbering"],
            json!({"property":"key_number","order_by":"name"})
        );
        let last = layout.elements.len() - 1;
        layout.elements[last]
            .table
            .as_mut()
            .unwrap()
            .numbering
            .as_mut()
            .unwrap()
            .property = "key number".into();
        assert!(validate(&layout).unwrap_err().contains("numbering"));
    }
    #[test]
    fn text_style_is_bounded_to_bundled_faces_and_omitted_when_unset() {
        let mut layout = default_layout();
        let title = layout
            .elements
            .iter()
            .position(|e| e.kind == ElementKind::Text)
            .unwrap();
        let unstyled = serde_json::to_value(&layout).unwrap();
        assert!(unstyled["elements"][title].get("text_style").is_none());
        layout.elements[title].text_style = Some(TextStyle {
            family: FontFamily::Serif,
            bold: true,
            italic: false,
            tracking_em: 0.25,
            ..Default::default()
        });
        validate(&layout).unwrap();
        let held = serde_json::to_value(&layout).unwrap();
        assert_eq!(
            held["elements"][title]["text_style"],
            json!({"family":"serif","bold":true,"tracking_em":0.25})
        );
        assert_eq!(serde_json::from_value::<Layout>(held).unwrap(), layout);
        layout.elements[title].text_style.as_mut().unwrap().italic = true;
        assert!(validate(&layout).unwrap_err().contains("serif italic"));
        layout.elements[title].text_style = Some(TextStyle {
            tracking_em: 0.6,
            ..Default::default()
        });
        assert!(validate(&layout).unwrap_err().contains("tracking_em"));
        layout.elements[title].text_style = None;
        let frame = layout
            .elements
            .iter()
            .position(|e| e.kind != ElementKind::Text)
            .unwrap();
        layout.elements[frame].text_style = Some(TextStyle::default());
        assert!(validate(&layout).unwrap_err().contains("not text"));
    }
    #[test]
    fn title_halo_is_authored_in_physical_units_and_has_no_implicit_ink() {
        let mut layout = default_layout();
        let title = layout
            .elements
            .iter()
            .position(|e| e.kind == ElementKind::Text)
            .unwrap();
        layout.elements[title].text_style = Some(TextStyle {
            halo: Some(TextHalo {
                color: "#EEF1F0".into(),
                width_mm: 0.4,
            }),
            ..Default::default()
        });
        validate(&layout).unwrap();
        let held = serde_json::to_value(&layout).unwrap();
        assert_eq!(
            held["elements"][title]["text_style"],
            json!({"halo":{"color":"#EEF1F0","width_mm":0.4}})
        );
        assert_eq!(serde_json::from_value::<Layout>(held).unwrap(), layout);
        for width_mm in [-0.1, 5.1, f64::NAN, f64::INFINITY] {
            layout.elements[title]
                .text_style
                .as_mut()
                .unwrap()
                .halo
                .as_mut()
                .unwrap()
                .width_mm = width_mm;
            assert!(validate(&layout).unwrap_err().contains("text halo"));
        }
        layout.elements[title].text_style.as_mut().unwrap().halo = Some(TextHalo {
            color: "url(#ink)".into(),
            width_mm: 0.4,
        });
        assert!(validate(&layout).unwrap_err().contains("text halo"));
    }
    #[test]
    fn layout_ink_identity_is_closed_and_preserves_authored_pens() {
        let colour = default_layout();
        let mut value = serde_json::to_value(&colour).unwrap();
        assert!(value.get("ink_mode").is_none());
        value["ink_mode"] = json!("monochrome");
        let monochrome: Layout = serde_json::from_value(value.clone()).unwrap();
        validate(&monochrome).unwrap();
        assert_eq!(
            monochrome.ink_mode,
            crate::style_resolution::Ink::Monochrome
        );
        assert_eq!(monochrome.styles, colour.styles);
        assert_eq!(monochrome.elements, colour.elements);
        assert_eq!(
            serde_json::to_value(&monochrome).unwrap()["ink_mode"],
            "monochrome"
        );
        value["ink_mode"] = json!("gray");
        assert!(serde_json::from_value::<Layout>(value.clone()).is_err());
        value["ink_mode"] = json!("colour");
        value["ink"] = json!("monochrome");
        assert!(serde_json::from_value::<Layout>(value)
            .unwrap_err()
            .to_string()
            .contains("unknown field"));
    }

    #[test]
    fn metric_corridor_roundtrips_and_rejects_invalid_distance() {
        let mut layout = default_layout();
        let source = serde_json::json!({"kind":"project_dsgrid_mv","buffer_m":200.,
            "corridor":{"distance_m":6.,"color":"#957197","width_mm":0.15,"opacity":0.3,"dash_mm":[2.,1.]}});
        layout.context_layers.push(PrintContextLayer {
            id: "mv".into(),
            label: "MV".into(),
            source: serde_json::from_value(source.clone()).unwrap(),
        });
        validate(&layout).unwrap();
        assert_eq!(
            serde_json::to_value(&layout.context_layers[0].source).unwrap(),
            source
        );
        for invalid in [0., -6., 1001.] {
            if let PrintContextSource::ProjectDsgridMv {
                corridor: Some(c), ..
            } = &mut layout.context_layers[0].source
            {
                c.distance_m = invalid;
            }
            assert!(validate(&layout).unwrap_err().contains("corridor"));
        }
    }

    #[test]
    fn viewport_fit_around_roundtrips_and_rejects_missing_or_duplicate_elements() {
        let mut layout = default_layout();
        let anchor = layout.elements[1].id.clone();
        layout.elements[0].viewport.as_mut().unwrap().fit_around = vec![anchor.clone()];
        validate(&layout).unwrap();
        let decoded: Layout =
            serde_json::from_value(serde_json::to_value(&layout).unwrap()).unwrap();
        assert_eq!(decoded, layout);
        for ids in [
            vec!["missing".into()],
            vec!["map".into()],
            vec![anchor.clone(), anchor],
        ] {
            layout.elements[0].viewport.as_mut().unwrap().fit_around = ids;
            assert!(validate(&layout).unwrap_err().contains("fit-around"));
        }
    }

    #[test]
    fn content_flow_requires_an_earlier_content_frame() {
        let mut layout = default_layout();
        let legend = layout
            .elements
            .iter()
            .position(|e| e.kind == ElementKind::Legend)
            .unwrap();
        let mut table = layout.elements[legend].clone();
        table.id = "quantities".into();
        table.kind = ElementKind::Table;
        table.legend = None;
        table.table = Some(TableBinding {
            layer: "info".into(),
            additional_layers: vec!["tapping_poles".into()],
            columns: vec!["quantity".into()],
            headings: vec!["Quantity".into()],
            widths: vec![10.],
            panels: 1,
            gap_mm: 3.,
            ..Default::default()
        });
        layout.elements.insert(legend, table);
        layout.elements[legend + 1].flow = Some(ContentFlow {
            after: "quantities".into(),
            placement: ContentPlacement::default(),
            gap_mm: 5.,
        });
        validate(&layout).unwrap();
        let encoded = serde_json::to_vec(&layout).unwrap();
        assert_eq!(serde_json::from_slice::<Layout>(&encoded).unwrap(), layout);
        for invalid in [
            vec!["info".into()],
            vec!["a".into(), "a".into()],
            vec!["../bad".into()],
        ] {
            let mut invalid_layout = layout.clone();
            invalid_layout.elements[legend]
                .table
                .as_mut()
                .unwrap()
                .additional_layers = invalid;
            assert!(validate(&invalid_layout).is_err());
        }
        layout.elements[legend + 1].flow.as_mut().unwrap().after = "legend".into();
        assert!(validate(&layout).unwrap_err().contains("earlier"));
        layout.elements[legend + 1].flow.as_mut().unwrap().after = "map".into();
        assert!(validate(&layout).is_err());
        layout.elements[legend + 1].flow.as_mut().unwrap().after = "quantities".into();
        layout.elements[legend + 1].flow.as_mut().unwrap().gap_mm = -1.;
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn measured_schedule_and_overview_controls_roundtrip_and_refuse_conflicts() {
        let mut layout = default_layout();
        let mut table = layout
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Legend)
            .unwrap()
            .clone();
        table.id = "schedule".into();
        table.kind = ElementKind::Table;
        table.legend = None;
        table.table = Some(TableBinding {
            layer: "lv_poles".into(),
            columns: vec!["pole_number".into(), "struct_type".into()],
            headings: vec!["Pole".into(), "Type".into()],
            widths: vec![20., 40.],
            panels: 3,
            row_mm: Some(3.5),
            sort_by: vec!["pole_number".into()],
            ..Default::default()
        });
        layout.elements.push(table);
        layout
            .elements
            .iter_mut()
            .find(|e| e.kind == ElementKind::Map)
            .unwrap()
            .viewport
            .as_mut()
            .unwrap()
            .fit_around
            .push("schedule".into());
        layout.composition = Some(Composition {
            movable: vec!["schedule".into()],
            gap_mm: 5.,
            min_font_scale: 1.,
            collision_margin_mm: 1.,
        });
        validate(&layout).unwrap();
        assert_eq!(
            serde_json::from_slice::<Layout>(&serde_json::to_vec(&layout).unwrap()).unwrap(),
            layout
        );
        layout
            .composition
            .as_mut()
            .unwrap()
            .movable
            .push("schedule".into());
        assert!(validate(&layout).is_err());
        layout.composition.as_mut().unwrap().movable.pop();
        layout.composition.as_mut().unwrap().min_font_scale = 0.69;
        assert!(validate(&layout).is_err());
        layout.composition.as_mut().unwrap().min_font_scale = 0.7;
        validate(&layout).unwrap();
        layout
            .elements
            .last_mut()
            .unwrap()
            .table
            .as_mut()
            .unwrap()
            .panels = 0;
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn print_sources_focus_and_order_are_closed_and_bounded() {
        let mut l = default_layout();
        l.context_layers = vec![
            PrintContextLayer {
                id: "survey_poles".into(),
                label: "Surveyed poles".into(),
                source: PrintContextSource::Survey {
                    form: "lv_poles_as_built".into(),
                },
            },
            PrintContextLayer {
                id: "local".into(),
                label: "Local geometry".into(),
                source: PrintContextSource::LocalLayer {
                    layer_id: "local-123".into(),
                },
            },
        ];
        l.layer_order = vec!["contours".into(), "roads".into(), "lv_lines".into()];
        l.focus = Some(PrintFocus {
            visible: true,
            buffer_m: 35.,
            outside_color: "#E4E7E9".into(),
            outside_opacity: 0.6,
            border_color: "#7C8792".into(),
            border_width_mm: 0.25,
            border_opacity: Some(0.2),
        });
        validate(&l).unwrap();
        assert_eq!(
            serde_json::from_slice::<Layout>(&serde_json::to_vec(&l).unwrap()).unwrap(),
            l
        );
        l.layer_order.push("roads".into());
        assert!(validate(&l).is_err());
        l.layer_order.pop();
        l.focus.as_mut().unwrap().border_opacity = Some(1.1);
        assert!(validate(&l).is_err());
        l.focus.as_mut().unwrap().border_opacity = None;
        l.focus.as_mut().unwrap().buffer_m = -1.;
        assert!(validate(&l).is_err());
    }

    fn evaluated_layout(request: serde_json::Value) -> Layout {
        let result = evaluate(&serde_json::to_vec(&request).unwrap()).unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        serde_json::from_value(result["layout"].clone()).unwrap()
    }

    #[test]
    fn default_and_roundtrip_are_valid_and_unknown_fields_refuse() {
        let layout = default_layout();
        validate(&layout).unwrap();
        let encoded =
            serde_json::to_vec(&serde_json::json!({"op":"validate","layout":layout})).unwrap();
        assert!(evaluate(&encoded).unwrap().contains("\"valid\":true"));
        assert!(evaluate(br#"{"op":"new","unknown":true}"#).is_err());
        assert_eq!(layout_schema()["additionalProperties"], false);
    }

    #[test]
    fn transformer_views_validate_in_the_shared_printing_kernel() {
        let result = evaluate(br#"{"op":"transformer_views","views":{"tx_1":{"primary_map":{"mode":"camera","center_wgs84":[29.7,-2.5],"scale_denominator":1000}}}}"#).unwrap();
        assert!(result.contains("\"scale_denominator\":1000.0"));
        assert!(evaluate(br#"{"op":"transformer_views","views":{"TX 1":{}}}"#).is_err());
        assert!(evaluate(br#"{"op":"transformer_views","views":{"TX_1":{}}}"#).is_err());
        assert!(evaluate(br#"{"op":"transformer_views","views":{"tx_1":{"map":{"mode":"fit","extent_wgs84":[30,-2,29,-1]}}}}"#).is_err());
    }

    #[test]
    fn a_render_plan_admits_a_preview_only_with_its_styles_and_some_map_data() {
        let mut layout: serde_json::Value =
            serde_json::from_str(&evaluate(br#"{"op":"new","paper":"A3"}"#).expect("a new layout"))
                .expect("layout json");
        let layout = layout["layout"].take();
        let mut with_style = layout.clone();
        with_style["style_refs"] =
            serde_json::json!({"lv_lines": "lv_lines_print", "poles": "poles_print"});
        let plan = |layout: &serde_json::Value,
                    refs: serde_json::Value,
                    count: usize|
         -> serde_json::Value {
            let request = serde_json::json!({"op":"render_plan","layout":layout,"available_style_refs":refs,"layer_count":count,"text":{"title":"T","project":"P","drawing":"D","source":"S"},"date":"2026-09-11"});
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).expect("plan"))
                .unwrap()
        };
        let ready = plan(
            &with_style,
            serde_json::json!(["lv_lines_print", "poles_print", "other"]),
            2,
        );
        assert_eq!(ready["ready"], true);
        assert_eq!(
            ready["print_style_refs"],
            serde_json::json!(["lv_lines_print", "poles_print"])
        );
        assert_eq!(ready["text"]["date"], "2026-09-11");
        assert_eq!(ready["text"]["title"], "T");
        let missing = plan(&with_style, serde_json::json!(["lv_lines_print"]), 2);
        assert_eq!(missing["ready"], false);
        assert_eq!(missing["refusals"][0]["code"], "print_style_unavailable");
        assert_eq!(missing["refusals"][0]["params"]["reference"], "poles_print");
        let empty = plan(&layout, serde_json::json!([]), 0);
        assert_eq!(empty["refusals"][0]["code"], "print_preview_needs_layers");
        assert_eq!(empty["print_style_refs"], serde_json::json!([]));
    }

    #[test]
    fn native_inventory_excludes_reserved_names_and_marks_unobserved_rooms_unknown() {
        let request = json!({"op":"printable_transformers","rows":[
            {"name":"combined_transformer"},{"name":"MV_DATA","kind":"individual"},
            {"name":"collisions"},{"name":"compounded_report"},{"name":"  "},
            {"name":"tx_a","metadata":{"version":"0x10"}}
        ],"limit":10});
        let out: serde_json::Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
                .unwrap();
        assert_eq!(out["total"], 1);
        assert_eq!(out["local_rooms_known"], false);
        assert_eq!(out["transformers"][0]["server_version"], 16.0);
        assert!(out["transformers"][0]["cached"].is_null());
        assert!(out["transformers"][0]["dirty"].is_null());
    }

    #[test]
    fn a_printable_inventory_is_one_command() {
        let request = serde_json::json!({"op":"printable_transformers","rows":[{"name":"b"},{"name":"a","kind":"individual"}],"rooms":[{"name":"A","owner_email":null,"local_dirty":true}],"current_user":"","limit":1});
        let out: serde_json::Value = serde_json::from_str(
            &evaluate(&serde_json::to_vec(&request).unwrap()).expect("inventory"),
        )
        .unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["transformers"][0]["name"], "a");
        assert_eq!(out["transformers"][0]["dirty"], true);
        assert_eq!(out["more"]["omitted"], 1);
        assert!(evaluate(br#"{"op":"printable_transformers","rows":[],"limit":0}"#).is_err());
    }

    #[test]
    fn a_project_output_patch_writes_the_export_row_and_the_reporter_view_nesting() {
        let request = serde_json::json!({"op":"project_output_patch",
            "current":[{"parameter":"tr-export-formats","value":"shp"},
                       {"parameter":"printing_views","value":{"tx_1":{"other":{"held":true}}}}],
            "selection":{"schema":"ds.design-output-selection/v1","prints":[
                {"layout_id":"detail","formats":["pdf"]}],"geospatial":["gpkg"],"tabular":[],
                "execution":{"pdf__detail":["desktop"]}},
            "layout_id":"detail",
            "views":{"tx_1":{"primary_map":{"mode":"camera","center_wgs84":[29.7,-2.5],"scale_denominator":1000}}}});
        let result: serde_json::Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
                .unwrap();
        let rows = result["project_settings"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "no row is added when both already exist");
        assert_eq!(rows[0]["parameter"], "tr-export-formats");
        assert_eq!(
            rows[0]["value"]["execution"],
            serde_json::json!({"pdf__detail":["desktop"]})
        );
        assert_eq!(rows[0]["value"]["geospatial"], serde_json::json!(["gpkg"]));
        // The layout is nested under the transformer, beside what was held.
        assert_eq!(
            rows[1]["value"]["tx_1"]["other"],
            serde_json::json!({"held":true})
        );
        assert_eq!(
            rows[1]["value"]["tx_1"]["detail"]["primary_map"]["scale_denominator"],
            serde_json::json!(1000.0)
        );

        // A sheet with neither row gets both, under the first alias.
        let bare = serde_json::json!({"op":"project_output_patch","current":[],
            "selection":{"schema":"ds.design-output-selection/v1","geospatial":["gpkg"]},
            "layout_id":"detail",
            "views":{"tx_1":{"primary_map":{"mode":"camera","center_wgs84":[29.7,-2.5],"scale_denominator":1000}}}});
        let created: serde_json::Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&bare).unwrap()).unwrap()).unwrap();
        let rows = created["project_settings"].as_array().unwrap();
        assert_eq!(rows[0]["parameter"], "design_export_format");
        assert_eq!(rows[1]["parameter"], "printing_views");

        // Views need the layout they were authored on, and are validated by
        // the same rule `transformer_views` applies.
        let mut missing = request.clone();
        missing["layout_id"] = serde_json::Value::Null;
        assert!(evaluate(&serde_json::to_vec(&missing).unwrap()).is_err());
        let mut invalid = request;
        invalid["views"] = serde_json::json!({"TX 1":{}});
        assert!(evaluate(&serde_json::to_vec(&invalid).unwrap()).is_err());
        assert!(evaluate(br#"{"op":"project_output_patch","current":[],"selection":{"schema":"ds.design-output-selection/v1"},"unknown":1}"#).is_err());
    }

    #[test]
    fn lifecycle_plan_is_one_printing_op_over_host_facts() {
        let plan: serde_json::Value = serde_json::from_str(
            &evaluate(
                br#"{"op":"lifecycle_plan","facts":{"intent":"save","library":"project","online":true,"editing":true,"loaded_matches":true,"held_revision":"abc"}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(plan["decision"], "update");
        assert_eq!(plan["expected_revision"], "abc");
        let refused: serde_json::Value = serde_json::from_str(
            &evaluate(
                br#"{"op":"lifecycle_plan","facts":{"intent":"read","library":"global","online":false}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(refused["decision"], "refuse");
        assert_eq!(refused["refusal"]["message_key"], "printing_no_cached_copy");
        assert!(evaluate(
            br#"{"op":"lifecycle_plan","facts":{"intent":"read","library":"global","surprise":1}}"#
        )
        .is_err());
    }

    #[test]
    fn output_selection_reads_whatever_the_project_stored_and_answers_placements() {
        let result: serde_json::Value = serde_json::from_str(
            &evaluate(br#"{"op":"output_selection","stored":"png__detail,xlsx"}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(result["selection"]["prints"][0]["layout_id"], "detail");
        assert_eq!(
            result["selection"]["prints"][0]["formats"],
            serde_json::json!(["png"])
        );
        assert_eq!(
            result["placements"],
            serde_json::json!({"xlsx":["desktop","web"],"png__detail":["desktop"]})
        );
        let restricted: serde_json::Value = serde_json::from_str(&evaluate(
            br#"{"op":"output_selection","stored":{"schema":"ds.design-output-selection/v1","geospatial":["gpkg"],"execution":{"geospatial":["web"]}}}"#,
        ).unwrap()).unwrap();
        assert_eq!(
            restricted["placements"],
            serde_json::json!({"gpkg":["web"]})
        );
        assert_eq!(
            evaluate(br#"{"op":"output_selection","stored":"gpkg","legacy_formats":["xlsx"]}"#),
            Err("pass a stored settings value alone, without a selection, legacy formats or placements".into())
        );
        // A row that holds `null` is a project that stored nothing, and reads
        // as an empty selection. Passing no `stored` at all is a different
        // request, and without an authoring input it is still refused.
        for empty in [
            br#"{"op":"output_selection","stored":null}"#.as_slice(),
            br#"{"op":"output_selection","stored":""}"#.as_slice(),
        ] {
            let read: serde_json::Value = serde_json::from_str(&evaluate(empty).unwrap()).unwrap();
            assert_eq!(read["selection"]["geospatial"], serde_json::json!([]));
            assert_eq!(read["placements"], serde_json::json!({}));
        }
        assert!(evaluate(br#"{"op":"output_selection"}"#).is_err());
    }

    /// The reply states the voltage-drop layout whether or not the selection
    /// carries it, and the settings patch writes a chosen `brief` through.
    #[test]
    fn output_selection_answers_the_voltage_drop_layout_and_the_patch_keeps_it() {
        let read = |request: &[u8]| -> serde_json::Value {
            serde_json::from_str(&evaluate(request).unwrap()).unwrap()
        };
        let default = read(br#"{"op":"output_selection","stored":"shp,voltage_drop"}"#);
        assert_eq!(default["voltage_drop_report"], "extended");
        assert!(default["selection"]["voltage_drop_report"].is_null());
        let brief = read(
            br#"{"op":"output_selection","selection":{"schema":"ds.design-output-selection/v1","tabular":["voltage_drop"],"voltage_drop_report":"brief"}}"#,
        );
        assert_eq!(brief["voltage_drop_report"], "brief");
        assert_eq!(brief["selection"]["voltage_drop_report"], "brief");
        let refused = evaluate(
            br#"{"op":"output_selection","stored":{"schema":"ds.design-output-selection/v1","voltage_drop_report":"summary"}}"#,
        )
        .unwrap_err();
        assert!(
            refused.contains("unknown voltage-drop report format `summary`"),
            "{refused}"
        );

        let patched = read(
            br#"{"op":"project_output_patch","current":[{"parameter":"design_export_format","value":"shp"}],"selection":{"schema":"ds.design-output-selection/v1","tabular":["voltage_drop"],"voltage_drop_report":"brief"}}"#,
        );
        assert_eq!(
            patched["project_settings"][0]["value"]["voltage_drop_report"],
            "brief"
        );
    }

    #[test]
    fn output_selection_can_require_the_layout_being_authored() {
        assert!(evaluate(
            br#"{"op":"output_selection","legacy_formats":["pdf__a3"],"required_layout":"a3"}"#
        )
        .is_ok());
        assert!(evaluate(
            br#"{"op":"output_selection","legacy_formats":["xlsx"],"required_layout":"a3"}"#
        )
        .is_err());
    }
    #[test]
    fn minimum_column_widths_are_ordered_and_bounded_by_caps() {
        let mut layout = default_layout();
        layout.elements.push(serde_json::from_value(serde_json::json!({"id":"info","kind":"table","rect":[20,20,100,100],
            "table":{"layer":"info","columns":["quantity"],"headings":["Quantity"],"widths":[1],"min_widths_mm":[6],"max_widths_mm":[10]}})).unwrap());
        validate(&layout).unwrap();
        for invalid in [vec![11.], vec![-1.], vec![f64::NAN], vec![6., 6.]] {
            layout
                .elements
                .last_mut()
                .unwrap()
                .table
                .as_mut()
                .unwrap()
                .min_widths_mm = invalid;
            assert!(validate(&layout).is_err());
        }
    }

    #[test]
    fn table_heading_measurement_is_optional_and_roundtrips() {
        let mut value = serde_json::json!({"layer":"customers","columns":["service_length"],"headings":["Service length"],"widths":[1]});
        let default: TableBinding = serde_json::from_value(value.clone()).unwrap();
        assert!(default.measure_headings);
        value["measure_headings"] = serde_json::json!(false);
        let data_only: TableBinding = serde_json::from_value(value).unwrap();
        assert!(!data_only.measure_headings);
        assert_eq!(
            serde_json::to_value(data_only).unwrap()["measure_headings"],
            false
        );
    }

    #[test]
    fn label_offsets_are_physical_bounded_and_roundtrip() {
        let mut layout = default_layout();
        layout
            .style_refs
            .insert("spans".into(), "master/spans_print".into());
        layout.style_overrides.insert(
            "spans".into(),
            serde_json::from_value(json!({"label":{"offset_mm":[0,-1.4]}})).unwrap(),
        );
        validate(&layout).unwrap();
        assert_eq!(
            serde_json::from_value::<Layout>(serde_json::to_value(&layout).unwrap()).unwrap(),
            layout
        );
        layout
            .style_overrides
            .get_mut("spans")
            .unwrap()
            .label
            .as_mut()
            .unwrap()
            .offset_mm = Some([0., 21.]);
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn invalid_frames_and_unheld_logos_refuse() {
        let mut layout = default_layout();
        layout.elements[0].rect[2] = 1000.;
        assert!(validate(&layout).is_err());
        layout = default_layout();
        layout.elements[0].rect[0] = f64::NAN;
        assert!(validate(&layout).is_err());
        layout = default_layout();
        layout.elements[1].kind = ElementKind::Logo;
        layout.elements[1].asset = "missing".into();
        assert!(validate(&layout).is_err());
        layout = default_layout();
        layout.assets.insert(
            "remote".into(),
            Asset {
                label: "Remote".into(),
                png_data_url: "https://example.test/logo.png".into(),
            },
        );
        assert!(validate(&layout).is_err());
    }
    /// Front matter: the default sheet with every map-bound element removed.
    fn map_free_page() -> Layout {
        let mut layout = default_layout();
        layout.id = "atlas-front-matter".into();
        layout.elements.retain(|e| {
            !matches!(
                e.kind,
                ElementKind::Map
                    | ElementKind::Legend
                    | ElementKind::NorthArrow
                    | ElementKind::ScaleBar
            )
        });
        layout
    }

    #[test]
    fn map_free_cover_and_convention_pages_are_admitted_as_front_matter() {
        let mut cover = map_free_page();
        cover.assets.insert(
            "logo-a".into(),
            Asset {
                label: "a.png".into(),
                png_data_url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ"
                    .into(),
            },
        );
        cover.elements.extend([
            serde_json::from_value(
                json!({"id":"cover_frame","kind":"rectangle","rect":[12,40,396,200]}),
            )
            .unwrap(),
            serde_json::from_value(
                json!({"id":"logo","kind":"logo","rect":[20,50,40,20],"asset":"logo-a"}),
            )
            .unwrap(),
        ]);
        let mut convention = map_free_page();
        convention.elements.push(
            serde_json::from_value(json!({
                "id":"names","kind":"table","rect":[20,50,200,180],"font_pt":8,
                "table":{"layer":"names","columns":["code","meaning"],"headings":["Code","Meaning"],
                    "widths":[1,3],"panels":1,"row_mm":5,"align":"left"}
            }))
            .unwrap(),
        );
        for page in [cover, convention] {
            assert!(page.elements.iter().all(|e| e.kind != ElementKind::Map));
            validate(&page).unwrap();
            // The command surface admits it unchanged: no stand-in map added.
            assert_eq!(
                evaluated_layout(json!({"op":"validate","layout":page})),
                page
            );
            // A host that always projects still refuses it.
            assert_eq!(
                validate_geographic(&page).unwrap_err(),
                "a geographic sheet needs a map frame; layout atlas-front-matter has none"
            );
        }
    }

    #[test]
    fn map_free_page_refuses_what_only_a_map_can_honour() {
        let refusal = |layout: &Layout| validate(layout).unwrap_err();
        let donor = default_layout();
        let with_element = |kind: ElementKind, id: &str| {
            let mut page = map_free_page();
            page.elements.push(Element {
                id: id.into(),
                ..donor
                    .elements
                    .iter()
                    .find(|e| e.kind == kind)
                    .unwrap()
                    .clone()
            });
            page
        };
        assert_eq!(
            refusal(&with_element(ElementKind::NorthArrow, "north")),
            "north arrow north needs a map frame; layout atlas-front-matter has none"
        );
        assert_eq!(
            refusal(&with_element(ElementKind::ScaleBar, "scale")),
            "scale bar scale needs a map frame; layout atlas-front-matter has none"
        );
        let mut scale_text = map_free_page();
        scale_text.elements[0].text = "{title} {scale}".into();
        assert_eq!(
            refusal(&scale_text),
            "text title binding {scale} needs a map frame; layout atlas-front-matter has none"
        );
        type LayoutMutation = (&'static str, fn(&mut Layout));
        let options: [LayoutMutation; 6] = [
            ("a focus area", |l| {
                l.focus = Some(
                    serde_json::from_value(json!({"buffer_m":50.,"outside_color":"#ffffff",
                        "outside_opacity":0.5,"border_color":"#000000","border_width_mm":0.3}))
                    .unwrap(),
                )
            }),
            ("automatic furniture composition", |l| {
                l.composition = Some(Composition {
                    movable: vec!["title".into()],
                    gap_mm: 5.,
                    min_font_scale: 1.,
                    collision_margin_mm: 1.,
                })
            }),
            ("network fitting", |l| {
                l.network_fit = Some(
                    serde_json::from_value(json!({"source_layers":["roads"],"clearance_mm":2.}))
                        .unwrap(),
                )
            }),
            ("a map scale limit", |l| {
                l.scale_limits = Some(
                    serde_json::from_value(json!({"min_denominator":500.,"max_denominator":5000.}))
                        .unwrap(),
                )
            }),
            ("map scale rounding", |l| l.scale_rounding = Some(500.)),
            ("transformer bounds", |l| {
                l.transformer_bounds = Some(TransformerBounds {
                    source_layers: vec!["poles".into()],
                    identity_field: "transfo".into(),
                    buffer_m: 10.,
                })
            }),
        ];
        for (what, set) in options {
            let mut page = map_free_page();
            set(&mut page);
            assert_eq!(
                refusal(&page),
                format!("{what} needs a map frame; layout atlas-front-matter has none")
            );
        }
    }

    #[test]
    fn legacy_layout_upgrades_to_explicit_paper_and_clipped_viewport() {
        let mut legacy = serde_json::to_value(default_layout()).unwrap();
        legacy["schema"] = json!(LEGACY_SCHEMA);
        legacy.as_object_mut().unwrap().remove("paper");
        legacy["elements"][0]
            .as_object_mut()
            .unwrap()
            .remove("viewport");
        let layout = evaluated_layout(json!({"op":"validate","layout":legacy}));
        assert_eq!(layout.schema, SCHEMA);
        assert_eq!(layout.paper.size, Paper::A3);
        assert_eq!(layout.paper.orientation, Orientation::Landscape);
        assert_eq!(
            layout.elements[0].viewport.as_ref().unwrap().view_slot,
            "primary_map"
        );
        assert!(layout.elements[0].viewport.as_ref().unwrap().clip);

        let mut custom = layout;
        custom.schema = LEGACY_SCHEMA.into();
        custom.page_mm = [1000., 700.];
        custom.paper = PaperSpec::default();
        custom.elements[0].viewport = None;
        let upgraded = upgrade(custom).unwrap();
        assert_eq!(
            upgraded.paper,
            PaperSpec {
                size: Paper::Custom {
                    short_mm: 700.,
                    long_mm: 1000.
                },
                orientation: Orientation::Landscape
            }
        );
    }

    #[test]
    fn category_sizes_need_a_field_and_scale_rounding_is_bounded() {
        let mut layout = default_layout();
        layout
            .style_refs
            .insert("roads".into(), "gt/roads_print".into());
        let mut change = PrintStyleOverride::default();
        change.size_by_value.insert("National Road".into(), 4.4);
        layout.style_overrides.insert("roads".into(), change);
        assert!(validate(&layout).is_err());
        layout.style_overrides.get_mut("roads").unwrap().field = Some("type".into());
        layout.scale_rounding = Some(100.);
        validate(&layout).unwrap();
        layout.scale_rounding = Some(0.);
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn template_label_overrides_validate_physical_limits() {
        let mut layout = default_layout();
        layout
            .style_refs
            .insert("roads".into(), "gt/roads_print".into());
        let mut change = PrintStyleOverride {
            label: Some(PrintLabelOverride {
                size_pt: Some(8.),
                ..Default::default()
            }),
            ..Default::default()
        };
        layout
            .style_overrides
            .insert("roads".into(), change.clone());
        validate(&layout).unwrap();
        change.label.as_mut().unwrap().size_pt = Some(0.);
        layout.style_overrides.insert("roads".into(), change);
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn a_label_only_administrative_boundary_is_a_valid_print_style() {
        let mut layout = default_layout();
        let mut style = PrintStyle {
            width_mm: 0.,
            stroke_opacity: 0.,
            label_field: "village".into(),
            ..Default::default()
        };
        layout
            .styles
            .insert("village_boundaries".into(), style.clone());
        // Zero width is the governed "labelled but never drawn" state, not a
        // refusal the operator has to work around with a 0.1 mm hairline.
        validate(&layout).expect("label-only administrative boundary");
        // A positive width still obeys the physical minimum.
        for refused in [0.01, -1., 10.5, f64::NAN] {
            style.width_mm = refused;
            layout
                .styles
                .insert("village_boundaries".into(), style.clone());
            assert!(
                validate(&layout).is_err(),
                "width {refused} must stay refused"
            );
        }
        style.width_mm = 0.;
        style.widths = BTreeMap::from([("built".into(), 0.), ("planned".into(), 0.4)]);
        layout
            .styles
            .insert("village_boundaries".into(), style.clone());
        validate(&layout).expect("a per-category width may also suppress its stroke");
        style.widths.insert("planned".into(), 0.01);
        layout.styles.insert("village_boundaries".into(), style);
        assert!(validate(&layout).is_err());
    }

    #[test]
    fn phase_selector_and_physical_hatch_bounds_are_explicit() {
        let mut hatch = PrintHatch {
            color: "#996633".into(),
            width_mm: 0.2,
            spacing_mm: 2.,
            angle_deg: 45.,
            match_field: "phase".into(),
            match_values: vec!["ii".into()],
        };
        assert!(hatch.validate().is_ok());
        assert!(hatch.matches(&serde_json::json!({"phase":"ii"})));
        assert!(!hatch.matches(&serde_json::json!({"phase":"i"})));
        assert!(!hatch.matches(&serde_json::json!({})));
        hatch.spacing_mm = 0.1;
        assert!(hatch.validate().is_err());
        let mut layout = default_layout();
        layout.transformer_bounds = Some(TransformerBounds {
            source_layers: vec!["tr".into(), "tr".into()],
            identity_field: "transfo".into(),
            buffer_m: 10.,
        });
        assert!(validate(&layout).is_err());
    }
    #[test]
    fn attribution_is_one_closed_layout_field_and_validated_by_every_reader() {
        let mut document = serde_json::to_value(default_layout()).unwrap();
        document["attribution"] = json!({"font_pt":7.,"color":"#595959","svg_font_family":"DejaVu Sans","pdf_font_family":"Helvetica","right_inset_pt":146.,"baseline_pt":7.});
        let layout: Layout = serde_json::from_value(document.clone()).unwrap();
        validate(&layout).unwrap();
        let canonical = serde_json::to_value(upgrade(layout).unwrap()).unwrap();
        assert_eq!(canonical["attribution"], document["attribution"]);
        let schema = layout_schema();
        assert!(schema["properties"]["attribution"].is_object());
        document["attribution"]["font_pts"] = json!(8.);
        assert!(serde_json::from_value::<Layout>(document.clone()).is_err());
        document["attribution"]
            .as_object_mut()
            .unwrap()
            .remove("font_pts");
        document["attribution"]["font_pt"] = json!(0.);
        assert!(validate(&serde_json::from_value(document).unwrap())
            .unwrap_err()
            .contains("print_attribution_invalid"));
    }
}
#[test]
fn monochrome_layout_cannot_resolve_a_colour_only_key() {
    let document = json!({"type":"line","paint":{"line-color":"#123456","line-width":1}});
    let key: crate::style_resolution::Key = serde_json::from_value(json!({"entity_class":"test_line","source_kind":"project_model","target":"print","role":"project"})).unwrap();
    let table: crate::style_resolution::Snapshot = serde_json::from_value(json!({
            "schema":"ds.style-resolution/v1","project_id":"test","revision_id":"a".repeat(64),
            "bindings":[{"key":key,"style_ref":"test/line_print","revision_id":"b".repeat(64),
                "content_sha256":crate::style_plan::style_digest(&document),"scope":"global","document":document}]
        })).unwrap();
    let mut layout = default_layout();
    layout.style_keys.insert("line".into(), key);
    layout.ink_mode = crate::style_resolution::Ink::Monochrome;
    let refusal = resolve_style_documents(layout, &table).unwrap_err();
    assert!(refusal.contains("style_resolution_unknown"));
    assert!(refusal.contains("monochrome"));
}
