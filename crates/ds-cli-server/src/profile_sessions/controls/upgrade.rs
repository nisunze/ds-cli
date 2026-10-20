//! Moving retained recovery off a Network without authored recovery. Such an
//! engine keeps only an opaque checkpoint that grows with calculated results
//! and refuses at its 64 MiB bound, so a retained pin to it would refuse every
//! later edit of a large model. When the current same-lane Network keeps
//! authored recovery and reproduces the exact original and the exact retained
//! head, Core re-pins the recovery to it: the original plus verified command
//! rows. When it cannot, the stale draft refuses with a typed discard remedy.
use super::*;

const AUTHORED_RECOVERY: [NetworkCapability; 4] = [
    NetworkCapability::ProfileSessionV1,
    NetworkCapability::ProfilePackageOpenV1,
    NetworkCapability::ProfileEditControlsV1,
    NetworkCapability::ProfileAuthoredCheckpointV1,
];

/// Whether a retained engine pin already keeps authored recovery.
pub(in crate::profile_sessions) fn pinned_authored(
    source: &ProfileSourceHeader,
) -> Result<bool, Failure> {
    let pin: ds_network_client::NetworkEnginePin =
        serde_json::from_value(source.engine.clone()).map_err(invalid)?;
    Ok(pin
        .identity
        .capabilities
        .contains(&NetworkCapability::ProfileAuthoredCheckpointV1))
}

/// The current same-lane Network when it keeps authored recovery and the
/// requested capability. Without one, the retained pin keeps its behavior.
pub(super) fn current_authored_engine(
    scope: &Scope,
    required: Option<&NetworkCapability>,
) -> Option<CapturedNetworkClient> {
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(2);
    let client = CapturedNetworkClient::capture_for_lane(&scope.lane, workers, &|| false).ok()?;
    let capabilities = &client.identity().capabilities;
    (AUTHORED_RECOVERY
        .iter()
        .all(|capability| capabilities.contains(capability))
        && required.is_none_or(|capability| capabilities.contains(capability)))
    .then_some(client)
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
    let retained = RetainedJournal {
        digests,
        sizes,
        chain: document.chain_sha256.clone(),
    };
    held_from(
        scope,
        session,
        upgraded,
        history.clone(),
        Some(retained),
        authorized,
    )
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

/// Re-pin an original-only recovery when the current engine reproduces it
/// exactly. Nothing authored can be lost, so any difference or engine refusal
/// keeps the retained pin instead.
pub(in crate::profile_sessions) fn original(
    scope: Scope,
    scratch: &Path,
    database: &Path,
    recovery: &ds_sync_store::ProfileRecovery,
    authorized: &dyn Fn() -> Result<(), Failure>,
    required: Option<NetworkCapability>,
) -> Result<Option<(Held, Vec<u8>)>, Failure> {
    let source = &recovery.source;
    if recovery.draft.is_some() || pinned_authored(source)? {
        return Ok(None);
    }
    let Some(client) = current_authored_engine(&scope, required.as_ref()) else {
        return Ok(None);
    };
    let opened = open_original(
        scope.clone(),
        scratch,
        &recovery.source_bytes,
        &source.session_id,
        authorized,
        required,
        Some(client),
    );
    let (value, snapshot) = match opened {
        Ok(opened) if same_original(&opened.0.source, source) => opened,
        Ok(_) => return Ok(None),
        Err(error)
            if matches!(
                error.code(),
                "core_profile_native_refused" | "core_profile_request_invalid"
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
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
    Ok(Some((value, snapshot)))
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
