//! One explicit migration from legacy `Layout.style_refs` to exact semantic
//! `style_keys`, including the keys its selected context sources declare,
//! decided against the governed style catalogue.
//!
//! A legacy ref names the document a template prints with today. The governed
//! catalogue keeps every installed paper original under the `project` role:
//! the four legacy global-template migrations reviewed on 2026-10-02 bind
//! every layer there, and [`plan`] reproduces each of them exactly. So a layer
//! migrates to the one `project`-role print key the catalogue declares for its
//! own entity class, and only when that key names the same document. Anything
//! else is a named finding listing the governed candidates, and the template is
//! left unchanged: no partial migration, no inference from a ref's name, no
//! other role, no second resolver. An owner settles a finding only by choosing
//! one exact declared key for that layer; a choice that prints another
//! document is reported as such.
//! Missing context keys come from the source owner's existing expansion and
//! require one exact catalogue declaration at the layout's ink. Authored
//! keys, pens and context parameters are retained.
use super::Layout;
use crate::style_resolution::{self, Ink, Key, Snapshot, Target};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The catalogue role that keeps the installed paper originals.
pub const LEGACY_ROLE: &str = "project";

/// One governed declaration: the exact key and the document it names.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatalogueBinding {
    pub key: Key,
    pub style_ref: String,
}

/// The governed key → document declarations a migration is decided against.
/// Documents are not needed to choose a key; printing resolves them later
/// through [`style_resolution::resolve`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub revision_id: String,
    pub bindings: Vec<CatalogueBinding>,
}

impl Catalogue {
    pub fn of(snapshot: &Snapshot) -> Self {
        Self {
            revision_id: snapshot.revision_id.clone(),
            bindings: snapshot
                .bindings
                .iter()
                .map(|binding| CatalogueBinding {
                    key: binding.key.clone(),
                    style_ref: binding.style_ref.clone(),
                })
                .collect(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        let invalid = |what: &str| Err(format!("print_style_keys_invalid: catalogue {what}"));
        if self.revision_id.len() != 64 || !self.revision_id.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return invalid("revision must be one SHA-256");
        }
        if self.bindings.len() > style_resolution::MAX_BINDINGS {
            return invalid("exceeds the governed table bound");
        }
        for binding in &self.bindings {
            binding.key.validate().map_err(|e| e.to_string())?;
            let reference = &binding.style_ref;
            if reference.is_empty() || reference.len() > 500 || reference.trim() != reference {
                return invalid("names an invalid style reference");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// No legacy ref or missing context key remains; nothing to write.
    Current,
    /// Every legacy ref and missing context key maps exactly; ready to save.
    Migrate,
    /// At least one legacy ref, context key or choice has no exact declaration.
    Blocked,
}

/// A key the owner may choose for a layer, and the document it prints.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub key: Key,
    pub style_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub layer: String,
    pub style_ref: String,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<Candidate>,
}

/// An owner's choice that prints another document than the legacy ref did.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DocumentChange {
    pub layer: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub status: Status,
    pub catalogue_revision: String,
    /// Legacy refs the stored template carries.
    pub legacy_refs: usize,
    /// Keys added for legacy refs and missing selected context declarations.
    pub style_keys: BTreeMap<String, Key>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub document_changes: Vec<DocumentChange>,
    pub findings: Vec<Finding>,
}

/// One template's migration request, as hosts and the dispatch bridge send it.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub layout: Layout,
    pub catalogue: Catalogue,
    /// Owner choices, layer id → exact governed key.
    #[serde(default)]
    pub choices: BTreeMap<String, Key>,
}

/// The plan, and the layout to save only when the plan is `migrate`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub plan: Plan,
    pub layout: Option<Layout>,
}

pub fn answer(request: Request) -> Result<Answer, String> {
    let (plan, layout) = plan(request.layout, &request.catalogue, &request.choices)?;
    Ok(Answer { plan, layout })
}

/// Plan the migration of one stored layout. The layout is returned only when
/// every legacy ref and missing context key has an exact declaration.
pub fn plan(
    layout: Layout,
    catalogue: &Catalogue,
    choices: &BTreeMap<String, Key>,
) -> Result<(Plan, Option<Layout>), String> {
    catalogue.validate()?;
    let layout = super::upgrade(layout)?;
    let ink = layout.ink_mode;
    let mut plan = Plan {
        status: Status::Current,
        catalogue_revision: catalogue.revision_id.clone(),
        legacy_refs: layout.style_refs.len(),
        style_keys: BTreeMap::new(),
        document_changes: Vec::new(),
        findings: Vec::new(),
    };
    for (layer, key) in choices {
        let finding = |code: &str, message: String| Finding {
            layer: layer.clone(),
            style_ref: layout.style_refs.get(layer).cloned().unwrap_or_default(),
            code: code.into(),
            message,
            candidates: Vec::new(),
        };
        if !layout.style_refs.contains_key(layer) || layout.style_keys.contains_key(layer) {
            plan.findings.push(finding(
                "style_choice_unused",
                format!("{layer} carries no legacy style ref to migrate"),
            ));
        } else if key.target != Target::Print || key.ink != Ink::Colour {
            plan.findings.push(finding(
                "style_choice_invalid",
                format!(
                    "a choice names one colour print key; the layout's ink selects monochrome, not {}",
                    key.name()
                ),
            ));
        }
    }
    for (layer, reference) in &layout.style_refs {
        // An authored key already wins at print time; its stale ref is dropped.
        if layout.style_keys.contains_key(layer) {
            continue;
        }
        let decided = match choices.get(layer) {
            Some(key) if key.target == Target::Print && key.ink == Ink::Colour => {
                chosen(catalogue, layer, reference, key, ink)
            }
            Some(_) => continue,
            None => governed(catalogue, layer, reference, ink),
        };
        match decided {
            Ok((key, style_ref)) => {
                if style_ref != *reference {
                    plan.document_changes.push(DocumentChange {
                        layer: layer.clone(),
                        from: reference.clone(),
                        to: style_ref,
                    });
                }
                plan.style_keys.insert(layer.clone(), key);
            }
            Err(finding) => plan.findings.push(finding),
        }
    }
    for (layer, key) in super::context::missing_context_style_keys(&layout) {
        let declared: Vec<Candidate> = print_bindings(catalogue, ink)
            .filter(|binding| binding.key == key)
            .collect();
        match declared.as_slice() {
            [_] => {
                plan.style_keys.insert(layer, key);
            }
            others => plan.findings.push(Finding {
                layer,
                style_ref: String::new(),
                code: if others.is_empty() {
                    "style_key_undeclared"
                } else {
                    "style_key_ambiguous"
                }
                .into(),
                message: format!(
                    "the selected context {} is declared {} times at the layout's ink",
                    key.name(),
                    others.len()
                ),
                candidates: declared,
            }),
        }
    }
    if !plan.findings.is_empty() {
        plan.status = Status::Blocked;
        plan.style_keys.clear();
        plan.document_changes.clear();
        return Ok((plan, None));
    }
    if layout.style_refs.is_empty() && plan.style_keys.is_empty() {
        return Ok((plan, None));
    }
    let mut migrated = layout;
    migrated.style_keys.extend(plan.style_keys.clone());
    migrated.style_refs.clear();
    if let Err(error) = super::validate(&migrated) {
        plan.findings.push(Finding {
            layer: String::new(),
            style_ref: String::new(),
            code: "style_migration_invalid".into(),
            message: error,
            candidates: Vec::new(),
        });
        plan.status = Status::Blocked;
        plan.style_keys.clear();
        return Ok((plan, None));
    }
    plan.status = Status::Migrate;
    Ok((plan, Some(migrated)))
}

/// The governed print declarations at the layout's ink, keyed as stored
/// (colour; the layout's ink selects monochrome at print time).
fn print_bindings(catalogue: &Catalogue, ink: Ink) -> impl Iterator<Item = Candidate> + '_ {
    catalogue
        .bindings
        .iter()
        .filter(move |b| b.key.target == Target::Print && b.key.ink == ink)
        .map(|b| Candidate {
            key: Key {
                ink: Ink::Colour,
                ..b.key.clone()
            },
            style_ref: b.style_ref.clone(),
        })
}

/// The layer's one `project`-role key, when it names the template's document.
fn governed(
    catalogue: &Catalogue,
    layer: &str,
    reference: &str,
    ink: Ink,
) -> Result<(Key, String), Finding> {
    let declared: Vec<Candidate> = print_bindings(catalogue, ink)
        .filter(|c| c.key.role == LEGACY_ROLE && c.key.entity_class == layer)
        .collect();
    let same: Vec<&Candidate> = declared
        .iter()
        .filter(|c| c.style_ref == reference)
        .collect();
    if let [exact] = same.as_slice() {
        return Ok((exact.key.clone(), exact.style_ref.clone()));
    }
    // The owner's alternatives: the layer's own governed keys, then every
    // project-role key that prints the template's exact document.
    let mut candidates = declared.clone();
    for candidate in print_bindings(catalogue, ink)
        .filter(|c| c.key.role == LEGACY_ROLE && c.style_ref == reference)
    {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    let (code, message) = match (same.len(), declared.len()) {
        (0, 0) => (
            "style_key_undeclared",
            format!("no governed {LEGACY_ROLE}-role print style declares {layer}; the template prints {reference}"),
        ),
        (0, _) => (
            "style_key_changes_document",
            format!(
                "the governed {LEGACY_ROLE}-role print style for {layer} is {}, not the template's {reference}",
                declared
                    .iter()
                    .map(|c| c.style_ref.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        (n, _) => (
            "style_key_ambiguous",
            format!(
                "{n} governed {LEGACY_ROLE}-role print styles declare {layer} with {reference}: {}",
                same.iter()
                    .map(|c| c.key.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
    };
    Err(Finding {
        layer: layer.into(),
        style_ref: reference.into(),
        code: code.into(),
        message,
        candidates,
    })
}

/// The owner's exact choice, admitted only when the catalogue declares it once.
fn chosen(
    catalogue: &Catalogue,
    layer: &str,
    reference: &str,
    key: &Key,
    ink: Ink,
) -> Result<(Key, String), Finding> {
    let declared: Vec<Candidate> = print_bindings(catalogue, ink)
        .filter(|c| c.key == *key)
        .collect();
    match declared.as_slice() {
        [exact] => Ok((exact.key.clone(), exact.style_ref.clone())),
        others => Err(Finding {
            layer: layer.into(),
            style_ref: reference.into(),
            code: if others.is_empty() {
                "style_choice_undeclared"
            } else {
                "style_choice_ambiguous"
            }
            .into(),
            message: format!(
                "the chosen {} is declared {} times by the governed catalogue",
                key.name(),
                others.len()
            ),
            candidates: Vec::new(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printing::{default_layout, resolve_style_documents};
    use crate::style_resolution::{Binding, SCHEMA};
    use serde_json::{json, Value};

    const EVIDENCE: &str = include_str!("../../tests/fixtures/printing-style-key-migration.json");

    fn evidence() -> Value {
        serde_json::from_str(EVIDENCE).unwrap()
    }
    fn governed_catalogue() -> Catalogue {
        serde_json::from_value(evidence()["catalogue"].clone()).unwrap()
    }
    fn key(entity: &str, source: &str, role: &str) -> Key {
        crate::printing::context::style_key(entity, source, role)
    }
    fn legacy(refs: &Value) -> Layout {
        let mut layout = default_layout();
        layout.style_refs = serde_json::from_value(refs.clone()).unwrap();
        layout
    }
    /// The governed table with a small valid document behind every binding,
    /// and the governed renderer policy behind its own key.
    fn table(catalogue: &Catalogue) -> Snapshot {
        let defaults: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/governed-renderer-defaults.json"
        ))
        .unwrap();
        Snapshot {
            schema: SCHEMA.into(),
            project_id: "czgmdwth_gisagara".into(),
            revision_id: catalogue.revision_id.clone(),
            seed_version: None,
            style_presets: None,
            missing_style_refs: Vec::new(),
            bindings: catalogue
                .bindings
                .iter()
                .map(|binding| {
                    let document = if binding.key == crate::printing::renderer_defaults::key() {
                        json!({"type":"line","paint":{},"layout":{},"metadata":{"print_renderer_defaults":defaults}})
                    } else {
                        json!({"type":"line","paint":{"line-width":0.6,"line-color":"#64748B"}})
                    };
                    Binding {
                        key: binding.key.clone(),
                        style_ref: binding.style_ref.clone(),
                        revision_id: "a".repeat(64),
                        content_sha256: crate::style_plan::style_digest(&document),
                        scope: "global".into(),
                        document,
                        presentation_tier: None,
                    }
                })
                .collect(),
        }
    }

    fn reviewed_mv_context() -> Layout {
        let mut layout = default_layout();
        layout.context_layers = serde_json::from_value(json!([{
            "id":"project_dsgrid_mv", "label":"Proposed MV · project DS Grid models",
            "source":{"kind":"project_dsgrid_mv", "buffer_m":200,
                "corridor":{"distance_m":6,"color":"#957197","width_mm":0.15,
                    "opacity":0.3,"dash_mm":[2,1]}}
        }]))
        .unwrap();
        for layer in ["dsgrid_mv_lines", "dsgrid_mv_structures"] {
            layout
                .style_keys
                .insert(layer.into(), key(layer, "project_model", "project"));
        }
        layout
    }

    // The producer seals only persisted style_keys. The consumer also asks for
    // the selected context source's expanded keys, which caused the real refusal.
    fn sealed_keys_receipt(
        layout: &Layout,
        snapshot: &Snapshot,
    ) -> crate::report_export::InputReceipt {
        let held = style_resolution::prepare_print(snapshot, &layout.style_keys).unwrap();
        let selected: BTreeMap<_, _> = layout
            .style_keys
            .iter()
            .map(|(layer, key)| {
                (
                    layer.clone(),
                    json!({"key":key,"style_ref":held["style_refs"][layer]}),
                )
            })
            .collect();
        let mut pins = held["style_revisions"].clone();
        for (reference, pin) in pins.as_object_mut().unwrap() {
            pin["style_ref"] = json!(reference);
        }
        let renderer = snapshot
            .bindings
            .iter()
            .find(|binding| binding.key == crate::printing::renderer_defaults::key())
            .unwrap();
        let sheets = json!({
            "printing_style_bindings":{"manifest_id":"standard-style-set/v1",
                "manifest_revision":snapshot.revision_id,"setups":{layout.id.clone():selected}},
            "printing_styles":held["print_styles"],"printing_style_revisions":pins,
            "printing_renderer_defaults":renderer
        })
        .to_string();
        crate::report_export::InputReceipt {
            schema: 1,
            country: "Rwanda".into(),
            sheets_sha256: crate::report_export::sha256_hex(sheets.as_bytes()),
            sheets_json: sheets,
            reference_semantic_sha256: "c".repeat(64),
            local_print_recipe: None,
        }
    }

    #[test]
    fn current_a0_a3_context_migration_completes_the_actual_sealed_capture() {
        let catalogue = governed_catalogue();
        let snapshot = table(&catalogue);
        for id in [
            "a0-landscape-huye-cjic-review",
            "a3-landscape-rwanda-project-lv",
        ] {
            let mut original = reviewed_mv_context();
            original.id = id.into();
            let capture = |layout: &Layout| {
                crate::printing::style_capture::Capture::from_sealed_receipt(
                    &snapshot.project_id,
                    "local",
                    "owner",
                    &"a".repeat(64),
                    &sealed_keys_receipt(layout, &snapshot),
                    &[layout.clone()],
                )
            };
            assert!(capture(&original)
                .unwrap_err()
                .contains("dsgrid_mv_angle_points"));
            let (plan, migrated) =
                super::plan(original.clone(), &catalogue, &BTreeMap::new()).unwrap();
            assert_eq!(plan.status, Status::Migrate, "{:?}", plan.findings);
            assert_eq!(plan.legacy_refs, 0);
            assert_eq!(
                plan.style_keys
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                [
                    "dsgrid_mv_angle_points",
                    "dsgrid_mv_corridor",
                    "dsgrid_mv_prohibited_zones"
                ]
            );
            assert!(plan.document_changes.is_empty());
            let migrated = migrated.unwrap();
            let held = capture(&migrated).unwrap().for_layout(&migrated).unwrap();
            assert_eq!(held["style_refs"].as_object().unwrap().len(), 5);
            assert_eq!(
                held["style_refs"]["dsgrid_mv_corridor"],
                "print_context/dsgrid_mv_corridor_print"
            );
            let mut restored = migrated.clone();
            restored.style_keys = original.style_keys.clone();
            assert_eq!(
                serde_json::to_value(restored).unwrap(),
                serde_json::to_value(original).unwrap()
            );
            assert!(crate::printing::context::missing_context_style_keys(&migrated).is_empty());
            let (again, layout) = super::plan(migrated, &catalogue, &BTreeMap::new()).unwrap();
            assert_eq!((again.status, layout.is_none()), (Status::Current, true));
        }
    }

    #[test]
    fn missing_ambiguous_and_near_context_declarations_block_without_partial_migration() {
        let expected = key("dsgrid_mv_angle_points", "project_model", "mv_context");
        for changed in [
            "missing",
            "duplicate",
            "screen",
            "wrong_ink",
            "wrong_source",
            "wrong_role",
        ] {
            let mut catalogue = governed_catalogue();
            let index = catalogue
                .bindings
                .iter()
                .position(|binding| binding.key == expected)
                .unwrap();
            match changed {
                "missing" => {
                    catalogue.bindings.remove(index);
                }
                "duplicate" => catalogue.bindings.push(catalogue.bindings[index].clone()),
                "screen" => catalogue.bindings[index].key.target = Target::Screen,
                "wrong_ink" => catalogue.bindings[index].key.ink = Ink::Monochrome,
                "wrong_source" => catalogue.bindings[index].key.source_kind = "saved_design".into(),
                "wrong_role" => catalogue.bindings[index].key.role = "project".into(),
                _ => unreachable!(),
            }
            let mut layout = reviewed_mv_context();
            layout
                .style_refs
                .insert("lv_lines".into(), "master/lv_lines_print".into());
            let before = serde_json::to_value(&layout).unwrap();
            let (plan, migrated) =
                super::plan(layout.clone(), &catalogue, &BTreeMap::new()).unwrap();
            assert_eq!(plan.status, Status::Blocked, "{changed}");
            assert!(
                migrated.is_none()
                    && plan.style_keys.is_empty()
                    && plan.document_changes.is_empty()
            );
            assert_eq!(plan.findings.len(), 1, "{changed}: {:?}", plan.findings);
            assert_eq!(plan.findings[0].layer, "dsgrid_mv_angle_points");
            assert_eq!(
                plan.findings[0].code,
                if changed == "duplicate" {
                    "style_key_ambiguous"
                } else {
                    "style_key_undeclared"
                }
            );
            assert_eq!(serde_json::to_value(layout).unwrap(), before);
        }
    }

    #[test]
    fn authored_context_keys_and_pens_are_retained_without_rebinding() {
        let mut layout = reviewed_mv_context();
        layout.styles.insert(
            "dsgrid_mv_angle_points".into(),
            serde_json::from_value(json!({"stroke":"#72502a","width_mm":0.3})).unwrap(),
        );
        layout.style_keys.insert(
            "dsgrid_mv_prohibited_zones".into(),
            key("dsgrid_mv_prohibited_zones", "project_model", "focused"),
        );
        let original = layout.clone();
        let (plan, migrated) =
            super::plan(layout, &governed_catalogue(), &BTreeMap::new()).unwrap();
        assert_eq!(plan.status, Status::Migrate);
        assert_eq!(
            plan.style_keys.keys().collect::<Vec<_>>(),
            ["dsgrid_mv_corridor"]
        );
        let mut migrated = migrated.unwrap();
        migrated.style_keys = original.style_keys.clone();
        assert_eq!(
            serde_json::to_value(migrated).unwrap(),
            serde_json::to_value(original).unwrap()
        );
    }

    #[test]
    fn context_migration_requires_the_authored_ink_and_stores_semantic_colour_keys() {
        let mut layout = reviewed_mv_context();
        layout.ink_mode = Ink::Monochrome;
        let mut catalogue = governed_catalogue();
        let (blocked, migrated) =
            super::plan(layout.clone(), &catalogue, &BTreeMap::new()).unwrap();
        assert_eq!(blocked.status, Status::Blocked);
        assert_eq!(blocked.findings.len(), 3);
        assert!(migrated.is_none());
        for binding in &mut catalogue.bindings {
            if binding.key.source_kind == "project_model" && binding.key.role == "mv_context" {
                binding.key.ink = Ink::Monochrome;
            }
        }
        let (plan, migrated) = super::plan(layout, &catalogue, &BTreeMap::new()).unwrap();
        assert_eq!(plan.status, Status::Migrate);
        assert!(plan.style_keys.values().all(|key| key.ink == Ink::Colour));
        assert_eq!(migrated.unwrap().ink_mode, Ink::Monochrome);
    }

    #[test]
    fn context_migration_does_not_add_a_corridor_the_layout_does_not_select() {
        let mut layout = reviewed_mv_context();
        if let crate::printing::PrintContextSource::ProjectDsgridMv { corridor, .. } =
            &mut layout.context_layers[0].source
        {
            *corridor = None;
        }
        let mut catalogue = governed_catalogue();
        catalogue
            .bindings
            .retain(|binding| binding.key.entity_class != "dsgrid_mv_corridor");
        let (plan, migrated) = super::plan(layout, &catalogue, &BTreeMap::new()).unwrap();
        assert_eq!(plan.status, Status::Migrate);
        assert_eq!(plan.style_keys.len(), 2);
        assert!(!migrated
            .unwrap()
            .style_keys
            .contains_key("dsgrid_mv_corridor"));
    }

    #[test]
    fn every_reviewed_legacy_template_migration_is_reproduced_exactly() {
        let catalogue = governed_catalogue();
        for reviewed in evidence()["reviewed_template_migrations"]
            .as_array()
            .unwrap()
        {
            let (plan, migrated) = super::plan(
                legacy(&reviewed["style_refs"]),
                &catalogue,
                &BTreeMap::new(),
            )
            .unwrap();
            assert_eq!(
                plan.status,
                Status::Migrate,
                "{}: {:?}",
                reviewed["layout_id"],
                plan.findings
            );
            let declared: BTreeMap<String, Key> =
                serde_json::from_value(reviewed["style_keys"].clone()).unwrap();
            assert_eq!(plan.style_keys, declared, "{}", reviewed["layout_id"]);
            assert!(plan.document_changes.is_empty());
            let migrated = migrated.unwrap();
            assert!(migrated.style_refs.is_empty());
            assert_eq!(migrated.style_keys, declared);
        }
    }

    /// The owner's 2026-10-05 failure: Gisagara's legacy A0 template refused
    /// its preview. It migrates every layer to the document it prints with,
    /// and names the two layers whose document the catalogue governs under
    /// another entity — never guessing between them.
    #[test]
    fn gisagara_a0_migrates_exactly_and_names_the_two_village_layers() {
        let catalogue = governed_catalogue();
        let template = legacy(&evidence()["gisagara_a0"]["style_refs"]);
        let refusal = resolve_style_documents(template.clone(), &table(&catalogue)).unwrap_err();
        assert!(
            refusal.starts_with("style_resolution_required"),
            "{refusal}"
        );

        let (plan, migrated) = super::plan(template.clone(), &catalogue, &BTreeMap::new()).unwrap();
        assert_eq!(plan.status, Status::Blocked);
        assert!(migrated.is_none() && plan.style_keys.is_empty());
        let named = plan
            .findings
            .iter()
            .map(|f| (f.layer.as_str(), f.code.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            named,
            [
                ("bounding_rectangles", "style_key_changes_document"),
                ("village_boundaries", "style_key_changes_document"),
            ]
        );
        let village = &plan.findings[1];
        assert_eq!(village.style_ref, "gt/rwanda_villages_print");
        assert_eq!(
            village.candidates,
            [
                Candidate {
                    key: key("village_boundaries", "cold_existing", "project"),
                    style_ref: "gt/village_boundaries_print".into(),
                },
                Candidate {
                    key: key("rwanda_villages", "cold_existing", "project"),
                    style_ref: "gt/rwanda_villages_print".into(),
                },
            ]
        );

        // The owner adopts the governed village document for both layers.
        let choices = BTreeMap::from([
            (
                "bounding_rectangles".to_string(),
                key("bounding_rectangles", "cold_existing", "project"),
            ),
            (
                "village_boundaries".to_string(),
                key("village_boundaries", "cold_existing", "project"),
            ),
        ]);
        let (plan, migrated) = super::plan(template, &catalogue, &choices).unwrap();
        assert_eq!(plan.status, Status::Migrate, "{:?}", plan.findings);
        assert_eq!(plan.legacy_refs, 56);
        assert_eq!(plan.style_keys.len(), 56);
        assert!(plan.style_keys.values().all(|k| k.role == LEGACY_ROLE));
        assert_eq!(
            plan.document_changes,
            [
                DocumentChange {
                    layer: "bounding_rectangles".into(),
                    from: "gt/rwanda_villages_print".into(),
                    to: "gt/village_boundaries_print".into(),
                },
                DocumentChange {
                    layer: "village_boundaries".into(),
                    from: "gt/rwanda_villages_print".into(),
                    to: "gt/village_boundaries_print".into(),
                },
            ]
        );
        let migrated = migrated.unwrap();
        let (resolved, capture) = resolve_style_documents(migrated.clone(), &table(&catalogue))
            .expect("the migrated template resolves its preview styles");
        let printed = &capture["style_refs"];
        let refs = evidence()["gisagara_a0"]["style_refs"].clone();
        for (layer, reference) in refs.as_object().unwrap() {
            let expected = if layer == "village_boundaries" || layer == "bounding_rectangles" {
                "gt/village_boundaries_print"
            } else {
                reference.as_str().unwrap()
            };
            assert_eq!(printed[layer], expected, "{layer}");
        }
        assert_eq!(resolved.style_keys, migrated.style_keys);
        // Planning the migrated template again finds nothing to write.
        let (again, layout) = super::plan(migrated, &catalogue, &BTreeMap::new()).unwrap();
        assert_eq!((again.status, layout.is_none()), (Status::Current, true));
    }

    #[test]
    fn undeclared_ambiguous_and_stray_choices_block_the_whole_template() {
        let mut catalogue = governed_catalogue();
        let mut refs =
            json!({"lv_lines": "master/lv_lines_print", "unknown_layer": "master/x_print"});
        let (plan, _) = super::plan(legacy(&refs), &catalogue, &BTreeMap::new()).unwrap();
        assert_eq!(plan.status, Status::Blocked);
        assert_eq!(plan.findings.len(), 1);
        assert_eq!(plan.findings[0].code, "style_key_undeclared");
        assert!(plan.findings[0].candidates.is_empty());

        refs = json!({"lv_lines": "master/lv_lines_print"});
        let duplicate = catalogue
            .bindings
            .iter()
            .find(|b| b.key == key("lv_lines", "saved_design", "project"))
            .cloned()
            .unwrap();
        catalogue.bindings.push(duplicate);
        let (plan, _) = super::plan(legacy(&refs), &catalogue, &BTreeMap::new()).unwrap();
        assert_eq!(plan.findings[0].code, "style_key_ambiguous");

        let catalogue = governed_catalogue();
        let stray = BTreeMap::from([("tr".to_string(), key("tr", "saved_design", "project"))]);
        let (plan, _) = super::plan(legacy(&refs), &catalogue, &stray).unwrap();
        assert_eq!(plan.findings[0].code, "style_choice_unused");
        let undeclared = BTreeMap::from([(
            "lv_lines".to_string(),
            key("lv_lines", "saved_design", "nowhere"),
        )]);
        let (plan, _) = super::plan(legacy(&refs), &catalogue, &undeclared).unwrap();
        assert_eq!(plan.findings[0].code, "style_choice_undeclared");
        assert_eq!(plan.status, Status::Blocked);
    }

    #[test]
    fn an_authored_key_wins_and_its_stale_ref_is_dropped() {
        let catalogue = governed_catalogue();
        let mut layout =
            legacy(&json!({"tr": "master/tr_print", "lv_lines": "master/lv_lines_print"}));
        layout
            .style_keys
            .insert("tr".into(), key("tr", "saved_design", "focused"));
        let (plan, migrated) = super::plan(layout, &catalogue, &BTreeMap::new()).unwrap();
        assert_eq!(plan.status, Status::Migrate);
        assert_eq!(plan.style_keys.keys().collect::<Vec<_>>(), ["lv_lines"]);
        let migrated = migrated.unwrap();
        assert!(migrated.style_refs.is_empty());
        assert_eq!(migrated.style_keys["tr"].role, "focused");
    }
}
