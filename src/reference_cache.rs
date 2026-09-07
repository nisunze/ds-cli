//! Versioned reference-query cache contract. Cloud source resolution belongs to
//! Brain; native/browser hosts persist exact responses and never infer readiness
//! from a catalog row or an incomplete download.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const PAGE_ROWS: usize = 1000;
pub const MAX_PAGE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RESOURCE_BYTES: usize = 64 * 1024 * 1024 * 1024;
pub const MAX_PAGES: usize = 100_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub resource_id: String,
    pub version: String,
}
impl Identity {
    pub fn validate(&self) -> Result<(), String> {
        if [&self.resource_id, &self.version].iter().any(|s| {
            s.len() != 64
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            return Err("invalid reference resource identity".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Page {
    pub resource_id: String,
    pub version: String,
    pub offset: usize,
    pub area: Option<Value>,
    pub feature_json: String,
    pub feature_count: usize,
    pub next_offset: Option<usize>,
    pub sha256: String,
    #[serde(default)]
    pub cache: String,
}
pub fn validate_page(
    identity: &Identity,
    offset: usize,
    page: &Page,
) -> Result<Vec<Value>, String> {
    identity.validate()?;
    if !offset.is_multiple_of(PAGE_ROWS)
        || offset / PAGE_ROWS >= MAX_PAGES
        || page.offset != offset
        || page.area.is_some()
        || page.resource_id != identity.resource_id
        || page.version != identity.version
    {
        return Err("reference page identity or offset mismatch".into());
    }
    if page.feature_json.len() > MAX_PAGE_BYTES
        || format!("{:x}", Sha256::digest(page.feature_json.as_bytes())) != page.sha256
    {
        return Err("reference page content digest mismatch".into());
    }
    let rows: Vec<Value> =
        serde_json::from_str(&page.feature_json).map_err(|_| "invalid reference feature array")?;
    if rows.len() != page.feature_count
        || rows.len() > PAGE_ROWS
        || page
            .next_offset
            .is_some_and(|next| next != offset + PAGE_ROWS || rows.len() != PAGE_ROWS)
    {
        return Err("incomplete or out-of-order reference page".into());
    }
    if rows.iter().any(|row| {
        row["type"] != "Feature"
            || !row["properties"].is_object()
            || !(row["geometry"].is_object() || row["geometry"].is_null())
    }) {
        return Err("invalid reference feature".into());
    }
    Ok(rows)
}
/// Feature envelope used while building a native spatial index.
pub fn feature_bounds(feature: &Value) -> Option<[f64; 4]> {
    ds_geo::viewport::geojson_bounds(feature)
}

pub fn validate_area(area: &Value) -> Result<(), String> {
    ds_geo::spatial_predicate::validate_area(area)
}

pub fn area_bounds(area: &Value) -> Result<[f64; 4], String> {
    ds_geo::spatial_predicate::area_bounds(area)
}

pub fn intersects_area(feature: &Value, area: &Value) -> Result<bool, String> {
    ds_geo::spatial_predicate::intersects_area(feature, area)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (Identity, Page) {
        let identity = Identity {
            resource_id: "a".repeat(64),
            version: "b".repeat(64),
        };
        let text=json!([{"type":"Feature","properties":{"label":"école <A>"},"geometry":{"type":"Point","coordinates":[30.,-2.]}}]).to_string();
        let page = Page {
            offset: 0,
            area: None,
            resource_id: identity.resource_id.clone(),
            version: identity.version.clone(),
            sha256: format!("{:x}", Sha256::digest(text.as_bytes())),
            feature_json: text,
            feature_count: 1,
            next_offset: None,
            cache: "source".into(),
        };
        (identity, page)
    }
    #[test]
    fn proves_bytes_and_rejects_cross_version_or_partial_finalization() {
        let (identity, mut page) = fixture();
        assert_eq!(validate_page(&identity, 0, &page).unwrap().len(), 1);
        page.next_offset = Some(1000);
        assert!(validate_page(&identity, 0, &page).is_err());
        page.next_offset = None;
        page.feature_json.push(' ');
        assert!(validate_page(&identity, 0, &page).is_err());
        let (_, mut page) = fixture();
        page.version = "c".repeat(64);
        assert!(validate_page(&identity, 0, &page).is_err());
    }
    #[test]
    fn area_selected_or_relocated_pages_cannot_declare_a_complete_install() {
        let (identity, mut page) = fixture();
        page.area = Some(
            json!({"type":"Polygon","coordinates":[[[29.,-3.],[31.,-3.],[31.,-1.],[29.,-1.],[29.,-3.]]]}),
        );
        assert!(validate_page(&identity, 0, &page).is_err());
        page.area = None;
        assert!(validate_page(&identity, PAGE_ROWS, &page).is_err());
    }
    #[test]
    fn area_validation_is_polygonal() {
        let (identity, page) = fixture();
        assert_eq!(validate_page(&identity, 0, &page).unwrap().len(), 1);
        assert!(validate_area(&json!({"type":"Point","coordinates":[0.,0.]})).is_err());
    }
}
