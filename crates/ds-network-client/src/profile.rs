//! Retained offline Profile client; no engineering or package decode in the host.
use super::*;
use ds_cli_exec::NetworkProfileProcess;
use ds_network_contracts::engine::ArtifactReceipt;
use ds_network_contracts::profile::*;
use serde_json::Value;

/// Exact acknowledged recovery bytes bound to the executable that produced them.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileCheckpoint {
    pub engine: NetworkEnginePin,
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub model_id: String,
    pub history: Value,
    pub sequence: u64,
}
#[derive(Debug)]
pub struct ProfileOutcome {
    pub reply: NativeProfileReplyV1,
    pub bytes: Vec<u8>,
}
/// One model and one immutable worker-affine process. An ordinary owner refusal
/// consumes its sequence but retains the session; transport/integrity failures
/// abort and reap it. Hosts persist `last_checkpoint` through their storage owner.
pub struct ProfileSession {
    process: NetworkProfileProcess,
    run: tempfile::TempDir,
    pin: NetworkEnginePin,
    model_id: String,
    history: Value,
    initial_revision: Value,
    sequence: u64,
    staged_bytes: usize,
    artifacts: usize,
    last_checkpoint: Option<ProfileCheckpoint>,
    closed: bool,
}
impl CapturedNetworkClient {
    pub fn open_profile_session(
        &self,
        input: &[u8],
        scratch: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<ProfileSession, NetworkFailure> {
        self.open_profile_session_with_timeout(input, scratch, TASK_TIMEOUT, cancel)
    }
    /// Explicit bounded deadline is retained for supervised hosts and acceptance.
    pub fn open_profile_session_with_timeout(
        &self,
        input: &[u8],
        scratch: &Path,
        timeout: Duration,
        cancel: &dyn Fn() -> bool,
    ) -> Result<ProfileSession, NetworkFailure> {
        if cancel() {
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "Profile opening was cancelled",
            ));
        }
        if !self
            .identity
            .capabilities
            .contains(&NetworkCapability::ProfileSessionV1)
        {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "captured Network has no Profile session capability",
            ));
        }
        let (model_id, history) = open_fence(input)?;
        let sm = fs::symlink_metadata(scratch)
            .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        if !sm.is_dir() || sm.file_type().is_symlink() {
            return Err(NetworkFailure::new(
                "network_scratch_invalid",
                "scratch must be an ordinary directory",
            ));
        }
        let run = tempfile::Builder::new()
            .prefix("ds-network-profile-")
            .tempdir_in(scratch)
            .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(run.path(), fs::Permissions::from_mode(0o700))
                .map_err(|e| NetworkFailure::new("network_scratch_invalid", e.to_string()))?;
        }
        let request = run.path().join("open.json");
        write_request(&request, input)?;
        let output = run.path().join("output");
        create_output(&output)?;
        if digest_file(
            self.executable(),
            MAX_EXECUTABLE_BYTES,
            "network_engine_changed",
        )? != self.executable_sha256
        {
            return Err(NetworkFailure::new(
                "network_engine_changed",
                "captured Network executable changed before Profile launch",
            ));
        }
        let mut process = self
            .engine
            .network_profile_session(
                &request,
                &output,
                self.identity.runtime.internal_worker_threads,
            )
            .map_err(supervision)?;
        let bytes = process.receive(timeout, cancel).map_err(supervision)?;
        let reply = decode_reply(&bytes, &self.identity)?;
        if reply.sequence != 0
            || reply.model_id != model_id
            || reply.history != history
            || reply.input_sha256 != digest_bytes(input)
            || reply.artifact.is_some()
            || reply.code.is_some()
            || reply.error.is_some()
            || reply.detail.is_some()
            || reply.closed
        {
            return Err(invalid(
                "Profile open reply differs from its seed, history or request digest",
            ));
        }
        if read_regular(&request, MAX_PROFILE_OPEN_BYTES, "network_request_changed")? != input
            || fs::read_dir(&output)
                .map_err(|e| invalid(e.to_string()))?
                .next()
                .is_some()
        {
            return Err(invalid(
                "Profile opening changed staged input or published an unsolicited artifact",
            ));
        }
        let initial_revision = history
            .get("initial_revision")
            .cloned()
            .ok_or_else(|| invalid("Profile history has no initial revision"))?;
        Ok(ProfileSession {
            process,
            run,
            pin: self.pin(),
            model_id,
            history,
            initial_revision,
            sequence: 0,
            staged_bytes: 0,
            artifacts: 0,
            last_checkpoint: None,
            closed: false,
        })
    }
}
impl ProfileSession {
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
    pub fn history(&self) -> &Value {
        &self.history
    }
    pub fn pin(&self) -> &NetworkEnginePin {
        &self.pin
    }
    pub fn process_id(&self) -> u32 {
        self.process.process_id()
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    pub fn last_checkpoint(&self) -> Option<&ProfileCheckpoint> {
        self.last_checkpoint.as_ref()
    }
    pub fn request(
        &mut self,
        operation: NativeProfileOperationV1,
        cancel: &dyn Fn() -> bool,
    ) -> Result<ProfileOutcome, NetworkFailure> {
        self.request_at(operation, Some(self.history.clone()), TASK_TIMEOUT, cancel)
    }
    /// Exact complete journal bytes are staged once inside this retained
    /// session, rather than fragmented into commands or oversized frames.
    pub fn reconcile_journal(
        &mut self,
        journal: &[u8],
        target_history: Value,
        cancel: &dyn Fn() -> bool,
    ) -> Result<ProfileOutcome, NetworkFailure> {
        if self.closed {
            return Err(NetworkFailure::new(
                "profile_session_closed",
                "Profile session cannot accept another journal",
            ));
        }
        if self.sequence >= MAX_PROFILE_REQUESTS {
            return Err(NetworkFailure::new(
                "profile_session_capacity",
                "Profile request capacity reached; retain the last checkpoint",
            ));
        }
        if !self
            .pin
            .identity
            .capabilities
            .contains(&NetworkCapability::ProfileAuthoredFactsV1)
        {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "captured Network has no exact Profile authored-facts capability",
            ));
        }
        if journal.is_empty()
            || journal.len() > MAX_PROFILE_JOURNAL_BYTES
            || !valid_history(&target_history)
        {
            return Err(NetworkFailure::new(
                "profile_replay_input_invalid",
                "exact journal bytes and target history must be bounded",
            ));
        }
        let file = format!("profile-input-{:010}.json", self.sequence + 1);
        let path = self.run.path().join(&file);
        write_request(&path, journal)?;
        let result = self.request(
            NativeProfileOperationV1::ReconcileJournal {
                journal: ArtifactReceipt {
                    file,
                    byte_count: journal.len(),
                    sha256: digest_bytes(journal),
                    schema: PROFILE_JOURNAL_SCHEMA.into(),
                },
                target_history,
            },
            cancel,
        );
        let retained = read_regular(&path, MAX_PROFILE_JOURNAL_BYTES, "network_request_changed");
        let cleanup = fs::remove_file(&path);
        if !matches!(retained, Ok(ref bytes) if bytes == journal) || cleanup.is_err() {
            self.abort();
            return Err(NetworkFailure::new(
                "network_request_changed",
                "staged Profile journal changed during reconciliation",
            ));
        }
        result
    }
    /// Stage one complete view without expanding the retained control frame.
    /// Capability admission precedes serialization/staging; exact bytes are checked
    /// again after the process reply, including refusals.
    pub fn view_at(
        &mut self,
        input: &NativeProfileViewInputV1,
        cached: bool,
        expected_history: Value,
        timeout: Duration,
        cancel: &dyn Fn() -> bool,
    ) -> Result<ProfileOutcome, NetworkFailure> {
        if self.closed {
            return Err(NetworkFailure::new(
                "profile_session_closed",
                "Profile session is closed",
            ));
        }
        if self.sequence >= MAX_PROFILE_REQUESTS {
            return Err(NetworkFailure::new(
                "profile_session_capacity",
                "Profile request capacity reached",
            ));
        }
        if !self
            .pin
            .identity
            .capabilities
            .contains(&NetworkCapability::ProfileViewV1)
        {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "captured Network has no complete Profile view capability",
            ));
        }
        if input.schema != PROFILE_VIEW_INPUT_SCHEMA || !valid_history(&expected_history) {
            return Err(NetworkFailure::new(
                "profile_view_input_invalid",
                "complete Profile view needs its exact schema and retained history",
            ));
        }
        let bytes = serde_json::to_vec(input).map_err(|e| invalid(e.to_string()))?;
        if bytes.len() > MAX_PROFILE_VIEW_INPUT_BYTES {
            return Err(NetworkFailure::new(
                "profile_view_input_invalid",
                "complete Profile view exceeds its input bound",
            ));
        }
        let file = format!("profile-input-{:010}.json", self.sequence + 1);
        let path = self.run.path().join(&file);
        if let Err(error) = write_request(&path, &bytes) {
            // create_new/write may have left an incomplete fixed-sequence file.
            // Retire this worker; its private TempDir cleans up on owner drop.
            self.abort();
            return Err(error);
        }
        let result = self.request_at(
            NativeProfileOperationV1::View {
                input: ArtifactReceipt {
                    file,
                    byte_count: bytes.len(),
                    sha256: digest_bytes(&bytes),
                    schema: PROFILE_VIEW_INPUT_SCHEMA.into(),
                },
                cached,
            },
            Some(expected_history),
            timeout,
            cancel,
        );
        let retained = read_regular(
            &path,
            MAX_PROFILE_VIEW_INPUT_BYTES,
            "network_request_changed",
        );
        let cleanup = fs::remove_file(&path);
        if !matches!(retained, Ok(ref value) if value == &bytes) || cleanup.is_err() {
            self.abort();
            return Err(NetworkFailure::new(
                "network_request_changed",
                "staged Profile view changed during execution",
            ));
        }
        result
    }
    pub fn request_at(
        &mut self,
        operation: NativeProfileOperationV1,
        expected_history: Option<Value>,
        timeout: Duration,
        cancel: &dyn Fn() -> bool,
    ) -> Result<ProfileOutcome, NetworkFailure> {
        if self.closed {
            return Err(NetworkFailure::new(
                "profile_session_closed",
                "Profile session is closed",
            ));
        }
        if matches!(
            operation,
            NativeProfileOperationV1::Snapshot | NativeProfileOperationV1::ReconcileJournal { .. }
        ) && !self
            .pin
            .identity
            .capabilities
            .contains(&NetworkCapability::ProfileAuthoredFactsV1)
        {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "captured Network has no exact Profile authored-facts capability",
            ));
        }
        if matches!(operation, NativeProfileOperationV1::View { .. })
            && !self
                .pin
                .identity
                .capabilities
                .contains(&NetworkCapability::ProfileViewV1)
        {
            return Err(NetworkFailure::new(
                "network_engine_incompatible",
                "captured Network has no complete Profile view capability",
            ));
        }
        if self.sequence >= MAX_PROFILE_REQUESTS {
            return Err(NetworkFailure::new(
                "profile_session_capacity",
                "Profile request capacity reached; retain the last checkpoint",
            ));
        }
        let request = NativeProfileRequestV1 {
            schema: PROFILE_REQUEST_SCHEMA.into(),
            sequence: self.sequence + 1,
            model_id: self.model_id.clone(),
            expected_history,
            operation,
        };
        let input_contract = match &request.operation {
            NativeProfileOperationV1::ReconcileJournal {
                journal,
                target_history,
            } => {
                if !valid_history(target_history) {
                    return Err(NetworkFailure::new(
                        "profile_replay_input_invalid",
                        "exact target history is required",
                    ));
                }
                Some((
                    journal,
                    PROFILE_JOURNAL_SCHEMA,
                    MAX_PROFILE_JOURNAL_BYTES,
                    "profile_replay_input_invalid",
                ))
            }
            NativeProfileOperationV1::View { input, .. } => Some((
                input,
                PROFILE_VIEW_INPUT_SCHEMA,
                MAX_PROFILE_VIEW_INPUT_BYTES,
                "profile_view_input_invalid",
            )),
            _ => None,
        };
        if let Some((receipt, schema, limit, code)) = input_contract {
            if receipt.file != format!("profile-input-{:010}.json", request.sequence)
                || receipt.schema != schema
                || receipt.byte_count == 0
                || receipt.byte_count > limit
                || !valid_hex(&receipt.sha256, 64)
            {
                return Err(NetworkFailure::new(
                    code,
                    "Profile input must name the fixed-sequence bounded private artifact",
                ));
            }
            let bytes = read_regular(&self.run.path().join(&receipt.file), limit, code)?;
            if bytes.len() != receipt.byte_count || digest_bytes(&bytes) != receipt.sha256 {
                return Err(NetworkFailure::new(
                    code,
                    "staged Profile input differs from its receipt",
                ));
            }
        }
        let input = serde_json::to_vec(&request).map_err(|e| invalid(e.to_string()))?;
        if input.len() + 1 > MAX_PROFILE_CONTROL_BYTES {
            return Err(NetworkFailure::new(
                "profile_request_bounded",
                "Profile control frame exceeds its bound",
            ));
        }
        let result = self.exchange(&input, &request, timeout, cancel);
        if result.is_err() && self.sequence != request.sequence {
            self.abort();
        }
        result
    }
    fn exchange(
        &mut self,
        input: &[u8],
        request: &NativeProfileRequestV1,
        timeout: Duration,
        cancel: &dyn Fn() -> bool,
    ) -> Result<ProfileOutcome, NetworkFailure> {
        let sequence = request.sequence;
        let checkpoint = matches!(
            request.operation,
            NativeProfileOperationV1::Checkpoint | NativeProfileOperationV1::Close
        );
        let close = matches!(request.operation, NativeProfileOperationV1::Close);
        let control = matches!(
            request.operation,
            NativeProfileOperationV1::Control { .. }
                | NativeProfileOperationV1::ReconcileJournal { .. }
        );
        let bytes = self
            .process
            .request(input, timeout, cancel)
            .map_err(supervision)?;
        if cancel() {
            self.cancel();
            return Err(NetworkFailure::new(
                "callee_cancelled",
                "Profile result arrived after cancellation",
            ));
        }
        let reply = decode_reply(&bytes, &self.pin.identity)?;
        if reply.sequence != sequence
            || reply.model_id != self.model_id
            || reply.input_sha256 != digest_bytes(input)
            || !valid_history(&reply.history)
            || reply.history.get("initial_revision") != Some(&self.initial_revision)
        {
            return Err(invalid(
                "Profile reply addresses another sequence, model, history or input",
            ));
        }
        if let Some(code) = &reply.code {
            if reply.artifact.is_some()
                || reply.closed
                || reply.history != self.history
                || code.is_empty()
                || code.len() > 128
                || reply
                    .error
                    .as_ref()
                    .is_none_or(|s| s.is_empty() || s.len() > 4096)
                || reply
                    .detail
                    .as_ref()
                    .is_some_and(|s| s.len() > MAX_IDENTITY_BYTES)
            {
                return Err(invalid(
                    "Profile refusal is malformed or changed retained history",
                ));
            }
            self.sequence = sequence;
            return Err(NetworkFailure {
                code: "network_engine_refused",
                message: reply.error.unwrap(),
                owner_error: None,
                owner_code: Some(code.clone()),
                owner_detail: reply.detail.map(|s| Box::new(Value::String(s))),
                owner_remedy: None,
                owner_invalid_input: None,
            });
        }
        if reply.error.is_some()
            || reply.detail.is_some()
            || reply.closed != close
            || (!control && reply.history != self.history)
        {
            return Err(invalid(
                "Profile success differs from its history or terminal contract",
            ));
        }
        if let NativeProfileOperationV1::ReconcileJournal { target_history, .. } =
            &request.operation
        {
            if &reply.history != target_history {
                return Err(invalid(
                    "Profile reconciliation differs from its exact target history",
                ));
            }
        }
        let artifact = reply
            .artifact
            .as_ref()
            .ok_or_else(|| invalid("Profile success has no artifact"))?;
        let expected_name = format!("profile-{sequence:010}.json");
        let expected_schema = if checkpoint {
            PROFILE_OPEN_SCHEMA
        } else {
            "ds.network.profile-result/v1"
        };
        if artifact.file != expected_name
            || artifact.schema != expected_schema
            || artifact.byte_count > MAX_PROFILE_ARTIFACT_BYTES
            || !valid_hex(&artifact.sha256, 64)
        {
            return Err(invalid("Profile artifact name, schema or bound differs"));
        }
        let output = self.run.path().join("output");
        let directory = fs::symlink_metadata(&output).map_err(|e| invalid(e.to_string()))?;
        if !directory.is_dir() || directory.file_type().is_symlink() {
            return Err(invalid("Profile output directory changed"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if directory.file_attributes() & 0x400 != 0 {
                return Err(invalid("Profile output became a reparse point"));
            }
        }
        let names = fs::read_dir(&output)
            .map_err(|e| invalid(e.to_string()))?
            .take(2)
            .map(|entry| entry.map(|e| e.file_name()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| invalid(e.to_string()))?;
        if names.len() != 1 || names[0] != std::ffi::OsStr::new(&expected_name) {
            return Err(invalid("Profile published unsolicited output"));
        }
        let artifact_path = output.join(&expected_name);
        let bytes = read_regular(
            &artifact_path,
            MAX_PROFILE_ARTIFACT_BYTES,
            "network_result_invalid",
        )?;
        if bytes.len() != artifact.byte_count || digest_bytes(&bytes) != artifact.sha256 {
            return Err(NetworkFailure::new(
                "network_result_digest_mismatch",
                "Profile artifact digest differs from its receipt",
            ));
        }
        let count = self
            .staged_bytes
            .checked_add(bytes.len())
            .filter(|n| *n <= MAX_PROFILE_STAGED_BYTES)
            .ok_or_else(|| invalid("Profile staged bytes exceed their cumulative bound"))?;
        if self.artifacts >= MAX_PROFILE_ARTIFACTS {
            return Err(invalid(
                "Profile artifact count exceeds its cumulative bound",
            ));
        }
        if checkpoint {
            let (model, history) = open_fence(&bytes)?;
            if model != self.model_id || history != reply.history {
                return Err(invalid(
                    "Profile checkpoint addresses another model or history",
                ));
            }
        } else {
            let _: Box<serde_json::value::RawValue> =
                serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
            if matches!(&request.operation, NativeProfileOperationV1::View { .. }) {
                let view: Value =
                    serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
                if view["model_id"] != self.model_id
                    || view["history"] != reply.history
                    || view["model_revision"] != reply.history["model_revision"]
                    || !validate_profile_view_shape(&view)
                {
                    return Err(invalid(
                        "Profile view differs from its retained model, history or complete result",
                    ));
                }
            }
            let snapshot = match &request.operation {
                NativeProfileOperationV1::Snapshot => Some(
                    serde_json::from_slice::<NativeProfileSnapshotV1>(&bytes)
                        .map_err(|e| invalid(e.to_string()))?,
                ),
                NativeProfileOperationV1::ReconcileJournal { journal, .. } => {
                    let result: NativeProfileReconcileV1 =
                        serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
                    if result.journal_sha256 != journal.sha256 {
                        return Err(invalid(
                            "Profile result differs from the exact staged journal digest",
                        ));
                    }
                    Some(result.snapshot)
                }
                _ => None,
            };
            if snapshot.is_some_and(|snapshot| {
                snapshot.schema != PROFILE_SNAPSHOT_SCHEMA
                    || snapshot.model_id != self.model_id
                    || snapshot.history != reply.history
            }) {
                return Err(invalid(
                    "Profile authored facts differ from their model or history",
                ));
            }
        }
        fs::remove_file(&artifact_path).map_err(|e| invalid(e.to_string()))?;
        self.sequence = sequence;
        self.history = reply.history.clone();
        self.staged_bytes = count;
        self.artifacts += 1;
        if checkpoint {
            self.last_checkpoint = Some(ProfileCheckpoint {
                engine: self.pin.clone(),
                bytes: bytes.clone(),
                sha256: artifact.sha256.clone(),
                model_id: self.model_id.clone(),
                history: self.history.clone(),
                sequence,
            });
        }
        if close {
            self.closed = true;
            self.process
                .finish(Duration::from_secs(5), cancel)
                .map_err(supervision)?;
        }
        Ok(ProfileOutcome { reply, bytes })
    }
    pub fn checkpoint(
        &mut self,
        cancel: &dyn Fn() -> bool,
    ) -> Result<ProfileOutcome, NetworkFailure> {
        self.request(NativeProfileOperationV1::Checkpoint, cancel)
    }
    pub fn close(&mut self, cancel: &dyn Fn() -> bool) -> Result<ProfileOutcome, NetworkFailure> {
        self.request(NativeProfileOperationV1::Close, cancel)
    }
    pub fn cancel(&mut self) {
        self.closed = true;
        self.process.cancel();
    }
    pub fn abort(&mut self) {
        self.closed = true;
        self.process.abort();
    }
}
impl Drop for ProfileSession {
    fn drop(&mut self) {
        self.abort();
    }
}
fn open_fence(input: &[u8]) -> Result<(String, Value), NetworkFailure> {
    if input.is_empty() || input.len() > MAX_PROFILE_OPEN_BYTES {
        return Err(NetworkFailure::new(
            "profile_seed_invalid",
            "Profile open exceeds its bound",
        ));
    }
    let open: NativeProfileOpenV1 = serde_json::from_slice(input)
        .map_err(|e| NetworkFailure::new("profile_seed_invalid", e.to_string()))?;
    if open.schema != PROFILE_OPEN_SCHEMA {
        return Err(NetworkFailure::new(
            "profile_seed_invalid",
            "Profile open schema is incompatible",
        ));
    }
    // Only transport identities are read. Network validates/replays engineering,
    // complete binding history, protected assets and calculated signatures.
    let seed: Value = serde_json::from_str(&open.seed_json)
        .map_err(|e| NetworkFailure::new("profile_seed_invalid", e.to_string()))?;
    let model = seed
        .get("model_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .ok_or_else(|| invalid("Profile seed has no bounded model identity"))?;
    let history = seed
        .get("history")
        .filter(|v| valid_history(v))
        .cloned()
        .ok_or_else(|| invalid("Profile seed has no history fence"))?;
    Ok((model.into(), history))
}
fn decode_reply(
    bytes: &[u8],
    identity: &EngineIdentity,
) -> Result<NativeProfileReplyV1, NetworkFailure> {
    let reply: NativeProfileReplyV1 = match serde_json::from_slice(bytes) {
        Ok(reply) => reply,
        Err(error) => {
            if let Ok(text) = std::str::from_utf8(bytes) {
                if let Ok(refusal) = read_refusal(text, identity) {
                    return Err(refusal);
                }
            }
            return Err(invalid(error.to_string()));
        }
    };
    if reply.schema != PROFILE_REPLY_SCHEMA || !same_identity(&reply.engine_identity, identity)? {
        return Err(invalid(
            "Profile reply schema or captured engine identity differs",
        ));
    }
    Ok(reply)
}
fn invalid(message: impl Into<String>) -> NetworkFailure {
    NetworkFailure::new("network_profile_reply_invalid", message)
}
fn supervision(failure: ds_cli_contract::outcome::Failure) -> NetworkFailure {
    let code = match failure.code() {
        "callee_session_closed" => "profile_session_closed",
        "callee_output_bounded" => "network_receipt_bounded",
        "callee_session_failed" => "profile_session_failed",
        code => static_code(code),
    };
    NetworkFailure::new(code, failure.to_string())
}

// Admission of the complete wire fence only; Network decides whether it matches
// authored history. No command replay, revision calculation or defaults here.
fn valid_history(value: &Value) -> bool {
    let revision = |key| {
        value
            .get(key)
            .and_then(Value::as_str)
            .and_then(|s| s.strip_prefix("rev:"))
            .is_some_and(|s| valid_hex(s, 64))
    };
    revision("initial_revision")
        && revision("model_revision")
        && value
            .get("history_pin")
            .and_then(Value::as_str)
            .is_some_and(|s| valid_hex(s, 64))
        && value
            .get("undo_depth")
            .and_then(Value::as_u64)
            .is_some_and(|n| usize::try_from(n).is_ok())
        && value
            .get("redo_depth")
            .and_then(Value::as_u64)
            .is_some_and(|n| usize::try_from(n).is_ok())
}
