//! The output envelope, error classes and exit codes — the automation
//! contract.
//!
//! One shape answers every invocation, success or not, so a caller writes one
//! handler rather than one per command. The envelope is versioned
//! independently of the binary: `ds` is the release, `contract` is the
//! command's own input/output version, and `v` is the envelope's.
//!
//! Errors are short on purpose. An agent needs four things to decide what to
//! do next — what failed, whether retrying could help, what would fix it, and
//! which command to run now — and none of those are served by a stack trace.

use std::fmt;

use serde::Serialize;
use serde_json::Value;

/// The envelope version. Raised only for a breaking change to the envelope
/// itself, never for a change inside `data`.
pub const ENVELOPE_VERSION: u32 = 1;

/// Process exit codes, grouped so a script can branch on a class without
/// parsing output.
///
/// The grouping is the contract: 2 through 6 each name a distinct thing the
/// caller could do about the failure. A code that told a caller only "it did
/// not work" would leave them exactly where an English log leaves them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitClass {
    /// The command ran and answered. Note that a *domain* answer of "this
    /// structure fails its criteria" is still a success: execution truth and
    /// engineering truth are reported separately.
    Success = 0,
    /// A defect in `ds`. Never expected; report it.
    Internal = 1,
    /// The invocation was wrong: unknown command or flag, missing required
    /// input, unparseable value, input outside its bound.
    InvalidInput = 2,
    /// The command is real but cannot run here: an engine, an external tool,
    /// a data asset or the desktop is missing. Retry after fixing the remedy.
    Unavailable = 3,
    /// No verified principal, or the principal may not do this.
    Unauthorized = 4,
    /// The caller's view of the world is stale, or two effects collided.
    Conflict = 5,
    /// The command ran and failed. The work did not happen.
    Failed = 6,
}

impl ExitClass {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn token(self) -> &'static str {
        match self {
            Self::Success => "ok",
            Self::Internal => "internal",
            Self::InvalidInput => "invalid_input",
            Self::Unavailable => "unavailable",
            Self::Unauthorized => "unauthorized",
            Self::Conflict => "conflict",
            Self::Failed => "failed",
        }
    }

    /// Whether an unchanged retry could plausibly succeed. `unavailable` and
    /// `conflict` say yes because the world, not the request, is what has to
    /// change.
    pub const fn retryable(self) -> bool {
        matches!(self, Self::Unavailable | Self::Conflict)
    }
}

/// A refusal, in the four terms a caller can act on.
///
/// The payload is boxed so `Failure` is one pointer wide. Every fallible
/// function in the CLI returns `Result<_, Failure>`, and a refusal is the
/// cold path by definition — paying one allocation when something goes wrong
/// is better than making every success path carry a 136-byte error variant.
#[derive(Debug, Clone)]
pub struct Failure(Box<Refusal>);

#[derive(Debug, Clone)]
pub struct Refusal {
    pub class: ExitClass,
    /// Stable, machine-matchable, snake_case. Never localized, never
    /// reworded. This is the field a script branches on.
    pub code: String,
    /// One sentence. No path echo beyond what the caller supplied, no
    /// credential-shaped text, no stack trace.
    pub message: String,
    /// The concrete thing that would fix it.
    pub remedy: Option<String>,
    /// Commands worth running next. Ordered most-useful first.
    pub next: Vec<String>,
    /// Bounded structured context, when a code alone is not enough — for
    /// example the accepted values of a rejected choice.
    pub detail: Option<Value>,
}

impl Failure {
    pub fn new(class: ExitClass, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self(Box::new(Refusal {
            class,
            code: code.into(),
            message: message.into(),
            remedy: None,
            next: Vec::new(),
            detail: None,
        }))
    }

    pub fn invalid(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ExitClass::InvalidInput, code, message)
    }

    pub fn unavailable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ExitClass::Unavailable, code, message)
    }

    pub fn unauthorized(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ExitClass::Unauthorized, code, message)
    }

    pub fn conflict(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ExitClass::Conflict, code, message)
    }

    pub fn failed(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ExitClass::Failed, code, message)
    }

    pub fn internal(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ExitClass::Internal, code, message)
    }

    pub fn remedy(mut self, remedy: impl Into<String>) -> Self {
        self.0.remedy = Some(remedy.into());
        self
    }

    pub fn next(mut self, command: impl Into<String>) -> Self {
        self.0.next.push(command.into());
        self
    }

    pub fn detail(mut self, detail: Value) -> Self {
        self.0.detail = Some(detail);
        self
    }

    pub fn class(&self) -> ExitClass {
        self.0.class
    }

    pub fn code(&self) -> &str {
        &self.0.code
    }

    pub fn message(&self) -> &str {
        &self.0.message
    }

    pub fn remedy_text(&self) -> Option<&str> {
        self.0.remedy.as_deref()
    }

    pub fn next_commands(&self) -> &[String] {
        &self.0.next
    }

    pub fn detail_value(&self) -> Option<&Value> {
        self.0.detail.as_ref()
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.0.code, self.0.message)
    }
}

impl std::error::Error for Failure {}

/// The success envelope, as serialized to stdout under `--output json`.
///
/// Field names are short because they are paid for on every single response,
/// and stable because they are the contract.
#[derive(Debug, Serialize)]
pub struct SuccessEnvelope<'a> {
    /// Envelope version.
    pub v: u32,
    /// Dotted command id, e.g. `network.inspect`.
    pub command: &'a str,
    /// That command's own contract version.
    pub contract: u32,
    pub status: &'static str,
    pub data: Value,
    /// Present only when the result was truncated or a continuation exists.
    /// Absent means "this is all of it" — which is itself information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope<'a> {
    pub v: u32,
    pub command: &'a str,
    pub contract: u32,
    pub status: &'static str,
    pub error: ErrorBody<'a>,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody<'a> {
    /// The exit class token, so a JSON consumer sees what an exit-code
    /// consumer sees.
    pub class: &'static str,
    pub code: &'a str,
    pub message: &'a str,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remedy: Option<&'a str>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    pub next: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<&'a Value>,
}

/// Build the error envelope for a failure. `command` is the dotted id when
/// one was resolved, or the closest thing to it — `ds` itself for a failure
/// that happened before any command was chosen.
pub fn error_envelope<'a>(
    command: &'a str,
    contract: u32,
    failure: &'a Failure,
) -> ErrorEnvelope<'a> {
    ErrorEnvelope {
        v: ENVELOPE_VERSION,
        command,
        contract,
        status: "error",
        error: ErrorBody {
            class: failure.class().token(),
            code: failure.code(),
            message: failure.message(),
            retryable: failure.class().retryable(),
            remedy: failure.remedy_text(),
            next: failure.next_commands(),
            detail: failure.detail_value(),
        },
    }
}

pub fn success_envelope<'a>(command: &'a str, contract: u32, data: Value) -> SuccessEnvelope<'a> {
    SuccessEnvelope {
        v: ENVELOPE_VERSION,
        command,
        contract,
        status: "ok",
        data,
        more: None,
    }
}
