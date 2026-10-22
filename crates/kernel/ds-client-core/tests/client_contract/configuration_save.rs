use super::*;

#[test]
fn current_brain_and_legacy_acknowledgements_use_the_same_save_and_verified_readback() {
    // ConfigHandler.SaveConfigViaReport emits message/records. Its metadata is
    // not a document; the subsequent GET supplies the authoritative sheets.
    for acknowledgement in [
        json!({"message":"Updated project_settings","records":1}),
        json!({"success":true}),
        json!({"message":"Updated project_settings","records":1,"future_metadata":{"hint":true}}),
    ] {
        let before = json!({"sheets":{"project_settings":[{"parameter":"limit","value":1}]}});
        let after = json!({"sheets":{"project_settings":[{"parameter":"limit","value":2}]},"future_document_metadata":true});
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        for response in [&before, &acknowledgement, &after] {
            transport
                .0
                .lock()
                .unwrap()
                .configurations
                .push_back(TransportResponse::new(
                    200,
                    serde_json::to_vec(response).unwrap(),
                ));
        }
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        let result = client
            .feeder_configuration(
                "project_a",
                Some(&ds_client_core::ProjectConfigurationChange::SetParameter {
                    sheet: "project_settings".into(),
                    parameter: "limit".into(),
                    raw: json!(2),
                }),
                NOW,
            )
            .unwrap();
        assert_eq!(result.summary["saved"], true);
        assert_eq!(result.document, after);
        let state = transport.0.lock().unwrap();
        assert_eq!(
            state.configuration_bodies,
            vec![
                json!({"action":"save_config","eds_project_id":"project_a","sheet_name":"project_settings","data":[{"parameter":"limit","value":2}]})
            ]
        );
        assert_eq!(
            &state.calls[state.calls.len() - 3..],
            &[
                "config GET /config/project_a",
                "config POST /report",
                "config GET /config/project_a"
            ]
        );
    }
}

#[test]
fn incompatible_save_receipts_refuse_once_without_inventing_a_document_or_retrying_the_write() {
    for bad in [
        json!([]),
        json!(42),
        json!(null),
        json!({"success":false}),
        json!({"success":"yes"}),
    ] {
        let before = json!({"sheets":{"project_settings":[{"parameter":"limit","value":1}]}});
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        for response in [&before, &bad] {
            transport
                .0
                .lock()
                .unwrap()
                .configurations
                .push_back(TransportResponse::new(
                    200,
                    serde_json::to_vec(response).unwrap(),
                ));
        }
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        let error = client
            .feeder_configuration(
                "project_a",
                Some(&ds_client_core::ProjectConfigurationChange::SetParameter {
                    sheet: "project_settings".into(),
                    parameter: "limit".into(),
                    raw: json!(2),
                }),
                NOW,
            )
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
        let state = transport.0.lock().unwrap();
        assert_eq!(state.configuration_bodies.len(), 1);
        assert_eq!(
            &state.calls[state.calls.len() - 2..],
            &["config GET /config/project_a", "config POST /report"]
        );
    }
}
