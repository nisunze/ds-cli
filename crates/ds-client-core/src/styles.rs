//! Guided style publication over the same captured native project and
//! authority. The publish body and every rule a receipt is admitted under are
//! the kernel's (`ds_command_kernel::style_document`), shared with the
//! browser; this module only carries them into the native client's errors.
use crate::{ClientError, ErrorKind, TransportResponse};
use ds_command_kernel::style_document as wire;
pub use ds_command_kernel::style_plan::StyleInstruction;
use serde_json::Value;

fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "invalid governed style response",
    )
}
/// The kernel's refusal, carried as the route's bounded refusal so callers
/// read the same words `ds` and the browser show.
fn refused(refusal: wire::Refusal) -> ClientError {
    ClientError::with_service_refusal(
        ErrorKind::UnreadableResponse,
        "invalid governed style response",
        crate::ServiceRefusal::new(0, Some(refusal.code), Some(&refusal.message)),
    )
}

/// The owner sentences a style write refusal carries. Static text, never a
/// response body: what ds-brain or the planner said crosses in the bounded
/// [`crate::ServiceRefusal`] beside it.
const PLAN_REFUSED: &str =
    "style instruction violates the published field, type, range or routing contract";
const STYLE_EXISTS: &str = "the governed style already exists and a create never overwrites it";
const WRITE_REFUSED: &str = "the governed style write boundary refused the document";
const WRITE_FORBIDDEN: &str =
    "the governed style write boundary refused this user's style capability";

/// The planner's refusal crosses as this code, the style domain's own.
const STYLE_REFUSED_CODE: &str = "style_refused";
/// A create the catalog shows is already published crosses as this code.
const STYLE_EXISTS_CODE: &str = "style_exists";

/// The largest refusal body read for its code and sentence.
const MAX_REFUSAL_BODY: usize = 64 * 1024;
/// How many of the backend's named issues one refusal sentence carries.
const MAX_REFUSAL_ISSUES: usize = 3;

/// The kernel planner's refusal, in the planner's words. Its sentences are
/// the kernel's own — an icon outside the catalog, a field the layer does not
/// declare, a size out of range — never a response body. Status 0 marks it as
/// decided here, before any request.
pub(crate) fn planner_refused(sentence: &str) -> ClientError {
    ClientError::with_service_refusal(
        ErrorKind::InvalidInput,
        PLAN_REFUSED,
        crate::ServiceRefusal::new(
            0,
            Some(if sentence.starts_with("style_digest_conflict:") {
                "style_digest_conflict"
            } else {
                STYLE_REFUSED_CODE
            }),
            Some(sentence),
        ),
    )
}

/// A create-only style the catalog already holds. Refused before the write,
/// with the planner's sentence naming the ref to customise instead.
pub(crate) fn exists(sentence: &str) -> ClientError {
    ClientError::with_service_refusal(
        ErrorKind::InvalidInput,
        STYLE_EXISTS,
        crate::ServiceRefusal::new(0, Some(STYLE_EXISTS_CODE), Some(sentence)),
    )
}

/// Classify one refused style publish, carrying ds-brain's own code,
/// sentence and the first issues it named.
///
/// `update_style` answers every refusal in the DS envelope: `409 CONFLICT`
/// when a create-only style (a `_print` variant, a seeded print-context
/// source) already exists, because it never overwrites; `400
/// VALIDATION_FAILED` / `MISSING_REQUIRED_FIELD` with `details.issues[]` when
/// the document or its routing is refused; `403` when the account lacks
/// `styles.edit` or the project. The shared tile classifier used to collapse
/// all of that into "the managed tile service refused the bounded operation",
/// which `ds` then rendered with a remedy about the project id (2026-09-27,
/// `ds style print create --ref gt/rivers`, whose `gt/rivers_print` existed).
/// Every other status keeps the tile classification it always had.
pub(crate) fn refusal(response: &TransportResponse) -> ClientError {
    let (kind, message) = match response.status {
        409 => (ErrorKind::InvalidInput, STYLE_EXISTS),
        400 | 404 | 422 => (ErrorKind::InvalidInput, WRITE_REFUSED),
        401 | 403 => (ErrorKind::AuthenticationRejected, WRITE_FORBIDDEN),
        status => return crate::authorized::classify_tile_error(status),
    };
    match envelope(response) {
        Some(served) => ClientError::with_service_refusal(kind, message, served),
        None => ClientError::new(kind, message),
    }
}

/// The DS envelope's `error` as a bounded refusal: its code in lower case,
/// its sentence, and up to [`MAX_REFUSAL_ISSUES`] of `details.issues` as
/// `<path>: <message>`. `None` when the body carried no envelope at all (a
/// gateway page, an outage).
fn envelope(response: &TransportResponse) -> Option<crate::ServiceRefusal> {
    if response.body.is_empty() || response.body.len() > MAX_REFUSAL_BODY {
        return None;
    }
    let value: Value = serde_json::from_slice(&response.body).ok()?;
    let error = value.get("error").filter(|error| error.is_object())?;
    let code = error["code"].as_str().map(str::to_ascii_lowercase);
    let issues = issue_sentences(&error["details"]["issues"]);
    let sentence = match (error["message"].as_str(), issues.is_empty()) {
        (Some(message), true) => Some(message.to_owned()),
        (Some(message), false) => Some(format!("{message}: {}", issues.join("; "))),
        (None, false) => Some(issues.join("; ")),
        (None, true) => None,
    };
    let served = crate::ServiceRefusal::new(response.status, code.as_deref(), sentence.as_deref());
    (served.code().is_some() || served.message().is_some()).then_some(served)
}

/// ds-brain names a refused document's issues as `{path, message}`, and an
/// expression's as `{property, severity, issue: {path, message}}`.
fn issue_sentences(issues: &Value) -> Vec<String> {
    let named: Vec<String> = issues
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|issue| {
            let message = issue["message"]
                .as_str()
                .or_else(|| issue["issue"]["message"].as_str())
                .filter(|message| !message.is_empty())?;
            let place = issue["path"]
                .as_str()
                .or_else(|| issue["property"].as_str())
                .filter(|place| !place.is_empty());
            Some(match place {
                Some(place) => format!("{place}: {message}"),
                None => message.to_owned(),
            })
        })
        .collect();
    let more = named.len().saturating_sub(MAX_REFUSAL_ISSUES);
    let mut shown: Vec<String> = named.into_iter().take(MAX_REFUSAL_ISSUES).collect();
    if more > 0 {
        shown.push(format!("+{more} more"));
    }
    shown
}

/// Whether one publish receipt is the style that was authored: the kernel's
/// answer, over the whole parsed response body.
pub(crate) fn admit_publish(
    expected: &Value,
    response: TransportResponse,
) -> Result<Value, ClientError> {
    if response.body.is_empty() || response.body.len() > crate::LAYERS_RESPONSE_LIMIT {
        return Err(bad());
    }
    let answered: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    wire::admit_publish(expected, &answered).map_err(refused)
}

#[derive(Clone)]
pub struct StyleEditReceipt {
    pub(crate) data: Value,
}
impl StyleEditReceipt {
    pub fn data(&self) -> &Value {
        &self.data
    }
}

/// One exact print revision operation under the captured project authority.
pub(crate) fn history<T: crate::AuthorizedTransport>(
    profile: &crate::ClientProfile,
    transport: &mut T,
    authority: crate::authorized::Authority<'_>,
    project: &str,
    command: &ds_command_kernel::style_history::Command,
) -> Result<Value, ClientError> {
    crate::authorized::validate_tiles(project)?;
    let request = ds_command_kernel::style_history::Request {
        project: project.into(),
        command: command.clone(),
    };
    ds_command_kernel::style_history::request_body(&request)
        .map_err(|sentence| planner_refused(&sentence))?;
    let empty = Value::Null;
    let response = transport.styles(crate::StylesCall {
        profile,
        payload: &empty,
        history: Some(&request),
        governance: None,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(refusal(&response));
    }
    if response.body.len() > 4 * 1024 * 1024 {
        return Err(bad());
    }
    let response: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    ds_command_kernel::style_history::admit(command, &response)
        .map_err(|sentence| planner_refused(&sentence))
}

fn governance_refused(sentence: &str) -> ClientError {
    let code = sentence
        .split_once(':')
        .map(|(code, _)| code)
        .filter(|code| {
            code.starts_with("style_resolution_")
                || *code == "style_governance_invalid"
                || *code == "project_context_changed"
        })
        .unwrap_or("style_governance_invalid");
    ClientError::with_service_refusal(
        ErrorKind::InvalidInput,
        PLAN_REFUSED,
        crate::ServiceRefusal::new(0, Some(code), Some(sentence)),
    )
}

/// Fixed actions carried under one captured project. Resolution remains the
/// kernel's sole function over the API's admitted exact document table.
pub(crate) fn governance<T: crate::AuthorizedTransport>(
    profile: &crate::ClientProfile,
    transport: &mut T,
    authority: crate::authorized::Authority<'_>,
    project: &str,
    command: &ds_command_kernel::style_governance::Command,
) -> Result<Value, ClientError> {
    crate::authorized::validate_tiles(project)?;
    let request = ds_command_kernel::style_governance::Request {
        project: project.into(),
        command: command.clone(),
    };
    ds_command_kernel::style_governance::request_body(&request)
        .map_err(|e| governance_refused(&e))?;
    let empty = Value::Null;
    let response = transport.styles(crate::StylesCall {
        profile,
        payload: &empty,
        history: None,
        governance: Some(&request),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        let kind = match response.status {
            401 | 403 => ErrorKind::AuthenticationRejected,
            500..=599 => ErrorKind::Transient,
            _ => ErrorKind::InvalidInput,
        };
        return Err(envelope(&response)
            .map(|served| ClientError::with_service_refusal(kind, WRITE_REFUSED, served))
            .unwrap_or_else(|| refusal(&response)));
    }
    if response.body.is_empty() || response.body.len() > crate::LAYERS_RESPONSE_LIMIT {
        return Err(bad());
    }
    let response: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    let admitted = ds_command_kernel::style_governance::admit(&request, &response)
        .map_err(|e| governance_refused(&e))?;
    if matches!(
        command,
        ds_command_kernel::style_governance::Command::PlanA4
            | ds_command_kernel::style_governance::Command::CreateA4 { .. }
    ) {
        let plan = if matches!(
            command,
            ds_command_kernel::style_governance::Command::CreateA4 { .. }
        ) {
            &admitted["plan"]
        } else {
            &admitted
        };
        if plan["principal"] != authority.email {
            return Err(governance_refused(
                "project_context_changed: A4 plan belongs to another captured principal",
            ));
        }
    }
    Ok(admitted)
}
