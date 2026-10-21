//! Read-only admission of one project-owned front-matter page against its
//! catalogue location. Composition and project identity binding stay separate.
use super::catalogue::{Location, FRONT_MATTER_ROLES};
use super::standards::{TemplateDocument, Templates, TEMPLATE_SCHEMA};
use serde_json::Value;

pub fn preview_subject(document: &TemplateDocument) -> Result<super::preview::SubjectKind, String> {
    let classification = super::catalogue::Classification {
        purpose: super::catalogue::Purpose::FrontMatter,
        role: document.role,
    };
    classification.admit()?;
    Ok(super::preview::SubjectKind::of(classification))
}

/// Optional source of a captured page, shared by headless and paired preview.
/// Its pins fence reuse; it grants no project access and fetches no document.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreviewSource {
    GovernedStandard {
        location: Location,
        document: TemplateDocument,
    },
    /// Original authenticated project-layout get receipt, captured offline.
    /// These pins identify data; the native room still admits the principal.
    ProjectLayout {
        project_id: String,
        id: String,
        revision_id: String,
        content_sha256: String,
        layout: super::Layout,
        /// Original get metadata; the body is retained separately above.
        receipt: Value,
    },
}
impl PreviewSource {
    pub fn render_layout(&self) -> super::Layout {
        let Self::GovernedStandard { document, .. } = self else {
            let Self::ProjectLayout { layout, .. } = self else {
                unreachable!()
            };
            return layout.clone();
        };
        let mut layout = document.layout.clone();
        layout.elements.retain(|element| {
            !(element.kind == super::ElementKind::Logo
                && document.logo_slots.contains(&element.asset)
                && !layout.assets.contains_key(&element.asset))
        });
        layout
    }
    pub fn admit(&self, project: &str, layout: &super::Layout) -> Result<(), String> {
        let Self::GovernedStandard { location, document } = self else {
            let Self::ProjectLayout {
                project_id,
                id,
                revision_id,
                content_sha256,
                layout: original,
                receipt,
            } = self
            else {
                unreachable!()
            };
            if project_id != project
                || !super::identifier(project_id)
                || id != &original.id
                || !super::identifier(id)
                || !revision_pin(revision_id)
                || &super::style_capture::layout_digest(original)? != content_sha256
                || original != layout
                || receipt["id"] != *id
                || receipt["revision"] != *revision_id
                || receipt["name"] != original.name
                || receipt["paper"]
                    != serde_json::to_value(&original.paper).map_err(|e| e.to_string())?
                || receipt["classification"]
                    != serde_json::to_value(&original.classification).map_err(|e| e.to_string())?
                || receipt
                    .get("project_id")
                    .is_some_and(|value| value.as_str() != Some(project))
            {
                return Err("print_document_invalid: saved project, id, revision, original layout or content pin differs".into());
            }
            return super::validate(original);
        };
        let Location::GovernedStandard {
            project_id,
            id,
            template_id,
            revision_id,
            ..
        } = location
        else {
            return Err("print_document_invalid: a governed standard location is required".into());
        };
        if project_id != project || &document.layout != layout {
            return Err(
                "print_document_invalid: captured project or authored layout differs".into(),
            );
        }
        let response = serde_json::json!({"scope":"project","project_id":project_id,"kind":"standard","id":id,
            "template_id":template_id,"revision_id":revision_id,"role":document.role,"document":document});
        admit(location, &response)?;
        Ok(())
    }
}

fn revision_pin(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
}

/// Capture one static saved page from a declared authenticated get response.
/// No read, geometry, model inventory or freshness port is used here.
pub fn capture_project_preview(
    project: String,
    response: Value,
    resolution: &crate::style_resolution::Snapshot,
    text: Value,
    date: String,
) -> Result<Value, String> {
    let layout: super::Layout = serde_json::from_value(response["layout"].clone())
        .map_err(|e| format!("print_document_invalid: saved layout: {e}"))?;
    let id = response["id"]
        .as_str()
        .ok_or("print_document_invalid: saved id missing")?
        .to_owned();
    let revision_id = response["revision"]
        .as_str()
        .ok_or("print_document_invalid: saved revision missing")?
        .to_owned();
    if project != resolution.project_id
        || response["name"] != layout.name
        || response["paper"] != serde_json::to_value(&layout.paper).map_err(|e| e.to_string())?
        || response["classification"]
            != serde_json::to_value(&layout.classification).map_err(|e| e.to_string())?
        || super::preview::expected_subject(&layout) != super::preview::SubjectKind::Layout
    {
        return Err(
            "print_document_invalid: static saved page metadata or policy project differs".into(),
        );
    }
    let mut receipt = response.clone();
    receipt
        .as_object_mut()
        .expect("saved object admitted")
        .remove("layout");
    let source = PreviewSource::ProjectLayout {
        project_id: project.clone(),
        id,
        revision_id,
        content_sha256: super::style_capture::layout_digest(&layout)?,
        layout: layout.clone(),
        receipt,
    };
    source.admit(&project, &layout)?;
    let (resolved, capture) = super::resolve_style_documents(layout.clone(), resolution)?;
    if resolved != layout {
        return Err(
            "print_document_invalid: a static saved page cannot substitute authored style bindings"
                .into(),
        );
    }
    let plan: Value = serde_json::from_str(&super::evaluate(&serde_json::to_vec(&serde_json::json!({
        "op":"render_plan","layout":layout,"available_style_refs":[],"layer_count":0,"text":text,"date":date
    })).map_err(|e|e.to_string())?)?).map_err(|e|e.to_string())?;
    if plan["ready"] != true {
        return Err("print_document_invalid: saved static capture is not ready".into());
    }
    Ok(
        serde_json::json!({"schema":"ds.print-layout-export/v1","render":{
        "layout":layout,"layers":[],"text":plan["text"],"print_styles":capture["print_styles"],
        "renderer_defaults":capture["renderer_defaults"]},"renderer_defaults":capture["renderer_defaults"],"source":source}),
    )
}

/// Assemble the existing public capture without engineering or acquisition
/// ports. Keep source placeholders intact until admitted producer transport.
pub fn capture_preview(
    location: Location,
    document: TemplateDocument,
    resolution: &crate::style_resolution::Snapshot,
    text: Value,
    date: String,
) -> Result<Value, String> {
    let source = PreviewSource::GovernedStandard {
        location,
        document: document.clone(),
    };
    source.admit(&resolution.project_id, &document.layout)?;
    if preview_subject(&document)? != super::preview::SubjectKind::Layout {
        return Err("printing_acquisition_required: this governed page needs an explicit geographic capture".into());
    }
    let rendered = source.render_layout();
    let (resolved, capture) = super::resolve_style_documents(rendered.clone(), resolution)?;
    if resolved != rendered {
        return Err("print_document_invalid: a static standard page cannot substitute its authored style bindings".into());
    }
    let mut facts = rendered;
    facts.classification = Some(super::catalogue::Classification {
        purpose: super::catalogue::Purpose::FrontMatter,
        role: document.role,
    });
    let plan: Value = serde_json::from_str(&super::evaluate(&serde_json::to_vec(&serde_json::json!({
        "op":"render_plan","layout":facts,"available_style_refs":[],"layer_count":0,"text":text,"date":date
    })).map_err(|error|error.to_string())?)?).map_err(|error|error.to_string())?;
    if plan["ready"] != true {
        return Err("print_document_invalid: static standard capture is not ready".into());
    }
    Ok(
        serde_json::json!({"schema":"ds.print-layout-export/v1","render":{
        "layout":document.layout,"layers":[],"text":plan["text"],"print_styles":capture["print_styles"],"renderer_defaults":capture["renderer_defaults"]},
        "renderer_defaults":capture["renderer_defaults"],"source":source}),
    )
}

/// Reject an unusable held location before the host acquires any document.
pub fn admit_location(location: &Location) -> Result<(), String> {
    let Location::GovernedStandard {
        project_id,
        id,
        template_id,
        revision_id,
        content_sha256,
    } = location
    else {
        return Err("print_document_invalid: a governed standard page location is required".into());
    };
    let digest = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    if !super::identifier(project_id)
        || !super::identifier(id)
        || !super::identifier(template_id)
        || !digest(revision_id)
    {
        return Err("print_document_invalid: the held standard location is invalid".into());
    }
    if !content_sha256.as_deref().is_some_and(digest) {
        return Err("print_document_invalid: the held catalogue lacks a valid standard page content pin; refresh the template library".into());
    }
    Ok(())
}

pub fn admit(location: &Location, response: &Value) -> Result<TemplateDocument, String> {
    admit_location(location)?;
    let Location::GovernedStandard {
        project_id,
        id,
        template_id,
        revision_id,
        content_sha256,
    } = location
    else {
        return Err("print_document_invalid: a governed standard page location is required".into());
    };
    let expected_content = content_sha256.as_ref().ok_or(
        "print_document_invalid: the held catalogue lacks a standard page content pin; refresh the template library",
    )?;
    if !super::identifier(project_id)
        || !super::identifier(id)
        || !super::identifier(template_id)
        || response["scope"] != "project"
        || response["project_id"] != *project_id
        || response["kind"] != "standard"
        || response["id"] != *id
        || response["template_id"] != *template_id
        || response["revision_id"] != *revision_id
    {
        return Err(
            "print_document_invalid: standard response differs from held project or revision"
                .into(),
        );
    }
    let document: TemplateDocument = serde_json::from_value(response["document"].clone())
        .map_err(|error| format!("print_document_invalid: unreadable standard page: {error}"))?;
    if document.template_id != *template_id
        || document.revision_id != *revision_id
        || document.content_sha256 != *expected_content
        || response["role"]
            != serde_json::to_value(document.role).map_err(|error| error.to_string())?
        || !FRONT_MATTER_ROLES.contains(&document.role)
        || revision_id.len() != 64
        || !revision_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("print_document_invalid: standard page differs from the held template, role or revision".into());
    }
    // This existing standard authority validates the document digest, paper,
    // typography, declared semantic layers and intentionally unbound logo slots.
    // Return the original document, preserving every authored layout field.
    let templates = Templates {
        schema: TEMPLATE_SCHEMA.into(),
        project_id: project_id.clone(),
        revision_id: revision_id.clone(),
        documents: vec![document],
    };
    let document = &templates.documents[0];
    templates.document_by_id(
        template_id,
        document.role,
        document.layout.paper.size,
        project_id,
    )?;
    Ok(document.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printing::{default_layout, ElementKind, Paper};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn fixture(template: &str, paper: Paper) -> (Location, Value) {
        let mut layout = default_layout();
        layout.styles.clear();
        layout.style_refs.clear();
        layout
            .elements
            .retain(|element| matches!(element.kind, ElementKind::Text | ElementKind::Rectangle));
        layout.paper.size = paper;
        if paper == Paper::A0 {
            layout.page_mm = [1189., 841.];
        }
        layout.attribution = Some(serde_json::from_value(json!({"font_pt":7,"color":"#595959",
            "svg_font_family":"DejaVu Sans","pdf_font_family":"Helvetica","right_inset_pt":146,"baseline_pt":7})).unwrap());
        let revision = "a".repeat(64);
        let mut document = TemplateDocument {
            template_id: template.into(),
            role: super::super::page_role::PageRole::Cover,
            revision_id: revision.clone(),
            content_sha256: String::new(),
            layout,
            logo_slots: vec![],
            layer_keys: BTreeMap::new(),
        };
        document.content_sha256 = document.content_digest().unwrap();
        let location = Location::GovernedStandard {
            project_id: "project_one".into(),
            id: "cover-page".into(),
            template_id: template.into(),
            revision_id: revision.clone(),
            content_sha256: Some(document.content_sha256.clone()),
        };
        let response = json!({"scope":"project","project_id":"project_one","kind":"standard","id":"cover-page",
            "template_id":template,"revision_id":revision,"role":"cover","document":document});
        (location, response)
    }
    #[test]
    fn mv_and_lv_front_matter_preserve_exact_authored_layouts() {
        for (template, paper) in [
            ("mv-booklet-a3-v1", Paper::A3),
            ("lv-combined-a0-v1", Paper::A0),
        ] {
            let (location, response) = fixture(template, paper);
            let admitted = admit(&location, &response).unwrap();
            assert_eq!(
                serde_json::to_value(admitted).unwrap(),
                response["document"]
            );
            let request = serde_json::to_vec(
                &json!({"op":"read_governed_standard","location":location,"response":response}),
            )
            .unwrap();
            let answer: Value =
                serde_json::from_str(&crate::printing::evaluate(&request).unwrap()).unwrap();
            assert_eq!(answer, response["document"]);
        }
    }
    #[test]
    fn crossed_project_id_template_revision_and_content_refuse() {
        let (location, response) = fixture("mv-booklet-a3-v1", Paper::A3);
        for field in ["project_id", "id", "template_id", "revision_id", "role"] {
            let mut altered = response.clone();
            altered[field] = json!("other");
            assert!(admit(&location, &altered).is_err(), "{field}");
        }
        let mut altered = response.clone();
        altered["document"]["layout"]["name"] = json!("Changed layout");
        assert!(admit(&location, &altered).is_err());
        let mut altered = response;
        altered["document"]["content_sha256"] = json!("b".repeat(64));
        assert!(admit(&location, &altered).is_err());
    }
    #[test]
    fn unbound_authored_logo_slots_stay_in_the_read_only_page() {
        let (mut location, mut response) = fixture("mv-booklet-a3-v1", Paper::A3);
        let mut document: TemplateDocument =
            serde_json::from_value(response["document"].clone()).unwrap();
        let mut logo = document.layout.elements[0].clone();
        logo.id = "client-logo".into();
        logo.kind = ElementKind::Logo;
        logo.asset = "client-logo-slot".into();
        document.layout.elements.push(logo);
        document.logo_slots.push("client-logo-slot".into());
        document.content_sha256 = document.content_digest().unwrap();
        if let Location::GovernedStandard { content_sha256, .. } = &mut location {
            *content_sha256 = Some(document.content_sha256.clone());
        }
        response["document"] = serde_json::to_value(&document).unwrap();
        let admitted = admit(&location, &response).unwrap();
        assert_eq!(
            serde_json::to_value(admitted).unwrap(),
            response["document"]
        );
        assert!(!document.layout.assets.contains_key("client-logo-slot"));
        let source = PreviewSource::GovernedStandard {
            location: location.clone(),
            document: document.clone(),
        };
        source.admit("project_one", &document.layout).unwrap();
        assert!(source.admit("other_project", &document.layout).is_err());
        let mut changed = document.layout.clone();
        changed.name = "Changed page".into();
        assert!(source.admit("project_one", &changed).is_err());
        assert_eq!(
            preview_subject(&document).unwrap(),
            super::super::preview::SubjectKind::Layout
        );
        let rendered = source.render_layout();
        assert!(!rendered
            .elements
            .iter()
            .any(|element| element.id == "client-logo"));
        assert!(document
            .layout
            .elements
            .iter()
            .any(|element| element.id == "client-logo"));
        let defaults: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/governed-renderer-defaults.json"
        ))
        .unwrap();
        let policy = json!({"type":"line","paint":{},"layout":{},"metadata":{"print_renderer_defaults":defaults}});
        let resolution: crate::style_resolution::Snapshot = serde_json::from_value(json!({
            "schema":"ds.style-resolution/v1","project_id":"project_one","revision_id":"a".repeat(64),
            "bindings":[{"key":super::super::renderer_defaults::key(),"style_ref":"test/renderer_defaults_print",
                "revision_id":"a".repeat(64),"content_sha256":crate::style_plan::style_digest(&policy),"scope":"catalogue","document":policy}]
        })).unwrap();
        let capture = capture_preview(
            location.clone(),
            document.clone(),
            &resolution,
            json!({"title":"Cover"}),
            "2026-10-12".into(),
        )
        .unwrap();
        assert_eq!(capture["render"]["layout"], response["document"]["layout"]);
        assert_eq!(capture["source"]["document"], response["document"]);
        assert_eq!(capture["render"]["layers"], json!([]));
        assert_eq!(capture["render"]["text"]["date"], "2026-10-12");
        let intent = json!({"op":"capture_governed_standard_preview","location":location,"document":document,
            "resolution":resolution,"text":{"title":"Cover"},"date":"2026-10-12"});
        let transported: Value = serde_json::from_str(
            &super::super::evaluate(&serde_json::to_vec(&intent).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(transported, capture);
        let mut crossed = resolution.clone();
        crossed.project_id = "foreign".into();
        assert!(capture_preview(
            location.clone(),
            document.clone(),
            &crossed,
            json!({}),
            String::new()
        )
        .is_err());
        let mut missing = resolution;
        missing.bindings.clear();
        assert!(capture_preview(location, document, &missing, json!({}), String::new()).is_err());
    }
    #[test]
    fn a_recomputed_body_digest_cannot_replace_the_catalogue_pin() {
        let (location, mut response) = fixture("mv-booklet-a3-v1", Paper::A3);
        let mut document: TemplateDocument =
            serde_json::from_value(response["document"].clone()).unwrap();
        document.layout.name = "Altered page at unchanged revision".into();
        document.content_sha256 = document.content_digest().unwrap();
        response["document"] = serde_json::to_value(document).unwrap();
        assert!(admit(&location, &response).is_err());
    }
    #[test]
    fn a_legacy_unpinned_location_refuses_before_document_acquisition() {
        let (mut location, _) = fixture("mv-booklet-a3-v1", Paper::A3);
        if let Location::GovernedStandard { content_sha256, .. } = &mut location {
            *content_sha256 = None;
        }
        assert!(admit_location(&location)
            .unwrap_err()
            .contains("refresh the template library"));
    }
    #[test]
    fn saved_static_project_pages_capture_original_receipts_without_model_inputs() {
        let defaults: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/governed-renderer-defaults.json"
        ))
        .unwrap();
        let policy = json!({"type":"line","paint":{},"layout":{},"metadata":{"print_renderer_defaults":defaults}});
        let resolution:crate::style_resolution::Snapshot=serde_json::from_value(json!({
            "schema":"ds.style-resolution/v1","project_id":"project","revision_id":"b".repeat(64),
            "bindings":[{"key":super::super::renderer_defaults::key(),"style_ref":"test/renderer_defaults_print",
                "revision_id":"a".repeat(64),"content_sha256":crate::style_plan::style_digest(&policy),"scope":"catalogue","document":policy}]
        })).unwrap();
        for (template, paper, role) in [
            (
                "mv-booklet-a3-v1",
                Paper::A3,
                super::super::page_role::PageRole::Cover,
            ),
            (
                "mv-booklet-a3-v1",
                Paper::A3,
                super::super::page_role::PageRole::Naming,
            ),
            (
                "lv-combined-a0-v1",
                Paper::A0,
                super::super::page_role::PageRole::Cover,
            ),
        ] {
            let (_, document) = fixture(template, paper);
            let mut layout: super::super::Layout =
                serde_json::from_value(document["document"]["layout"].clone()).unwrap();
            layout.classification = Some(super::super::catalogue::Classification {
                purpose: super::super::catalogue::Purpose::FrontMatter,
                role,
            });
            let response = json!({"id":layout.id,"revision":"c".repeat(64),"name":layout.name,"paper":layout.paper,"classification":layout.classification,"layout":layout});
            let capture = capture_project_preview(
                "project".into(),
                response.clone(),
                &resolution,
                json!({}),
                "2026-10-12".into(),
            )
            .unwrap();
            assert_eq!(capture["render"]["layout"], response["layout"]);
            assert_eq!(capture["render"]["layers"], json!([]));
            assert_eq!(capture["source"]["layout"], response["layout"]);
            assert_eq!(capture["source"]["revision_id"], response["revision"]);
            let source: PreviewSource = serde_json::from_value(capture["source"].clone()).unwrap();
            source.admit("project", &layout).unwrap();
            assert!(source.admit("foreign", &layout).is_err());
            let mut altered = layout.clone();
            altered.name = "changed body".into();
            assert!(source.admit("project", &altered).is_err());
            for field in ["project_id", "id", "revision_id", "content_sha256"] {
                let mut bad = capture["source"].clone();
                bad[field] = json!("foreign");
                let source: PreviewSource = serde_json::from_value(bad).unwrap();
                assert!(source.admit("project", &layout).is_err(), "{field}");
            }
            let mut bad = response;
            bad["revision"] = json!("");
            assert!(capture_project_preview(
                "project".into(),
                bad,
                &resolution,
                json!({}),
                String::new()
            )
            .is_err());
        }
    }
}
