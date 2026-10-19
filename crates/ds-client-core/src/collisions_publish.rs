//! Governed save of an edge-computed project collisions result.
//!
//! DS Core computes collisions with the Reporter addon from the saved rooms it
//! holds; this owner only carries the result and the transformer heads it was
//! computed from to ds-brain's `save_collisions` storage action, over the same
//! closed `/api/v1/data` save transport transformer saves use. ds-brain proves
//! the basis against the current heads and commits the stored document set;
//! it never computes. A result whose basis moved is refused, never merged.
use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ServiceRefusal,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const PUBLICATION_SCHEMA: &str = "ds.collisions-publication/v1";
/// The governed route admits a 32 MiB body; leave room for the envelope.
pub const MAX_PUBLICATION_BYTES: usize = 32 * 1024 * 1024 - 64 * 1024;
const MAX_TRANSFORMERS: usize = 2_000;
const LAYERS: [&str; 3] = [
    "collisions",
    "collisions_lv_lines",
    "collisions_service_cables",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionsPublication {
    pub schema: String,
    pub computed_at: String,
    pub engine: Value,
    pub basis: Vec<CollisionsBasisEntry>,
    pub input_errors: Vec<CollisionsInputError>,
    pub layers: BTreeMap<String, Value>,
    pub outcome: Value,
}

/// One transformer room the result was computed from, at the saved version
/// the edge actually read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionsBasisEntry {
    pub name: String,
    pub version: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollisionsInputError {
    pub transformer: String,
    pub error_message: String,
}

/// ds-brain's commit receipt for one publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollisionsPublicationReceipt {
    pub project_id: String,
    pub saved: bool,
    pub regions: u64,
    pub layers: BTreeMap<String, u64>,
    pub chunk_count: u64,
    pub transformers: u64,
    pub computed_at: String,
    pub updated_at: i64,
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn unreadable(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}

fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl CollisionsPublication {
    /// Client-side preflight: the shape ds-brain admits, before any transport.
    /// ds-brain repeats every check and adds the source-basis proof.
    pub fn validate(&self) -> Result<(), ClientError> {
        if self.schema != PUBLICATION_SCHEMA
            || self.computed_at.trim().is_empty()
            || !self.engine.is_object()
            || self.outcome["stage"] != "collision_detection"
            || self.outcome["status"] != "success"
        {
            return Err(invalid("collisions_publication_invalid"));
        }
        if self.basis.len() + self.input_errors.len() > MAX_TRANSFORMERS
            || (self.basis.is_empty() && !self.input_errors.is_empty())
        {
            return Err(invalid("collisions_publication_invalid"));
        }
        let mut seen = BTreeSet::new();
        for transformer in self.basis.iter().map(|entry| entry.name.as_str()).chain(
            self.input_errors
                .iter()
                .map(|entry| entry.transformer.as_str()),
        ) {
            if !name(transformer) || !seen.insert(transformer) {
                return Err(invalid("collisions_publication_invalid"));
            }
        }
        if self.layers.iter().any(|(layer, value)| {
            !LAYERS.contains(&layer.as_str()) || value["type"] != "FeatureCollection"
        }) {
            return Err(invalid("collisions_publication_invalid"));
        }
        Ok(())
    }
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    publication: &CollisionsPublication,
) -> Result<CollisionsPublicationReceipt, ClientError> {
    publication.validate()?;
    let body = serde_json::to_vec(&json!({
        "action": "save_collisions",
        "eds_project_id": project,
        "collisions": publication,
    }))
    .map_err(|_| invalid("collisions_publication_invalid"))?;
    if body.len() > MAX_PUBLICATION_BYTES {
        return Err(invalid("collisions_publication_too_large"));
    }
    let response = transport.transformer_save(crate::TransformerSaveCall {
        profile,
        authority: authority.call(),
        body: &body,
    })?;
    if response.body.len() > 1024 * 1024 {
        return Err(unreadable("collisions_publication_response_too_large"));
    }
    let raw: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    if response.status != 200 {
        let code = raw["error"]["code"].as_str().map(str::to_ascii_lowercase);
        let refusal = ServiceRefusal::with_details(
            response.status,
            code.as_deref(),
            raw["error"]["message"].as_str(),
            raw["data"].as_object(),
        );
        let kind = match response.status {
            400 | 409 | 413 | 422 | 423 => ErrorKind::InvalidInput,
            401 | 403 => ErrorKind::AuthenticationRejected,
            404 => ErrorKind::ResourceNotFound,
            _ => ErrorKind::Transient,
        };
        return Err(ClientError::with_service_refusal(
            kind,
            "the collisions publication was refused by its owner",
            refusal,
        ));
    }
    if raw.get("success").is_some_and(|value| value != true) {
        return Err(unreadable("collisions_publication_response_failed"));
    }
    let receipt = raw.get("data").unwrap_or(&raw);
    let layers = receipt["layers"]
        .as_object()
        .map(|layers| {
            layers
                .iter()
                .map(|(name, count)| count.as_u64().map(|count| (name.clone(), count)))
                .collect::<Option<BTreeMap<_, _>>>()
        })
        .unwrap_or_else(|| Some(BTreeMap::new()));
    let parsed = (|| {
        Some(CollisionsPublicationReceipt {
            project_id: project.to_owned(),
            saved: receipt["saved"].as_bool()?,
            regions: receipt["regions"].as_u64()?,
            layers: layers?,
            chunk_count: receipt["chunk_count"].as_u64()?,
            transformers: receipt["transformers"].as_u64()?,
            computed_at: receipt["computed_at"].as_str()?.to_owned(),
            updated_at: receipt["updated_at"].as_i64()?,
        })
    })()
    .ok_or_else(|| unreadable("collisions_publication_receipt_invalid"))?;
    if !parsed.saved
        || parsed.computed_at != publication.computed_at
        || parsed.transformers != publication.basis.len() as u64
        || parsed.chunk_count == 0
    {
        return Err(unreadable("collisions_publication_receipt_mismatch"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn publication() -> CollisionsPublication {
        serde_json::from_value(json!({
            "schema": PUBLICATION_SCHEMA,
            "computed_at": "2026-10-19T12:00:00Z",
            "engine": {"service": "ds-network-reporter"},
            "basis": [{"name": "tx_a", "version": 3}, {"name": "tx_b", "version": null}],
            "input_errors": [],
            "layers": {"collisions": {"type": "FeatureCollection", "features": []}},
            "outcome": {"stage": "collision_detection", "status": "success", "pairs": 0}
        }))
        .unwrap()
    }

    #[test]
    fn preflight_admits_the_owner_shape_and_refuses_everything_else() {
        assert!(publication().validate().is_ok());
        let mutations: [fn(&mut CollisionsPublication); 7] = [
            |p| p.schema = "ds.collisions-publication/v0".into(),
            |p| p.outcome["status"] = json!("error"),
            |p| p.basis[1].name = "TX B".into(),
            |p| p.basis[1].name = "tx_a".into(),
            |p| {
                p.layers.insert(
                    "collisions_mv_lines".into(),
                    json!({"type": "FeatureCollection"}),
                );
            },
            |p| p.engine = json!("reporter"),
            |p| {
                p.basis.clear();
                p.input_errors.push(CollisionsInputError {
                    transformer: "tx_c".into(),
                    error_message: "offline".into(),
                });
            },
        ];
        for mutate in mutations {
            let mut candidate = publication();
            mutate(&mut candidate);
            assert_eq!(
                candidate.validate().unwrap_err().kind(),
                ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn unknown_members_never_deserialize() {
        let mut value = serde_json::to_value(publication()).unwrap();
        value["force"] = json!(true);
        assert!(serde_json::from_value::<CollisionsPublication>(value).is_err());
    }

    struct SaveTransport {
        status: u16,
        response: Value,
        sent: Vec<Value>,
    }
    macro_rules! no_other_call {
        ($name:ident, $call:ident) => {
            fn $name(
                &mut self,
                _: crate::$call<'_>,
            ) -> Result<crate::TransportResponse, crate::TransportError> {
                panic!("a collisions publication made an unrelated call")
            }
        };
    }
    impl AuthorizedTransport for SaveTransport {
        no_other_call!(list_projects, ProjectListCall);
        no_other_call!(transformer_context, TransformerContextCall);
        no_other_call!(project_forms, ProjectFormsCall);
        no_other_call!(project_form_editor, ProjectFormEditorCall);
        no_other_call!(solar_snapshot, SolarSnapshotCall);
        no_other_call!(survey_query, SurveyQueryCall);
        no_other_call!(survey_entries_select, SurveyEntriesSelectCall);
        no_other_call!(survey_entries_changes, SurveyEntriesChangesCall);
        no_other_call!(survey_entry_create, SurveyEntryCreateCall);
        no_other_call!(project_data, ProjectDataCall);
        no_other_call!(upload_bytes, UploadBytesCall);
        no_other_call!(survey_control, SurveyControlCall);
        no_other_call!(styles, StylesCall);
        no_other_call!(layers, LayersCall);
        no_other_call!(tiles, TileCall);
        no_other_call!(project_report, ProjectReportCall);
        fn transformer_save(
            &mut self,
            call: crate::TransformerSaveCall<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            assert_eq!((call.method(), call.path()), ("POST", "/api/v1/data"));
            assert_eq!(call.bearer_token(), "bearer");
            self.sent
                .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
            Ok(crate::TransportResponse {
                status: self.status,
                body: serde_json::to_vec(&self.response).unwrap(),
            })
        }
    }

    fn publish(transport: &mut SaveTransport) -> Result<CollisionsPublicationReceipt, ClientError> {
        run(
            &crate::data_distribution::tests::profile_for_tests(),
            transport,
            Authority {
                bearer_token: "bearer",
                uid: "uid",
                email: "user@example.com",
                device_id: None,
            },
            "proj-1",
            &publication(),
        )
    }

    #[test]
    fn the_save_carries_the_publication_and_verifies_the_commit_receipt() {
        let mut transport = SaveTransport {
            status: 200,
            response: json!({"success": true, "data": {
                "saved": true, "regions": 0, "layers": {}, "chunk_count": 1,
                "transformers": 2, "computed_at": "2026-10-19T12:00:00Z", "updated_at": 1
            }}),
            sent: Vec::new(),
        };
        let receipt = publish(&mut transport).unwrap();
        assert_eq!(
            (receipt.project_id.as_str(), receipt.chunk_count),
            ("proj-1", 1)
        );
        let sent = &transport.sent[0];
        assert_eq!(sent["action"], "save_collisions");
        assert_eq!(sent["eds_project_id"], "proj-1");
        assert_eq!(sent["collisions"]["basis"][0]["version"], 3);
        assert!(sent["collisions"]["basis"][1]["version"].is_null());

        transport.response["data"]["computed_at"] = json!("2026-10-19T12:00:01Z");
        assert_eq!(
            publish(&mut transport).unwrap_err().kind(),
            ErrorKind::UnreadableResponse,
            "a receipt for another computation is never accepted"
        );
    }

    #[test]
    fn a_moved_basis_is_a_typed_refusal_with_its_counts() {
        let mut transport = SaveTransport {
            status: 409,
            response: json!({"success": false,
                "error": {"code": "COLLISIONS_BASIS_STALE", "message": "Run collision detection again."},
                "data": {"moved": ["tx_b"], "moved_count": 1, "missing_count": 0, "unknown_count": 0}}),
            sent: Vec::new(),
        };
        let error = publish(&mut transport).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        let refusal = error.service_refusal().expect("typed refusal");
        assert_eq!(refusal.code(), Some("collisions_basis_stale"));
        assert_eq!(refusal.detail_integer("moved_count"), Some(1));
    }
}
