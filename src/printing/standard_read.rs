//! Read-only admission of one project-owned front-matter page against its
//! catalogue location. Composition and project identity binding stay separate.
use super::catalogue::{Location, FRONT_MATTER_ROLES};
use super::standards::{TemplateDocument, Templates, TEMPLATE_SCHEMA};
use serde_json::Value;

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
}
