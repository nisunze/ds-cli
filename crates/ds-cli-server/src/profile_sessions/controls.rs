//! Core only carries authorized controls and durably retains exact checkpoints.
//! Native Network owns edit admission, engineering and transient Save exclusion.
use super::*;
use ds_sync_store::{MAX_PROFILE_CHECKPOINT_BYTES, ProfileDraftHeader};

pub(super) fn retain_checkpoint(
    value: &mut Held,
    database: &Path,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<(), Failure> {
    let checkpoint = value.session.checkpoint(&|| false).map_err(native)?;
    if checkpoint.bytes.is_empty() || checkpoint.bytes.len() > MAX_PROFILE_CHECKPOINT_BYTES {
        return Err(invalid(
            "Profile recovery checkpoint exceeds the protected 64 MiB bound",
        ));
    }
    authorized()?;
    let header = ProfileDraftHeader {
        schema: "ds.core.profile-draft/v1".into(),
        source: value.source.clone(),
        history: value.session.history().clone(),
        checkpoint_sha256: sha(&checkpoint.bytes),
        checkpoint_bytes: checkpoint.bytes.len() as u64,
    };
    Store::open(database)
        .map_err(source_error)?
        .retain_profile_draft(
            &source_scope(&value.scope),
            &value.source,
            &value.retained_history,
            &header,
            &checkpoint.bytes,
            now_ms(),
        )
        .map_err(source_error)?;
    value.retained_history = header.history;
    value.has_draft = true;
    authorized()
}

pub(super) fn execute(
    held: &mut BTreeMap<String, Held>,
    scope: &Scope,
    database: &Path,
    operation: Operation,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<Value, Failure> {
    let (id, model, expected) = operation_owner(&operation).expect("control owner");
    let (id, model, expected) = (id.to_owned(), model.to_owned(), expected.clone());
    let (native_op, preflight, mutates) = match operation {
        Operation::PackageCheckpoint { .. } => {
            (NativeProfileOperationV1::PackageCheckpoint, true, false)
        }
        Operation::EditMode { enabled, .. } => {
            (NativeProfileOperationV1::EditMode { enabled }, false, false)
        }
        Operation::Control { operation, .. } => {
            let mutates = operation.is_mutating();
            (
                NativeProfileOperationV1::Control {
                    operation: serde_json::to_value(operation).map_err(invalid)?,
                },
                mutates,
                mutates,
            )
        }
        Operation::SaveControl { operation, .. } => (
            NativeProfileOperationV1::SaveControl { operation },
            false,
            false,
        ),
        Operation::Discard { request, .. } => {
            (NativeProfileOperationV1::Discard { request }, true, true)
        }
        _ => unreachable!(),
    };
    let value = owned(held, scope, &id, &model, &expected)?;
    require_read_capability(
        &value.session.pin().identity.capabilities,
        if matches!(native_op, NativeProfileOperationV1::PackageCheckpoint) {
            NetworkCapability::ProfilePackageCheckpointV1
        } else {
            NetworkCapability::ProfileEditControlsV1
        },
    )?;
    let mut retire_on_failure = false;
    let result = (|| {
        if preflight {
            // Preserve a recoverable before-state before any authored effect.
            retire_on_failure = true;
            retain_checkpoint(value, database, authorized)?;
            retire_on_failure = false;
        }
        authorized()?;
        retire_on_failure = true;
        let result =
            match value
                .session
                .request_at(native_op.clone(), Some(expected), DEADLINE, &|| false)
            {
                Ok(result) => result,
                Err(error) => {
                    // A typed refusal has proved unchanged history and consumes only
                    // its sequence. In particular it must not release another Save.
                    if error.code() == "network_engine_refused" {
                        retire_on_failure = false;
                    }
                    return Err(native(error));
                }
            };
        authorized()?;
        let payload: Value = serde_json::from_slice(&result.bytes).map_err(invalid)?;
        if matches!(native_op, NativeProfileOperationV1::PackageCheckpoint) {
            if payload["schema"] != "ds.network.profile-package-checkpoint/v1"
                || payload["model_id"] != model
                || payload["history"] != *value.session.history()
                || !payload["model_revision"].is_u64()
                || payload["coordinate_system"] != value.coordinate_system
            {
                return Err(invalid("Native package checkpoint crossed its exact owner"));
            }
            let bytes = decode_package(
                payload["package_base64"]
                    .as_str()
                    .ok_or_else(|| invalid("No package encoding"))?,
                payload["package_sha256"]
                    .as_str()
                    .ok_or_else(|| invalid("No package SHA"))?,
            )?;
            if payload["package_bytes"].as_u64() != Some(bytes.len() as u64) {
                return Err(invalid("Package size differs"));
            }
        }
        let retain = mutates
            || matches!(native_op, NativeProfileOperationV1::PackageCheckpoint)
            || matches!(
                native_op,
                NativeProfileOperationV1::EditMode { enabled: true }
            )
            || matches!(&native_op,NativeProfileOperationV1::SaveControl {operation} if operation["action"]=="begin");
        if retain {
            retain_checkpoint(value, database, authorized)?;
        }
        if matches!(native_op, NativeProfileOperationV1::SaveControl { .. }) {
            value.saving = payload["saving"]
                .as_bool()
                .ok_or_else(|| invalid("Native Save has no exclusion state"))?;
        }
        let receipt = receipt(&id, value, &result.bytes, false)?;
        value.touched = Instant::now();
        retire_on_failure = false;
        Ok(receipt)
    })();
    if result.is_err() && retire_on_failure {
        // Never retry an uncertain mutation or keep an unacknowledged head.
        // Exact SQLite recovery remains available at its last committed history.
        held.remove(&id);
    }
    result
}

/// Restore only exact owner-validated checkpoint bytes. Engine selection must
/// still match the original capture; the current module selection is irrelevant.
pub(super) fn restore(
    scope: Scope,
    scratch: &Path,
    recovery: &ds_sync_store::ProfileRecovery,
    authorized: &dyn Fn() -> Result<(), Failure>,
    required: Option<NetworkCapability>,
) -> Result<(Held, Vec<u8>), Failure> {
    let (draft, bytes) = recovery
        .draft
        .as_ref()
        .ok_or_else(|| invalid("Exact draft recovery is required"))?;
    if recovery.source.scope != source_scope(&scope) || draft.source != recovery.source {
        return Err(invalid(
            "Profile recovery belongs to another captured owner",
        ));
    }
    let pin: ds_network_client::NetworkEnginePin =
        serde_json::from_value(recovery.source.engine.clone()).map_err(invalid)?;
    let client = CapturedNetworkClient::from_pin(&pin, &|| false).map_err(native)?;
    authorized()?;
    require_read_capability(
        &client.identity().capabilities,
        NetworkCapability::ProfileEditControlsV1,
    )?;
    if let Some(required) = required {
        require_read_capability(&client.identity().capabilities, required)?;
    }
    let mut session = client
        .open_profile_session_with_timeout(bytes, scratch, DEADLINE, &|| false)
        .map_err(native)?;
    if session.model_id() != recovery.source.model_id || session.history() != &draft.history {
        return Err(invalid(
            "Recovered checkpoint differs from its exact model/history",
        ));
    }
    let snapshot = session
        .request(NativeProfileOperationV1::Snapshot, &|| false)
        .map_err(native)?;
    authorized()?;
    let asset_count =
        serde_json::from_slice::<Value>(&snapshot.bytes).map_err(invalid)?["retained_asset_count"]
            .as_u64()
            .ok_or_else(|| invalid("Recovered snapshot has no asset count"))? as usize;
    Ok((
        Held {
            scope,
            package_sha256: recovery.source.package_sha256.clone(),
            source_revision: recovery.source.source_revision,
            coordinate_system: recovery.source.coordinate_system.clone(),
            retained_asset_count: asset_count,
            display_state: Default::default(),
            style: None,
            touched: Instant::now(),
            source: recovery.source.clone(),
            retained_history: draft.history.clone(),
            has_draft: true,
            saving: false,
            session,
        },
        snapshot.bytes,
    ))
}

pub(super) fn resume(
    held: &mut BTreeMap<String, Held>,
    scope: Scope,
    scratch: &Path,
    recovery: &ds_sync_store::ProfileRecovery,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<Value, Failure> {
    let id = recovery.source.session_id.clone();
    let (draft, _) = recovery
        .draft
        .as_ref()
        .ok_or_else(|| invalid("Exact draft is required"))?;
    authorized()?;
    let rehydrated = !held.contains_key(&id);
    let snapshot = if !rehydrated {
        let value = owned(held, &scope, &id, &recovery.source.model_id, &draft.history)?;
        if value.source != recovery.source || value.retained_history != draft.history {
            return Err(Failure::conflict(
                "core_profile_history_conflict",
                "Live Profile differs from durable recovery",
            ));
        }
        value
            .session
            .request(NativeProfileOperationV1::Snapshot, &|| false)
            .map_err(native)?
            .bytes
    } else {
        if held.len() >= MAX_SESSIONS {
            return Err(Failure::unavailable(
                "core_profile_capacity",
                "Core already retains eight Profile sessions",
            ));
        }
        let (value, snapshot) = restore(scope, scratch, recovery, authorized, None)?;
        authorized()?;
        held.insert(id.clone(), value);
        snapshot
    };
    authorized()?;
    let mut answer = receipt(&id, &held[&id], &snapshot, false)?;
    answer["rehydrated"] = json!(rehydrated);
    answer["recovered_draft"] = json!(true);
    Ok(answer)
}
