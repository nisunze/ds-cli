//! Protected native IO adapter for the existing Assets catalogue retention owner.
use super::*;
use ds_client_core::project_assets::{Command, IndexList};
use ds_command_kernel::assets::index_read::Scope;
use ds_layer_store::asset_catalogue::{self, Binding, Capture, Source};
fn local_failure(message: impl ToString) -> Failure {
    Failure::unavailable("assets_retained_unavailable", message.to_string())
        .remedy("retry online with --refresh under the same native account; inspect the owner-only local data directory if persistence still fails")
}
struct Provider<'a> {
    lane: &'a str,
    project: &'a str,
    list: &'a IndexList,
    fence: LayerScopeFence,
    binding: Binding,
}
impl Source for Provider<'_> {
    type Error = Failure;
    fn check_binding(&mut self) -> Result<(), Failure> {
        verify_layer_scope_fence_for_project(
            self.lane,
            &self.fence,
            &self.binding.uid,
            self.project,
        )?;
        if let Some(expected) = &self.binding.protected_email {
            let lane = Lane::parse(self.lane)?;
            if device::probe_context(lane)?
                .is_none_or(|c| !c.email().eq_ignore_ascii_case(expected))
            {
                return Err(Failure::conflict(
                    "project_context_changed",
                    "the protected device email changed during the catalogue read",
                )
                .remedy("repeat under the current native account"));
            }
        }
        Ok(())
    }
    fn capture(&mut self) -> Result<Capture, Failure> {
        let captured = project_assets_for_project(
            self.lane,
            self.project,
            &Command::IndexList(self.list.clone()),
            None,
        )?;
        if captured.identity().uid() != self.binding.uid
            || captured.identity().credential_audience_sha256() != self.fence.audience()
            || captured.lane() != self.lane
            || captured.project_id() != self.project
        {
            return Err(Failure::conflict(
                "project_context_changed",
                "the source capture differs from its protected native identity",
            )
            .remedy("repeat under the current native account and named project"));
        }
        Ok(Capture {
            scope: Scope {
                uid: captured.identity().uid().into(),
                email: captured.user_email().into(),
                lane: captured.lane().into(),
                audience: self.binding.audience.clone(),
                project: captured.project_id().into(),
            },
            value: captured.into_result(),
        })
    }
    fn access_refused(&self, error: &Failure) -> bool {
        error.class() == ds_cli_contract::outcome::ExitClass::Unauthorized
            || matches!(
                error.code(),
                "auth_revoked"
                    | "auth_identity_mismatch"
                    | "asset_class_forbidden"
                    | "asset_not_found"
            )
    }
    fn verify_page(&self, value: &Value) -> Result<(), Failure> {
        ds_client_core::project_assets::verify_index_page(self.project, self.list, value)
            .map_err(map_client)
    }
}
fn binding(lane: &str, project: &str) -> Result<(LayerScopeFence, Binding), Failure> {
    let fence = capture_layer_scope_fence_for_project(lane, project)?;
    let parsed = Lane::parse(lane)?;
    let email = device::probe_context(parsed)?.map(|c| c.email().to_owned());
    let binding = Binding {
        uid: fence.uid.clone(),
        lane: lane.into(),
        audience: format!("credential:{}", fence.audience),
        credential_instance: fence.credential.clone(),
        project: fence.project.clone(),
        protected_email: email,
    };
    Ok((fence, binding))
}
/// Offline probes local protected identity only. It never restores a session,
/// refreshes auth, fetches a directory, checks freshness or reads Brain.
pub fn read(lane: &str, project: &str, list: &IndexList, offline: bool) -> Result<Value, Failure> {
    if offline && list.refresh {
        return Err(Failure::invalid(
            "assets_offline_refresh_conflict",
            "--offline cannot rebuild the cloud index",
        )
        .remedy("omit --refresh for a retained read, or omit --offline to acquire"));
    }
    let query =
        ds_client_core::project_assets::index_read_query(project, list).map_err(map_client)?;
    let (fence, binding) = binding(lane, project)?;
    let root = ds_layer_store::default_root().map_err(local_failure)?;
    let mut source = Provider {
        lane,
        project,
        list,
        fence,
        binding: binding.clone(),
    };
    let now_ms = i64::try_from(now().saturating_mul(1000))
        .map_err(|_| local_failure("invalid host clock"))?;
    match asset_catalogue::consume(&root,&binding,query,offline,now_ms,&mut source) {
        Ok(value) => Ok(value),
        Err(asset_catalogue::Error::Source(error)) => Err(error),
        Err(asset_catalogue::Error::AcquisitionRequired) => Err(Failure::unavailable("assets_offline_acquisition_required", "no verified retained page matches this protected account, lane, credential, project and query")
            .remedy("run this exact list online once to retain it; offline never acquires missing data")),
        Err(asset_catalogue::Error::Superseded) => Err(Failure::conflict("project_context_changed", "the account or catalogue generation changed during acquisition")
            .remedy("repeat this bounded list under the current native account")),
        Err(asset_catalogue::Error::Local(error)) => Err(local_failure(error)),
    }
}

fn successful_mutation(project: &str, command: &Command, result: &Value, scope: &Scope) -> bool {
    match command {
        Command::Classify {
            asset_id,
            expected_version,
            ..
        } => {
            let receipt = serde_json::from_value::<
                ds_command_kernel::assets::index_read::MutationReceipt,
            >(json!({
                "project_id":result["asset"]["project_id"], "asset_id":result["asset"]["asset_id"],
                "version":result["asset"]["version"], "audit_id":result["audit_id"], "changed":result["changed"]
            }));
            receipt
                .ok()
                .and_then(|receipt| {
                    ds_command_kernel::assets::index_read::evaluate(
                        ds_command_kernel::assets::index_read::Request::Mutation {
                            scope: scope.clone(),
                            captured_scope: scope.clone(),
                            expected_asset_id: asset_id.clone(),
                            expected_version: *expected_version,
                            receipt,
                            epoch: 0,
                        },
                    )
                    .ok()
                })
                .is_some_and(|result| result["admitted"] == true)
        }
        // The native owner already verified finalize's exact bytes/digest after
        // upload. A successful ingest always creates one new catalogued asset.
        Command::Ingest(input) => {
            result["asset"]["project_id"] == project
                && result["asset"]["digest"] == format!("sha256:{}", input.sha256)
                && result["asset"]["bytes"].as_u64() == Some(input.size)
        }
        _ => false,
    }
}
/// Known, verified success only. No source check or acquisition for unused pages.
pub(super) fn after_write(
    lane: &str,
    project: &str,
    command: &Command,
    captured: &HeadlessNamedProject<Value>,
    fence: &LayerScopeFence,
) -> Result<bool, Failure> {
    let scope = Scope {
        uid: captured.identity().uid().into(),
        email: captured.user_email().into(),
        lane: captured.lane().into(),
        audience: format!(
            "credential:{}",
            captured.identity().credential_audience_sha256()
        ),
        project: captured.project_id().into(),
    };
    if !successful_mutation(project, command, captured.result(), &scope) {
        return Ok(false);
    }
    verify_layer_scope_fence_for_project(lane, fence, fence.uid(), project)?;
    let (current, binding) = binding(lane, project)?;
    if current.credential != fence.credential {
        return Err(local_failure("native credential changed after write"));
    }
    asset_catalogue::invalidate(
        &ds_layer_store::default_root().map_err(local_failure)?,
        &binding,
    )
    .map_err(local_failure)
}

#[cfg(test)]
mod catalogue_tests {
    use super::*;
    fn test_scope() -> Scope {
        Scope {
            uid: "u".into(),
            email: "owner@example.com".into(),
            lane: "canary".into(),
            audience: "credential:a".into(),
            project: "p".into(),
        }
    }
    fn classify() -> Command {
        Command::Classify {
            asset_id: "a_7kq3nr2v0b1c".into(),
            kind: None,
            status: Some("durable".into()),
            sensitivity: None,
            folder_id: None,
            owner: None,
            reason: None,
            expected_version: 3,
        }
    }
    #[test]
    fn only_exact_verified_classification_and_finalized_ingest_mark_stale() {
        let command = classify();
        let receipt = json!({"asset":{"asset_id":"a_7kq3nr2v0b1c","project_id":"p","version":4},"audit_id":"audit1","changed":["status"]});
        assert!(successful_mutation("p", &command, &receipt, &test_scope()));
        for (field, value) in [("changed", json!([])), ("audit_id", json!(""))] {
            let mut bad = receipt.clone();
            bad[field] = value;
            assert!(!successful_mutation("p", &command, &bad, &test_scope()));
        }
        for (field, value) in [
            ("project_id", json!("other")),
            ("asset_id", json!("different")),
            ("version", json!(5)),
        ] {
            let mut bad = receipt.clone();
            bad["asset"][field] = value;
            assert!(!successful_mutation("p", &command, &bad, &test_scope()));
        }
        assert!(!successful_mutation(
            "p",
            &Command::IndexTree { refresh: false },
            &receipt,
            &test_scope()
        ));
        let input = ds_client_core::project_assets::IngestRequest {
            name: "document.txt".into(),
            size: 10,
            sha256: "a".repeat(64),
            content_type: None,
            folder_id: None,
            sensitivity: None,
            kind: "doc".into(),
            format: "txt".into(),
        };
        let command = Command::Ingest(input.clone());
        let receipt = json!({"asset":{"project_id":"p","bytes":10,"digest":format!("sha256:{}",input.sha256)}});
        assert!(successful_mutation("p", &command, &receipt, &test_scope()));
        let mut failed = receipt;
        failed["asset"]["bytes"] = json!(9);
        assert!(!successful_mutation("p", &command, &failed, &test_scope()));
    }
    #[test]
    fn offline_refresh_refuses_before_any_protected_auth_or_source_acquisition() {
        // Invalid lane is intentional: this must stop before probing any profile
        // or protected identity, not fail at the later auth boundary.
        let list = IndexList {
            order: ds_client_core::project_assets::IndexOrder::Recent,
            folder: None,
            kind: None,
            status: None,
            sensitivity: None,
            since: None,
            limit: 50,
            cursor: None,
            refresh: true,
        };
        assert_eq!(
            read("invalid-lane", "p", &list, true).unwrap_err().code(),
            "assets_offline_refresh_conflict"
        );
    }
}
