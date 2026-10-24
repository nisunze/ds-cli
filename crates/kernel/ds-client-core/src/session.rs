use ds_command_kernel::{
    classify_firebase_auth_failure, validate_firebase_id_token, validate_firebase_login_response,
    FirebaseAuthFailureKind, LoginError, LoginErrorKind,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::project_report::{
    CompoundedArchiveCatalog, RetirementAction, RetirementReceipt, RetirementRequest,
    TransformerInventory, TransformerSet, TransformerStatusList,
};
use crate::weak_network::transient_status;
use crate::{
    ClientError, ClientProfile, ErrorKind, ProjectDirectory, ProjectFormSettingsEditor,
    ProjectFormsSnapshot, RefreshCall, SignInCall, SolarSnapshot, StoreError, SurveyEntriesChanges,
    SurveyEntriesChangesRequest, SurveyEntriesSelectRequest, SurveyEntriesSelection,
    SurveyEntryCreateReceipt, SurveyEntryCreateRequest, SurveyQueryRequest, SurveyQueryResult,
    TileOperationResult, TilePreflight, TileType, TransformerContext, Transport, TransportResponse,
    WeakNetwork,
};

const MAX_AUTH_RESPONSE: usize = crate::transport::AUTH_RESPONSE_LIMIT;
const MAX_STORED_REFRESH: usize = 64 * 1024;
const REFRESH_SKEW_SECONDS: u64 = 10 * 60;
const STORED_SCHEMA: &str = "ds-client.refresh/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreKey(String);

impl StoreKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Atomic durable storage for the refresh credential.
///
/// Implementations must protect bytes for the current OS user, provide an
/// exclusive cross-process lease, and implement compare-and-swap. The
/// serialized value contains identity binding plus one refresh token; it never
/// contains a password or ID token.
pub trait RefreshTokenStore {
    /// Acquire an exclusive cross-process lease for this key. The lease must
    /// remain held until `release` and cover load, Firebase exchange, and CAS.
    fn acquire(&mut self, key: &StoreKey) -> Result<(), StoreError>;
    fn load(&mut self, key: &StoreKey) -> Result<Option<Vec<u8>>, StoreError>;
    fn compare_and_swap(
        &mut self,
        key: &StoreKey,
        expected: Option<&[u8]>,
        replacement: Option<&[u8]>,
    ) -> Result<(), StoreError>;
    fn release(&mut self, key: &StoreKey) -> Result<(), StoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedUser {
    uid: String,
    email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAuthContext {
    uid: String,
    email: String,
    lane: &'static str,
    credential_audience_sha256: String,
    credential_instance_sha256: String,
}

impl StoredAuthContext {
    pub fn email(&self) -> &str {
        &self.email
    }
    pub fn uid(&self) -> &str {
        &self.uid
    }
    pub fn lane(&self) -> &str {
        self.lane
    }
    pub fn credential_audience_sha256(&self) -> &str {
        &self.credential_audience_sha256
    }
    /// Opaque login-instance identity, stable across refreshes. Not a credential.
    pub fn credential_instance_sha256(&self) -> &str {
        &self.credential_instance_sha256
    }
}

/// Inspect protected Firebase state without refreshing it or exposing email/token.
pub fn probe_stored_refresh(
    bytes: &[u8],
    profile: &ClientProfile,
) -> Result<StoredAuthContext, ClientError> {
    reject_oversized_stored(Some(bytes))?;
    let stored: StoredRefresh = serde_json::from_slice(bytes).map_err(|_| {
        ClientError::new(
            ErrorKind::DurableState,
            "the stored refresh credential is unreadable",
        )
    })?;
    validate_stored(&stored, profile)?;
    Ok(StoredAuthContext {
        credential_instance_sha256: stored.instance(),
        uid: stored.uid.clone(),
        email: stored.email.clone(),
        lane: profile.lane().token(),
        credential_audience_sha256: profile.credential_audience_sha256().to_owned(),
    })
}

impl AuthenticatedUser {
    pub fn uid(&self) -> &str {
        &self.uid
    }
    pub fn email(&self) -> &str {
        &self.email
    }
}

pub struct Client<T, S> {
    profile: ClientProfile,
    transport: T,
    store: S,
    session: Option<Session>,
    weak_network: WeakNetwork,
}

struct Session {
    stored: StoredRefresh,
    stored_bytes: SecretBuffer,
    id_token: String,
    expires_at: u64,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.id_token.zeroize();
    }
}

struct SecretBuffer(Vec<u8>);

impl SecretBuffer {
    fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl Drop for SecretBuffer {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
struct StoredRefresh {
    schema: String,
    credential_audience: String,
    uid: String,
    email: String,
    refresh_token: String,
    /// Optional for existing installations; the first refresh preserves the
    /// instance derived from their old credential rather than changing it.
    #[serde(default)]
    credential_instance_sha256: String,
}

fn legacy_login_instance(token: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"ds.firebase-login-instance/v1\0");
    hash.update((token.len() as u64).to_be_bytes());
    hash.update(token.as_bytes());
    format!("{:x}", hash.finalize())
}

impl StoredRefresh {
    fn instance(&self) -> String {
        if self.credential_instance_sha256.is_empty() {
            legacy_login_instance(&self.refresh_token)
        } else {
            self.credential_instance_sha256.clone()
        }
    }
}

impl Drop for StoredRefresh {
    fn drop(&mut self) {
        self.refresh_token.zeroize();
    }
}

impl<T: Transport, S: RefreshTokenStore> Client<T, S> {
    pub fn new(profile: ClientProfile, transport: T, store: S) -> Self {
        Self {
            profile,
            transport,
            store,
            session: None,
            weak_network: WeakNetwork::FIELD,
        }
    }

    /// Replace the [`WeakNetwork::FIELD`] schedule sign-in and token refresh
    /// retry a blink with. Only the pauses change; what counts as a blink,
    /// and that nothing definitive is retried, does not.
    pub fn with_weak_network(mut self, weak_network: WeakNetwork) -> Self {
        self.weak_network = weak_network;
        self
    }

    pub fn profile(&self) -> &ClientProfile {
        &self.profile
    }

    pub fn user(&self) -> Option<AuthenticatedUser> {
        self.session.as_ref().map(|session| AuthenticatedUser {
            uid: session.stored.uid.clone(),
            email: session.stored.email.clone(),
        })
    }

    pub fn sign_in(
        &mut self,
        email: &str,
        password: &str,
        now_epoch_seconds: u64,
    ) -> Result<AuthenticatedUser, ClientError> {
        validate_email(email)?;
        if password.is_empty() || password.len() > 4096 || password.contains(['\r', '\n']) {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "password input is empty or outside its protected-input bound",
            ));
        }
        // A dropped connection or a 5xx is retried. A 429 is not: it is
        // Firebase's password throttle, and another attempt is exactly what it
        // asks not to receive.
        let network = self.weak_network;
        let (outcome, retries) = network.run(
            || {
                self.transport.sign_in(SignInCall {
                    profile: &self.profile,
                    email,
                    password,
                })
            },
            |outcome| match outcome {
                Err(_) => true,
                Ok(response) => response.status != 429 && transient_status(response.status),
            },
        );
        let response = outcome.map_err(|error| ClientError::from(error).after_retries(retries))?;
        if response.status != 200 {
            return Err(classify_auth_error(response, false).after_retries(retries));
        }
        let mut login = validate_firebase_login_response(
            &response.body,
            Some(email),
            self.profile.firebase_project_id(),
            now_epoch_seconds,
        )
        .map_err(map_login_error)?;
        let identity = AuthenticatedUser {
            uid: login.principal().uid().to_owned(),
            email: login.principal().email().to_owned(),
        };
        let mut instance = [0_u8; 32];
        getrandom::fill(&mut instance).map_err(|_| {
            ClientError::new(
                ErrorKind::DurableState,
                "native login entropy is unavailable",
            )
        })?;
        let stored = StoredRefresh {
            schema: STORED_SCHEMA.to_owned(),
            credential_audience: self.profile.binding().to_owned(),
            uid: identity.uid.clone(),
            email: identity.email.clone(),
            refresh_token: login.take_refresh_token(),
            credential_instance_sha256: format!("{:x}", Sha256::digest(instance)),
        };
        let stored_bytes =
            SecretBuffer::new(serde_json::to_vec(&stored).map_err(|_| unreadable_auth())?);
        let key = self.store_key();
        self.store.acquire(&key)?;
        let operation = (|| {
            let previous = self.store.load(&key)?.map(SecretBuffer::new);
            reject_oversized_stored(previous.as_ref().map(SecretBuffer::as_slice))?;
            let result = self.store.compare_and_swap(
                &key,
                previous.as_ref().map(SecretBuffer::as_slice),
                Some(stored_bytes.as_slice()),
            );
            result.map_err(ClientError::from)
        })();
        finish_lease(operation, self.store.release(&key))?;
        self.session = Some(Session {
            stored,
            stored_bytes,
            id_token: login.take_id_token(),
            expires_at: login.expires_at_epoch_seconds(),
        });
        Ok(identity)
    }

    /// Load the refresh-only durable credential and immediately mint a new ID token.
    pub fn restore(
        &mut self,
        now_epoch_seconds: u64,
    ) -> Result<Option<AuthenticatedUser>, ClientError> {
        let key = self.store_key();
        self.store.acquire(&key)?;
        let operation = (|| {
            let Some(bytes) = self.store.load(&key)?.map(SecretBuffer::new) else {
                self.session = None;
                return Ok(None);
            };
            reject_oversized_stored(Some(bytes.as_slice()))?;
            let stored: StoredRefresh = serde_json::from_slice(bytes.as_slice()).map_err(|_| {
                ClientError::new(
                    ErrorKind::DurableState,
                    "the stored refresh credential is not its bounded contract",
                )
            })?;
            validate_stored(&stored, &self.profile)?;
            self.session = Some(Session {
                stored,
                stored_bytes: bytes,
                id_token: String::new(),
                expires_at: 0,
            });
            self.refresh_under_lease(now_epoch_seconds)?;
            Ok(self.user())
        })();
        finish_lease(operation, self.store.release(&key))
    }

    pub fn sign_out(&mut self) -> Result<(), ClientError> {
        let key = self.store_key();
        self.store.acquire(&key)?;
        let operation = (|| {
            let loaded = self.store.load(&key)?.map(SecretBuffer::new);
            reject_oversized_stored(loaded.as_ref().map(SecretBuffer::as_slice))?;
            let expected = loaded.as_ref().map(SecretBuffer::as_slice);
            let result = self.store.compare_and_swap(&key, expected, None);
            result?;
            Ok(())
        })();
        // Explicit logout is immediate in memory even when durable cleanup
        // needs host repair; no failed store operation may leave this process
        // presenting an authenticated session.
        self.session = None;
        finish_lease(operation, self.store.release(&key))
    }

    pub fn refresh(&mut self, now_epoch_seconds: u64) -> Result<(), ClientError> {
        let key = self.store_key();
        self.store.acquire(&key)?;
        let operation = self.refresh_under_lease(now_epoch_seconds);
        finish_lease(operation, self.store.release(&key))
    }

    fn refresh_under_lease(&mut self, now_epoch_seconds: u64) -> Result<(), ClientError> {
        let durable = self.store.load(&self.store_key())?.map(SecretBuffer::new);
        reject_oversized_stored(durable.as_ref().map(SecretBuffer::as_slice))?;
        let session_bytes = self
            .session
            .as_ref()
            .ok_or_else(signed_out)?
            .stored_bytes
            .as_slice();
        if durable.as_ref().map(SecretBuffer::as_slice) != Some(session_bytes) {
            self.session = None;
            return Err(ClientError::from(StoreError::Conflict));
        }
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        // The exchange is idempotent: the stored credential is replaced only
        // after a 200, under this same lease, so a blink is simply asked again.
        let network = self.weak_network;
        let (outcome, retries) = network.run(
            || {
                self.transport.refresh(RefreshCall {
                    profile: &self.profile,
                    refresh_token: &session.stored.refresh_token,
                })
            },
            |outcome| match outcome {
                Err(_) => true,
                Ok(response) => transient_status(response.status),
            },
        );
        let response = outcome.map_err(|error| ClientError::from(error).after_retries(retries))?;
        if response.status != 200 {
            let error = classify_auth_error(response, true).after_retries(retries);
            if error.is_permanent_session_failure() {
                return Err(self.clear_permanent_disposition(error));
            }
            return Err(error);
        }
        let mut wire: RefreshResponse = decode(response, MAX_AUTH_RESPONSE)?;
        let current = self.session.as_ref().ok_or_else(signed_out)?;
        if wire.user_id != current.stored.uid
            || wire.token_type != "Bearer"
            || !valid_project_metadata(wire.project_id.as_ref())
        {
            return Err(self.clear_permanent_disposition(identity_mismatch()));
        }
        validate_secret(&wire.refresh_token)?;
        let expires_in = parse_expiry(&wire.expires_in)?;
        let selected_token = match take_refresh_jwt(&mut wire) {
            Ok(token) => token,
            Err(error) => return Err(self.clear_permanent_disposition(error)),
        };
        let claim_expiry = validate_firebase_id_token(
            selected_token.as_str(),
            &current.stored.uid,
            &current.stored.email,
            self.profile.firebase_project_id(),
            now_epoch_seconds,
        )
        .map_err(map_login_error);
        let claim_expiry = match claim_expiry {
            Ok(claim_expiry) => claim_expiry,
            Err(error) => {
                return Err(self.clear_permanent_disposition(error));
            }
        };
        let replacement = StoredRefresh {
            schema: STORED_SCHEMA.to_owned(),
            credential_audience: self.profile.binding().to_owned(),
            uid: current.stored.uid.clone(),
            email: current.stored.email.clone(),
            refresh_token: std::mem::take(&mut wire.refresh_token),
            credential_instance_sha256: current.stored.instance(),
        };
        let replacement_bytes =
            SecretBuffer::new(serde_json::to_vec(&replacement).map_err(|_| unreadable_auth())?);
        self.store.compare_and_swap(
            &self.store_key(),
            Some(current.stored_bytes.as_slice()),
            Some(replacement_bytes.as_slice()),
        )?;
        self.session = Some(Session {
            stored: replacement,
            stored_bytes: replacement_bytes,
            id_token: selected_token.into_inner(),
            expires_at: claim_expiry.min(now_epoch_seconds.saturating_add(expires_in)),
        });
        Ok(())
    }

    pub fn list_projects(
        &mut self,
        now_epoch_seconds: u64,
    ) -> Result<ProjectDirectory, ClientError> {
        self.with_authority(now_epoch_seconds, |profile, transport, authority| {
            crate::authorized::list_projects(profile, transport, authority)
        })
    }

    /// Decide one headless device authorization under the restored USER.
    ///
    /// `POST /auth/device/approve` sits behind ds-brain's
    /// `FirebaseAuthentication` boundary: it takes a Firebase user id token and
    /// refuses a device credential with `DESKTOP_PRINCIPAL_REQUIRED`. The
    /// restored session IS that principal — the same one a Desktop window
    /// holds — so nothing about approving needs a window. There is deliberately
    /// no twin on [`crate::DeviceApiAuthorization`]: a device may not admit a
    /// sibling. The receipt is verified against this request and this
    /// principal before it is returned.
    pub fn device_approve(
        &mut self,
        request: &crate::DeviceApprovalRequest,
        now_epoch_seconds: u64,
    ) -> Result<crate::DeviceApprovalReceipt, ClientError> {
        self.with_authority(now_epoch_seconds, |profile, transport, authority| {
            crate::device_approval::run(profile, transport, authority, request)
        })
    }

    /// Execute one closed Sync Center control-plane call with a freshly
    /// restored Firebase session. The bearer remains inside the transport.
    pub fn sync_gateway(
        &mut self,
        request: &crate::NativeSyncRequest,
        now_epoch_seconds: u64,
    ) -> Result<Value, ClientError> {
        self.with_authority(now_epoch_seconds, |profile, transport, authority| {
            crate::authorized::sync_gateway(profile, transport, authority, request)
        })
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Read one transformer through the fixed context projection. Project
        /// membership remains a server-side decision on this authenticated call;
        /// hosts should pass their selected project from the fresh directory.
        feeder_configuration
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Read a persisted analysis under the exact saved-head fence. No local
        /// calculation, reserialization, or fallback is performed.
        transformer_analysis
    }

    pub fn transformer_context(
        &mut self,
        ds_project: &str,
        transformer: &str,
        now_epoch_seconds: u64,
    ) -> Result<TransformerContext, ClientError> {
        crate::authorized::validate_transformer_context(ds_project, transformer)?;
        self.with_authority(now_epoch_seconds, |profile, transport, authority| {
            crate::authorized::transformer_context(
                profile,
                transport,
                authority,
                ds_project,
                transformer,
                crate::TransformerContextProjection::Full,
            )
        })
    }

    /// Read persisted transformer layers for report integrity verification.
    /// Ordinary reads continue to use the governed full projection.
    pub fn transformer_context_saved(
        &mut self,
        ds_project: &str,
        transformer: &str,
        now_epoch_seconds: u64,
    ) -> Result<TransformerContext, ClientError> {
        crate::authorized::validate_transformer_context(ds_project, transformer)?;
        self.with_authority(now_epoch_seconds, |profile, transport, authority| {
            crate::authorized::transformer_context(
                profile,
                transport,
                authority,
                ds_project,
                transformer,
                crate::TransformerContextProjection::Saved,
            )
        })
    }

    /// Exact saved layers and governed transformer input in one read.
    pub fn transformer_context_for_processing(
        &mut self,
        ds_project: &str,
        transformer: &str,
        now_epoch_seconds: u64,
    ) -> Result<TransformerContext, ClientError> {
        crate::authorized::validate_transformer_context(ds_project, transformer)?;
        self.with_authority(now_epoch_seconds, |profile, transport, authority| {
            crate::authorized::transformer_context(
                profile,
                transport,
                authority,
                ds_project,
                transformer,
                crate::TransformerContextProjection::Process,
            )
        })
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Read the complete bounded project-form catalogue for one selected
        /// project. Membership remains a server-side decision on this call.
        project_forms
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Read one backend-owned settings editor for a form in the selected
        /// project. Membership and project-form admin authority are rechecked by
        /// ds-brain; no project or action override exists at this boundary.
        project_form_editor
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Capture one bounded governed Solar input snapshot for a city in the
        /// audience-fenced selected project. The project root is derived inside
        /// the fixed call and membership remains a server-side decision.
        solar_snapshot
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Run one bounded aggregate over one governed form in the selected
        /// project. The backend rechecks project/form/view authority and refreshes
        /// the survey mirror before executing the typed grammar.
        survey_query
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Select one bounded, live spatial receipt from a governed Survey form.
        /// The returned digest identifies these mutable mirror rows; it is not a
        /// datastore snapshot and a truncated selection must be narrowed.
        survey_entries_select
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Read one immutable-BigQuery-fence page of coalesced Survey mirror
        /// changes, including soft-delete tombstones. This is not Firestore
        /// snapshot/history and hard-deleted documents cannot appear.
        survey_entries_changes
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Create one Survey entry through the governed, atomically idempotent
        /// backend authority. The selected project is injected separately and is
        /// verified again against the exact 201 receipt.
        /// Replay a fully validated local plan under one captured native principal.
        survey_delete
    }

    crate::gateway_compat::gateway_domain_command!(session; survey_update);

    crate::gateway_compat::gateway_domain_command!(session; survey_entry_create);

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Read the current managed state for one selected project's survey or
        /// design tile archive.
        tile_list
    }

    crate::gateway_compat::gateway_domain_command!(session; tile_add);

    crate::gateway_compat::gateway_domain_command!(session; tile_remove);

    crate::gateway_compat::gateway_domain_command!(session; design_selections);

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One data-distribution action against the selected project: the
        /// reference catalogue, or one bounded derived print-context acquisition.
        data_distribution
    }

    crate::gateway_compat::gateway_domain_command!(session; printing);
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Read only the explicit project's governed standard and A4 documents.
        printing_standard
    }
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Plan, census or apply one copy of a project's network documents
        /// (`ds_project` is the destination).
        network_documents
    }
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Plan, census or apply the governed printing-template migration of one
        /// library (`ds_project` empty for the global one).
        print_style_keys
    }
    crate::gateway_compat::gateway_domain_command!(session; layer_config);
    crate::gateway_compat::gateway_domain_command!(session; style_catalog);
    crate::gateway_compat::gateway_domain_command!(session; layer_reorder);
    crate::gateway_compat::gateway_domain_command!(session; layer_default_visibility);
    crate::gateway_compat::gateway_domain_command!(session; project_data);
    crate::gateway_compat::gateway_domain_command!(session; status_processing);
    crate::gateway_compat::gateway_domain_command!(session; survey_photo);
    crate::gateway_compat::gateway_domain_command!(session; repair_transformer);
    crate::gateway_compat::gateway_domain_command!(session; reseal_transformer_digest);
    pub fn save_transformers(
        &mut self,
        project: &str,
        batch: &crate::TransformerSaveBatch,
        now: u64,
    ) -> Result<crate::TransformerSaveReceipt, ClientError> {
        batch.validate(project, self.profile.lane())?;
        self.with_authority(now, |profile, transport, authority| {
            crate::transformer_save::run(profile, transport, authority, project, batch)
        })
    }
    crate::gateway_compat::gateway_domain_command!(session; save_collisions);
    crate::gateway_compat::gateway_domain_command!(session; feedback);
    crate::gateway_compat::gateway_domain_command!(session; shared_assets);
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One governed catalogue action on the project's assets. `reader`
        /// carries the bytes of an ingest and nothing else. An index form
        /// answers `index_status: "served"` with the server index response.
        project_assets
    }
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One governed design annotation action — a tag, a group, a grouping,
        /// an enrichment or a comment thread — for one project.
        design_annotations
    }
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// The project's known-column visibility: read it, or set one field.
        known_columns
    }
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One preview-pinned material propagation through the report gate; the
        /// receipt comes back whole for the kernel to judge.
        material_propagation
    }
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Capture one exact source-authorized PDF grant without reading its bytes.
        authorize_asset_preview
    }
    /// One asset's row and its verified bytes, through the signed read.
    pub fn read_asset_bytes(
        &mut self,
        project: &str,
        asset_id: &str,
        now: u64,
    ) -> Result<(serde_json::Value, Vec<u8>), ClientError> {
        self.with_authority(now, |profile, transport, authority| {
            crate::project_assets::read_bytes(profile, transport, authority, project, asset_id)
        })
    }
    crate::gateway_compat::gateway_domain_command!(session; transformer_history_import);
    crate::gateway_compat::gateway_domain_command!(session; design_version_catalog);
    crate::gateway_compat::gateway_domain_command!(session; design_versions);
    crate::gateway_compat::gateway_domain_command!(session; design_attachments);
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One attachment revision's receipt and its verified bytes, through the
        /// signed read the `download` action authorizes.
        design_attachment_bytes
    }
    crate::gateway_compat::gateway_domain_command!(session; design_tags);
    crate::gateway_compat::gateway_domain_command!(session; design_migration);
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Survey data INTO `project` from the command's source, both explicit;
        /// the saved selection is neither read nor changed.
        survey_migration
    }
    crate::gateway_compat::gateway_domain_command!(session; report_artifact);
    crate::gateway_compat::gateway_domain_command!(session; remove_report_artifact);
    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One governed action on the GLOBAL reference publications.
        ///
        /// No project: these tiles belong to the product. Publishing needs the
        /// gateway's `global_tiles.manage`; reading needs a restored user.
        global_tiles
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One exact read of a national administrative hierarchy.
        ///
        /// No project: a country's boundaries belong to the country. The restored
        /// user is the identity the gateway sees, and the answer is the same on
        /// every host that asks.
        admin_bounds
    }

    /// One bounded read of a survey form's entries: every field value and
    /// the survey media each entry references, as the map reads them.
    pub fn survey_entries_read(
        &mut self,
        project: &str,
        request: &crate::SurveyEntriesReadRequest,
        now: u64,
    ) -> Result<crate::SurveyEntriesRead, ClientError> {
        self.with_authority(now, |profile, transport, authority| {
            crate::survey_entries_read::run(profile, transport, authority, project, request)
        })
    }

    /// One media grant per project whose survey photos this user fetches:
    /// the report export grant, checked once per project.
    pub fn survey_media_grant(
        &mut self,
        project: &str,
        others: &[&str],
        now: u64,
    ) -> Result<crate::MediaGrants, ClientError> {
        self.with_authority(now, |profile, transport, authority| {
            crate::survey_media_fetch::grant(profile, transport, authority, project, others)
        })
    }

    /// The grant a delivered report's photo links are written under: the
    /// named project, ds-brain's report default life (the browser's request).
    pub fn report_media_grant(
        &mut self,
        project: &str,
        now: u64,
    ) -> Result<crate::MediaGrants, ClientError> {
        self.with_authority(now, |profile, transport, authority| {
            crate::survey_media_fetch::grant_for(
                profile,
                transport,
                authority,
                project,
                &[],
                crate::survey_media_fetch::GrantUse::ReportLinks,
            )
        })
    }

    /// One `query_entries` body the core's survey hold built, sent under
    /// the restored user; the route's NDJSON comes back whole.
    pub fn survey_entries_query(
        &mut self,
        project: &str,
        body: &serde_json::Value,
        now: u64,
    ) -> Result<Vec<u8>, ClientError> {
        self.with_authority(now, |profile, transport, authority| {
            crate::survey_entries_read::query(profile, transport, authority, project, body)
        })
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One bounded read of platform reliability.
        ///
        /// No project: a platform is not a project, and nothing about fleet health
        /// is selected by one. The restored user is the identity the gateway sees,
        /// and `platform.admin` is what decides whether there is an answer.
        sre
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One governed action on the GLOBAL installation inventory.
        ///
        /// No project: an installation belongs to the product, so nothing here
        /// selects, fences or sends one. The restored user is the identity the
        /// gateway sees, and `platform.admin`/`app.admin` decides the answer.
        installs
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Create one project, or edit one project's properties.
        ///
        /// Global on create (the project does not exist yet) and named on update:
        /// the caller passes the project id, and ds-brain rechecks
        /// `project.properties.edit` on it. No context lease is taken: this
        /// command edits the project it names, never the selected one.
        project_properties
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One governed whole-project delete action. The project id is an address;
        /// ds-brain decides by the principal's `platform.admin`. No context lease
        /// is taken: the command deletes the project it names, never the selected one.
        project_deletion
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One selected account-private note action; no project or note catalogue read.
        personal_notes
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One account-scoped messaging or notification action. Brain authorizes
        /// every explicitly addressed conversation or project.
        messaging
    }

    /// One governed correspondence action, for one project — the same door
    /// as [`Self::project_management`], the actions the correspondence
    /// contract added.
    pub fn project_correspondence(
        &mut self,
        project: &str,
        action: &crate::project_correspondence::Action,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        crate::project_correspondence::validate_request(project, action)?;
        self.with_authority(now, |profile, transport, authority| {
            crate::project_correspondence::run(profile, transport, authority, project, action)
        })
    }

    /// One governed project-management read, for one project.
    ///
    /// Project-scoped, unlike [`Self::installs`]: a plan belongs to a project,
    /// so the caller names it and ds-brain rechecks membership. No window and
    /// no pairing — the bearer is the authority, which is what makes `ds pm`
    /// answer on a Server at all.
    pub fn project_management(
        &mut self,
        project: &str,
        command: &crate::project_management::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        crate::project_management::validate_request(project, command)?;
        self.with_authority(now, |profile, transport, authority| {
            crate::project_management::run(profile, transport, authority, project, command)
        })
    }

    pub fn member_form_grant(
        &mut self,
        project: &str,
        command: &crate::member_form_grants::Request,
        now: u64,
    ) -> Result<crate::member_form_grants::GrantResult, ClientError> {
        command.validate(project)?;
        self.with_authority(now, |profile, transport, authority| {
            crate::member_form_grants::run(profile, transport, authority, project, command)
        })
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// One governed action on the GLOBAL library and example catalog.
        ///
        /// No project id, because this catalog has none: a library release belongs
        /// to the product. The signed-in user is the authority and the gateway
        /// refuses a write without its publish capability.
        grid_catalog
    }
    crate::gateway_compat::gateway_domain_command!(session; grid_models);
    crate::gateway_compat::gateway_domain_command!(session; solar_project);
    crate::gateway_compat::gateway_domain_command!(session; survey_control);
    crate::gateway_compat::gateway_domain_command!(session; style_edit);
    crate::gateway_compat::gateway_domain_command!(session; style_history);
    crate::gateway_compat::gateway_domain_command!(session; style_governance);
    crate::gateway_compat::gateway_domain_command!(session; tile_status);

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Inspect the bounded source shape that the managed tile job would use.
        tile_preflight
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Request one managed tile run. The server retains admission, source,
        /// destination, lifecycle, and dispatch authority.
        tile_generate
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// List the selected project's published compounded archives.
        compounded_report_list
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Read the lifecycle inventory of the selected project's transformers,
        /// or of the exact requested names.
        transformer_inventory
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Read the status rows of the selected project's transformers, or of the
        /// exact requested names. The rows are returned as ds-brain sent them;
        /// this crate bounds them and reshapes nothing.
        transformer_status
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Retire or restore exact transformers reversibly. ds-brain owns the
        /// governance, ownership, lock, and lifecycle decisions per name.
        transformer_retirement
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Read one deleted transformer's verified delete backup, fetching its
        /// exact bytes when `fetch` is set.
        deleted_transformer_backup
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Sign the listed artifacts through the authorized signer and hand each
        /// fetched object's exact bytes and SHA-256 to `sink`, one at a time.
        download_artifacts
    }

    crate::gateway_compat::gateway_domain_command! {
        session;
        /// Create a deleted transformer again from one pinned, verified backup.
        restore_deleted_transformer
    }

    // Keep freshness and capture in one place. Every public method validates its
    // domain input before entering here; the existing domain owner runs once.
    fn with_authority<R>(
        &mut self,
        now_epoch_seconds: u64,
        operation: impl FnOnce(
            &ClientProfile,
            &mut T,
            crate::authorized::Authority<'_>,
        ) -> Result<R, ClientError>,
    ) -> Result<R, ClientError> {
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        operation(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
        )
    }

    fn ensure_fresh_session(&mut self, now_epoch_seconds: u64) -> Result<(), ClientError> {
        let needs_refresh = self.session.as_ref().is_none_or(|session| {
            session.id_token.is_empty()
                || now_epoch_seconds.saturating_add(REFRESH_SKEW_SECONDS) >= session.expires_at
        });
        if needs_refresh {
            self.refresh(now_epoch_seconds)?;
        }
        Ok(())
    }

    fn store_key(&self) -> StoreKey {
        StoreKey(format!(
            "ds-client/{}/{}",
            self.profile.lane().token(),
            self.profile.binding()
        ))
    }

    fn clear_permanent_disposition(&mut self, original: ClientError) -> ClientError {
        let key = self.store_key();
        let deletion = match self.session.as_ref() {
            Some(session) => Some(self.store.compare_and_swap(
                &key,
                Some(session.stored_bytes.as_slice()),
                None,
            )),
            None => None,
        };
        self.session = None;
        match deletion {
            None | Some(Ok(())) => original,
            Some(Err(StoreError::Conflict)) => ClientError::new(
                ErrorKind::Transient,
                "another client replaced the rejected credential; reload the durable session",
            ),
            Some(Err(StoreError::Unavailable | StoreError::UnsafeOrUnreadable)) => {
                ClientError::new(
                    ErrorKind::DurableState,
                    "Firebase invalidated the session but durable cleanup is still required",
                )
            }
        }
    }
}

/// Preserve the bounded, server-authored refusal envelope for a closed Sync
/// Center operation.  In particular, a compute-artifact conflict is a
/// durable state result for an adapter, not an undifferentiated transient.
pub(crate) fn sync_gateway_refusal(status: u16, body: &[u8]) -> ClientError {
    let envelope: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let refusal = crate::ServiceRefusal::new(
        status,
        envelope["error"]["code"].as_str(),
        envelope["error"]["message"]
            .as_str()
            .or_else(|| envelope["detail"].as_str()),
    );
    let kind = match status {
        400 | 409 | 422 => ErrorKind::InvalidInput,
        401 | 403 => ErrorKind::AuthenticationRejected,
        404 => ErrorKind::ResourceNotFound,
        408 | 425 | 429 | 500..=599 => ErrorKind::Transient,
        _ => ErrorKind::UnreadableResponse,
    };
    // Status is itself a closed fact even when the server's optional code or
    // sentence is absent/unacceptable. Solar's durable adapter uses 409 as a
    // terminal stale result and must not lose that fact.
    ClientError::with_service_refusal(kind, "native Sync Center operation was refused", refusal)
}

#[cfg(test)]
#[path = "session_sync_gateway_tests.rs"]
mod sync_gateway_tests;

#[derive(Deserialize)]
struct RefreshResponse {
    user_id: String,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    refresh_token: String,
    expires_in: String,
    token_type: String,
    #[serde(default)]
    project_id: Option<Value>,
}

impl Drop for RefreshResponse {
    fn drop(&mut self) {
        if let Some(token) = self.access_token.as_mut() {
            token.zeroize();
        }
        if let Some(token) = self.id_token.as_mut() {
            token.zeroize();
        }
        self.refresh_token.zeroize();
    }
}

struct SecretString(String);

impl SecretString {
    fn as_str(&self) -> &str {
        &self.0
    }

    fn into_inner(mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

fn take_refresh_jwt(response: &mut RefreshResponse) -> Result<SecretString, ClientError> {
    match (
        response.access_token.as_deref(),
        response.id_token.as_deref(),
    ) {
        (Some(access), Some(id)) if !access.is_empty() && access == id => response
            .access_token
            .take()
            .map(SecretString)
            .ok_or_else(identity_mismatch),
        (Some(access), None) if !access.is_empty() => response
            .access_token
            .take()
            .map(SecretString)
            .ok_or_else(identity_mismatch),
        (None, Some(id)) if !id.is_empty() => response
            .id_token
            .take()
            .map(SecretString)
            .ok_or_else(identity_mismatch),
        _ => Err(identity_mismatch()),
    }
}

fn valid_project_metadata(project_id: Option<&Value>) -> bool {
    match project_id {
        None => true,
        Some(Value::String(value)) => {
            !value.is_empty()
                && value.trim() == value
                && value.len() <= 128
                && !value.chars().any(char::is_control)
        }
        Some(Value::Number(value)) => value.to_string().len() <= 64,
        _ => false,
    }
}

fn validate_identity(
    uid: &str,
    response_email: &str,
    expected_email: &str,
) -> Result<AuthenticatedUser, ClientError> {
    if uid.is_empty()
        || uid.trim() != uid
        || uid.chars().count() > 128
        || uid.chars().any(char::is_control)
        || response_email.trim() != response_email
        || !response_email.eq_ignore_ascii_case(expected_email)
    {
        return Err(identity_mismatch());
    }
    if !valid_email(response_email) {
        return Err(identity_mismatch());
    }
    Ok(AuthenticatedUser {
        uid: uid.to_owned(),
        email: response_email.to_owned(),
    })
}

fn validate_email(email: &str) -> Result<(), ClientError> {
    if !valid_email(email) {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "email is empty, untrimmed, or outside the closed account-email contract",
        ));
    }
    Ok(())
}

fn valid_email(email: &str) -> bool {
    !email.is_empty()
        && email.trim() == email
        && email.chars().count() <= 320
        && !email.chars().any(char::is_control)
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.is_empty()
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        })
}

fn validate_secret(secret: &str) -> Result<(), ClientError> {
    if secret.is_empty() || secret.len() > 16 * 1024 || secret.contains(['\r', '\n']) {
        return Err(unreadable_auth());
    }
    Ok(())
}

fn validate_stored(stored: &StoredRefresh, profile: &ClientProfile) -> Result<(), ClientError> {
    if stored.schema != STORED_SCHEMA || stored.credential_audience != profile.binding() {
        return Err(ClientError::new(
            ErrorKind::DurableState,
            "the stored refresh credential belongs to another profile or schema",
        ));
    }
    if validate_identity(&stored.uid, &stored.email, &stored.email).is_err()
        || validate_secret(&stored.refresh_token).is_err()
        || (!stored.credential_instance_sha256.is_empty()
            && (stored.credential_instance_sha256.len() != 64
                || !stored
                    .credential_instance_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))))
    {
        return Err(ClientError::new(
            ErrorKind::DurableState,
            "the stored refresh credential has an invalid identity or secret",
        ));
    }
    Ok(())
}

fn reject_oversized_stored(bytes: Option<&[u8]>) -> Result<(), ClientError> {
    if bytes.is_some_and(|bytes| bytes.len() > MAX_STORED_REFRESH) {
        return Err(ClientError::new(
            ErrorKind::DurableState,
            "the stored refresh credential exceeded its 64 KiB bound",
        ));
    }
    Ok(())
}

fn finish_lease<T>(
    operation: Result<T, ClientError>,
    release: Result<(), StoreError>,
) -> Result<T, ClientError> {
    if release.is_err() {
        return Err(ClientError::new(
            ErrorKind::DurableState,
            "the exclusive refresh-credential lease could not be released",
        ));
    }
    operation
}

fn parse_expiry(value: &str) -> Result<u64, ClientError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|seconds| (60..=86_400).contains(seconds))
        .ok_or_else(unreadable_auth)
}

fn map_login_error(error: LoginError) -> ClientError {
    match error.kind() {
        LoginErrorKind::IdentityMismatch => identity_mismatch(),
        LoginErrorKind::InvalidInput | LoginErrorKind::UnreadableResponse => unreadable_auth(),
    }
}

fn decode<T: for<'de> Deserialize<'de>>(
    response: TransportResponse,
    limit: usize,
) -> Result<T, ClientError> {
    if response.body.len() > limit {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "the service response exceeded its operation bound",
        ));
    }
    serde_json::from_slice(&response.body).map_err(|_| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "the service response is not its closed JSON contract",
        )
    })
}

fn classify_auth_error(response: TransportResponse, refresh: bool) -> ClientError {
    if response.body.len() > MAX_AUTH_RESPONSE {
        return unreadable_auth();
    }
    if matches!(response.status, 408 | 425 | 429 | 500..=599) {
        return ClientError::new(
            ErrorKind::Transient,
            "the authentication service temporarily refused the request",
        );
    }
    if !refresh {
        let (kind, message) = match classify_firebase_auth_failure(response.status, &response.body)
        {
            FirebaseAuthFailureKind::InvalidCredentials => (
                ErrorKind::InvalidCredentials,
                "Firebase did not accept the email and password",
            ),
            FirebaseAuthFailureKind::PasswordSignInUnavailable => (
                ErrorKind::PasswordSignInUnavailable,
                "Firebase password sign-in is unavailable for this account or project",
            ),
            FirebaseAuthFailureKind::AccountDisabled => {
                (ErrorKind::AccountDisabled, "Firebase disabled this account")
            }
            FirebaseAuthFailureKind::Transient => (
                ErrorKind::Transient,
                "the authentication service temporarily refused the request",
            ),
            FirebaseAuthFailureKind::Rejected => (
                ErrorKind::AuthenticationRejected,
                "Firebase rejected the authentication request",
            ),
            FirebaseAuthFailureKind::UnreadableResponse => (
                ErrorKind::UnreadableResponse,
                "the Firebase failure response is outside the closed authentication contract",
            ),
        };
        return ClientError::new(kind, message);
    }
    let code = serde_json::from_slice::<Value>(&response.body)
        .ok()
        .and_then(|body| body["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let code = code.split([':', ' ']).next().unwrap_or_default();
    if refresh
        && matches!(
            code,
            "INVALID_REFRESH_TOKEN" | "TOKEN_EXPIRED" | "USER_DISABLED" | "USER_NOT_FOUND"
        )
    {
        return ClientError::new(
            ErrorKind::PermanentlyRevoked,
            "Firebase permanently rejected the stored refresh credential",
        );
    }
    ClientError::new(
        ErrorKind::AuthenticationRejected,
        "Firebase rejected the authentication request",
    )
}

fn unreadable_auth() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the Firebase response is outside the closed authentication contract",
    )
}

fn identity_mismatch() -> ClientError {
    ClientError::new(
        ErrorKind::IdentityMismatch,
        "Firebase UID, email, JWT subject, or audience did not remain bound",
    )
}

fn signed_out() -> ClientError {
    ClientError::new(
        ErrorKind::SignedOut,
        "no refresh credential is loaded for this client profile",
    )
}

#[cfg(test)]
#[path = "session_authority_tests.rs"]
mod authority_tests;
