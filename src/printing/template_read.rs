//! Admission of one saved layout selected by its exact catalogue location.
use super::{
    catalogue::{Location, Purpose},
    Layout,
};
use serde_json::Value;

pub fn admit_location(location: &Location) -> Result<&str, String> {
    match location {
        Location::Layout { id } if super::identifier(id) => Ok(id),
        _ => Err(
            "print_document_invalid: transformer SVG preview requires an exact layout location"
                .into(),
        ),
    }
}
pub fn admit(location: &Location, response: &Value) -> Result<Layout, String> {
    let id = admit_location(location)?;
    let revision = response["revision"].as_str().unwrap_or_default();
    if response["id"] != id
        || revision.len() != 64
        || !revision
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("print_document_invalid: saved layout identity or revision differs from the selected location".into());
    }
    let layout: Layout = serde_json::from_value(response["layout"].clone())
        .map_err(|e| format!("print_document_invalid: {e}"))?;
    super::validate(&layout)?;
    if layout.id != id
        || layout.mv.is_some()
        || layout
            .classification
            .is_some_and(|c| c.purpose != Purpose::LvTransformer)
    {
        return Err(
            "print_document_invalid: selected layout is not a transformer SVG sheet".into(),
        );
    }
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn exact_saved_layout_is_admitted_and_crossed_identity_revision_or_subject_refuses() {
        let mut layout = super::super::default_layout();
        layout.id = "focus-a0".into();
        let location = Location::Layout {
            id: layout.id.clone(),
        };
        let response = json!({"id":layout.id,"revision":"a".repeat(64),"layout":layout});
        assert_eq!(admit(&location, &response).unwrap(), layout);
        for field in ["id", "revision"] {
            let mut bad = response.clone();
            bad[field] = json!("other");
            assert!(admit(&location, &bad).is_err());
        }
        let mut bad = response.clone();
        bad["layout"]["id"] = json!("other");
        assert!(admit(&location, &bad).is_err());
        let mut bad = response;
        bad["layout"]["classification"] = json!({"purpose":"project","role":"overview"});
        assert!(admit(&location, &bad).is_err());
        assert!(admit_location(&Location::Layout {
            id: "../other".into()
        })
        .is_err());
    }
}
