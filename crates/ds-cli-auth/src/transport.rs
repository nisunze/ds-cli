//! Fixed-origin ureq adapter for ds-client-core's closed calls.

use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ds_client_core::{
    ProjectFormEditorCall, ProjectFormsCall, ProjectListCall, RefreshCall, SignInCall,
    SolarSnapshotCall, SurveyEntriesSelectCall, SurveyQueryCall, TransformerContextCall, Transport,
    TransportError, TransportResponse,
};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
static CORRELATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub struct NativeTransport;

impl Transport for NativeTransport {
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
        debug_assert_eq!(call.path(), "/api/v1/data");
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
            "https://fixture.ue.gateway.dev/api/v1/data"
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
}
