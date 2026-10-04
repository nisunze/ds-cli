//! Exact governed style additions held beside the server's immutable sheets.
//! Hosts capture authority once; renderers admit bytes and never choose a head.
use super::{context, renderer_defaults, Layout};
use crate::style_resolution::{self, Key, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SCHEMA: &str = "ds.print-style-context/v1";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutCapture {
    pub keys: BTreeMap<String, Key>,
    pub held: Value,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub schema: String,
    pub project: String,
    pub lane: String,
    pub principal: String,
    pub credential_audience_sha256: String,
    /// Unchanged provenance of the server-issued receipt, never recomputed.
    pub server_sheets_sha256: String,
    /// Semantic source fence, independent of the server's exact JSON spelling.
    pub source_sheets_sha256: String,
    pub layouts: BTreeMap<String, LayoutCapture>,
}
pub fn source_digest(value: &Value) -> Result<String, String> {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), canonical(v)))
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }
    serde_json::to_vec(&canonical(value))
        .map(|bytes| crate::report_export::sha256_hex(&bytes))
        .map_err(|e| format!("print_style_capture_invalid: {e}"))
}
pub fn layout_digest(layout: &Layout) -> Result<String, String> {
    source_digest(&serde_json::to_value(layout).map_err(|e| e.to_string())?)
}
fn keys(layout: &Layout) -> BTreeMap<String, Key> {
    let mut keys = layout.style_keys.clone();
    keys.extend(context::missing_context_style_keys(layout));
    for key in keys.values_mut() {
        key.ink = layout.ink_mode;
    }
    keys
}
impl Capture {
    pub fn new(
        snapshot: &Snapshot,
        project: &str,
        lane: &str,
        principal: &str,
        audience: &str,
        server_sheets_sha256: &str,
        sheets: &Value,
        layouts: &[Layout],
    ) -> Result<Self, String> {
        if snapshot.project_id != project {
            return Err("print_style_capture_context_mismatch: resolver project changed".into());
        }
        let renderer = renderer_defaults::resolve(snapshot)?;
        let mut captured = BTreeMap::new();
        let mut total_bytes = 0usize;
        for layout in layouts {
            let id = layout_digest(layout)?;
            if captured.contains_key(&id) {
                continue;
            }
            let keys = keys(layout);
            let mut held =
                style_resolution::prepare_print(snapshot, &keys).map_err(|e| e.to_string())?;
            held["renderer_defaults"] =
                serde_json::to_value(&renderer).map_err(|e| e.to_string())?;
            let item = LayoutCapture { keys, held };
            total_bytes = total_bytes
                .checked_add(serde_json::to_vec(&item).map_err(|e| e.to_string())?.len())
                .ok_or("print_style_capture_invalid: capture size overflow")?;
            if total_bytes > 32 * 1024 * 1024 || captured.len() >= 2000 {
                return Err(
                    "print_style_capture_invalid: selected style capture exceeds its bounded size"
                        .into(),
                );
            }
            captured.insert(id, item);
        }
        let result = Self {
            schema: SCHEMA.into(),
            project: project.into(),
            lane: lane.into(),
            principal: principal.into(),
            credential_audience_sha256: audience.into(),
            server_sheets_sha256: server_sheets_sha256.into(),
            source_sheets_sha256: source_digest(sheets)?,
            layouts: captured,
        };
        result.validate_scope(project, lane, principal, audience, sheets)?;
        Ok(result)
    }
    pub fn validate_scope(
        &self,
        project: &str,
        lane: &str,
        principal: &str,
        audience: &str,
        sheets: &Value,
    ) -> Result<(), String> {
        let digest = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if self.schema != SCHEMA
            || self.project != project
            || self.lane != lane
            || self.principal != principal
            || self.credential_audience_sha256 != audience
            || !matches!(lane, "stable" | "canary")
            || project.trim().is_empty()
            || project.len() > 500
            || principal.len() > 500
            || principal.trim().is_empty()
            || !digest(audience)
            || !digest(&self.server_sheets_sha256)
            || self.layouts.len() > 2000
        {
            return Err(
                "print_style_capture_context_mismatch: authenticated print source changed".into(),
            );
        }
        if self.source_sheets_sha256 != source_digest(sheets)? {
            return Err("print_style_capture_stale: project sheets changed after capture".into());
        }
        for (source, layout) in &self.layouts {
            if !digest(source) {
                return Err("print_style_capture_invalid: invalid layout source digest".into());
            }
            let refs = serde_json::from_value(layout.held["style_refs"].clone())
                .map_err(|e| format!("print_style_capture_invalid: {e}"))?;
            style_resolution::admit_print_capture(project, &refs, &layout.held)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn for_layout(&self, layout: &Layout) -> Result<Value, String> {
        let held = self
            .layouts
            .get(&layout_digest(layout)?)
            .ok_or("print_style_capture_stale: final effective layout changed after capture")?;
        if held.keys != keys(layout) {
            return Err("print_style_capture_stale: selected style keys changed".into());
        }
        let refs = serde_json::from_value(held.held["style_refs"].clone())
            .map_err(|e| format!("print_style_capture_invalid: {e}"))?;
        style_resolution::admit_print_capture(&self.project, &refs, &held.held)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn source() -> (Snapshot, Layout, Value) {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/printing-source-bindings.json"
        ))
        .unwrap();
        let snapshot = Snapshot {
            schema: style_resolution::SCHEMA.into(),
            project_id: "project-a".into(),
            revision_id: "b".repeat(64),
            seed_version: None,
            style_presets: None,
            missing_style_refs: vec![],
            bindings: serde_json::from_value(fixture["bindings"].clone()).unwrap(),
        };
        let mut layout = super::super::default_layout();
        let mut context = context::derived_layers().remove(0);
        context.source = super::super::PrintContextSource::ProjectDsgridMv {
            buffer_m: 200.,
            corridor: Some(super::super::PrintCorridor {
                distance_m: 6.,
                color: Some("#957197".into()),
                width_mm: Some(0.15),
                opacity: Some(0.3),
                dash_mm: Some(vec![2., 1.]),
            }),
        };
        for id in super::super::map::MV_PRINT_LAYER_IDS {
            layout.style_refs.insert(id.into(), format!("held/{id}"));
        }
        layout.context_layers = vec![context];
        (
            snapshot,
            layout,
            json!({"project_settings":[{"parameter":"report_locale","value":"en"}]}),
        )
    }
    #[test]
    fn authentic_missing_corridor_materializes_without_mutating_held_source() {
        let (snapshot, layout, sheets) = source();
        let before = serde_json::to_vec(&layout).unwrap();
        let capture = Capture::new(
            &snapshot,
            "project-a",
            "stable",
            "user-a",
            &"a".repeat(64),
            &"c".repeat(64),
            &sheets,
            &[layout.clone()],
        )
        .unwrap();
        let held = capture.for_layout(&layout).unwrap();
        assert_eq!(
            held["style_refs"]["dsgrid_mv_corridor"],
            "print_context/dsgrid_mv_corridor_print"
        );
        assert_eq!(held["style_refs"].as_object().unwrap().len(), 1);
        assert_eq!(serde_json::to_vec(&layout).unwrap(), before);
        assert_eq!(capture.server_sheets_sha256, "c".repeat(64));
        assert!(held["renderer_defaults"].is_object());
    }
    #[test]
    fn cross_project_identity_stale_configuration_layout_and_document_refuse() {
        let (snapshot, mut layout, sheets) = source();
        let capture = Capture::new(
            &snapshot,
            "project-a",
            "stable",
            "user-a",
            &"a".repeat(64),
            &"c".repeat(64),
            &sheets,
            &[layout.clone()],
        )
        .unwrap();
        for (project, lane, principal, audience) in [
            ("project-b", "stable", "user-a", "a".repeat(64)),
            ("project-a", "canary", "user-a", "a".repeat(64)),
            ("project-a", "stable", "user-b", "a".repeat(64)),
            ("project-a", "stable", "user-a", "d".repeat(64)),
        ] {
            assert!(capture
                .validate_scope(project, lane, principal, &audience, &sheets)
                .is_err());
        }
        let mut changed = sheets.clone();
        changed["project_settings"][0]["value"] = json!("fr");
        assert!(capture
            .validate_scope("project-a", "stable", "user-a", &"a".repeat(64), &changed)
            .unwrap_err()
            .contains("stale"));
        layout.name.push_str(" changed");
        assert!(capture.for_layout(&layout).unwrap_err().contains("stale"));
        let mut wrong = capture.clone();
        let held = &mut wrong.layouts.values_mut().next().unwrap().held;
        held["print_styles"]["print_context/dsgrid_mv_corridor_print"]["paint"]["line-color"] =
            json!("#123456");
        assert!(wrong
            .validate_scope("project-a", "stable", "user-a", &"a".repeat(64), &sheets)
            .is_err());
    }
}
