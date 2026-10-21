use serde::Deserialize;

use crate::{ClientError, ErrorKind, SurveyFormReadServiceCode, TransportResponse};

const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_CODE_BYTES: usize = 128;

#[derive(Deserialize)]
struct Envelope {
    success: bool,
    error: ErrorWire,
}

#[derive(Deserialize)]
struct ErrorWire {
    code: String,
}

pub(crate) fn decode_service_error(
    response: &TransportResponse,
    response_limit: usize,
) -> Option<ClientError> {
    if response.body.len() > response_limit.min(MAX_ERROR_BODY_BYTES) {
        return None;
    }
    let envelope: Envelope = serde_json::from_slice(&response.body).ok()?;
    if envelope.success
        || envelope.error.code.is_empty()
        || envelope.error.code.len() > MAX_CODE_BYTES
        || envelope.error.code.chars().any(char::is_control)
    {
        return None;
    }
    let (kind, message, code) = match (response.status, envelope.error.code.as_str()) {
        (403, "SURVEY_PROJECT_ACCESS_DENIED") => (
            ErrorKind::AuthenticationRejected,
            "the verified user does not have access to the selected project",
            SurveyFormReadServiceCode::ProjectAccessDenied,
        ),
        (403, "SURVEY_FORM_ACCESS_DENIED") => (
            ErrorKind::AuthenticationRejected,
            "the survey form is outside the verified user's project grant",
            SurveyFormReadServiceCode::FormAccessDenied,
        ),
        (404, "SURVEY_FORM_BINDING_NOT_FOUND") => (
            ErrorKind::ResourceNotFound,
            "the survey form is not bound to the selected project",
            SurveyFormReadServiceCode::FormBindingNotFound,
        ),
        (409, "SURVEY_FORM_NOT_PARTICIPATING") => (
            ErrorKind::InvalidInput,
            "the bound project form is not enabled for survey reads",
            SurveyFormReadServiceCode::FormNotParticipating,
        ),
        _ => return None,
    };
    Some(ClientError::with_survey_form_read_service_code(
        kind, message, code,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, code: &str) -> TransportResponse {
        TransportResponse::new(
            status,
            serde_json::to_vec(&serde_json::json!({
                "success": false,
                "error": { "code": code, "message": "server prose is not trusted" },
                "timestamp": "2026-09-07T00:00:00Z",
                "service": "data-solutions-backend",
                "version": "1.0.0"
            }))
            .unwrap(),
        )
    }

    #[test]
    fn exact_form_authority_codes_survive_without_backend_prose() {
        let cases = [
            (
                403,
                "SURVEY_PROJECT_ACCESS_DENIED",
                SurveyFormReadServiceCode::ProjectAccessDenied,
            ),
            (
                403,
                "SURVEY_FORM_ACCESS_DENIED",
                SurveyFormReadServiceCode::FormAccessDenied,
            ),
            (
                404,
                "SURVEY_FORM_BINDING_NOT_FOUND",
                SurveyFormReadServiceCode::FormBindingNotFound,
            ),
            (
                409,
                "SURVEY_FORM_NOT_PARTICIPATING",
                SurveyFormReadServiceCode::FormNotParticipating,
            ),
        ];
        for (status, raw, expected) in cases {
            let error = decode_service_error(&response(status, raw), 64 * 1024).unwrap();
            assert_eq!(error.survey_form_read_service_code(), Some(expected));
            assert!(!error.to_string().contains("server prose"));
        }
        assert!(
            decode_service_error(&response(404, "SURVEY_FORM_NOT_PARTICIPATING"), 64 * 1024,)
                .is_none()
        );
    }
}
