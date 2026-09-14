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
}

impl ServiceRefusal {
    /// The longest server sentence this boundary will carry.
    pub const MAX_MESSAGE_BYTES: usize = 400;
    /// The longest server code this boundary will carry.
    pub const MAX_CODE_BYTES: usize = 64;

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
        }
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
        }
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
        }
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

    pub const fn is_transient(&self) -> bool {
        matches!(self.kind, ErrorKind::Transient)
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
        formatter.write_str(self.message)
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
