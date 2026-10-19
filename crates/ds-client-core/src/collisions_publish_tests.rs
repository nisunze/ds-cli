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
