//! A retained A4 PDF belongs to one project template and saved transformer.
//! Its binding is provenance and an input fence, never an upload authority.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "ds.printing-preview/v1";
pub const TEMPLATE: &str = "voltage-drop-a4-v1";
pub const VARIANT: &str = "printing-preview-voltage-drop-a4-v1";

/// A retained head is read under the authenticated project and requested
/// transformer. This checks its captured provenance, without reading current
/// inputs: changes to those inputs do not invalidate already published bytes.
pub fn validate_head(
    project: &str,
    transformer: &str,
    head: &crate::compute_artifact_inventory::Head,
) -> Result<(), String> {
    let binding = head
        .printing_preview
        .as_ref()
        .ok_or("printing_preview_invalid: retained binding is missing")?;
    if head.engine != "network_reporter"
        || head.operation != format!("export-{transformer}")
        || head.variant != VARIANT
        || binding.input_base_fingerprint(project, transformer)? != head.input_base_fingerprint
        || head.outputs.len() != 1
        || head.outputs[0].output_id != "voltage_drop_pdf"
        || head.outputs[0].format != "voltage_drop_pdf"
        || head.outputs[0].content_type != "application/pdf"
        || head.outputs[0].paper_size.as_deref() != Some("A4")
        || head.outputs[0].presentation.is_some()
        || !head.engine_version.as_deref().is_some_and(|engine| {
            crate::report::publication_state(engine, true, false)
                == Ok(crate::report::PublicationState::Pending)
        })
        || !head
            .engine_build_manifest_sha256
            .as_deref()
            .is_some_and(digest)
    {
        return Err("printing_preview_invalid: retained head differs from its project, transformer or release PDF".into());
    }
    crate::compute_artifact_inventory::decode_head(
        &serde_json::to_vec(head).map_err(|error| error.to_string())?,
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub schema: String,
    pub template_id: String,
    pub template_revision_id: String,
    pub template_html_sha256: String,
    pub analysis_sha256: String,
    pub sheets_sha256: String,
    pub report_input_fingerprint: String,
    pub renderer_style_ref: String,
    pub renderer_revision_id: String,
    pub renderer_content_sha256: String,
    pub transformer_revision: i64,
    pub content_digest: String,
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

impl Binding {
    pub fn validate(&self) -> Result<(), String> {
        let reference = self.renderer_style_ref.split('/').collect::<Vec<_>>();
        if self.schema != SCHEMA
            || self.template_id != TEMPLATE
            || self.transformer_revision <= 0
            || ![
                &self.template_revision_id,
                &self.template_html_sha256,
                &self.analysis_sha256,
                &self.sheets_sha256,
                &self.report_input_fingerprint,
                &self.renderer_content_sha256,
                &self.content_digest,
            ]
            .iter()
            .all(|value| digest(value))
            || !id(&self.renderer_revision_id)
            || reference.is_empty()
            || reference.len() > 2
            || !reference.iter().all(|part| id(part))
            || !reference
                .last()
                .is_some_and(|part| part.ends_with("_print"))
        {
            return Err("printing_preview_invalid: exact A4, saved analysis and renderer policy pins are required".into());
        }
        Ok(())
    }

    /// Length-framed UTF-8 fields, in this exact order, shared with Brain.
    pub fn input_base_fingerprint(
        &self,
        project: &str,
        transformer: &str,
    ) -> Result<String, String> {
        self.validate()?;
        if !id(project) || !crate::report_export::valid_transformer_identity(transformer) {
            return Err(
                "printing_preview_invalid: explicit project and canonical transformer are required"
                    .into(),
            );
        }
        let revision = self.transformer_revision.to_string();
        let mut hash = Sha256::new();
        hash.update(b"ds.printing-preview/input-base/v1\0");
        for value in [
            project,
            transformer,
            &self.template_id,
            &self.template_revision_id,
            &self.template_html_sha256,
            &self.analysis_sha256,
            &self.sheets_sha256,
            &self.report_input_fingerprint,
            &self.renderer_style_ref,
            &self.renderer_revision_id,
            &self.renderer_content_sha256,
            revision.as_str(),
            &self.content_digest,
        ] {
            hash.update((value.len() as u64).to_be_bytes());
            hash.update(value.as_bytes());
        }
        Ok(format!("{:x}", hash.finalize()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> Binding {
        Binding {
            schema: SCHEMA.into(),
            template_id: TEMPLATE.into(),
            template_revision_id: "a".repeat(64),
            template_html_sha256: "b".repeat(64),
            analysis_sha256: "c".repeat(64),
            sheets_sha256: "d".repeat(64),
            report_input_fingerprint: "1".repeat(64),
            renderer_style_ref: "renderer_defaults_print".into(),
            renderer_revision_id: "revision_1".into(),
            renderer_content_sha256: "e".repeat(64),
            transformer_revision: 2,
            content_digest: "f".repeat(64),
        }
    }
    #[test]
    fn every_captured_source_and_project_fences_the_pdf() {
        let value = binding();
        let base = value
            .input_base_fingerprint("project_a", "transformer_a")
            .unwrap();
        assert_eq!(
            base,
            "5a787f70c8603e0467bb3b927f2e5af0d91fadffb1ec06b691e49775a7161f55"
        );
        for changed in ["project_b", "project_a_b"] {
            assert_ne!(
                base,
                value
                    .input_base_fingerprint(changed, "transformer_a")
                    .unwrap()
            );
        }
        let mut changed = value.clone();
        changed.analysis_sha256 = "0".repeat(64);
        assert_ne!(
            base,
            changed
                .input_base_fingerprint("project_a", "transformer_a")
                .unwrap()
        );
        changed = value.clone();
        changed.renderer_revision_id = "revision_2".into();
        assert_ne!(
            base,
            changed
                .input_base_fingerprint("project_a", "transformer_a")
                .unwrap()
        );
        assert_ne!(
            base,
            value
                .input_base_fingerprint("project_a", "transformer_b")
                .unwrap()
        );
    }
    #[test]
    fn foreign_template_bad_pins_and_unversioned_analysis_refuse() {
        let mut value = binding();
        value.template_id = "another-template".into();
        assert!(value.validate().is_err());
        value = binding();
        value.transformer_revision = 0;
        assert!(value.validate().is_err());
        value = binding();
        value.analysis_sha256 = "C".repeat(64);
        assert!(value.validate().is_err());
        value = binding();
        value.renderer_style_ref = "renderer_defaults".into();
        assert!(value.validate().is_err());
    }

    #[test]
    fn retained_head_checks_its_project_and_transformer_without_current_inputs() {
        let binding = binding();
        let mut head: crate::compute_artifact_inventory::Head = serde_json::from_value(
            serde_json::json!({
                "printing_preview": binding,
                "work_id": "work_1", "engine": "network_reporter",
                "operation": "export-transformer_a", "variant": VARIANT,
                "engine_version": format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
                "engine_build_manifest_sha256": "b".repeat(64),
                "input_base_fingerprint": binding.input_base_fingerprint("project_a", "transformer_a").unwrap(),
                "head_revision": 1, "updated_at": "2026-10-05T12:00:00Z",
                "outputs": [{"output_id":"voltage_drop_pdf", "format":"voltage_drop_pdf",
                    "filename":crate::report_formats::report_filename("transformer_a", "voltage_drop_pdf").unwrap(),
                    "content_type":"application/pdf", "sha256":"c".repeat(64),
                    "size_bytes":42, "paper_size":"A4"}]
            }),
        ).unwrap();
        validate_head("project_a", "transformer_a", &head).unwrap();
        assert!(validate_head("project_b", "transformer_a", &head).is_err());
        assert!(validate_head("project_a", "transformer_b", &head).is_err());
        head.engine_version = Some(crate::report::DEVELOPMENT_ENGINE.into());
        assert!(validate_head("project_a", "transformer_a", &head).is_err());
    }
}
