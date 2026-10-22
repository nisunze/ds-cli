//! Restoring retained recovery through the selected compatible Network. An
//! authored journal is replayed without changing any retained payload byte.
//! A Network without authored recovery keeps only an opaque checkpoint that grows with calculated results
//! and refuses at its 64 MiB bound, so a retained pin to it would refuse every
//! later edit of a large model. When the current same-lane Network keeps
//! authored recovery and reproduces the exact original and the exact retained
//! head, Core re-pins the recovery to it: the original plus verified command
//! rows. A selected legacy engine may keep the opaque format. Failed admission
//! leaves every retained draft and history available.
use super::*;

/// Selection and actual operation capabilities govern a restored worker.
/// Retained executable provenance is not a cross-build compatibility gate.
pub(super) fn selected_engine(
    scope: &Scope,
    required: Option<&NetworkCapability>,
) -> Result<CapturedNetworkClient, Failure> {
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(2);
    let client =
        CapturedNetworkClient::capture_for_lane(&scope.lane, workers, &|| false).map_err(native)?;
    let capabilities = &client.identity().capabilities;
    require_read_capability(capabilities, NetworkCapability::ProfileSessionV1)?;
    require_read_capability(capabilities, NetworkCapability::ProfilePackageOpenV1)?;
    if let Some(required) = required {
        require_read_capability(capabilities, required.clone())?;
    }
    Ok(client)
}

/// Equal retained originals apart from the engine pin.
pub(in crate::profile_sessions) fn same_original(
    candidate: &ProfileSourceHeader,
    retained: &ProfileSourceHeader,
) -> bool {
    let mut candidate = candidate.clone();
    candidate.engine = retained.engine.clone();
    &candidate == retained
}

pub(in crate::profile_sessions) fn upgrade_required(
    source: &ProfileSourceHeader,
    history: &Value,
    reason: impl std::fmt::Display,
) -> Failure {
    let discard = json!({"action":"discard_recovery","session_id":source.session_id,
        "model_id":source.model_id,"expected_history":history});
    Failure::conflict(
        "core_profile_draft_stale",
        format!("This Profile draft was kept by a Network without authored recovery, and the current Network cannot reproduce it exactly ({reason})"),
    )
    .remedy("Discard the stale draft with core.profile-session action discard_recovery (detail.discard; its unsaved edits are lost), then reopen the package")
    .detail(json!({"session_id":source.session_id,"model_id":source.model_id,
        "expected_history":history,"discard":discard}))
}

/// An owner refusal means the engines disagree about this exact recovery;
/// transport and supervision failures keep their own typed remedy.
fn lossless(source: &ProfileSourceHeader, history: &Value, error: NetworkFailure) -> Failure {
    if error.code() == "network_engine_refused" {
        upgrade_required(source, history, error)
    } else {
        native(error)
    }
}

/// Convert an opaque v1 draft. The current engine reopens it without its
/// disposable results and emits the complete authored journal; that journal
/// replayed onto the current engine's original must reach the same head. One
/// worker runs at a time, and SQLite changes only after both proofs.
pub(super) fn checkpoint(
    scope: Scope,
    scratch: &Path,
    database: &Path,
    recovery: &ds_sync_store::ProfileRecovery,
    client: &CapturedNetworkClient,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<(Held, Vec<u8>), Failure> {
    let Some(ProfileDraft::Checkpoint { header, bytes }) = &recovery.draft else {
        return Err(invalid("An opaque v1 draft is required"));
    };
    let source = &recovery.source;
    let history = &header.history;
    let mut retained: serde_json::Map<String, Value> =
        serde_json::from_slice(bytes).map_err(invalid)?;
    retained.insert("calculated".into(), Value::Null);
    retained.insert("analysis".into(), Value::Null);
    let staged = serde_json::to_vec(&retained).map_err(invalid)?;
    drop(retained);
    let document = {
        let mut session = client
            .open_profile_session_with_timeout(&staged, scratch, DEADLINE, &|| false)
            .map_err(|error| lossless(source, history, error))?;
        if session.model_id() != source.model_id || session.history() != history {
            return Err(upgrade_required(source, history, "another retained head"));
        }
        authorized()?;
        session
            .authored_checkpoint(None, &|| false)
            .map_err(|error| lossless(source, history, error))?
            .1
    };
    drop(staged);
    authorized()?;
    let digests: Vec<String> = document
        .entries
        .iter()
        .map(|entry| profile_authored_entry_sha256(entry))
        .collect();
    let sizes: Vec<u64> = document.entries.iter().map(|e| e.len() as u64).collect();
    if !document.admitted_shape()
        || document.base_entries != 0
        || document.model_id != source.model_id
        || &document.history != history
        || profile_authored_chain(digests.iter().map(String::as_str)) != document.chain_sha256
    {
        return Err(invalid(
            "Native authored checkpoint differs from its exact command chain",
        ));
    }
    let mut session = current_original(client, scratch, recovery, history, authorized)?;
    let deadline = DEADLINE
        .saturating_add(Duration::from_millis(250).saturating_mul(document.entry_count as u32))
        .min(Duration::from_secs(30 * 60));
    session
        .restore_authored(
            &serde_json::to_vec(&document).map_err(invalid)?,
            deadline,
            &|| false,
        )
        .map_err(|error| lossless(source, history, error))?;
    if session.model_id() != source.model_id || session.history() != history {
        return Err(upgrade_required(
            source,
            history,
            "another head after replay onto the original",
        ));
    }
    let mut upgraded = source.clone();
    upgraded.engine = serde_json::to_value(session.pin()).map_err(invalid)?;
    let (journal, metadata) = journal_header(&document, &upgraded, &sizes)?;
    let entries: Vec<&[u8]> = document.entries.iter().map(|e| e.as_bytes()).collect();
    let retained = RetainedJournal {
        digests,
        sizes,
        chain: document.chain_sha256.clone(),
    };
    let held = held_from(
        scope.clone(),
        session,
        upgraded.clone(),
        history.clone(),
        Some(retained),
        authorized,
    )?;
    authorized()?;
    Store::open(database)
        .map_err(source_error)?
        .upgrade_profile_recovery(
            &source_scope(&scope),
            source,
            history,
            &upgraded,
            Some((&journal, &metadata, &entries)),
            now_ms(),
        )
        .map_err(source_error)?;
    authorized()?;
    Ok(held)
}

/// An engine without authored-journal support still restores its supported
/// opaque checkpoint. Original and snapshot admission precede provenance-only
/// retention; no format conversion or executable-identity match is required.
pub(super) fn opaque(
    scope: Scope,
    scratch: &Path,
    database: &Path,
    recovery: &ds_sync_store::ProfileRecovery,
    client: &CapturedNetworkClient,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<(Held, Vec<u8>), Failure> {
    let Some(ProfileDraft::Checkpoint { header, bytes }) = &recovery.draft else {
        return Err(invalid("An opaque v1 draft is required"));
    };
    drop(current_original(
        client,
        scratch,
        recovery,
        &header.history,
        authorized,
    )?);
    let session = client
        .open_profile_session_with_timeout(bytes, scratch, DEADLINE, &|| false)
        .map_err(native)?;
    let mut upgraded = recovery.source.clone();
    upgraded.engine = serde_json::to_value(session.pin()).map_err(invalid)?;
    let held = held_from(
        scope.clone(),
        session,
        upgraded.clone(),
        header.history.clone(),
        None,
        authorized,
    )?;
    if upgraded.engine != recovery.source.engine {
        authorized()?;
        Store::open(database)
            .map_err(source_error)?
            .upgrade_profile_recovery(
                &source_scope(&scope),
                &recovery.source,
                &header.history,
                &upgraded,
                None,
                now_ms(),
            )
            .map_err(source_error)?;
        authorized()?;
    }
    Ok(held)
}

/// Replay the exact retained journal through the selected engine. The snapshot
/// and every typed restore receipt are admitted before the atomic provenance
/// update, which must retain all existing authored bytes.
pub(super) fn journal(
    scope: Scope,
    scratch: &Path,
    database: &Path,
    recovery: &ds_sync_store::ProfileRecovery,
    journal: &ds_sync_store::ProfileJournalDraft,
    client: &CapturedNetworkClient,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<(Held, Vec<u8>), Failure> {
    let source = &recovery.source;
    let (session, retained) = restore_journal(client, scratch, recovery, journal, authorized)?;
    let mut upgraded = source.clone();
    upgraded.engine = serde_json::to_value(session.pin()).map_err(invalid)?;
    let held = held_from(
        scope.clone(),
        session,
        upgraded.clone(),
        journal.header.history.clone(),
        Some(retained),
        authorized,
    )?;
    if upgraded.engine != source.engine {
        let mut header = journal.header.clone();
        header.source = upgraded.clone();
        let entries: Vec<&[u8]> = journal
            .entries
            .iter()
            .map(|entry| entry.bytes.as_slice())
            .collect();
        authorized()?;
        Store::open(database)
            .map_err(source_error)?
            .upgrade_profile_recovery(
                &source_scope(&scope),
                source,
                &header.history,
                &upgraded,
                Some((&header, &journal.metadata, &entries)),
                now_ms(),
            )
            .map_err(source_error)?;
        authorized()?;
    }
    Ok(held)
}

/// Open the retained original through the current engine at its exact
/// recorded revision, CRS, model and initial history.
fn current_original(
    client: &CapturedNetworkClient,
    scratch: &Path,
    recovery: &ds_sync_store::ProfileRecovery,
    history: &Value,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<ProfileSession, Failure> {
    let source = &recovery.source;
    let prepared = client
        .prepare_profile_open(&recovery.source_bytes, scratch, DEADLINE, &|| false)
        .map_err(|error| lossless(source, history, error))?
        .result;
    authorized()?;
    if prepared.source_revision != source.source_revision
        || prepared.coordinate_system != source.coordinate_system
    {
        return Err(upgrade_required(
            source,
            history,
            "another original revision or CRS",
        ));
    }
    let input = serde_json::to_vec(&prepared.open).map_err(invalid)?;
    let session = client
        .open_profile_session_with_timeout(&input, scratch, DEADLINE, &|| false)
        .map_err(|error| lossless(source, history, error))?;
    authorized()?;
    if session.model_id() != source.model_id || session.history() != &source.initial_history {
        return Err(upgrade_required(
            source,
            history,
            "another initial model history",
        ));
    }
    Ok(session)
}

/// Restore an original with the selected engine. Only exact domain identity
/// allows its provenance to change; an incompatible reply leaves SQLite alone.
pub(in crate::profile_sessions) fn original(
    scope: Scope,
    scratch: &Path,
    database: &Path,
    recovery: &ds_sync_store::ProfileRecovery,
    authorized: &dyn Fn() -> Result<(), Failure>,
    required: Option<NetworkCapability>,
) -> Result<(Held, Vec<u8>), Failure> {
    let source = &recovery.source;
    if recovery.draft.is_some() {
        return Err(invalid("An original without a draft is required"));
    }
    let client = selected_engine(&scope, required.as_ref())?;
    let (value, snapshot) = open_original(
        scope.clone(),
        scratch,
        &recovery.source_bytes,
        &source.session_id,
        authorized,
        required,
        Some(client),
    )?;
    if !same_original(&value.source, source) {
        return Err(Failure::conflict(
            "core_profile_source_changed",
            "Selected Network differs from the retained original model/history/revision/CRS",
        ));
    }
    if value.source.engine != source.engine {
        authorized()?;
        Store::open(database)
            .map_err(source_error)?
            .upgrade_profile_recovery(
                &source_scope(&scope),
                source,
                &source.initial_history,
                &value.source,
                None,
                now_ms(),
            )
            .map_err(source_error)?;
        authorized()?;
    }
    Ok((value, snapshot))
}

/// Explicitly drop the retained recovery of a session that is not live, at its
/// exact model and retained head: the remedy for a stale draft no current
/// Network reproduces. Its unsaved edits are lost; nothing is restored first.
pub(in crate::profile_sessions) fn discard(
    held: &BTreeMap<String, Held>,
    scope: &Scope,
    database: &Path,
    session_id: &str,
    model_id: &str,
    expected_history: &Value,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<Value, Failure> {
    authorized()?;
    if held.contains_key(session_id) {
        return Err(Failure::conflict(
            "core_profile_session_active",
            "This Profile session is live; its recovery follows the live session",
        )
        .remedy("Use the live session's discard and close actions"));
    }
    let owner = source_scope(scope);
    let recovery = Store::open_read_only(database)
        .map_err(source_error)?
        .map(|store| store.profile_recovery(&owner, session_id))
        .transpose()
        .map_err(source_error)?
        .flatten()
        .ok_or_else(|| {
            Failure::unavailable(
                "core_profile_session_unavailable",
                "No retained Profile recovery belongs to this exact account/lane/project",
            )
            .remedy("Read the refusal that named this recovery and use its exact session")
        })?;
    let retained = recovery
        .draft
        .as_ref()
        .map_or(&recovery.source.initial_history, ProfileDraft::history);
    if recovery.source.model_id != model_id || retained != expected_history {
        return Err(Failure::conflict(
            "core_profile_history_conflict",
            "Retained recovery differs from the exact requested model/history",
        )
        .remedy("Use the exact session, model and history named by the refusal"));
    }
    authorized()?;
    Store::open(database)
        .map_err(source_error)?
        .remove_profile_recovery(&owner, &recovery.source, retained)
        .map_err(source_error)?;
    authorized()?;
    let mut receipt = closed_source_receipt(scope, &recovery.source);
    receipt["history"] = retained.clone();
    receipt["discarded"] = json!(true);
    Ok(receipt)
}
