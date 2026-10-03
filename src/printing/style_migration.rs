//! One explicit migration from legacy `Layout.style_refs` to exact semantic
//! `style_keys`. A context layer takes the key its source declares for new
//! bindings ([`super::context::context_style_key`]); the printed network takes
//! the template's own role. The governed API table must resolve every key to
//! the document the template already prints with. Anything else is a named
//! finding and the template is left unchanged: no partial migration, no
//! inference from a ref's name, and no second resolver.
use super::{context, Layout, PrintContextLayer};
use crate::style_resolution::{self, Ink, Key, Snapshot, Target};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// No legacy ref remains; nothing to write.
    Current,
    /// Every legacy ref maps exactly; the migrated layout is ready to save.
    Migrate,
    /// At least one legacy ref has no exact governed key.
    Blocked,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Finding {
    pub layer: String,
    pub style_ref: String,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub status: Status,
    pub resolution_revision: String,
    /// Proposed keys for the layers that carried legacy refs.
    pub style_keys: BTreeMap<String, Key>,
    pub findings: Vec<Finding>,
    /// The layout to save, present only when `status` is `migrate`.
    #[serde(skip)]
    pub layout: Option<Layout>,
}

/// Plan the migration of one stored layout against one governed table.
pub fn plan(layout: &Layout, snapshot: &Snapshot) -> Result<Plan, String> {
    style_resolution::validate_snapshot(snapshot).map_err(|e| e.to_string())?;
    let mut plan = Plan {
        status: Status::Current,
        resolution_revision: snapshot.revision_id.clone(),
        style_keys: BTreeMap::new(),
        findings: Vec::new(),
        layout: None,
    };
    if layout.style_refs.is_empty() {
        return Ok(plan);
    }
    let sources: BTreeMap<String, &PrintContextLayer> = layout
        .context_layers
        .iter()
        .flat_map(|source| {
            context::expanded_layer_ids(source)
                .into_iter()
                .map(move |id| (id, source))
        })
        .collect();
    let role = if layout.mv.is_some() {
        "mv_context"
    } else {
        "focused"
    };
    for (layer, reference) in &layout.style_refs {
        // An authored key already wins at print time; its stale ref is dropped.
        if layout.style_keys.contains_key(layer) {
            continue;
        }
        let finding = |code, message: String| Finding {
            layer: layer.clone(),
            style_ref: reference.clone(),
            code,
            message,
        };
        let key = match sources.get(layer) {
            Some(source) => context::context_style_key(source, layer),
            None => match network_key(snapshot, layer, reference, role, layout.ink_mode) {
                Ok(key) => key,
                Err((code, message)) => {
                    plan.findings.push(finding(code, message));
                    continue;
                }
            },
        };
        let mut probe = key.clone();
        probe.ink = layout.ink_mode;
        match style_resolution::resolve(snapshot, &probe) {
            Ok(binding) if binding.style_ref == *reference => {
                plan.style_keys.insert(layer.clone(), key);
            }
            Ok(binding) => plan.findings.push(finding(
                "style_key_changes_document",
                format!(
                    "{} resolves to {}, not the template's {reference}",
                    probe.name(),
                    binding.style_ref
                ),
            )),
            Err(refusal) => plan.findings.push(finding(
                if refusal.code == "style_resolution_ambiguous" {
                    "style_key_ambiguous"
                } else {
                    "style_key_undeclared"
                },
                refusal.message,
            )),
        }
    }
    if !plan.findings.is_empty() {
        plan.status = Status::Blocked;
        return Ok(plan);
    }
    let mut migrated = layout.clone();
    migrated.style_keys.extend(plan.style_keys.clone());
    migrated.style_refs.clear();
    if let Err(error) = super::validate(&migrated) {
        plan.findings.push(Finding {
            layer: String::new(),
            style_ref: String::new(),
            code: "style_migration_invalid",
            message: error,
        });
        plan.status = Status::Blocked;
        return Ok(plan);
    }
    plan.status = Status::Migrate;
    plan.layout = Some(migrated);
    Ok(plan)
}

/// The printed network's key: the one source locality the governed table
/// declares for this exact layer, role and document.
fn network_key(
    snapshot: &Snapshot,
    layer: &str,
    reference: &str,
    role: &str,
    ink: Ink,
) -> Result<Key, (&'static str, String)> {
    let declared = snapshot
        .bindings
        .iter()
        .filter(|b| {
            b.key.target == Target::Print
                && b.key.ink == ink
                && b.key.entity_class == layer
                && b.key.role == role
        })
        .collect::<Vec<_>>();
    let same = declared
        .iter()
        .filter(|b| b.style_ref == reference)
        .collect::<Vec<_>>();
    match (same.as_slice(), declared.is_empty()) {
        ([binding], _) => Ok(Key {
            ink: Ink::Colour,
            ..binding.key.clone()
        }),
        ([], true) => Err((
            "style_key_undeclared",
            format!("no governed {role} print style declares {layer}"),
        )),
        ([], false) => Err((
            "style_key_changes_document",
            format!(
                "governed {role} print styles for {layer} name {}, not the template's {reference}",
                declared
                    .iter()
                    .map(|b| b.style_ref.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
        _ => Err((
            "style_key_ambiguous",
            format!(
                "{} source localities declare {role} {layer} with {reference}",
                same.len()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printing::{default_layout, PrintContextSource};
    use crate::style_resolution::{Binding, SCHEMA};
    use serde_json::json;

    fn key(entity: &str, source: &str, role: &str) -> Key {
        context::style_key(entity, source, role)
    }
    fn table(rows: &[(Key, &str)]) -> Snapshot {
        Snapshot {
            schema: SCHEMA.into(),
            project_id: "fixture".into(),
            revision_id: "b".repeat(64),
            seed_version: None,
            style_presets: None,
            missing_style_refs: Vec::new(),
            bindings: rows
                .iter()
                .map(|(key, style_ref)| {
                    let document =
                        json!({"type":"line","paint":{"line-width":0.6,"line-color":"#64748B"}});
                    Binding {
                        key: key.clone(),
                        style_ref: (*style_ref).into(),
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
    fn legacy() -> Layout {
        let mut layout = default_layout();
        layout
            .style_refs
            .insert("lv_lines".into(), "master/lv_lines_print".into());
        layout
            .style_refs
            .insert("airports".into(), "gt/airports_print".into());
        layout.context_layers.push(PrintContextLayer {
            id: "airports".into(),
            label: "Airports".into(),
            source: PrintContextSource::Catalog {
                country: "Rwanda".into(),
                layer: "airports".into(),
            },
        });
        layout
    }
    fn governed() -> Snapshot {
        table(&[
            (
                key("lv_lines", "saved_design", "focused"),
                "master/lv_lines_print",
            ),
            (
                key("lv_lines", "saved_design", "project"),
                "master/lv_lines_print",
            ),
            (
                key("airports", "cold_existing", "context"),
                "gt/airports_print",
            ),
            (
                key("airports", "cold_existing", "project"),
                "gt/airports_print",
            ),
        ])
    }

    #[test]
    fn exact_refs_become_keys_and_the_legacy_refs_are_removed() {
        let plan = plan(&legacy(), &governed()).unwrap();
        assert_eq!(plan.status, Status::Migrate, "{:?}", plan.findings);
        assert_eq!(
            plan.style_keys["lv_lines"],
            key("lv_lines", "saved_design", "focused")
        );
        assert_eq!(
            plan.style_keys["airports"],
            key("airports", "cold_existing", "context")
        );
        let migrated = plan.layout.unwrap();
        assert!(migrated.style_refs.is_empty());
        assert_eq!(migrated.style_keys.len(), 2);
        // Applying it again is a no-op: the second run finds nothing to write.
        let again = super::plan(&migrated, &governed()).unwrap();
        assert_eq!(again.status, Status::Current);
        assert!(again.layout.is_none());
    }

    #[test]
    fn a_key_that_resolves_to_another_document_blocks_the_whole_template() {
        let mut table = governed();
        table.bindings[2].style_ref = "gt/airports_v2_print".into();
        let plan = plan(&legacy(), &table).unwrap();
        assert_eq!(plan.status, Status::Blocked);
        assert!(plan.layout.is_none());
        assert_eq!(plan.findings.len(), 1);
        assert_eq!(plan.findings[0].layer, "airports");
        assert_eq!(plan.findings[0].code, "style_key_changes_document");
    }

    #[test]
    fn an_undeclared_role_or_layer_is_named_never_guessed() {
        let mut layout = legacy();
        layout.style_refs.insert(
            "unconnected_customers".into(),
            "master/customers_print".into(),
        );
        let mut table = governed();
        table.bindings.push(
            table.bindings[0].clone(), // a duplicate exact key is ambiguous
        );
        let plan = plan(&layout, &table).unwrap();
        assert_eq!(plan.status, Status::Blocked);
        let codes = plan
            .findings
            .iter()
            .map(|f| (f.layer.as_str(), f.code))
            .collect::<Vec<_>>();
        assert!(codes.contains(&("unconnected_customers", "style_key_undeclared")));
        assert!(codes.contains(&("lv_lines", "style_key_ambiguous")));
    }
}
