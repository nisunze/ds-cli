//! Exact, governed style choice. Hosts supply an API snapshot and a semantic
//! key; no renderer owns a layer-name, source or paper-role fallback.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "ds.style-resolution/v1";
pub const MAX_BINDINGS: usize = 2_000;
pub const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Screen,
    Print,
}

/// Paper ink is independent of source locality and focus. The omitted value
/// retains the original colour tuple and its serialized digest.
#[derive(
    Debug, Default, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum Ink {
    #[default]
    Colour,
    Monochrome,
}
impl Ink {
    fn is_colour(&self) -> bool {
        *self == Self::Colour
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub entity_class: String,
    pub source_kind: String,
    pub target: Target,
    pub role: String,
    #[serde(default, skip_serializing_if = "Ink::is_colour")]
    pub ink: Ink,
}
impl Key {
    pub fn name(&self) -> String {
        let name = format!(
            "({}, {}, {}, {})",
            self.entity_class,
            self.source_kind,
            if self.target == Target::Screen {
                "screen"
            } else {
                "print"
            },
            self.role
        );
        if self.ink == Ink::Monochrome {
            format!("{name}; ink=monochrome")
        } else {
            name
        }
    }
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.target == Target::Screen && self.ink != Ink::Colour {
            return Err(refused(
                "style_resolution_invalid",
                "ink=monochrome requires target=print",
            ));
        }
        for (name, text) in [
            ("entity_class", &self.entity_class),
            ("source_kind", &self.source_kind),
            ("role", &self.role),
        ] {
            if !bounded(text, 200) {
                return Err(refused(
                    "style_resolution_invalid",
                    format!("{name} must be one exact declared value"),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub key: Key,
    pub style_ref: String,
    pub revision_id: String,
    pub content_sha256: String,
    pub scope: String,
    pub document: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_tier: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: String,
    pub project_id: String,
    pub revision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_presets: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_style_refs: Vec<String>,
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub project: String,
    pub snapshot: Snapshot,
    pub key: Key,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Refusal {
    pub code: String,
    pub message: String,
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Refusal {}
fn refused(code: &str, message: impl Into<String>) -> Refusal {
    Refusal {
        code: code.into(),
        message: message.into(),
    }
}
fn bounded(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text.trim() == text
        && !text.chars().any(char::is_control)
}
fn digest(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|c| c.is_ascii_hexdigit())
}

/// The sole choice function. The API has already captured project and scoped
/// precedence; this function accepts exactly one declared tuple and exact
/// immutable content. It never constructs a style id or a pen.
pub fn resolve(snapshot: &Snapshot, key: &Key) -> Result<Binding, Refusal> {
    key.validate()?;
    validate_snapshot(snapshot)?;
    let mut matches = snapshot
        .bindings
        .iter()
        .filter(|binding| binding.key == *key);
    let binding = matches.next().ok_or_else(|| {
        refused(
            "style_resolution_unknown",
            format!("no governed style declares {}", key.name()),
        )
    })?;
    if matches.next().is_some() {
        return Err(refused(
            "style_resolution_ambiguous",
            format!(
                "several governed styles declare {}; repair the resolution table",
                key.name()
            ),
        ));
    }
    Ok(admit_binding(binding, key)?.clone())
}

/// Admit an already-selected, held API reply through the same authority and
/// content checks as `resolve`. This never selects a different document or
/// invents a resolution snapshot for a legacy captured consumer request.
pub fn admit_binding<'a>(binding: &'a Binding, key: &Key) -> Result<&'a Binding, Refusal> {
    key.validate()?;
    if binding.key != *key {
        return Err(refused(
            "style_resolution_key_mismatch",
            format!("the held document does not declare {}", key.name()),
        ));
    }
    if !bounded(&binding.style_ref, 500)
        || !bounded(&binding.scope, 500)
        || !digest(&binding.revision_id)
        || !digest(&binding.content_sha256)
        || binding
            .presentation_tier
            .is_some_and(|tier| !(0..=1_000_000).contains(&tier))
    {
        return Err(refused(
            "style_resolution_invalid",
            format!("{} has incomplete document authority", key.name()),
        ));
    }
    if binding.document["metadata"]
        .get("physical_print_style")
        .is_some()
    {
        if key.target != Target::Print {
            return Err(refused(
                "style_resolution_invalid",
                "physical_print_style requires a print target",
            ));
        }
        crate::printing::style_overrides::physical_style(&binding.document).map_err(|error| {
            refused(
                "style_resolution_invalid",
                format!("{} ({}) is invalid: {error}", key.name(), binding.style_ref),
            )
        })?;
    }
    crate::style_validation::validate(&binding.document).map_err(|error| {
        refused(
            "style_resolution_invalid",
            format!("{} ({}) is invalid: {error}", key.name(), binding.style_ref),
        )
    })?;
    if crate::style_plan::style_digest(&binding.document) != binding.content_sha256 {
        return Err(refused(
            "style_resolution_revision_mismatch",
            format!(
                "{} ({}) does not match its pinned content digest",
                key.name(),
                binding.style_ref
            ),
        ));
    }
    Ok(binding)
}

pub fn validate_snapshot(snapshot: &Snapshot) -> Result<(), Refusal> {
    if snapshot.schema != SCHEMA
        || !bounded(&snapshot.project_id, 200)
        || !digest(&snapshot.revision_id)
        || snapshot.bindings.len() > MAX_BINDINGS
    {
        return Err(refused(
            "style_resolution_invalid",
            "the governed resolution snapshot is missing its schema, project, revision or bounds",
        ));
    }
    if snapshot.missing_style_refs.len() > MAX_BINDINGS
        || snapshot.missing_style_refs.iter().any(|r| !bounded(r, 500))
    {
        return Err(refused(
            "style_resolution_invalid",
            "missing-style declarations exceed the table bound",
        ));
    }
    Ok(())
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_BYTES {
        return Err("style_resolution_invalid: request exceeds 32 MiB".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("style_resolution_invalid: {error}"))?;
    if request.project != request.snapshot.project_id {
        return Err(
            "project_context_changed: resolution snapshot belongs to another project".into(),
        );
    }
    serde_json::to_string(
        &resolve(&request.snapshot, &request.key).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

pub fn request_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Request)).expect("closed resolver schema")
}

/// Native print preparation consumes this output unchanged. Physical styles,
/// typography and scaling stay in the resolved documents / governed Layout;
/// the reporter gets held documents and performs no mutable catalogue reads.
pub fn prepare_print(snapshot: &Snapshot, keys: &BTreeMap<String, Key>) -> Result<Value, Refusal> {
    if keys.len() > 100 {
        return Err(refused(
            "style_resolution_invalid",
            "a print capture supports at most 100 style bindings",
        ));
    }
    let mut references = BTreeMap::new();
    let mut documents = BTreeMap::new();
    let mut revisions = BTreeMap::new();
    for (layer, key) in keys {
        if !bounded(layer, 200) || key.target != Target::Print {
            return Err(refused(
                "style_resolution_invalid",
                format!("print layer {layer} needs an exact print key"),
            ));
        }
        let binding = resolve(snapshot, key)?;
        if let Some(previous) = revisions.insert(
            binding.style_ref.clone(),
            json!({"revision_id":binding.revision_id,"content_sha256":binding.content_sha256}),
        ) {
            if previous != revisions[&binding.style_ref] {
                return Err(refused(
                    "style_resolution_ambiguous",
                    format!(
                        "{} resolves to inconsistent immutable documents",
                        binding.style_ref
                    ),
                ));
            }
        }
        references.insert(layer.clone(), binding.style_ref.clone());
        documents.insert(binding.style_ref, binding.document);
    }
    Ok(
        json!({"project_id":snapshot.project_id,"resolution_revision":snapshot.revision_id,"style_refs":references,"print_styles":documents,"style_revisions":revisions}),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionPin {
    revision_id: String,
    content_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrintCapture {
    project_id: String,
    resolution_revision: String,
    style_refs: BTreeMap<String, String>,
    print_styles: BTreeMap<String, Value>,
    style_revisions: BTreeMap<String, RevisionPin>,
}
/// Admit the sole resolver's held capture at a native preview boundary. This
/// validates authority and exact content; it makes no style choice and permits
/// no extra document, cross-project capture or unpinned mutable head.
pub fn admit_print_capture(
    project: &str,
    layout_refs: &BTreeMap<String, String>,
    captured: &Value,
) -> Result<Value, Refusal> {
    let capture: PrintCapture = serde_json::from_value(captured.clone()).map_err(|e| {
        refused(
            "style_resolution_invalid",
            format!("invalid print capture: {e}"),
        )
    })?;
    if capture.project_id != project {
        return Err(refused(
            "project_context_changed",
            "print styles belong to another project",
        ));
    }
    let refs: std::collections::BTreeSet<_> = layout_refs.values().collect();
    if !digest(&capture.resolution_revision)
        || capture.style_refs != *layout_refs
        || layout_refs.len() > 100
        || capture.print_styles.len() != refs.len()
        || capture.style_revisions.len() != refs.len()
    {
        return Err(refused(
            "style_resolution_invalid",
            "held print capture must match the exact layout references and resolution revision",
        ));
    }
    for reference in refs {
        let document = capture.print_styles.get(reference).ok_or_else(|| {
            refused(
                "style_resolution_invalid",
                format!("print style {reference} is not held"),
            )
        })?;
        let pin = capture.style_revisions.get(reference).ok_or_else(|| {
            refused(
                "style_resolution_invalid",
                format!("print style {reference} has no immutable revision"),
            )
        })?;
        if !digest(&pin.revision_id)
            || !digest(&pin.content_sha256)
            || crate::style_plan::style_digest(document) != pin.content_sha256
        {
            return Err(refused(
                "style_resolution_revision_mismatch",
                format!("print style {reference} does not match its pinned content"),
            ));
        }
        crate::style_validation::validate(document).map_err(|e| {
            refused(
                "style_resolution_invalid",
                format!("print style {reference}: {e}"),
            )
        })?;
    }
    serde_json::to_value(capture).map_err(|e| refused("style_resolution_invalid", e.to_string()))
}

/// Add admitted held style content after the original receipt has become an
/// explicit local preview. Original source provenance survives unchanged;
/// this receipt cannot be delivered or published as a governed report.
pub fn overlay_local_preview(
    project: &str,
    layout_refs: &BTreeMap<String, String>,
    preview: &crate::report_export::InputReceipt,
    captured: &Value,
) -> Result<crate::report_export::InputReceipt, Refusal> {
    preview
        .validate()
        .map_err(|e| refused("style_resolution_invalid", e))?;
    if preview
        .local_print_recipe
        .as_ref()
        .and_then(|p| p.preview.as_ref())
        .is_none()
    {
        return Err(refused("style_resolution_invalid","held style overlay requires an explicit local preview of the original governed receipt"));
    }
    let admitted = admit_print_capture(project, layout_refs, captured)?;
    let mut sheets: Value = serde_json::from_str(&preview.sheets_json)
        .map_err(|e| refused("style_resolution_invalid", e.to_string()))?;
    for (field, values) in [
        ("printing_styles", &admitted["print_styles"]),
        ("printing_style_revisions", &admitted["style_revisions"]),
    ] {
        if sheets[field].is_null() {
            sheets[field] = json!({});
        }
        let target = sheets[field].as_object_mut().ok_or_else(|| {
            refused(
                "style_resolution_invalid",
                format!("{field} must be an object"),
            )
        })?;
        target.extend(values.as_object().unwrap().clone());
    }
    sheets["printing_style_resolution"] = json!({"project_id":project,"resolution_revision":admitted["resolution_revision"],"style_revisions":admitted["style_revisions"]});
    let mut result = preview.clone();
    result.sheets_json = serde_json::to_string(&sheets)
        .map_err(|e| refused("style_resolution_invalid", e.to_string()))?;
    result.sheets_sha256 = crate::report_export::sha256_hex(result.sheets_json.as_bytes());
    result
        .validate()
        .map_err(|e| refused("style_resolution_invalid", e))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(source: &str, target: Target, role: &str) -> Key {
        Key {
            entity_class: "lv_line".into(),
            source_kind: source.into(),
            target,
            role: role.into(),
            ink: Ink::Colour,
        }
    }
    #[test]
    fn ink_is_a_governed_dimension_with_unchanged_colour_defaults() {
        let colour = key("saved_design", Target::Print, "focused");
        let mut explicit = serde_json::to_value(&colour).unwrap();
        assert!(explicit.get("ink").is_none());
        explicit["ink"] = json!("colour");
        assert_eq!(serde_json::from_value::<Key>(explicit).unwrap(), colour);
        let mut monochrome = colour.clone();
        monochrome.ink = Ink::Monochrome;
        let mut table = snapshot();
        let error = resolve(&table, &monochrome).unwrap_err();
        assert_eq!(error.code, "style_resolution_unknown");
        assert!(error.message.contains("ink=monochrome"));
        let mut mono_binding = table.bindings[3].clone();
        mono_binding.key = monochrome.clone();
        mono_binding.style_ref = "independent-monochrome".into();
        table.bindings.push(mono_binding);
        assert_eq!(resolve(&table, &colour).unwrap().style_ref, "paper-focused");
        assert_eq!(
            resolve(&table, &monochrome).unwrap().style_ref,
            "independent-monochrome"
        );
        monochrome.target = Target::Screen;
        assert_eq!(
            resolve(&table, &monochrome).unwrap_err().code,
            "style_resolution_invalid"
        );
        let mut ambiguous = table.bindings.last().unwrap().clone();
        ambiguous.key.ink = Ink::Colour;
        table.bindings.push(ambiguous);
        assert_eq!(
            resolve(&table, &colour).unwrap_err().code,
            "style_resolution_ambiguous"
        );
    }
    fn snapshot() -> Snapshot {
        let bindings = [
            (key("live_geojson", Target::Screen, "map"), "focused-live"),
            (key("vector_tile", Target::Screen, "map"), "published-tile"),
            (key("cold_existing", Target::Screen, "map"), "cold-existing"),
            (
                key("saved_design", Target::Print, "focused"),
                "paper-focused",
            ),
            (
                key("saved_design", Target::Print, "context"),
                "paper-context",
            ),
            (
                key("saved_design", Target::Print, "mv_context"),
                "paper-mv-context",
            ),
        ]
        .into_iter()
        .map(|(key, style_ref)| {
            let document = json!({"type":"line","paint":{"line-width":0.6,"line-color":"#64748B"}});
            Binding {
                key,
                style_ref: style_ref.into(),
                revision_id: "a".repeat(64),
                content_sha256: crate::style_plan::style_digest(&document),
                scope: "global".into(),
                document,
                presentation_tier: None,
            }
        })
        .collect();
        Snapshot {
            schema: SCHEMA.into(),
            project_id: "fixture".into(),
            revision_id: "b".repeat(64),
            seed_version: Some("fixture-v1".into()),
            style_presets: None,
            missing_style_refs: Vec::new(),
            bindings,
        }
    }
    #[test]
    fn held_reply_admission_shares_resolver_authority_without_a_second_choice() {
        let table = snapshot();
        let held = &table.bindings[0];
        assert_eq!(
            serde_json::to_value(admit_binding(held, &held.key).unwrap()).unwrap(),
            serde_json::to_value(held).unwrap()
        );
        assert_eq!(
            serde_json::to_value(resolve(&table, &held.key).unwrap()).unwrap(),
            serde_json::to_value(held).unwrap()
        );
        let mut wrong_key = held.key.clone();
        wrong_key.source_kind = "another-source".into();
        assert_eq!(
            admit_binding(held, &wrong_key).unwrap_err().code,
            "style_resolution_key_mismatch"
        );
        let mut changed = held.clone();
        changed.document["paint"]["line-width"] = json!(99);
        assert_eq!(
            admit_binding(&changed, &held.key).unwrap_err().code,
            "style_resolution_revision_mismatch"
        );
        changed = held.clone();
        changed.revision_id.clear();
        assert_eq!(
            admit_binding(&changed, &held.key).unwrap_err().code,
            "style_resolution_invalid"
        );
    }
    #[test]
    fn held_physical_style_uses_the_closed_print_owner() {
        let baselines: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/governed-physical-print-baselines.json"
        ))
        .unwrap();
        let document = baselines["documents"][0]["document"].clone();
        let mut binding = snapshot().bindings.remove(3);
        binding.document = document;
        binding.content_sha256 = crate::style_plan::style_digest(&binding.document);
        admit_binding(&binding, &binding.key).unwrap();
        binding.key.target = Target::Screen;
        assert_eq!(
            admit_binding(&binding, &binding.key).unwrap_err().code,
            "style_resolution_invalid"
        );
        binding.key.target = Target::Print;
        binding.document["metadata"]["physical_print_style"]
            .as_object_mut()
            .unwrap()
            .remove("stroke_opacity");
        binding.content_sha256 = crate::style_plan::style_digest(&binding.document);
        assert_eq!(
            admit_binding(&binding, &binding.key).unwrap_err().code,
            "style_resolution_invalid"
        );
    }
    #[test]
    fn entity_sources_and_paper_roles_have_independent_authority() {
        let snapshot = snapshot();
        let mut refs = std::collections::BTreeSet::new();
        for binding in &snapshot.bindings {
            refs.insert(resolve(&snapshot, &binding.key).unwrap().style_ref);
        }
        assert_eq!(refs.len(), 6);
        let error = resolve(&snapshot, &key("vector_tile", Target::Print, "context")).unwrap_err();
        assert_eq!(error.code, "style_resolution_unknown");
        assert!(error
            .message
            .contains("(lv_line, vector_tile, print, context)"));
        let mut ambiguous = snapshot.clone();
        ambiguous.bindings.push(ambiguous.bindings[0].clone());
        assert_eq!(
            resolve(&ambiguous, &ambiguous.bindings[0].key)
                .unwrap_err()
                .code,
            "style_resolution_ambiguous"
        );
        let mut changed = snapshot.clone();
        changed.bindings[0].document["paint"]["line-width"] = json!(9);
        assert_eq!(
            resolve(&changed, &changed.bindings[0].key)
                .unwrap_err()
                .code,
            "style_resolution_revision_mismatch"
        );
    }
    #[test]
    fn print_capture_retains_exact_pens_and_version_pins_and_project_fence() {
        let snapshot = snapshot();
        let keys = BTreeMap::from([
            (
                "focused".into(),
                key("saved_design", Target::Print, "focused"),
            ),
            (
                "adjacent".into(),
                key("saved_design", Target::Print, "context"),
            ),
        ]);
        let held = prepare_print(&snapshot, &keys).unwrap();
        assert_eq!(held["style_refs"]["focused"], "paper-focused");
        assert_eq!(held["style_refs"]["adjacent"], "paper-context");
        assert_eq!(
            held["print_styles"]["paper-focused"],
            snapshot.bindings[3].document
        );
        assert_eq!(
            held["style_revisions"]["paper-context"]["revision_id"],
            "a".repeat(64)
        );
        let refs = serde_json::from_value(held["style_refs"].clone()).unwrap();
        assert!(admit_print_capture("fixture", &refs, &held).is_ok());
        assert_eq!(
            admit_print_capture("other", &refs, &held).unwrap_err().code,
            "project_context_changed"
        );
        let mut tampered = held.clone();
        tampered["print_styles"]["paper-focused"]["paint"]["line-width"] = json!(99);
        assert_eq!(
            admit_print_capture("fixture", &refs, &tampered)
                .unwrap_err()
                .code,
            "style_resolution_revision_mismatch"
        );
        let request = Request {
            project: "other".into(),
            key: snapshot.bindings[0].key.clone(),
            snapshot,
        };
        assert!(evaluate(&serde_json::to_vec(&request).unwrap())
            .unwrap_err()
            .starts_with("project_context_changed:"));
    }
    #[test]
    fn local_preview_overlay_keeps_original_source_digest_and_cannot_be_published() {
        let mut table = snapshot();
        for binding in table
            .bindings
            .iter_mut()
            .filter(|b| b.key.target == Target::Print)
        {
            binding.style_ref.push_str("_print");
        }
        let capture = prepare_print(
            &table,
            &BTreeMap::from([(
                "lv_lines".into(),
                key("saved_design", Target::Print, "focused"),
            )]),
        )
        .unwrap();
        let refs = serde_json::from_value(capture["style_refs"].clone()).unwrap();
        let mut layout = crate::printing::default_layout();
        layout.style_refs = refs;
        let sheets=json!({"printing_setups":[],"project_settings":[{"parameter":"design_export_format","value":["xlsx","shp"]}],"pole_types":[{"height":9,"material":"steel"}]}).to_string();
        let source = crate::report_export::InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: "Rwanda".into(),
            sheets_sha256: crate::report_export::sha256_hex(sheets.as_bytes()),
            sheets_json: sheets,
            reference_semantic_sha256: "a".repeat(64),
        };
        let original = source.clone();
        assert!(overlay_local_preview("fixture", &layout.style_refs, &source, &capture).is_err());
        let preview = crate::report_export::preview::with_layout(&source, &layout).unwrap();
        let merged =
            overlay_local_preview("fixture", &layout.style_refs, &preview.receipt, &capture)
                .unwrap();
        assert_eq!(source, original);
        assert_eq!(
            merged.local_print_recipe,
            preview.receipt.local_print_recipe
        );
        assert_eq!(
            merged
                .local_print_recipe
                .as_ref()
                .unwrap()
                .source_sheets_sha256,
            source.sheets_sha256
        );
        let effective: Value = serde_json::from_str(&merged.sheets_json).unwrap();
        assert_eq!(
            effective["printing_styles"]["paper-focused_print"],
            table.bindings[3].document
        );
        assert_eq!(
            effective["pole_types"],
            json!([{"height":9,"material":"steel"}])
        );
        assert!(merged.local_print_recipe.is_some());
        assert!(preview.selection.require_deliverable().is_err());
    }
}
