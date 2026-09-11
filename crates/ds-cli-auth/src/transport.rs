//! Fixed-origin ureq adapter for ds-client-core's closed calls.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ds_client_core::{
    ProjectFormEditorCall, ProjectFormsCall, ProjectListCall, ProjectReportCall, RefreshCall,
    SignInCall, SolarSnapshotCall, SurveyEntriesChangesCall, SurveyEntriesSelectCall,
    SurveyEntryCreateCall, SurveyQueryCall, SyncGatewayCall, TileCall, TransformerContextCall,
    Transport, TransportError, TransportResponse,
};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
static CORRELATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub struct NativeTransport;

impl Transport for NativeTransport {
    fn sync_gateway(
        &mut self,
        call: SyncGatewayCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = format!("{}{}", call.gateway_origin(), call.path());
        let result = ureq::post(url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        bounded(result.map_err(classify)?, call.response_limit())
    }

    fn project_configuration(
        &mut self,
        call: ds_client_core::ProjectConfigurationCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = format!("{}{}", call.gateway_origin(), call.path());
        let request = if call.method() == "GET" {
            ureq::get(url).force_send_body()
        } else {
            ureq::post(url)
        };
        let result = request
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        bounded(result.map_err(classify)?, call.response_limit())
    }

    fn sign_in(&mut self, call: SignInCall<'_>) -> Result<TransportResponse, TransportError> {
        let body = call.body();
        let response = ureq::post(call.endpoint())
            .query("key", call.firebase_api_key())
            .content_type(call.content_type())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(CALL_TIMEOUT))
            .build()
            .send(body.as_bytes())
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn refresh(&mut self, call: RefreshCall<'_>) -> Result<TransportResponse, TransportError> {
        let body = call.body();
        let response = ureq::post(call.endpoint())
            .query("key", call.firebase_api_key())
            .content_type(call.content_type())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(CALL_TIMEOUT))
            .build()
            .send(body.as_bytes())
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn list_projects(
        &mut self,
        call: ProjectListCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "GET");
        debug_assert_eq!(call.path(), "/api/v1/user/projects");
        // Match ds-web's ordinary request semantics: absent a larger explicit
        // user action, one HTTP request is its own action. A future batched
        // call may supply one shared action id from the core instead.
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let result = ureq::get(call.url())
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(CALL_TIMEOUT))
            .build()
            .call();
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn transformer_context(
        &mut self,
        call: TransformerContextCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/report");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = transformer_context_url(call.gateway_origin());
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn project_forms(
        &mut self,
        call: ProjectFormsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/project-forms");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = project_forms_url(call.gateway_origin());
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn project_form_editor(
        &mut self,
        call: ProjectFormEditorCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/project-forms");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = project_forms_url(call.gateway_origin());
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn solar_snapshot(
        &mut self,
        call: SolarSnapshotCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/solar");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = solar_snapshot_url(call.gateway_origin());
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn survey_query(
        &mut self,
        call: SurveyQueryCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/survey/query");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = survey_query_url(call.gateway_origin());
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn survey_entries_select(
        &mut self,
        call: SurveyEntriesSelectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/survey/entries/select");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = survey_entries_select_url(call.gateway_origin());
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn survey_entries_changes(
        &mut self,
        call: SurveyEntriesChangesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/survey/entries/changes");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = survey_entries_changes_url(call.gateway_origin());
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn survey_entry_create(
        &mut self,
        call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/entries/mutate");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = survey_entry_create_url(call.gateway_origin());
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    /// Stream one artifact to its DS-minted storage session under the shared
    /// resumable protocol.
    ///
    /// The whole body of this call is `crate::upload`, which drives
    /// `ds_command_kernel::transfer` — the same state machine the desktop shell
    /// drives. It probes before writing, resumes from the server's committed
    /// prefix, streams bounded chunks, and never restarts a session because a
    /// probe was inconclusive. No DS credential is attached: under
    /// `authority: storage_session` the session URI is the whole credential.
    fn upload_bytes(
        &mut self,
        mut call: ds_client_core::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        // A credential: copied out before the reader is borrowed, scrubbed on
        // drop, and never logged or formatted into an error.
        let session_uri = Zeroizing::new(call.uri().to_owned());
        let size = call.size();
        crate::upload::transfer(
            &session_uri,
            crate::upload::SessionOrigin::Storage,
            size,
            call.reader(),
            // `ds` runs one synchronous command per process and has no
            // cancellation source of its own; a signal ends the process. The
            // seam is the kernel's, so a caller that gains one wires it here
            // rather than reinterpreting the protocol.
            &|| false,
        )
    }

    fn project_data(
        &mut self,
        call: ds_client_core::ProjectDataCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/project_data");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = format!(
            "{}{}",
            call.gateway_origin(),
            ds_client_core::PROJECT_DATA_PATH
        );
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn solar_project(
        &mut self,
        call: ds_client_core::SolarProjectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = format!("{}{}", call.gateway_origin(), call.path());
        let request = if call.method() == "GET" {
            ureq::get(url).force_send_body()
        } else {
            ureq::post(url)
        };
        let result = request
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(if call.method() == "GET" {
                &[]
            } else {
                body.as_bytes()
            });
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn survey_control(
        &mut self,
        call: ds_client_core::SurveyControlCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = format!("{}{}", call.gateway_origin(), call.path());
        let request = if call.method() == "GET" {
            ureq::get(url).force_send_body()
        } else {
            ureq::post(url)
        };
        let result = request
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(if call.method() == "GET" {
                &[]
            } else {
                body.as_bytes()
            });
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn styles(
        &mut self,
        call: ds_client_core::StylesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/styles");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = format!("{}{}", call.gateway_origin(), ds_client_core::STYLES_PATH);
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn printing(
        &mut self,
        call: ds_client_core::PrintingCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/printing");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = format!("{}{}", call.gateway_origin(), ds_client_core::PRINTING_PATH);
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn data_distribution(
        &mut self,
        call: ds_client_core::DataDistributionCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/data-distribution");
        debug_assert_eq!(call.timeout_seconds(), 180);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = data_distribution_url(call.gateway_origin());
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    /// One public reference-bundle object, streamed into the caller's sink.
    ///
    /// This call carries NO DS credential: no bearer, api key, user email or
    /// app id, and no correlation ids either — the object is public and the
    /// origin is not ours, so nothing of ours travels with the request. The
    /// core pinned the host before this adapter saw the URL, and the bytes are
    /// verified by the core (`ds_client_core::download_bundle`), not here.
    fn download_bundle(
        &mut self,
        call: ds_client_core::BundleDownloadCall<'_>,
        sink: &mut dyn Write,
    ) -> Result<(), TransportError> {
        debug_assert_eq!(call.method(), "GET");
        fetch_bundle(
            call.url(),
            BundleOrigin::Storage,
            call.response_limit(),
            call.timeout_seconds(),
            sink,
        )
    }

    fn design_selections(
        &mut self,
        call: ds_client_core::DesignSelectionsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/design/selections");
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = format!(
            "{}{}",
            call.gateway_origin(),
            ds_client_core::DESIGN_SELECTIONS_PATH
        );
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn layers(
        &mut self,
        call: ds_client_core::LayersCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/layers");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = format!("{}{}", call.gateway_origin(), ds_client_core::LAYERS_PATH);
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn tiles(&mut self, call: TileCall<'_>) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/api/v1/tiles");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = tiles_url(call.gateway_origin());
        let result = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn project_report(
        &mut self,
        call: ProjectReportCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        self.send_project_report(call)
    }
}

impl NativeTransport {
    fn send_project_report(
        &mut self,
        call: ProjectReportCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.path(), "/report");
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = project_report_url(call.gateway_origin());
        let mut request = ureq::post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer);
        // The compounded deliverable is Fast-lane only; ds-brain warns on a
        // lane-aware action without the header and defaults to the retired
        // Standard lane, so the core names the lane and the adapter sends it.
        if let Some(lane) = call.processing_lane() {
            request = request.header(call.processing_lane_header(), lane);
        }
        let result = request
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }
}

fn transformer_context_url(origin: &str) -> String {
    format!("{origin}{}", ds_client_core::TRANSFORMER_CONTEXT_PATH)
}

fn project_forms_url(origin: &str) -> String {
    format!("{origin}{}", ds_client_core::PROJECT_FORMS_PATH)
}

fn solar_snapshot_url(origin: &str) -> String {
    format!("{origin}{}", ds_client_core::SOLAR_SNAPSHOT_PATH)
}

fn survey_query_url(origin: &str) -> String {
    format!("{origin}{}", ds_client_core::SURVEY_QUERY_PATH)
}

fn survey_entries_select_url(origin: &str) -> String {
    format!("{origin}{}", ds_client_core::SURVEY_ENTRIES_SELECT_PATH)
}

fn survey_entries_changes_url(origin: &str) -> String {
    format!("{origin}{}", ds_client_core::SURVEY_ENTRIES_CHANGES_PATH)
}

fn survey_entry_create_url(origin: &str) -> String {
    format!("{origin}{}", ds_client_core::SURVEY_ENTRY_CREATE_PATH)
}

fn tiles_url(origin: &str) -> String {
    format!("{origin}{}", ds_client_core::TILES_PATH)
}

fn data_distribution_url(origin: &str) -> String {
    format!("{origin}{}", ds_client_core::DATA_DISTRIBUTION_PATH)
}

/// Where a bundle `GET` may go. Production names only the TLS storage host
/// the core pinned; tests name a scripted loopback listener, which is the one
/// thing this adapter can do that production cannot.
#[derive(Clone, Copy)]
enum BundleOrigin {
    Storage,
    #[cfg(test)]
    Loopback,
}

/// The slice handed to the sink per read. Bounded memory: a bundle is never
/// held whole.
const BUNDLE_READ_BYTES: usize = 64 * 1024;

/// Stream one `GET` body into `sink`, at most `limit` bytes. Redirects are
/// off — a redirect could move the fetch to an origin nobody reviewed — and a
/// status other than 200 is a refusal with nothing written.
fn fetch_bundle(
    url: &str,
    origin: BundleOrigin,
    limit: u64,
    timeout_seconds: u64,
    sink: &mut dyn Write,
) -> Result<(), TransportError> {
    let agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .https_only(matches!(origin, BundleOrigin::Storage))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(Duration::from_secs(timeout_seconds)))
        .build()
        .new_agent();
    let mut response = agent
        .get(url)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(classify)?;
    if response.status().as_u16() != 200 {
        return Err(TransportError::Unreachable);
    }
    let mut reader = response.body_mut().with_config().limit(limit).reader();
    let mut buffer = [0u8; BUNDLE_READ_BYTES];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| TransportError::Unreachable)?;
        if read == 0 {
            break;
        }
        sink.write_all(&buffer[..read])
            .map_err(|_| TransportError::Unreachable)?;
    }
    sink.flush().map_err(|_| TransportError::Unreachable)
}

fn project_report_url(origin: &str) -> String {
    format!("{origin}{}", ds_client_core::PROJECT_REPORT_PATH)
}

fn bounded(
    mut response: ureq::http::Response<ureq::Body>,
    limit: usize,
) -> Result<TransportResponse, TransportError> {
    let status = response.status().as_u16();
    let mut body = Zeroizing::new(Vec::with_capacity(limit.min(64 * 1024)));
    response
        .body_mut()
        .with_config()
        .limit(limit.saturating_add(1) as u64)
        .reader()
        .read_to_end(&mut body)
        .map_err(|_| TransportError::Unreachable)?;
    Ok(TransportResponse::new(status, std::mem::take(&mut *body)))
}

fn classify(error: ureq::Error) -> TransportError {
    if matches!(error, ureq::Error::Timeout(_)) {
        TransportError::TimedOut
    } else {
        TransportError::Unreachable
    }
}

fn correlation_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = CORRELATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(b"ds-native-correlation/v1");
    hasher.update(std::process::id().to_be_bytes());
    hasher.update(nanos.to_be_bytes());
    hasher.update(sequence.to_be_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

fn correlation_headers() -> (String, String) {
    let request_id = correlation_id();
    (request_id.clone(), request_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    /// One scripted loopback `GET` answer. Returns the port and the thread that
    /// yields the request line and the lower-cased header names it saw.
    fn serve_once(
        status: u16,
        body: Vec<u8>,
    ) -> (u16, std::thread::JoinHandle<(String, Vec<String>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("one request");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut request_line = String::new();
            reader.read_line(&mut request_line).expect("request line");
            let mut names = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("header line");
                if line.trim().is_empty() {
                    break;
                }
                names.push(
                    line.split(':')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_ascii_lowercase(),
                );
            }
            let head = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).expect("head");
            stream.write_all(&body).expect("body");
            stream.flush().expect("flush");
            (request_line, names)
        });
        (port, worker)
    }

    /// Mirrors `upload::tests::a_storage_session_carries_no_ds_credential`:
    /// the bundle host is not ours, so nothing of ours travels with the GET.
    #[test]
    fn a_bundle_download_carries_no_ds_credential() {
        let body = b"a national reference bundle".to_vec();
        let (port, worker) = serve_once(200, body.clone());
        let mut sink = Vec::new();
        fetch_bundle(
            &format!("http://127.0.0.1:{port}/edcl/reference/roads.geojsonl.gz"),
            BundleOrigin::Loopback,
            body.len() as u64 + 1,
            30,
            &mut sink,
        )
        .expect("the scripted object streams");
        let (request_line, names) = worker.join().expect("server thread");
        assert_eq!(sink, body);
        assert!(
            request_line.starts_with("GET /edcl/reference/roads.geojsonl.gz HTTP/1.1"),
            "{request_line}"
        );
        for forbidden in [
            "authorization",
            "x-forwarded-authorization",
            "x-api-key",
            "x-user-email",
            "x-app-id",
            "x-ds-processing-lane",
            "x-request-id",
            "x-ds-action-id",
            "cookie",
        ] {
            assert!(
                !names.iter().any(|name| name == forbidden),
                "{forbidden} must never reach the bundle host"
            );
        }
    }

    #[test]
    fn a_bundle_download_refuses_a_non_200_answer_and_writes_nothing() {
        let (port, worker) = serve_once(404, b"<xml>NoSuchKey</xml>".to_vec());
        let mut sink = Vec::new();
        let refused = fetch_bundle(
            &format!("http://127.0.0.1:{port}/edcl/reference/missing.geojsonl.gz"),
            BundleOrigin::Loopback,
            1024,
            30,
            &mut sink,
        );
        let _ = worker.join();
        assert_eq!(refused, Err(TransportError::Unreachable));
        assert!(sink.is_empty());
    }

    /// Production names only the TLS storage origin: a plaintext URL is
    /// refused before any socket opens, so the host pin cannot be downgraded.
    #[test]
    fn a_bundle_download_never_leaves_tls_in_production() {
        let mut sink = Vec::new();
        assert_eq!(
            fetch_bundle(
                "http://127.0.0.1:9/edcl/reference/roads.geojsonl.gz",
                BundleOrigin::Storage,
                16,
                5,
                &mut sink,
            ),
            Err(TransportError::Unreachable)
        );
        assert!(sink.is_empty());
    }

    #[test]
    fn data_distribution_wire_target_and_limits_are_fixed() {
        assert_eq!(
            data_distribution_url("https://fixture.ue.gateway.dev"),
            "https://fixture.ue.gateway.dev/api/v1/data-distribution"
        );
        assert_eq!(ds_client_core::DATA_DISTRIBUTION_METHOD, "POST");
        assert_eq!(ds_client_core::DATA_DISTRIBUTION_TIMEOUT_SECONDS, 180);
        assert_eq!(
            ds_client_core::DATA_DISTRIBUTION_RESPONSE_LIMIT,
            32 * 1024 * 1024
        );
        assert_eq!(
            ds_client_core::DATA_DISTRIBUTION_ACTIONS,
            ["list_datasets", "query_print_context"]
        );
        assert_eq!(
            ds_client_core::BUNDLE_DOWNLOAD_HOST,
            "storage.googleapis.com"
        );
    }

    #[test]
    fn correlation_values_are_fresh_bounded_and_nonsecret() {
        let (one, action) = correlation_headers();
        let (two, _) = correlation_headers();
        assert_ne!(one, two);
        assert_eq!(one, action);
        assert!(one.len() <= ds_client_core::CORRELATION_ID_MAX_BYTES);
        assert_eq!(one.len(), 36);
        assert_eq!(one.as_bytes()[14], b'4');
        assert_eq!(one.matches('-').count(), 4);
    }

    #[test]
    fn transformer_wire_target_and_limits_are_fixed() {
        assert_eq!(
            transformer_context_url("https://fixture.ue.gateway.dev"),
            "https://fixture.ue.gateway.dev/report"
        );
        assert_eq!(ds_client_core::TRANSFORMER_CONTEXT_METHOD, "POST");
        assert_eq!(ds_client_core::TRANSFORMER_CONTEXT_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::TRANSFORMER_CONTEXT_RESPONSE_LIMIT,
            64 * 1024 * 1024
        );
    }

    #[test]
    fn project_forms_wire_target_and_limits_are_fixed() {
        assert_eq!(
            project_forms_url("https://fixture.ue.gateway.dev"),
            "https://fixture.ue.gateway.dev/api/v1/project-forms"
        );
        assert_eq!(ds_client_core::PROJECT_FORMS_METHOD, "POST");
        assert_eq!(ds_client_core::PROJECT_FORMS_ACTION, "activate");
        assert_eq!(
            ds_client_core::PROJECT_FORM_EDITOR_ACTION,
            "settings_editor"
        );
        assert_eq!(ds_client_core::PROJECT_FORMS_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::PROJECT_FORMS_RESPONSE_LIMIT,
            32 * 1024 * 1024
        );
    }

    #[test]
    fn solar_snapshot_wire_target_and_limits_are_fixed() {
        assert_eq!(
            solar_snapshot_url("https://fixture.ue.gateway.dev"),
            "https://fixture.ue.gateway.dev/api/v1/solar"
        );
        assert_eq!(ds_client_core::SOLAR_SNAPSHOT_METHOD, "POST");
        assert_eq!(ds_client_core::SOLAR_SNAPSHOT_ACTION, "desktop_snapshot");
        assert_eq!(ds_client_core::SOLAR_SNAPSHOT_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::SOLAR_SNAPSHOT_RESPONSE_LIMIT,
            32 * 1024 * 1024
        );
    }

    #[test]
    fn survey_query_wire_target_and_limits_are_fixed() {
        assert_eq!(
            survey_query_url("https://fixture.ue.gateway.dev"),
            "https://fixture.ue.gateway.dev/api/v1/survey/query"
        );
        assert_eq!(ds_client_core::SURVEY_QUERY_METHOD, "POST");
        assert_eq!(ds_client_core::SURVEY_QUERY_TIMEOUT_SECONDS, 120);
        assert_eq!(ds_client_core::SURVEY_QUERY_RESPONSE_LIMIT, 1024 * 1024);
    }

    #[test]
    fn survey_entries_select_wire_target_and_limits_are_fixed() {
        assert_eq!(
            survey_entries_select_url("https://fixture.ue.gateway.dev"),
            "https://fixture.ue.gateway.dev/api/v1/survey/entries/select"
        );
        assert_eq!(ds_client_core::SURVEY_ENTRIES_SELECT_METHOD, "POST");
        assert_eq!(ds_client_core::SURVEY_ENTRIES_SELECT_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT,
            1024 * 1024
        );
    }

    #[test]
    fn survey_entries_changes_wire_target_and_limits_are_fixed() {
        assert_eq!(
            survey_entries_changes_url("https://fixture.ue.gateway.dev"),
            "https://fixture.ue.gateway.dev/api/v1/survey/entries/changes"
        );
        assert_eq!(ds_client_core::SURVEY_ENTRIES_CHANGES_METHOD, "POST");
        assert_eq!(ds_client_core::SURVEY_ENTRIES_CHANGES_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT,
            1024 * 1024
        );
    }

    #[test]
    fn survey_entry_create_wire_target_and_limits_are_fixed() {
        assert_eq!(
            survey_entry_create_url("https://fixture.ue.gateway.dev"),
            "https://fixture.ue.gateway.dev/api/v1/entries/mutate"
        );
        assert_eq!(ds_client_core::SURVEY_ENTRY_CREATE_METHOD, "POST");
        assert_eq!(ds_client_core::SURVEY_ENTRY_CREATE_OPERATION, "create");
        assert_eq!(ds_client_core::SURVEY_ENTRY_CREATE_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::SURVEY_ENTRY_CREATE_RESPONSE_LIMIT,
            1024 * 1024
        );
    }

    #[test]
    fn project_report_wire_target_and_limits_are_fixed() {
        assert_eq!(
            project_report_url("https://fixture.ue.gateway.dev"),
            "https://fixture.ue.gateway.dev/report"
        );
        assert_eq!(ds_client_core::PROJECT_REPORT_METHOD, "POST");
        assert_eq!(ds_client_core::PROJECT_REPORT_TIMEOUT_SECONDS, 600);
        assert_eq!(ds_client_core::PROJECT_REPORT_READ_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::PROJECT_REPORT_RESPONSE_LIMIT,
            8 * 1024 * 1024
        );
        assert_eq!(
            ds_client_core::PROCESSING_LANE_HEADER,
            "X-DS-Processing-Lane"
        );
    }

    #[test]
    fn tiles_wire_target_and_limits_are_fixed() {
        assert_eq!(
            tiles_url("https://fixture.ue.gateway.dev"),
            "https://fixture.ue.gateway.dev/api/v1/tiles"
        );
        assert_eq!(ds_client_core::TILES_METHOD, "POST");
        assert_eq!(ds_client_core::TILE_TIMEOUT_SECONDS, 120);
        assert_eq!(ds_client_core::TILE_RESPONSE_LIMIT, 32 * 1024 * 1024);
    }
}
