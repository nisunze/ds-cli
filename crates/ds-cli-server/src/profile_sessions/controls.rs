//! Core only carries authorized controls and durably retains exact authored
//! recovery. Native Network owns edit admission, engineering and transient Save
//! exclusion. Calculated results stay in the worker's RAM and never reach SQLite.
use super::*;
use ds_network_contracts::profile::{
    NativeProfileAuthoredBaseV1, NativeProfileAuthoredCheckpointV1, profile_authored_chain,
    profile_authored_entry_sha256,
};
use ds_sync_store::{
    MAX_PROFILE_CHECKPOINT_BYTES, PROFILE_JOURNAL_DRAFT_SCHEMA, ProfileDraft, ProfileDraftHeader,
    ProfileJournalDraftHeader,
};

/// Durable authored rows of the current journal draft, as Core retained them.
#[derive(Clone, Debug, Default)]
pub(super) struct RetainedJournal {
    digests: Vec<String>,
    sizes: Vec<u64>,
    chain: String,
}

pub(super) fn retain_checkpoint(
    value: &mut Held,
    database: &Path,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<(), Failure> {
    if value
        .session
        .pin()
        .identity
        .capabilities
        .contains(&NetworkCapability::ProfileAuthoredCheckpointV1)
    {
        return retain_journal(value, database, authorized);
    }
    retain_legacy_checkpoint(value, database, authorized)
}

/// Retain only canonical authored state: the commands after the durable prefix,
/// the history cursor and opaque lineage/binding metadata. An edit writes its
/// own row(s); undo/redo, edit mode and package emission rewrite only metadata.
fn retain_journal(
    value: &mut Held,
    database: &Path,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<(), Failure> {
    let base = value
        .retained_journal
        .as_ref()
        .map(|retained| NativeProfileAuthoredBaseV1 {
            entry_count: retained.digests.len() as u64,
            chain_sha256: retained.chain.clone(),
        });
    let (_, document) = value
        .session
        .authored_checkpoint(base, &|| false)
        .map_err(native)?;
    let incoming: Vec<String> = document
        .entries
        .iter()
        .map(|entry| profile_authored_entry_sha256(entry))
        .collect();
    let retained = value.retained_journal.clone().unwrap_or_default();
    // A matched base sends only new commands; a complete journal keeps every
    // unchanged leading row and replaces the abandoned tail.
    let (keep, start) = if document.base_entries > 0 {
        if retained.digests.len() as u64 != document.base_entries {
            return Err(invalid(
                "Native authored checkpoint crossed its durable prefix",
            ));
        }
        (retained.digests.len(), 0)
    } else {
        let keep = retained
            .digests
            .iter()
            .zip(&incoming)
            .take_while(|(left, right)| left == right)
            .count();
        (keep, keep)
    };
    let mut digests = retained.digests[..keep].to_vec();
    let mut sizes = retained.sizes[..keep].to_vec();
    digests.extend(incoming[start..].iter().cloned());
    sizes.extend(document.entries[start..].iter().map(|e| e.len() as u64));
    if digests.len() as u64 != document.entry_count
        || profile_authored_chain(digests.iter().map(String::as_str)) != document.chain_sha256
        || &document.history != value.session.history()
    {
        return Err(invalid(
            "Native authored checkpoint differs from its exact command chain",
        ));
    }
    let appended: Vec<&[u8]> = document.entries[start..]
        .iter()
        .map(|entry| entry.as_bytes())
        .collect();
    let mut metadata = document.clone();
    metadata.entries.clear();
    metadata.base_entries = 0;
    let metadata = serde_json::to_vec(&metadata).map_err(invalid)?;
    let header = ProfileJournalDraftHeader {
        schema: PROFILE_JOURNAL_DRAFT_SCHEMA.into(),
        source: value.source.clone(),
        history: document.history.clone(),
        entry_count: document.entry_count,
        entry_bytes: sizes.iter().sum(),
        chain_sha256: document.chain_sha256.clone(),
        metadata_sha256: sha(&metadata),
        metadata_bytes: metadata.len() as u64,
    };
    authorized()?;
    Store::open(database)
        .map_err(source_error)?
        .retain_profile_journal_draft(
            &source_scope(&value.scope),
            &value.source,
            &value.retained_history,
            value
                .retained_journal
                .as_ref()
                .map(|retained| retained.chain.as_str()),
            &header,
            &metadata,
            keep as u64,
            &appended,
            now_ms(),
        )
        .map_err(source_error)?;
    value.retained_history = header.history;
    value.retained_journal = Some(RetainedJournal {
        digests,
        sizes,
        chain: document.chain_sha256,
    });
    value.has_draft = true;
    authorized()
}

/// An older captured Network without authored recovery keeps its opaque bounded
/// checkpoint. Its remedy is an updated Network, never a larger bound.
fn retain_legacy_checkpoint(
    value: &mut Held,
    database: &Path,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<(), Failure> {
    let checkpoint = value.session.checkpoint(&|| false).map_err(native)?;
    if checkpoint.bytes.is_empty() || checkpoint.bytes.len() > MAX_PROFILE_CHECKPOINT_BYTES {
        return Err(Failure::invalid(
            "core_profile_request_invalid",
            "Profile recovery checkpoint exceeds the protected 64 MiB bound",
        )
        .remedy("Update Network to retain authored Profile recovery without calculated results, then reopen the exact package")
        .detail(json!({"component":"network","required_capability":NetworkCapability::ProfileAuthoredCheckpointV1})));
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
    // Capability/bounds admission precedes a checkpoint or any worker request.
    // A large control against an older engine must preserve the live session.
    value
        .session
        .check_control_transport(&native_op, Some(&expected))
        .map_err(native)?;
    let mut retire_on_failure = false;
    let result = (|| {
        // Preserve a recoverable before-state before any authored effect. A
        // retained draft at this exact history already is that before-state.
        if preflight && !(value.has_draft && value.retained_history == *value.session.history()) {
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
    let draft = recovery
        .draft
        .as_ref()
        .ok_or_else(|| invalid("Exact draft recovery is required"))?;
    if recovery.source.scope != source_scope(&scope) || draft.source() != &recovery.source {
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
    let (session, retained_journal) = match draft {
        ProfileDraft::Checkpoint { bytes, .. } => (
            client
                .open_profile_session_with_timeout(bytes, scratch, DEADLINE, &|| false)
                .map_err(native)?,
            None,
        ),
        ProfileDraft::Journal(journal) => {
            let (session, retained) =
                restore_journal(&client, scratch, recovery, journal, authorized)?;
            (session, Some(retained))
        }
    };
    if session.model_id() != recovery.source.model_id || session.history() != draft.history() {
        return Err(invalid(
            "Recovered checkpoint differs from its exact model/history",
        ));
    }
    let mut session = session;
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
            retained_history: draft.history().clone(),
            retained_journal,
            has_draft: true,
            saving: false,
            session,
        },
        snapshot.bytes,
    ))
}

/// Reopen the exact original through the same pinned engine, then replay the
/// retained authored rows through Network. Core verifies their chain against
/// the retained header; Network re-verifies chain, replay, bindings and lineage.
fn restore_journal(
    client: &CapturedNetworkClient,
    scratch: &Path,
    recovery: &ds_sync_store::ProfileRecovery,
    journal: &ds_sync_store::ProfileJournalDraft,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<(ProfileSession, RetainedJournal), Failure> {
    require_read_capability(
        &client.identity().capabilities,
        NetworkCapability::ProfileAuthoredCheckpointV1,
    )?;
    let header = &journal.header;
    let mut document: NativeProfileAuthoredCheckpointV1 =
        serde_json::from_slice(&journal.metadata).map_err(invalid)?;
    let digests: Vec<String> = journal
        .entries
        .iter()
        .map(|entry| entry.sha256.clone())
        .collect();
    if document.history != header.history
        || document.entry_count != header.entry_count
        || document.chain_sha256 != header.chain_sha256
        || document.model_id != recovery.source.model_id
        || !document.entries.is_empty()
        || profile_authored_chain(digests.iter().map(String::as_str)) != header.chain_sha256
    {
        return Err(invalid(
            "Retained authored rows differ from their exact recovery header",
        ));
    }
    document.entries = journal
        .entries
        .iter()
        .map(|entry| String::from_utf8(entry.bytes.clone()))
        .collect::<Result<_, _>>()
        .map_err(invalid)?;
    if !document.admitted_shape() {
        return Err(invalid(
            "Retained authored recovery is not a closed checkpoint",
        ));
    }
    let staged = serde_json::to_vec(&document).map_err(invalid)?;
    let prepared = client
        .prepare_profile_open(&recovery.source_bytes, scratch, DEADLINE, &|| false)
        .map_err(package_native)?
        .result;
    authorized()?;
    if prepared.source_revision != recovery.source.source_revision
        || prepared.coordinate_system != recovery.source.coordinate_system
    {
        return Err(Failure::conflict(
            "core_profile_source_changed",
            "Exact original model/history/CRS/managed engine changed during recovery",
        ));
    }
    let input = serde_json::to_vec(&prepared.open).map_err(invalid)?;
    let mut session = client
        .open_profile_session_with_timeout(&input, scratch, DEADLINE, &|| false)
        .map_err(native)?;
    if session.model_id() != recovery.source.model_id
        || session.history() != &recovery.source.initial_history
    {
        return Err(Failure::conflict(
            "core_profile_source_changed",
            "Exact original model/history/CRS/managed engine changed during recovery",
        ));
    }
    authorized()?;
    // Replay cost is proportional to the retained commands, not the model.
    let deadline = DEADLINE
        .saturating_add(Duration::from_millis(250).saturating_mul(header.entry_count as u32))
        .min(Duration::from_secs(30 * 60));
    session
        .restore_authored(&staged, deadline, &|| false)
        .map_err(native)?;
    Ok((
        session,
        RetainedJournal {
            digests,
            sizes: journal
                .entries
                .iter()
                .map(|entry| entry.bytes.len() as u64)
                .collect(),
            chain: header.chain_sha256.clone(),
        },
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
    let draft = recovery
        .draft
        .as_ref()
        .ok_or_else(|| invalid("Exact draft is required"))?;
    authorized()?;
    let rehydrated = !held.contains_key(&id);
    let snapshot = if !rehydrated {
        let value = owned(
            held,
            &scope,
            &id,
            &recovery.source.model_id,
            draft.history(),
        )?;
        if value.source != recovery.source || value.retained_history != *draft.history() {
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
