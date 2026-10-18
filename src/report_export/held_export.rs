//! Closed caller-held ordinary GIS export. Admission is shared by every host;
//! hosts prove current authority and execute the existing report/Sync owners.
use super::{jcs, validate_layers, InputReceipt};
use crate::report_formats::{selected_plan, DesignOutputSelection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SCHEMA: &str = "ds.report-held-export/v1";
pub const MAX_BYTES: usize = 128 * 1024 * 1024;
pub const INVALID: &str = "report_held_export_invalid";
pub const UNSAVED: &str = "report_held_export_source_unsaved";
pub const CHANGED: &str = "report_held_export_source_changed";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub project: String,
    pub lane: String,
    pub uid: String,
    pub credential_audience_sha256: String,
    pub transformer: String,
    pub server_version: i64,
    pub server_known: bool,
    pub local_dirty: bool,
    pub content_digest: String,
    pub receipt: InputReceipt,
    pub layers: BTreeMap<String, Value>,
    pub selection: DesignOutputSelection,
    /// Original caller grant, in the reporter's closed wire spelling. The
    /// Reporter alone admits its scope; no host mints or widens a grant.
    pub media_grant: Option<MediaGrant>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MediaGrant {
    pub resolver_origin: String,
    pub bucket: String,
    pub prefix: String,
    pub token: String,
    pub expires_at: String,
}
/// Minimal retained-cache capture. This owns the conversion for native and WASM
/// adapters; neither UI nor host interprets saved-source or output policy.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub project: String,
    pub transformer: String,
    pub room: Option<Room>,
    pub receipt: InputReceipt,
    pub selection: Option<DesignOutputSelection>,
    pub media_grant: Option<MediaGrant>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub source: String,
    pub transformer_name: String,
    pub server_known: bool,
    pub local_dirty: bool,
    pub server_version: Option<i64>,
    pub layers: BTreeMap<String, Value>,
}
impl Capture {
    pub fn prepare(self, lane: &str, uid: &str, audience: &str) -> Result<Request, Refusal> {
        let room = self.room.ok_or_else(|| Refusal { code: UNSAVED, message: "Open and save this transformer through its design owner before an ordinary held export".into() })?;
        if room.source != "lv" || room.transformer_name != self.transformer {
            return Err(invalid("Held room names another transformer or source"));
        }
        let request = Request {
            schema: SCHEMA.into(),
            project: self.project,
            lane: lane.into(),
            uid: uid.into(),
            credential_audience_sha256: audience.into(),
            transformer: self.transformer,
            server_version: room.server_version.unwrap_or(0),
            server_known: room.server_known,
            local_dirty: room.local_dirty,
            content_digest: jcs::layers_content_digest(&room.layers).map_err(invalid)?,
            receipt: self.receipt,
            layers: room.layers,
            selection: self.selection.ok_or_else(|| {
                invalid("Select SHP, KMZ and/or XLSX explicitly for this held export")
            })?,
            media_grant: self.media_grant,
        };
        request.validate()?;
        Ok(request)
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}
fn invalid(message: impl Into<String>) -> Refusal {
    Refusal {
        code: INVALID,
        message: message.into(),
    }
}
impl Request {
    pub fn decode(bytes: &[u8]) -> Result<Self, Refusal> {
        if bytes.len() > MAX_BYTES {
            return Err(invalid("Held export exceeds 128 MiB"));
        }
        let request: Self = serde_json::from_slice(bytes)
            .map_err(|_| invalid("Use the closed ds.report-held-export/v1 input"))?;
        request.validate()?;
        Ok(request)
    }
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != SCHEMA
            || !crate::execution_context::valid_project(&self.project)
            || !matches!(self.lane.as_str(), "stable" | "canary")
            || self.uid.is_empty()
            || self.uid.len() > 256
            || !super::exact_lower_sha256(&self.credential_audience_sha256)
        {
            return Err(invalid(
                "Exact project, principal, lane and audience are required",
            ));
        }
        super::reportable_transformer(&self.transformer).map_err(invalid)?;
        if self.local_dirty || !self.server_known || self.server_version <= 0 {
            return Err(Refusal { code: UNSAVED, message: "Save the held transformer through its governed design owner before an ordinary export; no design is saved by reporting".into() });
        }
        self.receipt.validate().map_err(invalid)?;
        if self.receipt.local_print_recipe.is_some() {
            return Err(invalid(
                "An ordinary export cannot carry a local print recipe",
            ));
        }
        validate_layers(&self.layers).map_err(invalid)?;
        if !super::exact_lower_sha256(&self.content_digest)
            || jcs::layers_content_digest(&self.layers).map_err(invalid)? != self.content_digest
        {
            return Err(invalid(
                "Held layer bytes differ from their saved content digest",
            ));
        }
        let outputs = selected_plan(
            &self.receipt.sheets().map_err(invalid)?,
            false,
            Some(&self.selection),
        )
        .map_err(invalid)?;
        if outputs
            .iter()
            .any(|output| !matches!(output.output_id.as_str(), "shp" | "kmz" | "xlsx"))
        {
            return Err(invalid("This held export supports an explicit SHP, KMZ and/or XLSX selection; use the owning print operation for other outputs"));
        }
        if let Some(grant) = &self.media_grant {
            let bytes =
                serde_json::to_vec(grant).map_err(|_| invalid("Malformed held media grant"))?;
            if bytes.len() > 64 * 1024 {
                return Err(invalid("Held media grant exceeds 64 KiB"));
            }
        }
        Ok(())
    }
    pub fn authority(
        &self,
        project: &str,
        lane: &str,
        uid: &str,
        audience: &str,
    ) -> Result<(), Refusal> {
        if self.project != project
            || self.lane != lane
            || self.uid != uid
            || self.credential_audience_sha256 != audience
        {
            return Err(Refusal { code: "project_context_changed", message: "Held export differs from the authenticated project, principal, lane or audience".into() });
        }
        Ok(())
    }
    pub fn saved_source(&self, version: Option<u64>, digest: Option<&str>) -> Result<(), Refusal> {
        if version != u64::try_from(self.server_version).ok()
            || digest != Some(self.content_digest.as_str())
        {
            return Err(Refusal { code: CHANGED, message: "The saved transformer revision or content changed; retain this held room and explicitly refresh or save it before exporting".into() });
        }
        Ok(())
    }
    pub fn media_document(&self) -> Option<Vec<u8>> {
        self.media_grant.as_ref().map(|grant| {
            serde_json::to_vec(&serde_json::json!({"grants":[grant]})).expect("string-only grant")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn request() -> Request {
        let layers: BTreeMap<String, Value> = serde_json::from_value(json!({"lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30,-2]},"properties":{"id":"p1"}}]}})).unwrap();
        let sheets_json =
            json!({"project_settings":[{"parameter":"report_formats","value":"voltage_drop"}]})
                .to_string();
        Request { schema: SCHEMA.into(), project:"project-a".into(), lane:"canary".into(), uid:"owner".into(), credential_audience_sha256:"a".repeat(64), transformer:"tr_1".into(), server_version:7, server_known:true, local_dirty:false, content_digest:jcs::layers_content_digest(&layers).unwrap(), receipt:InputReceipt {local_print_recipe:None,schema:1,country:"Burundi".into(),sheets_sha256:super::super::sha256_hex(sheets_json.as_bytes()),sheets_json,reference_semantic_sha256:"b".repeat(64)}, layers, selection:serde_json::from_value(json!({"schema":"ds.design-output-selection/v1","prints":[],"geospatial":["shp","kmz"],"tabular":["xlsx"]})).unwrap(),media_grant:None }
    }
    #[test]
    fn selected_gis_needs_no_analysis_and_preserves_bytes() {
        let r = request();
        let bytes = serde_json::to_vec(&r).unwrap();
        let admitted = Request::decode(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&admitted).unwrap(), bytes);
        assert!(admitted
            .saved_source(Some(7), Some(&r.content_digest))
            .is_ok());
        assert!(admitted.media_document().is_none());
    }
    #[test]
    fn dirty_unsaved_and_changed_sources_refuse_before_execution() {
        for change in [0, 1, 2] {
            let mut r = request();
            match change {
                0 => r.local_dirty = true,
                1 => r.server_known = false,
                _ => r.server_version = 0,
            };
            assert_eq!(r.validate().unwrap_err().code, UNSAVED);
        }
        let r = request();
        assert_eq!(
            r.saved_source(Some(8), Some(&r.content_digest))
                .unwrap_err()
                .code,
            CHANGED
        );
        assert_eq!(
            r.saved_source(Some(7), Some(&"c".repeat(64)))
                .unwrap_err()
                .code,
            CHANGED
        );
        assert!(r
            .authority("project-b", "canary", "owner", &"a".repeat(64))
            .is_err());
    }
    #[test]
    fn source_receipt_selection_and_unknown_fields_remain_closed() {
        let mut r = request();
        r.layers.get_mut("lv_poles").unwrap()["features"][0]["properties"]["id"] = json!("changed");
        assert!(r.validate().is_err());
        let mut r = request();
        r.receipt.sheets_json.push(' ');
        assert!(r.validate().is_err());
        let mut r = request();
        r.selection.tabular.push("voltage_drop".into());
        assert!(r.validate().is_err());
        let mut r = request();
        r.selection.geospatial.clear();
        r.selection.tabular.clear();
        assert!(r.validate().is_err());
        let mut raw = serde_json::to_value(request()).unwrap();
        raw["argv"] = json!(["bad"]);
        assert!(Request::decode(&serde_json::to_vec(&raw).unwrap()).is_err());
    }
}
