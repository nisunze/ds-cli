//! Explicit native document semantic metadata and byte transfer. Extraction is external.
use crate::{
    authorized::Authority, shared_assets::DownloadCall, AuthorizedTransport, ClientError,
    ClientProfile, ErrorKind, UploadBytesCall,
};
use ds_command_kernel::assets::semantic::{self, Command, Finalize};
use serde_json::{json, Value};
use std::io::Read;

fn bad(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}
fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}
fn metadata<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    let payload = command
        .payload(project)
        .map_err(|_| invalid("invalid pinned document semantic request"))?;
    let receipt = crate::project_assets::call(profile, transport, authority, payload)?;
    command
        .verify_response(project, &receipt)
        .map_err(|_| bad("semantic response differs from its exact project/source/output pins"))?;
    Ok(receipt)
}
fn read_url<'a>(receipt: &'a Value) -> Result<&'a str, ClientError> {
    let raw = receipt["download_url"]
        .as_str()
        .ok_or_else(|| bad("semantic read has no grant"))?;
    let url = url::Url::parse(raw).map_err(|_| bad("semantic read grant is malformed"))?;
    let generation = receipt["output_generation"]
        .as_u64()
        .ok_or_else(|| bad("semantic output generation is missing"))?
        .to_string();
    let generations: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| key == "generation")
        .map(|(_, value)| value.into_owned())
        .collect();
    if raw.len() > 8192
        || url.scheme() != "https"
        || url.host_str() != Some("storage.googleapis.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || generations != [generation]
    {
        return Err(bad(
            "semantic read grant is not the exact admitted storage generation",
        ));
    }
    Ok(raw)
}
fn retained_receipt(mut receipt: Value) -> Value {
    if let Some(fields) = receipt.as_object_mut() {
        for key in ["download_url", "upload_url", "expires_at"] {
            fields.remove(key);
        }
    }
    receipt
}
/// Every action uses the existing captured native authority and fixed Assets
/// transport. Prepare deliberately consumes the selected external Markdown,
/// uploads it through the issued grant, then verifies the exact Bulk commit.
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
    reader: Option<&mut dyn Read>,
) -> Result<Value, ClientError> {
    command
        .payload(project)
        .map_err(|_| invalid("invalid pinned document semantic request"))?;
    if matches!(command, Command::Prepare(_)) != reader.is_some() {
        return Err(invalid("semantic import requires the selected Markdown reader; metadata reads never consume a file"));
    }
    let receipt = metadata(profile, transport, authority, project, command)?;
    match command {
        Command::Read(_) => {
            let size = receipt["output"]["bytes"]
                .as_u64()
                .ok_or_else(|| bad("semantic output size is missing"))?;
            let url = read_url(&receipt)?;
            let response = transport.shared_asset_bytes(DownloadCall::new(url, size as usize))?;
            if response.status != 200
                || response.body.len() as u64 != size
                || semantic::digest(&response.body) != receipt["output"]["sha256"]
            {
                return Err(bad(
                    "semantic output bytes differ from their committed digest/size",
                ));
            }
            let markdown = std::str::from_utf8(&response.body)
                .ok()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| bad("semantic output is not nonempty UTF-8 Markdown"))?;
            Ok(json!({"receipt":retained_receipt(receipt),"markdown":markdown}))
        }
        Command::Prepare(prepare) if receipt["state"] == "uploading" => {
            let uri = receipt["upload_url"]
                .as_str()
                .ok_or_else(|| bad("semantic upload grant missing"))?;
            crate::solar_project::validate_upload_uri(uri)?;
            crate::project_data::uploaded(transport.upload_bytes(UploadBytesCall {
                uri,
                size: prepare.output.bytes,
                reader: reader.expect("admitted selected reader"),
            })?)?;
            let finalize = Command::Finalize(Finalize {
                selection: prepare.selection.clone(),
                derivative_id: prepare
                    .selection
                    .derivative_id(project)
                    .map_err(|_| invalid("invalid derivative key"))?,
                manifest_sha256: prepare
                    .manifest_sha256(project)
                    .map_err(|_| invalid("invalid derivative manifest"))?,
            });
            metadata(profile, transport, authority, project, &finalize).map(retained_receipt)
        }
        _ => Ok(retained_receipt(receipt)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ClientProfileInput, DeploymentLane, SharedAssetsCall, TransportError, TransportResponse,
        CLIENT_PROFILE_SCHEMA,
    };
    use std::collections::VecDeque;
    fn profile_input(lane: DeploymentLane) -> ClientProfileInput {
        let gateway_origin = match lane {
            DeploymentLane::Stable => "https://eds-gateway-3c0q477h.ue.gateway.dev",
            DeploymentLane::Canary => "https://eds-gateway-canary-3c0q477h.ue.gateway.dev",
        };
        ClientProfileInput {
            schema_version: CLIENT_PROFILE_SCHEMA.to_owned(),
            lane,
            source_revision: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            descriptor_sha256: "a".repeat(64),
            firebase_project_id: "firebase-project".to_owned(),
            firebase_api_key: "public-firebase-key".to_owned(),
            gateway_api_key: "public-gateway-key".to_owned(),
            gateway_origin: gateway_origin.to_owned(),
            auth_link_begin_method: "POST".to_owned(),
            auth_link_begin_path: "/api/v1/auth/device/begin".to_owned(),
            auth_link_status_method: "POST".to_owned(),
            auth_link_status_path: "/api/v1/auth/device/status".to_owned(),
            auth_link_complete_method: "POST".to_owned(),
            auth_link_complete_path: "/api/v1/auth/device/complete".to_owned(),
            auth_device_refresh_method: "POST".to_owned(),
            auth_device_refresh_path: "/api/v1/auth/device/refresh".to_owned(),
            auth_device_list_method: "GET".to_owned(),
            auth_device_list_path: "/api/v1/auth/devices".to_owned(),
            auth_device_read_method: "GET".to_owned(),
            auth_device_read_path_template: "/api/v1/auth/devices/{device_id}".to_owned(),
            auth_device_revoke_method: "DELETE".to_owned(),
            auth_device_revoke_path_template: "/api/v1/auth/devices/{device_id}".to_owned(),
            project_list_method: "GET".to_owned(),
            project_list_path: "/api/v1/user/projects".to_owned(),
            transformer_context_method: "POST".to_owned(),
            transformer_context_path: "/report".to_owned(),
            transformer_context_action: "get_transformers_data".to_owned(),
            transformer_context_fields: "full".to_owned(),
            project_forms_method: "POST".to_owned(),
            project_forms_path: "/api/v1/project-forms".to_owned(),
            project_forms_action: "activate".to_owned(),
            project_form_editor_action: "settings_editor".to_owned(),
            solar_snapshot_method: "POST".to_owned(),
            solar_snapshot_path: "/api/v1/solar".to_owned(),
            solar_snapshot_action: "desktop_snapshot".to_owned(),
            survey_query_method: "POST".to_owned(),
            survey_query_path: "/api/v1/survey/query".to_owned(),
            survey_entries_select_method: "POST".to_owned(),
            survey_entries_select_path: "/api/v1/survey/entries/select".to_owned(),
            survey_entries_changes_method: "POST".to_owned(),
            survey_entries_changes_path: "/api/v1/survey/entries/changes".to_owned(),
            survey_entry_create_method: "POST".to_owned(),
            survey_entry_create_path: "/v1/projects/{firebase_project}/databases/(default)/documents:commit".to_owned(),
            survey_entry_create_operation: "create".to_owned(),
            project_data_method: "POST".to_owned(),
            project_data_path: "/api/v1/project_data".to_owned(),
            project_data_actions: ["list", "upload_start", "upload", "delete"]
                .map(str::to_owned)
                .to_vec(),
            survey_control_routes: crate::SURVEY_CONTROL_ROUTES.map(str::to_owned).to_vec(),
            styles_method: "POST".to_owned(),
            styles_path: "/api/v1/styles".to_owned(),
            styles_action: "update_style".to_owned(),
            printing_method: "POST".into(),
            printing_path: "/api/v1/printing".into(),
            printing_actions: crate::PRINTING_ACTIONS.map(str::to_owned).to_vec(),
            layers_method: "POST".to_owned(),
            layers_path: "/api/v1/layers".to_owned(),
            layers_actions: crate::LAYERS_ACTIONS.map(str::to_owned).to_vec(),
            tiles_method: "POST".to_owned(),
            tiles_path: "/api/v1/tiles".to_owned(),
            tiles_actions: crate::TILES_ACTIONS
                .iter()
                .map(|action| (*action).to_owned())
                .collect(),
            project_report_method: "POST".to_owned(),
            project_report_path: "/report".to_owned(),
            project_report_actions: vec![
                "download_transfo".to_owned(),
                "list_compounded_reports".to_owned(),
                "transformer_inventory".to_owned(),
                "retire_transformer".to_owned(),
                "restore_transformer".to_owned(),
                "list_transformers_status".to_owned(),
            ],
            design_attachments_method: "POST".into(),
            design_attachments_path: "/api/v1/design/attachments".into(),
            design_attachments_actions: crate::DESIGN_ATTACHMENTS_ACTIONS
                .map(str::to_owned)
                .to_vec(),
            design_versions_method: "POST".into(),
            design_versions_path: "/api/v1/design/versions".into(),
            design_versions_actions: crate::DESIGN_VERSIONS_ACTIONS.map(str::to_owned).to_vec(),
            design_selections_method: "POST".to_owned(),
            design_selections_path: "/api/v1/design/selections".to_owned(),
            design_selections_actions: vec![
                "list".to_owned(),
                "get".to_owned(),
                "save".to_owned(),
                "archive".to_owned(),
                "promote_task".to_owned(),
            ],
            data_distribution_method: "POST".to_owned(),
            data_distribution_path: "/api/v1/data-distribution".to_owned(),
            data_distribution_actions: vec![
                "list_datasets".to_owned(),
                "query_print_context".to_owned(),
            ],
            sre_overview_method: "GET".to_owned(),
            sre_overview_path: "/api/v1/sre/overview".to_owned(),
            sre_events_method: "POST".to_owned(),
            sre_events_path: "/api/v1/data".to_owned(),
            sre_events_actions: vec!["query_table".to_owned()],
            admin_bounds_method: "GET".to_owned(),
            admin_bounds_path: "/api/v1/admin/rwanda".to_owned(),
            admin_bounds_actions: vec!["children".to_owned(), "geometry".to_owned()],
        }
    }

    #[test]
    fn semantic_signed_grant_is_exact_generation_and_never_carries_authority_elsewhere() {
        let receipt = json!({"output_generation":29,"download_url":"https://storage.googleapis.com/b/o?generation=29&signed=test"});
        assert!(read_url(&receipt).is_ok());
        for url in [
            "https://example.test/o?generation=29",
            "https://storage.googleapis.com/o?generation=30",
            "https://storage.googleapis.com/o?generation=29&generation=29",
            "http://storage.googleapis.com/o?generation=29",
            "https://user@storage.googleapis.com/o?generation=29",
        ] {
            let mut changed = receipt.clone();
            changed["download_url"] = json!(url);
            assert!(read_url(&changed).is_err());
        }
        let clean = retained_receipt(
            json!({"state":"ready","download_url":"secret","upload_url":"secret","expires_at":"tomorrow","output_generation":29}),
        );
        assert_eq!(clean, json!({"state":"ready","output_generation":29}));
    }
    #[test]
    fn semantic_finalize_registry_is_a_single_fixed_bulk_admission() {
        let route = crate::gateway_operation("domains.assets.semantic_finalize").unwrap();
        assert_eq!(route.path, "/bulk/api/v1/data");
        assert_eq!(route.method, "POST");
        assert_eq!(route.actions, ["finalize_asset_semantic"]);
        assert_eq!(route.response_cap_bytes, 1024 * 1024);
        assert_eq!(route.timeout_s, 120);
        assert_eq!(route.retry_class, "none");
    }

    fn profile() -> ClientProfile {
        ClientProfile::validate(profile_input(DeploymentLane::Canary)).unwrap()
    }
    fn authority() -> Authority<'static> {
        Authority {
            bearer_token: "test",
            uid: "owner",
            email: "owner@example.test",
            device_id: None,
        }
    }
    fn prepare() -> semantic::Prepare {
        semantic::Prepare {
            selection: semantic::Selection {
                source: semantic::SourcePin {
                    kind: semantic::SourceKind::PmAttachment,
                    id: "pma_pdf".into(),
                    version: 3,
                    generation: 29,
                    size_bytes: 7,
                    content_type: "application/pdf".into(),
                    measured_sha256: "a".repeat(64),
                },
                extractor: semantic::Extractor {
                    name: "markitdown".into(),
                    version: "0.1.8".into(),
                    options_sha256: semantic::digest(b"{}"),
                },
            },
            output: semantic::Output {
                sha256: semantic::digest(b"# Ready\n"),
                bytes: 8,
            },
        }
    }
    fn receipt(p: &semantic::Prepare, state: &str) -> Value {
        let mut value = json!({"schema":semantic::SCHEMA,"project_id":"project","derivative_id":p.selection.derivative_id("project").unwrap(),"manifest_sha256":p.manifest_sha256("project").unwrap(),"source":p.selection.source,"extractor":p.selection.extractor,"output":{"sha256":p.output.sha256,"bytes":p.output.bytes,"content_type":semantic::MARKDOWN_TYPE},"state":state,"source_sha256_basis":"native_measured"});
        if state == "ready" {
            value["output_generation"] = json!(31);
            value["replayed"] = json!(false);
        } else {
            value["upload_url"] = json!(
                "https://storage.googleapis.com/upload/storage/v1/b/project/o?upload_id=test"
            );
            value["expires_at"] = json!("tomorrow");
        }
        value
    }
    #[derive(Default)]
    struct Source {
        calls: Vec<(String, Value)>,
        replies: VecDeque<Value>,
        bytes: VecDeque<Vec<u8>>,
        uploaded: usize,
    }
    impl AuthorizedTransport for Source {
        fn list_projects(
            &mut self,
            _: crate::transport::ProjectListCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn transformer_context(
            &mut self,
            _: crate::transport::TransformerContextCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn project_forms(
            &mut self,
            _: crate::transport::ProjectFormsCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn project_form_editor(
            &mut self,
            _: crate::transport::ProjectFormEditorCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn solar_snapshot(
            &mut self,
            _: crate::transport::SolarSnapshotCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn survey_query(
            &mut self,
            _: crate::transport::SurveyQueryCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn survey_entries_select(
            &mut self,
            _: crate::transport::SurveyEntriesSelectCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn survey_entries_changes(
            &mut self,
            _: crate::transport::SurveyEntriesChangesCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn survey_entry_create(
            &mut self,
            _: crate::transport::SurveyEntryCreateCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn project_data(
            &mut self,
            _: crate::transport::ProjectDataCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn survey_control(
            &mut self,
            _: crate::transport::SurveyControlCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn styles(
            &mut self,
            _: crate::transport::StylesCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn layers(
            &mut self,
            _: crate::transport::LayersCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn tiles(
            &mut self,
            _: crate::transport::TileCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn project_report(
            &mut self,
            _: crate::transport::ProjectReportCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            panic!("unused owner must not acquire")
        }
        fn shared_assets(
            &mut self,
            call: SharedAssetsCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            assert_eq!(call.canonical_uid(), "owner");
            assert_eq!(call.canonical_email(), "owner@example.test");
            let body: Value = serde_json::from_slice(call.body().as_bytes()).unwrap();
            self.calls.push((call.path(), body));
            Ok(TransportResponse::new(
                200,
                serde_json::to_vec(
                    &json!({"success":true,"data":self.replies.pop_front().unwrap()}),
                )
                .unwrap(),
            ))
        }
        fn upload_bytes(
            &mut self,
            call: UploadBytesCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            let mut bytes = Vec::new();
            call.reader.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"# Ready\n");
            assert_eq!(call.size, 8);
            self.uploaded += 1;
            Ok(TransportResponse::new(200, vec![]))
        }
        fn shared_asset_bytes(
            &mut self,
            call: DownloadCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            assert_eq!(call.response_limit(), 8);
            Ok(TransportResponse::new(200, self.bytes.pop_front().unwrap()))
        }
    }
    #[test]
    fn explicit_import_is_run_metadata_one_upload_one_fixed_bulk_finalize_and_ready_replay_has_no_upload(
    ) {
        let p = prepare();
        let profile = profile();
        let command = Command::Prepare(p.clone());
        let mut source = Source {
            replies: [receipt(&p, "uploading"), receipt(&p, "ready")].into(),
            ..Default::default()
        };
        let result = run(
            &profile,
            &mut source,
            authority(),
            "project",
            &command,
            Some(&mut &b"# Ready\n"[..]),
        )
        .unwrap();
        assert_eq!(result["state"], "ready");
        assert_eq!(source.uploaded, 1);
        assert_eq!(source.calls.len(), 2);
        assert_eq!(source.calls[0].0, "/api/v1/assets");
        assert_eq!(source.calls[1].0, "/bulk/api/v1/data");
        assert!(source.calls[1].1.get("ds_project").is_none());
        assert_eq!(source.calls[1].1["eds_project_id"], "project");
        let mut replay = Source {
            replies: [receipt(&p, "ready")].into(),
            ..Default::default()
        };
        run(
            &profile,
            &mut replay,
            authority(),
            "project",
            &command,
            Some(&mut &b"# Ready\n"[..]),
        )
        .unwrap();
        assert_eq!(replay.calls.len(), 1);
        assert_eq!(replay.uploaded, 0);
        let mut unused = Source::default();
        assert!(run(
            &profile,
            &mut unused,
            authority(),
            "project",
            &command,
            None
        )
        .is_err());
        assert!(unused.calls.is_empty());
    }
    #[test]
    fn semantic_read_verifies_exact_bytes_and_strips_ephemeral_grants() {
        let p = prepare();
        let profile = profile();
        let command = Command::Read(p.selection.clone());
        let mut ready = receipt(&p, "ready");
        ready["download_url"] =
            json!("https://storage.googleapis.com/b/o?generation=31&signed=test");
        ready["expires_at"] = json!("tomorrow");
        let mut source = Source {
            replies: [ready.clone()].into(),
            bytes: [b"# Ready\n".to_vec()].into(),
            ..Default::default()
        };
        let result = run(
            &profile,
            &mut source,
            authority(),
            "project",
            &command,
            None,
        )
        .unwrap();
        assert_eq!(result["markdown"], "# Ready\n");
        assert!(result["receipt"].get("download_url").is_none());
        assert!(result["receipt"].get("expires_at").is_none());
        let mut corrupt = Source {
            replies: [ready].into(),
            bytes: [b"changed!".to_vec()].into(),
            ..Default::default()
        };
        assert!(run(
            &profile,
            &mut corrupt,
            authority(),
            "project",
            &command,
            None
        )
        .is_err());
    }
}
