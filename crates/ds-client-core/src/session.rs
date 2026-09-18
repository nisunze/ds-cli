use ds_command_kernel::{
    classify_firebase_auth_failure, validate_firebase_id_token, validate_firebase_login_response,
    FirebaseAuthFailureKind, LoginError, LoginErrorKind,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::project_report::{
    CompoundedArchive, CompoundedReportReceipt, CompoundedReportRequest, RetirementAction,
    RetirementReceipt, RetirementRequest, TransformerInventory, TransformerSet,
    TransformerStatusList,
};
use crate::{
    ClientError, ClientProfile, ErrorKind, ProjectDirectory, ProjectFormSettingsEditor,
    ProjectFormsSnapshot, RefreshCall, SignInCall, SolarSnapshot, StoreError, SurveyEntriesChanges,
    SurveyEntriesChangesRequest, SurveyEntriesSelectRequest, SurveyEntriesSelection,
    SurveyEntryCreateReceipt, SurveyEntryCreateRequest, SurveyQueryRequest, SurveyQueryResult,
    TileOperationResult, TilePreflight, TileType, TransformerContext, Transport, TransportResponse,
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
    lane: &'static str,
    credential_audience_sha256: String,
    credential_instance_sha256: String,
}

impl StoredAuthContext {
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
        }
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
        let response = self.transport.sign_in(SignInCall {
            profile: &self.profile,
            email,
            password,
        })?;
        if response.status != 200 {
            return Err(classify_auth_error(response, false));
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
        let response = self.transport.refresh(RefreshCall {
            profile: &self.profile,
            refresh_token: &session.stored.refresh_token,
        })?;
        if response.status != 200 {
            let error = classify_auth_error(response, true);
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
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::list_projects(
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

    /// Execute one closed Sync Center control-plane call with a freshly
    /// restored Firebase session. The bearer remains inside the transport.
    pub fn sync_gateway(
        &mut self,
        request: &crate::NativeSyncRequest,
        now_epoch_seconds: u64,
    ) -> Result<Value, ClientError> {
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::sync_gateway(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            request,
        )
    }

    /// Read one transformer through the fixed context projection. Project
    /// membership remains a server-side decision on this authenticated call;
    /// hosts should pass their selected project from the fresh directory.
    pub fn feeder_configuration(
        &mut self,
        project: &str,
        bounds: Option<&crate::ProjectConfigurationChange>,
        now: u64,
    ) -> Result<crate::FeederConfiguration, ClientError> {
        crate::authorized::validate_project_report(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::feeder_configuration(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            bounds,
        )
    }

    pub fn transformer_context(
        &mut self,
        ds_project: &str,
        transformer: &str,
        now_epoch_seconds: u64,
    ) -> Result<TransformerContext, ClientError> {
        crate::authorized::validate_transformer_context(ds_project, transformer)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::transformer_context(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            transformer,
        )
    }

    /// Read the complete bounded project-form catalogue for one selected
    /// project. Membership remains a server-side decision on this call.
    pub fn project_forms(
        &mut self,
        ds_project: &str,
        now_epoch_seconds: u64,
    ) -> Result<ProjectFormsSnapshot, ClientError> {
        crate::authorized::validate_project_forms(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::project_forms(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
        )
    }

    /// Read one backend-owned settings editor for a form in the selected
    /// project. Membership and project-form admin authority are rechecked by
    /// ds-brain; no project or action override exists at this boundary.
    pub fn project_form_editor(
        &mut self,
        ds_project: &str,
        form_slug: &str,
        now_epoch_seconds: u64,
    ) -> Result<ProjectFormSettingsEditor, ClientError> {
        crate::authorized::validate_project_form_editor(ds_project, form_slug)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::project_form_editor(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            form_slug,
        )
    }

    /// Capture one bounded governed Solar input snapshot for a city in the
    /// audience-fenced selected project. The project root is derived inside
    /// the fixed call and membership remains a server-side decision.
    pub fn solar_snapshot(
        &mut self,
        ds_project: &str,
        template_id: &str,
        now_epoch_seconds: u64,
    ) -> Result<SolarSnapshot, ClientError> {
        crate::authorized::validate_solar_snapshot(ds_project, template_id)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::solar_snapshot(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            template_id,
        )
    }

    /// Run one bounded aggregate over one governed form in the selected
    /// project. The backend rechecks project/form/view authority and refreshes
    /// the survey mirror before executing the typed grammar.
    pub fn survey_query(
        &mut self,
        ds_project: &str,
        query: &SurveyQueryRequest,
        now_epoch_seconds: u64,
    ) -> Result<SurveyQueryResult, ClientError> {
        crate::authorized::validate_survey_query(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::survey_query(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            query,
        )
    }

    /// Select one bounded, live spatial receipt from a governed Survey form.
    /// The returned digest identifies these mutable mirror rows; it is not a
    /// datastore snapshot and a truncated selection must be narrowed.
    pub fn survey_entries_select(
        &mut self,
        ds_project: &str,
        request: &SurveyEntriesSelectRequest,
        now_epoch_seconds: u64,
    ) -> Result<SurveyEntriesSelection, ClientError> {
        crate::authorized::validate_survey_entries_select(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::survey_entries_select(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            request,
        )
    }

    /// Read one immutable-BigQuery-fence page of coalesced Survey mirror
    /// changes, including soft-delete tombstones. This is not Firestore
    /// snapshot/history and hard-deleted documents cannot appear.
    pub fn survey_entries_changes(
        &mut self,
        ds_project: &str,
        request: &SurveyEntriesChangesRequest,
        now_epoch_seconds: u64,
    ) -> Result<SurveyEntriesChanges, ClientError> {
        crate::authorized::validate_survey_entries_changes(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::survey_entries_changes(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            request,
        )
    }

    /// Create one Survey entry through the governed, atomically idempotent
    /// backend authority. The selected project is injected separately and is
    /// verified again against the exact 201 receipt.
    pub fn survey_entry_create(
        &mut self,
        project_id: &str,
        request: &SurveyEntryCreateRequest,
        now_epoch_seconds: u64,
    ) -> Result<SurveyEntryCreateReceipt, ClientError> {
        crate::authorized::validate_survey_entry_create(project_id)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::survey_entry_create(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project_id,
            request,
        )
    }

    /// Read the current managed state for one selected project's survey or
    /// design tile archive.
    pub fn tile_list(
        &mut self,
        ds_project: &str,
        include_global: bool,
        now_epoch_seconds: u64,
    ) -> Result<crate::TileCatalog, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;

        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::tile_list(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            include_global,
        )
    }

    pub fn tile_add(
        &mut self,
        ds_project: &str,
        tile_type: TileType,
        source_project: &str,
        now_epoch_seconds: u64,
    ) -> Result<crate::TileMutation, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        crate::tiles::validate_catalog_id(source_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::tile_add(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            tile_type,
            source_project,
        )
    }

    pub fn tile_remove(
        &mut self,
        ds_project: &str,
        tile_id: &str,
        scope: crate::TileScope,
        now_epoch_seconds: u64,
    ) -> Result<crate::TileMutation, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        crate::tiles::validate_catalog_id(tile_id)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::tile_remove(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            tile_id,
            scope,
        )
    }

    pub fn design_selections(
        &mut self,
        ds_project: &str,
        request: &crate::DesignSelectionRequest,
        now_epoch_seconds: u64,
    ) -> Result<crate::DesignSelectionAnswer, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        request.validate()?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::design_selections(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            request,
        )
    }

    /// One data-distribution action against the selected project: the
    /// reference catalogue, or one bounded derived print-context acquisition.
    pub fn data_distribution(
        &mut self,
        ds_project: &str,
        request: &crate::DataDistributionRequest,
        now_epoch_seconds: u64,
    ) -> Result<serde_json::Value, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        request.validate()?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::data_distribution(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            request,
        )
    }

    pub fn printing(
        &mut self,
        ds_project: &str,
        request: &crate::PrintingRequest,
        now_epoch_seconds: u64,
    ) -> Result<serde_json::Value, ClientError> {
        if !ds_project.is_empty() {
            crate::authorized::validate_tiles(ds_project)?;
        }
        request.validate()?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::printing(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            request,
        )
    }
    pub fn layer_config(
        &mut self,
        ds_project: &str,
        refresh: bool,
        now_epoch_seconds: u64,
    ) -> Result<crate::LayerSnapshot, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::layer_config(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            refresh,
        )
    }
    pub fn style_catalog(
        &mut self,
        ds_project: &str,
        now_epoch_seconds: u64,
    ) -> Result<crate::StyleSnapshot, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::style_catalog(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
        )
    }
    pub fn layer_reorder(
        &mut self,
        ds_project: &str,
        orders: &[crate::LayerOrder],
        now_epoch_seconds: u64,
    ) -> Result<crate::LayerOrderReceipt, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        crate::layers::validate_orders(orders)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::layer_reorder(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            orders,
        )
    }
    pub fn layer_default_visibility(
        &mut self,
        ds_project: &str,
        defaults: &[crate::LayerVisibilityDefault],
        now_epoch_seconds: u64,
    ) -> Result<crate::LayerVisibilityDefaultReceipt, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        crate::layers::validate_visibility_defaults(defaults)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::layer_default_visibility(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            defaults,
        )
    }
    pub fn project_data(
        &mut self,
        project: &str,
        command: crate::ProjectDataCommand<'_>,
        now: u64,
    ) -> Result<crate::ProjectDataReceipt, ClientError> {
        command.validate()?;
        crate::authorized::validate_tiles(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::project_data(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn status_processing(
        &mut self,
        project: &str,
        command: crate::StatusProcessingCommand<'_>,
        now: u64,
    ) -> Result<crate::StatusProcessingReceipt, ClientError> {
        crate::authorized::validate_tiles(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::status_processing(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn survey_photo(
        &mut self,
        project: &str,
        command: crate::survey_photo::Command<'_>,
        now: u64,
    ) -> Result<crate::survey_photo::Receipt, ClientError> {
        crate::authorized::validate_tiles(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::survey_photo::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn save_transformers(
        &mut self,
        project: &str,
        batch: &crate::TransformerSaveBatch,
        now: u64,
    ) -> Result<crate::TransformerSaveReceipt, ClientError> {
        batch.validate(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::transformer_save::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            batch,
        )
    }
    pub fn feedback(
        &mut self,
        command: &crate::feedback::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate()?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::feedback::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            command,
        )
    }
    pub fn shared_assets(
        &mut self,
        project: &str,
        command: &crate::shared_assets::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::shared_assets::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn design_versions(
        &mut self,
        project: &str,
        command: &crate::design_versions::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project).map_err(|_| {
            ClientError::new(
                crate::ErrorKind::InvalidInput,
                "invalid published version request; use distinct vN IDs or --to head",
            )
        })?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::design_versions::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn design_attachments(
        &mut self,
        project: &str,
        command: &crate::design_attachments::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::design_attachments::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn design_tags(
        &mut self,
        project: &str,
        command: &crate::design_tags::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::design_tags::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn report_artifact(
        &mut self,
        project: &str,
        command: &crate::report_artifact::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::report_artifact::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn grid_models(
        &mut self,
        project: &str,
        command: &crate::grid_models::Command,
        now: u64,
    ) -> Result<crate::grid_models::Receipt, ClientError> {
        command.validate(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::grid_models::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn solar_project(
        &mut self,
        project: &str,
        command: &crate::solar_project::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::solar_project::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn survey_control(
        &mut self,
        project: Option<&str>,
        command: &crate::SurveyControlCommand,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate()?;
        self.ensure_fresh_session(now)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::survey_control_run::run(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            project,
            command,
        )
    }
    pub fn style_edit(
        &mut self,
        ds_project: &str,
        reference: &str,
        instruction: &crate::StyleInstruction,
        apply: bool,
        now_epoch_seconds: u64,
    ) -> Result<crate::StyleEditReceipt, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::style_edit(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            reference,
            instruction,
            apply,
        )
    }
    pub fn tile_status(
        &mut self,
        ds_project: &str,
        tile_type: TileType,
        now_epoch_seconds: u64,
    ) -> Result<TileOperationResult, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::tile_status(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            tile_type,
        )
    }

    /// Inspect the bounded source shape that the managed tile job would use.
    pub fn tile_preflight(
        &mut self,
        ds_project: &str,
        tile_type: TileType,
        now_epoch_seconds: u64,
    ) -> Result<TilePreflight, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::tile_preflight(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            tile_type,
        )
    }

    /// Request one managed tile run. The server retains admission, source,
    /// destination, lifecycle, and dispatch authority.
    pub fn tile_generate(
        &mut self,
        ds_project: &str,
        tile_type: TileType,
        force: bool,
        now_epoch_seconds: u64,
    ) -> Result<TileOperationResult, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::tile_generate(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            tile_type,
            force,
        )
    }

    /// Request one compounded report deliverable for the selected project.
    /// ds-brain owns scope resolution, freshness, composition, and
    /// publication; the receipt names the archive and every exclusion.
    pub fn compounded_report(
        &mut self,
        ds_project: &str,
        request: &CompoundedReportRequest,
        now_epoch_seconds: u64,
    ) -> Result<CompoundedReportReceipt, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::compounded_report(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            request,
        )
    }

    /// List the selected project's published compounded archives.
    pub fn compounded_report_list(
        &mut self,
        ds_project: &str,
        now_epoch_seconds: u64,
    ) -> Result<Vec<CompoundedArchive>, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::compounded_report_list(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
        )
    }

    /// Read the lifecycle inventory of the selected project's transformers,
    /// or of the exact requested names.
    pub fn transformer_inventory(
        &mut self,
        ds_project: &str,
        requested: &TransformerSet,
        now_epoch_seconds: u64,
    ) -> Result<TransformerInventory, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::transformer_inventory(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            requested,
        )
    }

    /// Read the status rows of the selected project's transformers, or of the
    /// exact requested names. The rows are returned as ds-brain sent them;
    /// this crate bounds them and reshapes nothing.
    pub fn transformer_status(
        &mut self,
        ds_project: &str,
        requested: &TransformerSet,
        now_epoch_seconds: u64,
    ) -> Result<TransformerStatusList, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::transformer_status(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            requested,
        )
    }

    /// Retire or restore exact transformers reversibly. ds-brain owns the
    /// governance, ownership, lock, and lifecycle decisions per name.
    pub fn transformer_retirement(
        &mut self,
        ds_project: &str,
        action: RetirementAction,
        request: &RetirementRequest,
        now_epoch_seconds: u64,
    ) -> Result<RetirementReceipt, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_fresh_session(now_epoch_seconds)?;
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        crate::authorized::transformer_retirement(
            &self.profile,
            &mut self.transport,
            crate::authorized::Authority {
                bearer_token: &session.id_token,
                uid: &session.stored.uid,
                email: &session.stored.email,
                device_id: None,
            },
            ds_project,
            action,
            request,
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
mod sync_gateway_tests {
    use super::*;

    #[test]
    fn compute_artifact_refusals_keep_status_for_a_durable_adapter() {
        let invalid = sync_gateway_refusal(
            400,
            br#"{"error":{"code":"VALIDATION_FAILED","message":"invalid declaration"}}"#,
        );
        assert_eq!(invalid.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            invalid.service_refusal().map(|refusal| refusal.status()),
            Some(400)
        );

        let conflict = sync_gateway_refusal(
            409,
            br#"{"error":{"code":"CONFLICT","message":"base moved"}}"#,
        );
        assert_eq!(conflict.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            conflict.service_refusal().map(|refusal| refusal.status()),
            Some(409)
        );

        let denied = sync_gateway_refusal(403, br#"{"error":{"code":"DENIED"}}"#);
        assert_eq!(denied.kind(), ErrorKind::AuthenticationRejected);
        assert_eq!(
            denied.service_refusal().map(|refusal| refusal.status()),
            Some(403)
        );
    }
}

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
