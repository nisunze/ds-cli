//! MV publication setup in the shared printing library. This owns documents,
//! selection and inheritance, never model geometry or engineering decisions.
use super::{
    mv_settings::{SheetFormat, SheetSettings},
    ElementKind, Layout,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const SETTING: &str = "mv_printing_setup";
pub const SELECTION_SCHEMA: &str = "ds.mv-print-selection/v1";
pub const STANDARD: &str = "mv-a3-plan-profile/v1";

/// The publication's requested ink is one dimension of the kernel/API key.
pub fn paper_style_key(settings: &SheetSettings) -> crate::style_resolution::Key {
    crate::style_resolution::Key {
        entity_class: "mv_booklet".into(),
        source_kind: "project_model".into(),
        target: crate::style_resolution::Target::Print,
        role: "project".into(),
        ink: match settings.ink_mode {
            super::mv_settings::InkMode::Monochrome => crate::style_resolution::Ink::Monochrome,
            super::mv_settings::InkMode::ReferenceAccents => crate::style_resolution::Ink::Colour,
        },
    }
}

/// Resolve paper and companion policy through the same project-fenced selector.
/// The semantic paper key belongs to the publication family, never to a host.
pub fn resolve_print_bindings(
    setup: &Resolved,
    snapshot: &crate::style_resolution::Snapshot,
) -> Result<
    (
        crate::style_resolution::Binding,
        crate::style_resolution::Binding,
    ),
    String,
> {
    if snapshot.project_id != setup.project_id {
        return Err("project_context_changed: MV style table belongs to another project".into());
    }
    let key = paper_style_key(&setup.settings);
    let paper =
        crate::style_resolution::resolve(snapshot, &key).map_err(|error| error.to_string())?;
    let renderer = crate::style_resolution::resolve(snapshot, &super::renderer_defaults::key())
        .map_err(|error| error.to_string())?;
    super::renderer_defaults::admit(&renderer)?;
    Ok((paper, renderer))
}

/// Canonize approved front matter in the existing layout library. Adoption
/// remains the library owner's revision-fenced copy; this is a pure authoring
/// intent, not a parallel registry or a publication.
pub fn standardize(mut layout: Layout) -> Result<Layout, String> {
    use super::mv_booklet::PageRole;
    use super::mv_settings::*;
    let template = layout.mv.as_mut().ok_or(
        "mv_print_standard_invalid: an approved MV layout with cover and naming is required",
    )?;
    if template.pages.len() < 2
        || template.pages[..2]
            .iter()
            .any(|page| matches!(page, Page::PlanProfile | Page::Generated { .. }))
    {
        return Err(
            "mv_print_standard_invalid: preserve the approved cover and naming pages first".into(),
        );
    }
    let mut pages = template.pages[..2].to_vec();
    pages.extend([
        Page::Generated {
            role: PageRole::SheetIndex,
        },
        Page::Generated {
            role: PageRole::KeyPlan,
        },
        Page::Generated {
            role: PageRole::LegendNotes,
        },
        Page::PlanProfile,
    ]);
    // Existing optional context documents retain their approved bytes and
    // order after the standard drawing slot. Reapplying the intent is stable.
    pages.extend(
        template
            .pages
            .iter()
            .skip(2)
            .filter(|page| !matches!(page, Page::PlanProfile | Page::Generated { .. }))
            .cloned(),
    );
    template.pages = pages;
    for key in [
        "contract_title",
        "formal_client",
        "formal_employer",
        "formal_consultant",
        "formal_contractor",
        "external_version",
        "issue_date",
    ] {
        if !template
            .required_text
            .iter()
            .any(|existing| existing == key)
        {
            template.required_text.push(key.into());
        }
    }
    let settings = &mut template.settings;
    settings.format = SheetFormat::Advanced;
    settings.ink_mode = InkMode::ReferenceAccents;
    settings.horizontal_scale = Some(1500);
    settings.vertical_scale = Some(500);
    settings.plan_scale = Some(1500);
    settings.panel_order = PanelOrder::ProfileTop;
    settings.structure_label_orientation = StructureLabelOrientation::Vertical;
    settings.structure_label_rows = vec![
        vec![StructureLabelField::EngineeringNumber],
        vec![
            StructureLabelField::EngineeringName,
            StructureLabelField::Comment1,
            StructureLabelField::Comment2,
            StructureLabelField::Comment3,
        ],
        vec![StructureLabelField::StructureDescription],
    ];
    settings.special_clearance_hair_anchor = Some(SpecialClearanceHairAnchor::GroundOffsetGuide);
    settings.show_feature_codes = Some(false);
    settings.show_clearance_thresholds = Some(true);
    settings.show_attachment_points = Some(true);
    settings.show_span_labels = Some(true);
    settings.show_profile_grid = true;
    settings.show_obstacle_sticks = false;
    settings.show_profile_continuations = true;
    settings.profile_elevation_breaks = true;
    settings.break_support_context = BreakSupportContext::Once;
    settings.long_axis_plot = true;
    settings.plan_angle_policy = PlanAnglePolicy::PreserveIfFit;
    settings.angle_gap_mm = 7.;
    settings.minimum_angle_deg = 0.;
    settings.plan_buffer_m = 6.;
    super::validate(&layout)?;
    Ok(layout)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Template {
    /// Ordered pages, including exactly one geometry slot.
    pub pages: Vec<Page>,
    #[serde(deserialize_with = "approved_settings")]
    pub settings: SheetSettings,
    /// Binding keys which must have approved nonempty values before output.
    pub required_text: Vec<String>,
    pub allowed_model_fields: Vec<ModelField>,
    /// Existing layout asset ids, in approved title-block order.
    pub logos: Vec<String>,
    /// The current renderer embeds this bundled font in both preview and PDF.
    pub font_family: String,
}
// Legacy low-level sheet settings keep their decoding defaults. An adopted
// standard must declare every presentation choice explicitly instead.
fn approved_settings<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<SheetSettings, D::Error> {
    let value = Value::deserialize(decoder)?;
    let exemplar = serde_json::to_value(SheetSettings::new(SheetFormat::Advanced, "schema"))
        .expect("settings shape");
    for key in exemplar.as_object().expect("settings object").keys() {
        if !matches!(key.as_str(), "drawing_revision" | "drawing_date") && value.get(key).is_none()
        {
            return Err(serde::de::Error::custom(format!("mv_print_template_invalid: settings.{key}: explicitly approve every presentation choice")));
        }
    }
    for key in ["codes", "orientation", "font_size_pt", "placement"] {
        if value["feature_label_style"].get(key).is_none() {
            return Err(serde::de::Error::custom(format!("mv_print_template_invalid: settings.feature_label_style.{key}: explicitly approve this presentation choice")));
        }
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Page {
    Layout {
        layout: Box<Layout>,
    },
    /// Preserve previously approved front matter. No re-typesetting or inferred text.
    PreservedPdf {
        asset_id: String,
        sha256: String,
    },
    /// Only this exact approved identity binding may vary between models.
    ModelPreservedPdf {
        bindings: Vec<ModelPageBinding>,
    },
    PlanProfile,
    /// Standard computed front matter uses the held model and final sheet plan.
    Generated {
        role: super::mv_booklet::PageRole,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelPageBinding {
    pub model_identity: String,
    pub asset_id: String,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PreservedPage {
    pub page_index: usize,
    pub asset_id: String,
    pub sha256: String,
}
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ModelField {
    Identity,
    Title,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub schema: String,
    pub layout_id: String,
    pub revision: String,
    /// Operator-owned publication version, unrelated to model hashes.
    pub external_version: String,
    /// Fixed issue date; never today's date or an export timestamp.
    pub issue_date: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub id: String,
    pub revision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Resolved {
    pub schema: String,
    pub project_id: String,
    pub selection: Selection,
    pub inherited_from: Origin,
    pub layout: Layout,
    pub model_fields: BTreeMap<ModelField, String>,
    pub preserved_pages: Vec<PreservedPage>,
    pub settings: SheetSettings,
    pub text: BTreeMap<String, String>,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub code: String,
    pub message_key: String,
    pub field: String,
    pub remedy: String,
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}: {}", self.code, self.field, self.remedy)
    }
}
fn refuse(code: &str, field: &str, remedy: &str) -> Refusal {
    Refusal {
        code: code.into(),
        message_key: code.into(),
        field: field.into(),
        remedy: remedy.into(),
    }
}
fn valid_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.trim() == value
        && value.len() <= limit
        && !value.chars().any(char::is_control)
}
pub fn valid_revision(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_revision)
}
impl Selection {
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != SELECTION_SCHEMA
            || !super::identifier(&self.layout_id)
            || !valid_revision(&self.revision)
        {
            return Err(refuse("mv_print_selection_invalid", SETTING, "Select one exact adopted project layout id and its revision using ds report project mv-setup set."));
        }
        let number = self
            .external_version
            .strip_prefix("Version ")
            .or_else(|| self.external_version.strip_prefix('v'))
            .unwrap_or(&self.external_version);
        if number.is_empty()
            || number.len() > 4
            || number.starts_with('0')
            || !number.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(refuse("mv_print_issue_invalid", "external_version", "Approve a simple positive publication number, such as Version 1, v1 or 1; internal draft/status/hash labels are not publication versions."));
        }
        let format =
            time::format_description::parse("[year]-[month]-[day]").expect("fixed date format");
        if self.issue_date.len() != 10 || time::Date::parse(&self.issue_date, &format).is_err() {
            return Err(refuse(
                "mv_print_issue_invalid",
                "issue_date",
                "Supply the approved calendar issue date as YYYY-MM-DD; export never advances it.",
            ));
        }
        Ok(())
    }
}

pub fn validate_template(layout: &Layout, template: &Template) -> Result<(), Refusal> {
    if layout.page_mm != [420., 297.] || template.font_family != "DejaVu Sans" {
        return Err(refuse("mv_print_template_invalid", "paper/font_family", "Use A3 landscape and the bundled DejaVu Sans font supported identically by preview and PDF."));
    }
    if template.pages.len() < 2
        || template.pages.len() > 32
        || template
            .pages
            .iter()
            .filter(|p| matches!(p, Page::PlanProfile))
            .count()
            != 1
    {
        return Err(refuse(
            "mv_print_template_invalid",
            "pages",
            "Author 2..32 ordered pages including approved front matter, with exactly one plan_profile slot.",
        ));
    }
    // The actual A3 title furniture is the existing layout document and is
    // painted by the shared renderer. The engine projection owns the two
    // drawing panels; no title rectangle may overwrite those panels.
    if layout.elements.iter().any(|e| {
        !matches!(
            e.kind,
            ElementKind::Text | ElementKind::Logo | ElementKind::Rectangle
        ) || e.rect[1] < 271.
    }) {
        return Err(refuse("mv_print_template_invalid", "elements", "Author shared A3 title-block text/logo/rectangle furniture in the bottom 26 mm; plan/profile panels remain engine-owned."));
    }
    let drawn_logos: Vec<_> = layout
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Logo)
        .map(|e| e.asset.clone())
        .collect();
    if drawn_logos != template.logos {
        return Err(refuse("mv_print_template_invalid", "logos", "Place the approved logos as ordinary shared layout elements in the declared party order."));
    }
    let mut ids = BTreeSet::new();
    for page in &template.pages {
        match page {
            Page::Layout { layout: page } => {
                if page.mv.is_some()
                    || page.page_mm != [420., 297.]
                    || page.elements.iter().any(|e| {
                        matches!(
                            e.kind,
                            ElementKind::Map
                                | ElementKind::Table
                                | ElementKind::Legend
                                | ElementKind::ScaleBar
                                | ElementKind::NorthArrow
                        )
                    })
                {
                    return Err(refuse("mv_print_template_invalid", "pages.layout", "Use ordinary A3 text/logo/rectangle layout pages; geometry belongs in the plan_profile slot."));
                }
                super::validate(page)
                    .map_err(|e| refuse("mv_print_template_invalid", "pages.layout", &e))?;
                if !ids.insert(page.id.clone()) {
                    return Err(refuse(
                        "mv_print_template_invalid",
                        "pages.layout.id",
                        "Use unique page identities.",
                    ));
                }
            }
            Page::PreservedPdf { asset_id, sha256 } => {
                if !super::identifier(asset_id)
                    || !valid_digest(sha256)
                    || !ids.insert(asset_id.clone())
                {
                    return Err(refuse(
                        "mv_print_template_invalid",
                        "pages.preserved_pdf",
                        "Name a unique governed asset id and the exact approved PDF SHA-256.",
                    ));
                }
            }
            Page::ModelPreservedPdf { bindings } => {
                let mut identities = BTreeSet::new();
                if !template
                    .allowed_model_fields
                    .contains(&ModelField::Identity)
                    || bindings.is_empty()
                    || bindings.len() > 64
                {
                    return Err(refuse("mv_print_model_binding_invalid", "pages.model_preserved_pdf", "Declare 1..64 approved model identity bindings and explicitly permit the identity field."));
                }
                for binding in bindings {
                    if !valid_text(&binding.model_identity, 200)
                        || !identities.insert(&binding.model_identity)
                        || !super::identifier(&binding.asset_id)
                        || !valid_digest(&binding.sha256)
                    {
                        return Err(refuse("mv_print_model_binding_invalid", "pages.model_preserved_pdf", "Approve each exact model identity once with its governed PDF asset id and SHA-256; ambiguous bindings are forbidden."));
                    }
                }
            }
            Page::PlanProfile => {}
            Page::Generated { role } => {
                if !matches!(
                    role,
                    super::mv_booklet::PageRole::SheetIndex
                        | super::mv_booklet::PageRole::KeyPlan
                        | super::mv_booklet::PageRole::LegendNotes
                ) || !ids.insert(format!("generated_{role:?}"))
                {
                    return Err(refuse("mv_print_template_invalid", "pages.generated", "Generate each sheet_index, key_plan or legend_notes once; cover and naming remain approved layouts or pinned artifacts."));
                }
            }
        }
    }
    let allowed: BTreeSet<_> = template.allowed_model_fields.iter().collect();
    if allowed.len() != template.allowed_model_fields.len() {
        return Err(refuse(
            "mv_print_template_invalid",
            "allowed_model_fields",
            "List each allowed identity/title difference once.",
        ));
    }
    for (key, field) in [
        ("model_identity", ModelField::Identity),
        ("model_title", ModelField::Title),
    ] {
        if template.required_text.iter().any(|k| k == key) && !allowed.contains(&field) {
            return Err(refuse("mv_print_template_invalid", "allowed_model_fields", "Explicitly permit each required model identity/title field; project text cannot bypass a forbidden model field."));
        }
    }
    let required: BTreeSet<_> = template.required_text.iter().collect();
    if required.is_empty()
        || required.len() != template.required_text.len()
        || required.len() > 64
        || required.iter().any(|key| !super::identifier(key))
    {
        return Err(refuse(
            "mv_print_template_invalid",
            "required_text",
            "Declare 1..64 unique required title/front-matter binding keys.",
        ));
    }
    if template.logos.is_empty()
        || template.logos.len() > 6
        || template
            .logos
            .iter()
            .any(|id| !layout.assets.contains_key(id))
    {
        return Err(refuse("mv_print_template_invalid", "logos", "Select 1..6 approved embedded assets from this printing layout in their approved order."));
    }
    template
        .settings
        .scales()
        .and_then(|_| template.settings.plan_options())
        .and_then(|_| template.settings.label_rows().map(|_| ()))
        .and_then(|_| template.settings.feature_label_style.validate())
        .map_err(|e| refuse("mv_print_template_invalid", "settings", &e))?;
    if template.settings.format == SheetFormat::Advanced
        && (template.settings.horizontal_scale.is_none()
            || template.settings.vertical_scale.is_none()
            || template.settings.plan_scale != template.settings.horizontal_scale)
    {
        return Err(refuse("mv_print_template_invalid", "settings.scales", "Declare explicit horizontal, vertical and matching plan scales for advanced A3 sheets."));
    }
    Ok(())
}

pub fn selection(sheets: &Value) -> Result<Selection, Refusal> {
    let rows = sheets["project_settings"].as_array().ok_or_else(|| {
        refuse(
            "mv_print_setup_missing",
            SETTING,
            "Read project settings and select one adopted MV setup.",
        )
    })?;
    let matches: Vec<_> = rows
        .iter()
        .filter(|r| crate::report_formats::setting_parameter_key(r).as_deref() == Some(SETTING))
        .collect();
    if matches.len() != 1 {
        return Err(refuse("mv_print_setup_missing", SETTING, "Save exactly one canonical MV setup selection in project settings; do not infer it from names or output formats."));
    }
    let result: Selection = serde_json::from_value(matches[0]["value"].clone()).map_err(|_| refuse("mv_print_selection_invalid", SETTING, "Use the ds.mv-print-selection/v1 schema with one layout revision, version and issue date."))?;
    result.validate()?;
    Ok(result)
}

pub fn resolve(
    sheets: &Value,
    project_id: &str,
    model_fields: BTreeMap<ModelField, String>,
) -> Result<Resolved, Refusal> {
    if !valid_text(project_id, 128)
        || !project_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(refuse(
            "mv_print_project_mismatch",
            "project_id",
            "Supply the exact authorized project context.",
        ));
    }
    let selected = selection(sheets)?;
    let setups = sheets["printing_setups"].as_array().ok_or_else(|| refuse("mv_print_setup_not_adopted", "printing_setups", "Adopt the global printing revision with ds report layout copy, then refresh project inputs."))?;
    let rows: Vec<_> = setups
        .iter()
        .filter(|r| r["id"].as_str() == Some(&selected.layout_id))
        .collect();
    if rows.len() != 1 {
        return Err(refuse(
            "mv_print_setup_not_adopted",
            "layout_id",
            "Adopt the selected revision into this project's existing printing library.",
        ));
    }
    let row = rows[0];
    if row["revision"].as_str() != Some(&selected.revision) {
        return Err(refuse("mv_print_setup_revision_changed", "revision", "Review the revised project layout, then intentionally select its exact revision; the selected setup is never advanced automatically."));
    }
    let layout: Layout = serde_json::from_value(row["layout"].clone()).map_err(|_| {
        refuse(
            "mv_print_template_invalid",
            "layout",
            "Refresh the adopted printing document using a matching DS release.",
        )
    })?;
    if layout.id != selected.layout_id {
        return Err(refuse(
            "mv_print_template_invalid",
            "layout.id",
            "Repair the selected layout identity.",
        ));
    }
    super::validate(&layout).map_err(|e| refuse("mv_print_template_invalid", "layout", &e))?;
    let origin: Origin = serde_json::from_value(row["inherited_from"].clone()).map_err(|_| refuse("mv_print_setup_not_adopted", "inherited_from", "Copy an exact global template revision into the project; preserve its library lineage."))?;
    if origin.scope != "global"
        || origin.project_id.is_some()
        || !super::identifier(&origin.id)
        || !valid_revision(&origin.revision)
    {
        return Err(refuse(
            "mv_print_setup_not_adopted",
            "inherited_from",
            "Adopt an exact global revision with the existing printing library copy action.",
        ));
    }
    let template = layout.mv.as_ref().ok_or_else(|| {
        refuse(
            "mv_print_template_invalid",
            "mv",
            "Select a printing layout carrying the approved MV publication setup.",
        )
    })?;
    for (field, value) in &model_fields {
        if !template.allowed_model_fields.contains(field) || !valid_text(value, 200) {
            return Err(refuse("mv_print_model_override_forbidden", "model_fields", "Supply only explicitly permitted, nonempty model identity/title differences; inherit all other project settings."));
        }
    }
    let mut settings = template.settings.clone();
    for (field, value) in [
        ("project_title", &settings.project_title),
        ("sheet_title", &settings.sheet_title),
        ("title_country", &settings.title_country),
        ("title_employer", &settings.title_employer),
        ("title_contractor", &settings.title_contractor),
    ] {
        if !valid_text(value, 200) {
            return Err(refuse(
                "mv_print_text_missing",
                field,
                "Approve this field in the project printing setup before printing.",
            ));
        }
    }
    let mut text = layout.title_block.clone();
    for (key, value) in [
        ("project_title", &settings.project_title),
        ("sheet_title", &settings.sheet_title),
        ("title_country", &settings.title_country),
        ("title_employer", &settings.title_employer),
        ("title_consultant", &settings.title_consultant),
        ("title_contractor", &settings.title_contractor),
        ("title_programme", &settings.title_programme),
        ("title_subject", &settings.title_subject),
    ] {
        if text.get(key).is_some_and(|approved| approved != value) {
            return Err(refuse("mv_print_text_conflict",key,"Keep one approved value for this field in the adopted layout and its sheet settings."));
        }
        text.insert(key.into(), value.clone());
    }

    text.insert("external_version".into(), selected.external_version.clone());
    text.insert("issue_date".into(), selected.issue_date.clone());
    for (field, value) in &model_fields {
        text.insert(
            match field {
                ModelField::Identity => "model_identity",
                ModelField::Title => "model_title",
            }
            .into(),
            value.clone(),
        );
    }
    // Validate project-owned text before returning a missing model binding.
    let mut required = template.required_text.iter().collect::<Vec<_>>();
    required.sort_by_key(|key| matches!(key.as_str(), "model_identity" | "model_title"));
    for key in required {
        if text.get(key).is_none_or(|v| !valid_text(v, 2000)) {
            return Err(refuse("mv_print_text_missing", key, "Approve the required text in the project layout, or supply its explicitly allowed model identity/title binding."));
        }
    }
    settings.drawing_revision = selected.external_version.clone();
    settings.drawing_date = selected.issue_date.clone();
    if let Some(title) = model_fields.get(&ModelField::Title) {
        settings.sheet_title = title.clone();
        text.insert("sheet_title".into(), title.clone());
    }
    let mut preserved_pages = Vec::new();
    for (page_index, page) in template.pages.iter().enumerate() {
        let pin = match page {
            Page::PreservedPdf { asset_id, sha256 } => Some((asset_id, sha256)),
            Page::ModelPreservedPdf { bindings } => {
                let identity = model_fields.get(&ModelField::Identity).ok_or_else(||
                    refuse("mv_print_model_binding_missing", "model_identity", "Supply the exact approved model identity to select its preserved page; adopt an explicit binding if it is missing."))?;
                let binding = bindings.iter().find(|b| &b.model_identity == identity).ok_or_else(||
                    refuse("mv_print_model_binding_missing", "model_identity", "This identity has no approved preserved page binding. Review and pin its exact asset in the same project layout, then intentionally select that revision."))?;
                Some((&binding.asset_id, &binding.sha256))
            }
            _ => None,
        };
        if let Some((asset_id, sha256)) = pin {
            preserved_pages.push(PreservedPage {
                page_index,
                asset_id: asset_id.clone(),
                sha256: sha256.clone(),
            });
        }
    }
    let mut resolved = Resolved {
        schema: "ds.mv-print-resolved/v1".into(),
        project_id: project_id.into(),
        selection: selected,
        inherited_from: origin,
        layout,
        model_fields,
        preserved_pages,
        settings,
        text,
        sha256: String::new(),
    };
    resolved.sha256 = digest(&resolved)?;
    Ok(resolved)
}
fn digest(resolved: &Resolved) -> Result<String, Refusal> {
    let mut value = serde_json::to_value(resolved).map_err(|_| {
        refuse(
            "mv_print_template_invalid",
            "receipt",
            "Update DS to a matching printing release.",
        )
    })?;
    value
        .as_object_mut()
        .expect("resolved object")
        .remove("sha256");
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&value).expect("serializable receipt"))
    ))
}
impl Resolved {
    /// Re-resolve at every consumer: a receipt cannot bypass document policy.
    pub fn validate(&self, project: &str) -> Result<(), Refusal> {
        if self.project_id != project {
            return Err(refuse(
                "mv_print_project_mismatch",
                "project_id",
                "Use the printing receipt for this exact project.",
            ));
        }
        let sheets = json!({"project_settings":[{"parameter":SETTING,"value":self.selection}],"printing_setups":[{"id":self.layout.id,"revision":self.selection.revision,"layout":self.layout,"inherited_from":self.inherited_from}]});
        let expected = resolve(&sheets, project, self.model_fields.clone())?;
        if &expected != self {
            return Err(refuse(
                "mv_print_receipt_changed",
                "receipt",
                "Resolve the selected project setup again; do not edit the resolved receipt.",
            ));
        }
        Ok(())
    }
}
pub fn patch_selection(rows: &[Value], selected: &Selection) -> Result<Vec<Value>, Refusal> {
    selected.validate()?;
    crate::report_formats::settings_rows_for_patch(rows).map_err(|_| {
        refuse(
            "mv_print_selection_invalid",
            "project_settings",
            "Repair the project parameter sheet before selecting a setup.",
        )
    })?;
    let mut next = rows.to_vec();
    let indices: Vec<_> = next
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            crate::report_formats::setting_parameter_key(r).as_deref() == Some(SETTING)
        })
        .map(|(i, _)| i)
        .collect();
    if indices.len() > 1 {
        return Err(refuse(
            "mv_print_selection_invalid",
            SETTING,
            "Remove duplicate MV setup rows; only one canonical selection is permitted.",
        ));
    }
    match indices.first() {
        Some(&i) => next[i]["value"] = json!(selected),
        None => next.push(json!({"parameter":SETTING,"value":selected})),
    }
    Ok(next)
}
pub fn inspect(sheets: &Value, project_id: Option<&str>) -> Value {
    let selected = match selection(sheets) {
        Ok(value) => value,
        Err(error) => {
            return json!({"selection":Value::Null,"configured":false,"ready":false,"refusal":error})
        }
    };
    let Some(project_id) = project_id else {
        return json!({"selection":selected,"configured":false,"ready":false,"refusal":refuse("mv_print_project_mismatch", "project_id", "Inspect the selected setup with explicit authorized project context.")});
    };
    match resolve(sheets, project_id, BTreeMap::new()) {
        Ok(resolved) => {
            json!({"selection":selected,"configured":true,"ready":false,"refusal":Value::Null,"setup_sha256":resolved.sha256,"requires":["model_geometry","held_pdf_assets"]})
        }
        Err(error) => {
            let model_required =
                matches!(
                    error.code.as_str(),
                    "mv_print_text_missing" | "mv_print_model_binding_missing"
                ) && matches!(error.field.as_str(), "model_identity" | "model_title");
            json!({"selection":selected,"configured":model_required,"ready":false,"refusal":error})
        }
    }
}

pub fn selection_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Selection)).expect("selection schema")
}
pub fn resolved_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Resolved)).expect("resolved schema")
}
