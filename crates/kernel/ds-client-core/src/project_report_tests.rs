//! Inline tests moved out of project_report.rs (owner rule: tests in their own files).

mod composition_tests {
    use super::super::*;
    use serde_json::json;
    #[test]
    fn archive_catalog_retains_server_authoring_json_and_does_not_invent_legacy_recipes() {
        let response = |body: Value| TransportResponse {
            status: 200,
            body: serde_json::to_vec(&body).unwrap(),
        };
        let template = json!({"schema":"ds.compounded-report/v1","transformers":[],
            "archive_layout":{"group_depth":null,"transformer_folders":true,"combine_per_group":false},"force":false});
        let schema = json!({"type":"object","additionalProperties":false});
        let read = decode_compounded_list(response(json!({"compounded_reports":[],"composition_template":template,"composition_schema":schema})), 1_000_000).unwrap();
        assert!(read.is_empty());
        assert_eq!(read.composition_template(), Some(&template));
        assert_eq!(read.composition_schema(), Some(&schema));
        let legacy =
            decode_compounded_list(response(json!({"compounded_reports":[]})), 1_000_000).unwrap();
        assert_eq!(legacy.composition_template(), None);
        assert_eq!(legacy.composition_schema(), None);
        for malformed in [json!([]), json!("x".repeat(65536))] {
            assert!(decode_compounded_list(
                response(json!({"compounded_reports":[],"composition_template":malformed})),
                1_000_000
            )
            .is_err());
        }
    }
}

mod deleted_backup_tests {
    use super::super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};

    const BUCKET: &str = "ds-backups";
    const OBJECT: &str = "transformers/1788000000-p1-tx_a.dsgrid";

    fn ok(body: Value) -> TransportResponse {
        TransportResponse {
            status: 200,
            body: serde_json::to_vec(&body).unwrap(),
        }
    }

    fn receipt(bytes: &[u8]) -> Value {
        json!({"bucket": BUCKET, "object": OBJECT, "generation": 17,
               "sha256": format!("{:x}", Sha256::digest(bytes)), "byte_length": bytes.len()})
    }

    fn read_body(bytes: &[u8], url: &str) -> Value {
        json!({"eds_project_id": "p1", "action": "read_transformer_backup", "name": "tx_a",
               "source_revision": 4, "layers": ["lv_poles", "lv_lines"], "backup": receipt(bytes),
               "download_url": url, "expires_at": "2026-10-09T10:15:00Z"})
    }

    fn signed() -> String {
        format!(
            "https://storage.googleapis.com/{BUCKET}/{OBJECT}?X-Goog-Signature=abc&generation=17"
        )
    }

    #[test]
    fn recovery_rides_the_fixed_route_without_widening_the_declared_profile_actions() {
        assert!(!crate::PROJECT_REPORT_ACTIONS.contains(&ACTION_READ_DELETED_BACKUP));
        assert!(!crate::PROJECT_REPORT_ACTIONS.contains(&ACTION_RESTORE_DELETED));
        let read = DeletedBackupRequest::read("tx_a".into(), OBJECT.into(), None, None).unwrap();
        let body: Value =
            serde_json::from_slice(&ProjectReportOperation::ReadDeletedBackup(&read).body("p1"))
                .unwrap();
        assert_eq!(
            body,
            json!({"action": "read_transformer_backup", "eds_project_id": "p1",
                   "transformers": ["tx_a"], "backup": {"object": OBJECT}})
        );
        let digest = "a".repeat(64);
        let restore =
            DeletedBackupRequest::restore("tx_a".into(), OBJECT.into(), 17, digest.clone())
                .unwrap();
        let body: Value =
            serde_json::from_slice(&ProjectReportOperation::RestoreDeleted(&restore).body("p1"))
                .unwrap();
        assert_eq!(body["action"], "restore_deleted_transformer");
        assert_eq!(
            body["backup"],
            json!({"object": OBJECT, "generation": 17, "sha256": digest})
        );
    }

    #[test]
    fn requests_hold_the_servers_bounds_before_a_round_trip() {
        for (object, generation, sha) in [
            ("", None, None),
            (" transformers/a.dsgrid", None, None),
            ("transformers\\a.dsgrid", None, None),
            (OBJECT, Some(0), None),
            (OBJECT, None, Some("ABC")),
        ] {
            assert!(
                DeletedBackupRequest::read(
                    "tx_a".into(),
                    object.into(),
                    generation,
                    sha.map(Into::into)
                )
                .is_err(),
                "{object:?} {generation:?} {sha:?}"
            );
        }
        assert!(DeletedBackupRequest::read(String::new(), OBJECT.into(), None, None).is_err());
    }

    #[test]
    fn a_read_is_admitted_only_for_the_asked_object_and_an_exact_storage_locator() {
        let bytes = b"native dsgrid bytes".to_vec();
        let request = DeletedBackupRequest::read("tx_a".into(), OBJECT.into(), None, None).unwrap();
        let read =
            decode_deleted_backup_read(ok(read_body(&bytes, &signed())), 1 << 20, "p1", &request)
                .expect("verified read");
        assert_eq!(read.backup().generation(), 17);
        assert_eq!(read.layers(), ["lv_poles", "lv_lines"]);
        assert!(read.bytes().is_none());
        assert!(!format!("{:?}", read.backup()).contains("Signature"));
        let gs = DeletedBackupRequest::read(
            "tx_a".into(),
            format!("gs://{BUCKET}/{OBJECT}"),
            Some(17),
            None,
        )
        .unwrap();
        assert!(
            decode_deleted_backup_read(ok(read_body(&bytes, &signed())), 1 << 20, "p1", &gs)
                .is_ok()
        );
        for url in [
            format!("https://example.com/{BUCKET}/{OBJECT}?sig=1"),
            format!("https://storage.googleapis.com/{BUCKET}/transformers/other.dsgrid?sig=1"),
            format!("https://storage.googleapis.com/other/{OBJECT}?sig=1"),
            format!("http://storage.googleapis.com/{BUCKET}/{OBJECT}?sig=1"),
        ] {
            assert_eq!(
                decode_deleted_backup_read(ok(read_body(&bytes, &url)), 1 << 20, "p1", &request)
                    .unwrap_err()
                    .kind(),
                ErrorKind::UnreadableResponse,
                "{url}"
            );
        }
        let pinned_elsewhere =
            DeletedBackupRequest::read("tx_a".into(), OBJECT.into(), Some(18), None).unwrap();
        assert!(decode_deleted_backup_read(
            ok(read_body(&bytes, &signed())),
            1 << 20,
            "p1",
            &pinned_elsewhere
        )
        .is_err());
        assert!(decode_deleted_backup_read(
            ok(read_body(&bytes, &signed())),
            1 << 20,
            "p2",
            &request
        )
        .is_err());
    }

    struct BackupBytes {
        status: u16,
        body: Vec<u8>,
        calls: usize,
    }
    macro_rules! no_other_call {
        ($name:ident, $call:ident) => {
            fn $name(
                &mut self,
                _: crate::$call<'_>,
            ) -> Result<crate::TransportResponse, crate::TransportError> {
                panic!("a backup fetch made an unrelated call")
            }
        };
    }
    impl crate::AuthorizedTransport for BackupBytes {
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
        fn grid_model_bytes(
            &mut self,
            call: crate::grid_models::DownloadCall<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            self.calls += 1;
            assert_eq!(call.url(), signed());
            Ok(crate::TransportResponse::new(
                self.status,
                self.body.clone(),
            ))
        }
    }

    #[test]
    fn fetched_backup_bytes_are_admitted_only_at_the_receipts_length_and_digest() {
        let bytes = b"native dsgrid bytes".to_vec();
        let request = DeletedBackupRequest::read("tx_a".into(), OBJECT.into(), None, None).unwrap();
        let decode = || {
            decode_deleted_backup_read(ok(read_body(&bytes, &signed())), 1 << 20, "p1", &request)
                .unwrap()
        };
        let mut good = BackupBytes {
            status: 200,
            body: bytes.clone(),
            calls: 0,
        };
        let mut read = decode();
        fetch_deleted_backup(&mut good, &mut read).expect("verified bytes");
        assert_eq!(read.bytes(), Some(bytes.as_slice()));
        let mut altered = bytes.clone();
        altered[0] ^= 1;
        for (status, body) in [
            (200, altered),
            (200, bytes[1..].to_vec()),
            (403, bytes.clone()),
        ] {
            let mut transport = BackupBytes {
                status,
                body,
                calls: 0,
            };
            let mut read = decode();
            assert!(fetch_deleted_backup(&mut transport, &mut read).is_err());
            assert!(read.bytes().is_none());
            assert_eq!(transport.calls, 1);
        }
    }

    #[test]
    fn a_restoration_is_the_asked_transformer_from_the_pinned_backup() {
        let bytes = b"native dsgrid bytes".to_vec();
        let backup = receipt(&bytes);
        let digest = backup["sha256"].as_str().unwrap().to_owned();
        let request =
            DeletedBackupRequest::restore("tx_a".into(), OBJECT.into(), 17, digest).unwrap();
        let body = |state: &str, generation: i64| {
            let mut backup = backup.clone();
            backup["generation"] = json!(generation);
            json!({"eds_project_id": "p1", "action": "restore_deleted_transformer",
                   "restoration": {"name": "tx_a", "state": state, "restored_at": 1788000000000_i64,
                                   "restored_by": "owner@example.com", "source_revision": 4,
                                   "version": 5, "layers": ["lv_poles"], "backup": backup}})
        };
        let restored = decode_deleted_restoration(ok(body("active", 17)), 1 << 20, "p1", &request)
            .expect("restored");
        assert_eq!(restored.version(), 5);
        assert_eq!(restored.backup().generation(), 17);
        assert!(
            decode_deleted_restoration(ok(body("retired", 17)), 1 << 20, "p1", &request).is_ok()
        );
        assert!(
            decode_deleted_restoration(ok(body("deleted", 17)), 1 << 20, "p1", &request).is_err()
        );
        assert!(
            decode_deleted_restoration(ok(body("active", 18)), 1 << 20, "p1", &request).is_err()
        );
    }

    #[test]
    fn recovery_refusals_keep_the_closed_code_and_the_servers_sentence() {
        let refused = |status: u16, body: &str| {
            deleted_backup_refusal(&TransportResponse {
                status,
                body: body.as_bytes().to_vec(),
            })
        };
        let exists = refused(
            409,
            r#"{"error_code":"transformer_exists","detail":"a transformer document with this name exists"}"#,
        );
        assert_eq!(exists.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            exists.service_refusal().unwrap().code(),
            Some("transformer_exists")
        );
        let denied = refused(403, r#"{"detail":"requires transformer.delete"}"#);
        assert_eq!(denied.kind(), ErrorKind::AuthenticationRejected);
        assert_eq!(denied.service_refusal().unwrap().code(), None);
        let unknown = refused(422, r#"{"error_code":"made_up","detail":"x"}"#);
        assert_eq!(unknown.service_refusal().unwrap().code(), None);
        assert_eq!(refused(503, "").kind(), ErrorKind::Transient);
    }
}

mod artifact_download_tests {
    use super::super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};

    const BUCKET: &str = "ds-prod.firebasestorage.app";

    #[test]
    fn identities_come_from_gs_storage_and_legacy_firebase_locators_without_their_bearer_query() {
        let gs =
            artifact_identity(&format!("gs://{BUCKET}/p1/report_lv/TX 1/report.xlsx")).unwrap();
        assert_eq!(gs.object(), "p1/report_lv/TX 1/report.xlsx");
        let signed = artifact_identity(&format!(
            "https://storage.googleapis.com/{BUCKET}/p1/report_lv/TX%201/report.xlsx?X-Goog-Signature=abc"
        ))
        .unwrap();
        assert_eq!(signed, gs);
        let firebase = artifact_identity(&format!(
            "https://firebasestorage.googleapis.com/v0/b/{BUCKET}/o/p1%2Freport_lv%2FTX%201%2Freport.xlsx?alt=media&token=secret"
        ))
        .unwrap();
        assert_eq!(firebase, gs);
        assert!(!firebase.locator().contains("token"));
        // The browser form older report rows list (Kayonza-Gatsibo's plan) is
        // the same object, under the same scheme and authority checks.
        let browser = artifact_identity(&format!(
            "https://storage.cloud.google.com/{BUCKET}/p1/report_lv/TX%201/report.xlsx"
        ))
        .unwrap();
        assert_eq!(browser, gs);
        for bad in [
            "http://storage.cloud.google.com/bucket/a.pdf",
            "https://storage.cloud.google.com:8443/bucket/a.pdf",
            "https://user@storage.cloud.google.com/bucket/a.pdf",
            "https://storage.cloud.google.com/bucket",
            "https://storage.cloud.google.com/bucket/../a.pdf",
            "",
            "gs://bucket",
            "gs://bucket/a/../b.pdf",
            "gs://bucket//b.pdf",
            "http://storage.googleapis.com/bucket/a.pdf",
            "https://example.com/bucket/a.pdf",
            "https://storage.googleapis.com/bucket/a%ZZ.pdf",
            "file:///etc/passwd",
        ] {
            assert!(artifact_identity(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_request_names_each_object_once_and_refuses_an_unusable_or_unbounded_plan() {
        let one = format!("gs://{BUCKET}/p1/a.pdf");
        let request = ArtifactDownloadRequest::new([one.clone(), one.clone()]).unwrap();
        assert_eq!(request.identities().len(), 1);
        assert!(ArtifactDownloadRequest::new(Vec::<String>::new()).is_err());
        assert!(ArtifactDownloadRequest::new([one.clone(), "nope".into()]).is_err());
        let many: Vec<String> = (0..=MAX_ARTIFACT_DOWNLOADS)
            .map(|index| format!("gs://{BUCKET}/p1/{index}.pdf"))
            .collect();
        assert!(ArtifactDownloadRequest::new(many).is_err());
        let body: Value =
            serde_json::from_slice(&ProjectReportOperation::SignUrls(&request).body("p1")).unwrap();
        assert_eq!(
            body,
            json!({"action": "sign_url", "eds_project_id": "p1", "paths": [one]})
        );
        assert!(!crate::PROJECT_REPORT_ACTIONS.contains(&ACTION_SIGN_URL));
    }

    struct SignAndFetch {
        signed: Value,
        objects: std::collections::BTreeMap<String, (u16, Vec<u8>)>,
        fetched: Vec<String>,
    }
    macro_rules! no_other_call {
        ($name:ident, $call:ident) => {
            fn $name(
                &mut self,
                _: crate::$call<'_>,
            ) -> Result<crate::TransportResponse, crate::TransportError> {
                panic!("an artifact download made an unrelated call")
            }
        };
    }
    impl crate::AuthorizedTransport for SignAndFetch {
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
        fn project_report(
            &mut self,
            call: crate::ProjectReportCall<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            assert_eq!(call.operation.action(), "sign_url");
            Ok(crate::TransportResponse::new(
                200,
                serde_json::to_vec(&self.signed).unwrap(),
            ))
        }
        fn grid_model_bytes(
            &mut self,
            call: crate::grid_models::DownloadCall<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            self.fetched.push(call.url().to_owned());
            let (status, body) = self.objects.get(call.url()).cloned().expect("signed url");
            Ok(crate::TransportResponse::new(status, body))
        }
    }

    fn signed_url(object: &str) -> String {
        format!("https://storage.googleapis.com/{BUCKET}/{object}?X-Goog-Signature=fresh")
    }

    type Downloaded = (
        Result<Vec<(ArtifactIdentity, ArtifactOutcome)>, ClientError>,
        Vec<ArtifactFile>,
    );

    fn download(transport: &mut SignAndFetch, request: &ArtifactDownloadRequest) -> Downloaded {
        let profile = crate::data_distribution::tests::profile_for_tests();
        let mut files = Vec::new();
        let outcome = download_artifacts(
            &profile,
            transport,
            crate::authorized::Authority {
                bearer_token: "bearer",
                uid: "uid",
                email: "user@example.com",
                device_id: None,
            },
            "p1",
            request,
            &mut |file| {
                files.push(file);
                Ok(())
            },
        );
        (outcome, files)
    }

    #[test]
    fn every_listed_artifact_is_signed_fetched_byte_exact_refused_or_failed_by_name() {
        let report = b"PK\x03\x04 workbook".to_vec();
        let gs = |object: &str| format!("gs://{BUCKET}/{object}");
        let request = ArtifactDownloadRequest::new([
            gs("p1/report_lv/tx_a/report.xlsx"),
            gs("p1/report_lv/tx_b/report.xlsx"),
            gs("other/report_lv/tx_c/report.xlsx"),
        ])
        .unwrap();
        let mut transport = SignAndFetch {
            signed: json!({"mode": "signed",
                "urls": {gs("p1/report_lv/tx_a/report.xlsx"): signed_url("p1/report_lv/tx_a/report.xlsx"),
                         gs("p1/report_lv/tx_b/report.xlsx"): signed_url("p1/report_lv/tx_b/report.xlsx")},
                "refused": {gs("other/report_lv/tx_c/report.xlsx"): "not a member of the project"}}),
            objects: [
                (
                    signed_url("p1/report_lv/tx_a/report.xlsx"),
                    (200, report.clone()),
                ),
                (
                    signed_url("p1/report_lv/tx_b/report.xlsx"),
                    (404, Vec::new()),
                ),
            ]
            .into_iter()
            .collect(),
            fetched: Vec::new(),
        };
        let (outcomes, files) = download(&mut transport, &request);
        let outcomes = outcomes.expect("download");
        let digest = format!("{:x}", Sha256::digest(&report));
        assert_eq!(
            outcomes[0].1,
            ArtifactOutcome::Fetched {
                bytes: report.len() as u64,
                sha256: digest.clone()
            }
        );
        assert!(
            matches!(&outcomes[1].1, ArtifactOutcome::Failed { reason } if reason.contains("404"))
        );
        assert_eq!(
            outcomes[2].1,
            ArtifactOutcome::Refused {
                reason: "not a member of the project".into()
            }
        );
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].bytes, report);
        assert_eq!(files[0].sha256, digest);
        assert_eq!(
            transport.fetched.len(),
            2,
            "a refused identity is never fetched"
        );
    }

    #[test]
    fn a_signer_answer_that_omits_an_identity_or_signs_another_object_is_unreadable() {
        let gs = format!("gs://{BUCKET}/p1/a.pdf");
        let request = ArtifactDownloadRequest::new([gs.clone()]).unwrap();
        for signed in [
            json!({"mode": "signed", "urls": {}}),
            json!({"mode": "signed", "urls": {gs.clone(): signed_url("p1/b.pdf")}}),
            json!({"mode": "signed", "urls": {gs.clone(): "https://example.com/p1/a.pdf"}}),
            json!({"mode": "unsigned", "urls": {gs.clone(): signed_url("p1/a.pdf")}}),
        ] {
            let mut transport = SignAndFetch {
                signed,
                objects: Default::default(),
                fetched: Vec::new(),
            };
            let (outcome, files) = download(&mut transport, &request);
            assert_eq!(outcome.unwrap_err().kind(), ErrorKind::UnreadableResponse);
            assert!(files.is_empty() && transport.fetched.is_empty());
        }
    }
}
