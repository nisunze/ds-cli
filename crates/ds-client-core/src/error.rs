pub use ds_command_kernel::survey_query::SurveyQueryServiceCode;
use std::fmt;

/// Stable failure classification for a host/CLI adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidInput,
    SignedOut,
    InvalidCredentials,
    PasswordSignInUnavailable,
    AccountDisabled,
    AuthenticationRejected,
    ResourceNotFound,
    PermanentlyRevoked,
    IdentityMismatch,
    Transient,
    UnreadableResponse,
    DurableState,
    /// The deployment this lane talks to does not expose the route at all.
    ///
    /// Distinct from `ResourceNotFound`, which is a fact about the caller's
    /// project or object. A gateway that has never been taught a path answers
    /// the same 404 as a backend that looked and found nothing, and collapsing
    /// the two told every operator of the three unrouted Survey read paths
    /// that their project or form was invisible to them.
    RouteUnavailable,
}

/// Shared authority refusal returned by each governed Survey read route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurveyFormReadServiceCode {
    ProjectAccessDenied,
    FormAccessDenied,
    FormBindingNotFound,
    FormNotParticipating,
}

/// Closed backend refusal code for the bounded Survey entry selection route.
///
/// A host may use this only to choose bounded operator guidance. Backend error
/// messages, details, and response bytes never cross the core boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurveyEntriesSelectServiceCode {
    TooExpensive,
    TooLarge,
    SyncFailed,
    MirrorInvalid,
    Invalid,
    Unavailable,
    Failed,
    ScopeNotFound,
}

/// Closed backend refusal code for the governed project-report route.
///
/// The report route refuses for reasons a caller can act on — a prerequisite
/// that is not prepared, a grouping that moved under the request, nothing to
/// package — and the status code alone cannot tell them apart: `409` covers
/// two unrelated causes, and `424` is the one a whole compounded run ends on.
/// Without this the caller received one sentence for all of
/// `400|409|422|423|424`, which is what made a blocked reporting session
/// unreadable from the headless lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectReportServiceCode {
    /// The owner refused before ZIP publication because inputs are not current.
    CombinedInputsNotCurrent,
    /// A device report must finish publication before combined preparation.
    CombinedInputsPublicationPending,
    /// The run finished but no transformer had an individual report to package.
    NoIndividualArtifacts,
    /// The applied grouping moved after the request pinned its digest.
    GroupingStale,
    /// The applied grouping does not cover every requested transformer.
    GroupingIncomplete,
}

/// Closed backend refusal code for the fenced Survey entry changes route.
///
/// This remains separate from spatial-selection refusals because the two
/// operations have different retry, cursor, and snapshot-fence semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurveyEntriesChangesServiceCode {
    Invalid,
    CursorInvalid,
    FenceExpired,
    TooExpensive,
    TooLarge,
    MirrorInvalid,
    SnapshotUnavailable,
    Unavailable,
    SyncFailed,
    Failed,
    ScopeNotFound,
}

/// Closed backend refusal code for one governed Survey entry create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurveyEntryCreateServiceCode {
    /// Local provider refusal before IO: DS device JWTs cannot authorize Firestore.
    FirebaseSessionRequired,
    Invalid,
    Unauthorized,
    PermissionDenied,
    ScopeNotFound,
    FormDisabled,
    ProjectReadOnly,
    IdempotencyConflict,
    AlreadyExists,
    Failed,
}

/// Closed refusal code of `POST /api/v1/auth/device/approve`.
///
/// The route sits behind ds-brain's `FirebaseAuthentication` boundary and
/// answers with `deviceauth` codes (`internal/deviceauth/errors.go`), each of
/// which names a different next move: a device credential can never approve
/// a sibling, an unknown or expired request must be begun again, a decided one
/// must be re-read, a fingerprint mismatch must not be approved at all. The
/// status alone cannot tell the two `409`s apart, and folding the `403` into a
/// generic authentication refusal would hide the one fact a headless operator
/// needs — that the CREDENTIAL kind, not the account, is what was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceApprovalServiceCode {
    /// `403 DESKTOP_PRINCIPAL_REQUIRED`: the bearer was not a Firebase user
    /// token. A device credential presented itself, or no principal did.
    DesktopPrincipalRequired,
    /// `404 DEVICE_AUTHORIZATION_NOT_FOUND`: no request by that id on this lane.
    AuthorizationNotFound,
    /// `409 DEVICE_FINGERPRINT_MISMATCH`: the fingerprint sent is not the one
    /// the request was begun with. Never approve; compare on both devices.
    FingerprintMismatch,
    /// `409 DEVICE_APPROVAL_CONFLICT`: already decided; re-read, never retry.
    ApprovalConflict,
    /// `410 DEVICE_AUTHORIZATION_EXPIRED`: deliberately expired; begin again.
    AuthorizationExpired,
    /// `400 DEVICE_REQUEST_INVALID`: the strict decoder or the decision token.
    RequestInvalid,
    /// `503 DEVICE_AUTH_DISABLED`: this deployment carries no device
    /// authorization environment. No retry against this revision can succeed.
    AuthorityUnavailable,
}

/// Closed refusal code of `POST /api/v1/projects` for `create` and `update`.
///
/// The route answers with ds-brain's standard envelope
/// (`internal/router/handlers_unified.go`, `pkg/response`). The two `403`s
/// name different next moves — a missing capability is granted by a project
/// admin, a read-only project is unarchived first — and the status alone
/// cannot tell them apart, so the envelope code is matched exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectPropertiesServiceCode {
    /// `400 VALIDATION_FAILED`: the payload was refused by the handler's own
    /// checks (a missing name, an unknown project type, a bad CRS).
    Invalid,
    /// `403 INSUFFICIENT_PERMISSIONS`: the caller lacks `project.create`, or
    /// `project.properties.edit` on this project (project admin).
    CapabilityMissing,
    /// `403 PROJECT_ARCHIVED` / `PROJECT_EXPIRED`: the lifecycle write gate.
    ProjectReadOnly,
    /// `404 PROJECT_NOT_FOUND`: no project carries this id on this lane.
    ProjectNotFound,
}

/// One governed route's own refusal, bounded and sanitized at this boundary.
///
/// The crate's standing rule is that backend error text stays behind the
/// coarse public classification, and it holds everywhere a status code is
/// enough to act on. Print-layout validation is the documented exception: the
/// only actionable half of `unknown field `composition`, expected one of …` IS
/// the sentence, and collapsing it into "the response did not match its closed
/// contract" is precisely what made the 2026-09-10 production failure
/// unreadable from every client. So the sentence crosses — as a bounded,
/// sanitized, typed value that can never carry a URL, a control character, an
/// unbounded body or a header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRefusal {
    status: u16,
    code: Option<String>,
    message: Option<String>,
    /// The ids and numbers the route's rule names, bounded and scalar: a
    /// `record_id`, a `bound`, an `external_url`. Never a nested body.
    details: Vec<(String, ServiceRefusalDetail)>,
}

/// One scalar a governed route named in its refusal's `details`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceRefusalDetail {
    Text(String),
    Integer(i64),
    Flag(bool),
}

impl ServiceRefusal {
    /// The longest server sentence this boundary will carry.
    pub const MAX_MESSAGE_BYTES: usize = 400;
    /// The longest server code this boundary will carry.
    pub const MAX_CODE_BYTES: usize = 64;
    /// The most named details this boundary will carry.
    pub const MAX_DETAILS: usize = 16;
    /// The longest detail text this boundary will carry — long enough for
    /// the https link a reference refusal names, never a body.
    pub const MAX_DETAIL_BYTES: usize = 2048;

    /// Build one from a raw server envelope. Anything outside the bounds is
    /// dropped rather than truncated into a different claim, except the
    /// message, which is truncated on a character boundary and marked.
    ///
    /// Public so a host can prove, in its own tests, what it renders for a
    /// given refusal. It constructs no `ClientError` and reaches no
    /// transport: sanitizing bytes is all it can do.
    pub fn new(status: u16, code: Option<&str>, message: Option<&str>) -> Self {
        Self {
            status,
            code: code.and_then(sanitized_code),
            message: message.and_then(sanitized_message),
            details: Vec::new(),
        }
    }

    /// The same, carrying the scalar details a route named — a JSON object
    /// whose string, integer and boolean members survive, each key an
    /// identifier, at most [`Self::MAX_DETAILS`] of them in key order, every
    /// text sanitized like a message and bounded at
    /// [`Self::MAX_DETAIL_BYTES`]. Anything else in the object is dropped:
    /// a nested value is a body, and a body never crosses this boundary.
    pub fn with_details(
        status: u16,
        code: Option<&str>,
        message: Option<&str>,
        details: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Self {
        let mut refusal = Self::new(status, code, message);
        let Some(details) = details else {
            return refusal;
        };
        let mut keys: Vec<&String> = details.keys().collect();
        keys.sort();
        for key in keys {
            if refusal.details.len() >= Self::MAX_DETAILS {
                break;
            }
            let Some(name) = sanitized_code(key) else {
                continue;
            };
            let value = match &details[key] {
                serde_json::Value::String(text) => {
                    let Some(text) = sanitized_detail(text) else {
                        continue;
                    };
                    ServiceRefusalDetail::Text(text)
                }
                serde_json::Value::Number(number) => match number.as_i64() {
                    Some(integer) => ServiceRefusalDetail::Integer(integer),
                    None => continue,
                },
                serde_json::Value::Bool(flag) => ServiceRefusalDetail::Flag(*flag),
                _ => continue,
            };
            refusal.details.push((name, value));
        }
        refusal
    }

    /// The named details, in key order.
    pub fn details(&self) -> &[(String, ServiceRefusalDetail)] {
        &self.details
    }

    /// One named detail's text, when the route named it as text.
    pub fn detail_text(&self, name: &str) -> Option<&str> {
        self.details.iter().find_map(|(key, value)| match value {
            ServiceRefusalDetail::Text(text) if key == name => Some(text.as_str()),
            _ => None,
        })
    }

    /// One named detail's number, when the route named it as a number.
    pub fn detail_integer(&self, name: &str) -> Option<i64> {
        self.details.iter().find_map(|(key, value)| match value {
            ServiceRefusalDetail::Integer(integer) if key == name => Some(*integer),
            _ => None,
        })
    }

    /// The exact HTTP status the governed route answered with.
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// The route's own machine-readable code, when it named one.
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// The route's own sentence, when it carried one a caller can act on.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

/// A code is an identifier or it is nothing: lowercase ASCII, digits and
/// underscores only, so it can never smuggle text a host would render.
fn sanitized_code(code: &str) -> Option<String> {
    let code = code.trim();
    if code.is_empty()
        || code.len() > ServiceRefusal::MAX_CODE_BYTES
        || !code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return None;
    }
    Some(code.to_owned())
}

/// A detail text keeps control characters out and is bounded; unlike a
/// message it keeps an address, because the address IS the detail a
/// reference refusal names (`external_url`) and the reason a `read` of an
/// external reference can show its link.
fn sanitized_detail(text: &str) -> Option<String> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return None;
    }
    if cleaned.len() > ServiceRefusal::MAX_DETAIL_BYTES {
        return None;
    }
    Some(cleaned)
}

/// A sentence keeps its words and loses everything else: control characters
/// collapse to spaces, any token carrying a scheme separator is replaced
/// rather than trimmed, and the result is bounded on a character boundary.
fn sanitized_message(message: &str) -> Option<String> {
    let cleaned = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let mut words = cleaned
        .split_whitespace()
        .map(|word| {
            if word.contains("://") {
                "[address removed]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if words.is_empty() {
        return None;
    }
    if words.len() > ServiceRefusal::MAX_MESSAGE_BYTES {
        let mut end = ServiceRefusal::MAX_MESSAGE_BYTES;
        while end > 0 && !words.is_char_boundary(end) {
            end -= 1;
        }
        words.truncate(end);
        words.push('\u{2026}');
    }
    Some(words)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientError {
    kind: ErrorKind,
    message: &'static str,
    /// The governed route's own bounded refusal, when it authored one.
    service_refusal: Option<ServiceRefusal>,
    survey_entries_select_service_code: Option<SurveyEntriesSelectServiceCode>,
    survey_entries_changes_service_code: Option<SurveyEntriesChangesServiceCode>,
    survey_entry_create_service_code: Option<SurveyEntryCreateServiceCode>,
    project_report_service_code: Option<ProjectReportServiceCode>,
    survey_form_read_service_code: Option<SurveyFormReadServiceCode>,
    survey_query_service_code: Option<SurveyQueryServiceCode>,
    device_approval_service_code: Option<DeviceApprovalServiceCode>,
    project_properties_service_code: Option<ProjectPropertiesServiceCode>,
    /// How many times a weak link was retried before this error stood.
    retries: u8,
}

impl ClientError {
    pub(crate) const fn new(kind: ErrorKind, message: &'static str) -> Self {
        Self {
            kind,
            message,
            service_refusal: None,
            survey_entries_select_service_code: None,
            survey_entries_changes_service_code: None,
            survey_entry_create_service_code: None,
            project_report_service_code: None,
            survey_form_read_service_code: None,
            survey_query_service_code: None,
            device_approval_service_code: None,
            project_properties_service_code: None,
            retries: 0,
        }
    }

    /// A named report refusal that also carries the route's bounded, scalar
    /// facts (for a combined run: which rooms had no report, and the closed
    /// cause code of each), never its sentences.
    pub(crate) fn with_project_report_refusal(
        kind: ErrorKind,
        message: &'static str,
        code: ProjectReportServiceCode,
        refusal: ServiceRefusal,
    ) -> Self {
        let mut error = Self::with_project_report_service_code(kind, message, code);
        error.service_refusal = Some(refusal);
        error
    }

    pub(crate) const fn with_project_report_service_code(
        kind: ErrorKind,
        message: &'static str,
        code: ProjectReportServiceCode,
    ) -> Self {
        Self {
            kind,
            message,
            service_refusal: None,
            survey_entries_select_service_code: None,
            survey_entries_changes_service_code: None,
            survey_entry_create_service_code: None,
            project_report_service_code: Some(code),
            survey_form_read_service_code: None,
            survey_query_service_code: None,
            device_approval_service_code: None,
            project_properties_service_code: None,
            retries: 0,
        }
    }

    pub(crate) const fn with_survey_entries_select_service_code(
        kind: ErrorKind,
        message: &'static str,
        code: SurveyEntriesSelectServiceCode,
    ) -> Self {
        Self {
            kind,
            message,
            service_refusal: None,
            survey_entries_select_service_code: Some(code),
            survey_entries_changes_service_code: None,
            survey_entry_create_service_code: None,
            project_report_service_code: None,
            survey_form_read_service_code: None,
            survey_query_service_code: None,
            device_approval_service_code: None,
            project_properties_service_code: None,
            retries: 0,
        }
    }

    pub(crate) const fn with_survey_entries_changes_service_code(
        kind: ErrorKind,
        message: &'static str,
        code: SurveyEntriesChangesServiceCode,
    ) -> Self {
        Self {
            kind,
            message,
            service_refusal: None,
            survey_entries_select_service_code: None,
            survey_entries_changes_service_code: Some(code),
            survey_entry_create_service_code: None,
            project_report_service_code: None,
            survey_form_read_service_code: None,
            survey_query_service_code: None,
            device_approval_service_code: None,
            project_properties_service_code: None,
            retries: 0,
        }
    }

    pub(crate) const fn with_survey_entry_create_service_code(
        kind: ErrorKind,
        message: &'static str,
        code: SurveyEntryCreateServiceCode,
    ) -> Self {
        Self {
            kind,
            message,
            service_refusal: None,
            survey_entries_select_service_code: None,
            survey_entries_changes_service_code: None,
            survey_entry_create_service_code: Some(code),
            project_report_service_code: None,
            survey_form_read_service_code: None,
            survey_query_service_code: None,
            device_approval_service_code: None,
            project_properties_service_code: None,
            retries: 0,
        }
    }

    pub(crate) const fn with_survey_form_read_service_code(
        kind: ErrorKind,
        message: &'static str,
        code: SurveyFormReadServiceCode,
    ) -> Self {
        Self {
            kind,
            message,
            service_refusal: None,
            survey_entries_select_service_code: None,
            survey_entries_changes_service_code: None,
            survey_entry_create_service_code: None,
            project_report_service_code: None,
            survey_form_read_service_code: Some(code),
            survey_query_service_code: None,
            device_approval_service_code: None,
            project_properties_service_code: None,
            retries: 0,
        }
    }

    /// Carry one governed route's own bounded refusal alongside the coarse
    /// classification. Not `const`: the sentence is owned and sanitized.
    pub(crate) fn with_service_refusal(
        kind: ErrorKind,
        message: &'static str,
        refusal: ServiceRefusal,
    ) -> Self {
        Self {
            kind,
            message,
            service_refusal: Some(refusal),
            survey_entries_select_service_code: None,
            survey_entries_changes_service_code: None,
            survey_entry_create_service_code: None,
            project_report_service_code: None,
            survey_form_read_service_code: None,
            survey_query_service_code: None,
            device_approval_service_code: None,
            project_properties_service_code: None,
            retries: 0,
        }
    }

    pub(crate) const fn with_survey_query_service_code(
        kind: ErrorKind,
        message: &'static str,
        code: SurveyQueryServiceCode,
    ) -> Self {
        let mut error = Self::new(kind, message);
        error.survey_query_service_code = Some(code);
        error
    }

    pub const fn survey_query_service_code(&self) -> Option<SurveyQueryServiceCode> {
        self.survey_query_service_code
    }

    pub(crate) const fn with_device_approval_service_code(
        kind: ErrorKind,
        message: &'static str,
        code: DeviceApprovalServiceCode,
    ) -> Self {
        let mut error = Self::new(kind, message);
        error.device_approval_service_code = Some(code);
        error
    }

    /// The device-approval route's own refusal, when it named one.
    pub const fn device_approval_service_code(&self) -> Option<DeviceApprovalServiceCode> {
        self.device_approval_service_code
    }

    /// A project-properties refusal: the route's typed code, and its own
    /// bounded sentence when it authored one a project admin can act on.
    pub(crate) fn with_project_properties_service_code(
        kind: ErrorKind,
        message: &'static str,
        code: ProjectPropertiesServiceCode,
        refusal: Option<ServiceRefusal>,
    ) -> Self {
        let mut error = Self::new(kind, message);
        error.project_properties_service_code = Some(code);
        error.service_refusal = refusal;
        error
    }

    /// The project-properties route's own refusal, when it named one.
    pub const fn project_properties_service_code(&self) -> Option<ProjectPropertiesServiceCode> {
        self.project_properties_service_code
    }

    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The governed route's own refusal, when it authored one a caller can
    /// act on. Present only where a route's status alone cannot say what to
    /// do about it.
    pub fn service_refusal(&self) -> Option<&ServiceRefusal> {
        self.service_refusal.as_ref()
    }

    pub const fn survey_entries_select_service_code(
        &self,
    ) -> Option<SurveyEntriesSelectServiceCode> {
        self.survey_entries_select_service_code
    }

    /// The governed report route's own refusal, when it named one.
    pub const fn project_report_service_code(&self) -> Option<ProjectReportServiceCode> {
        self.project_report_service_code
    }

    pub const fn survey_entries_changes_service_code(
        &self,
    ) -> Option<SurveyEntriesChangesServiceCode> {
        self.survey_entries_changes_service_code
    }

    pub const fn survey_entry_create_service_code(&self) -> Option<SurveyEntryCreateServiceCode> {
        self.survey_entry_create_service_code
    }

    pub const fn survey_form_read_service_code(&self) -> Option<SurveyFormReadServiceCode> {
        self.survey_form_read_service_code
    }

    /// Record that the [`crate::WeakNetwork`] policy retried this call before
    /// a transient error stood, so the message says so instead of reading as
    /// one blink. A definitive answer that arrived after a retry is left as it
    /// came: the retry did not fail, the request did.
    pub(crate) const fn after_retries(mut self, retries: u8) -> Self {
        if matches!(self.kind, ErrorKind::Transient) {
            self.retries = retries;
        }
        self
    }

    /// How many retries the weak-network policy spent before this error: zero
    /// when the call was not retried.
    pub const fn retries(&self) -> u8 {
        self.retries
    }

    pub const fn is_permanent_session_failure(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::PermanentlyRevoked | ErrorKind::IdentityMismatch
        )
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)?;
        match self.retries {
            0 => Ok(()),
            1 => formatter.write_str(" (retried once)"),
            retries => write!(formatter, " (retried {retries} times)"),
        }
    }
}

impl std::error::Error for ClientError {}

/// A bounded transport failure. It must never contain request data or secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    Unreachable,
    TimedOut,
}

/// A bounded durable-state failure. It must never contain stored bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    Unavailable,
    Conflict,
    UnsafeOrUnreadable,
}

impl From<TransportError> for ClientError {
    fn from(_: TransportError) -> Self {
        Self::new(
            ErrorKind::Transient,
            "the authentication or project service is temporarily unreachable",
        )
    }
}

impl From<StoreError> for ClientError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Conflict => Self::new(
                ErrorKind::Transient,
                "another client rotated the refresh credential; reload and retry",
            ),
            StoreError::Unavailable | StoreError::UnsafeOrUnreadable => Self::new(
                ErrorKind::DurableState,
                "the refresh-credential store could not complete an atomic update",
            ),
        }
    }
}
