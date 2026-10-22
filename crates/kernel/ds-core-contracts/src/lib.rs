//! Shared application boundary types consumed directly by native hosts and CLI adapters.

use serde_json::Value;
use std::fmt;

pub mod envelope;
pub use envelope::{CommandEnvelope, CommandRefusal};

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
    /// Set when the class default is wrong for this refusal.
    pub retryable: Option<bool>,
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
            retryable: None,
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

    /// Replace the sentence while keeping the class, code, remedy and next
    /// step a shared mapping already chose.
    ///
    /// An adapter can add the exact HTTP status and route words it learns
    /// after classifying a refusal. A compatibility alias can also label that
    /// refusal without changing its code, remedy or detail.
    /// Rebuilding the failure to say so would hand a constructor a code it
    /// computed, which `tests/refusal_coverage.rs` cannot read and must
    /// therefore refuse; this keeps the code a literal at the site that owns
    /// it.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.0.message = message.into();
        self
    }

    /// Mark a refusal whose unchanged retry cannot succeed even though its
    /// class usually allows one: a conflict whose remedy is to re-read the
    /// world and decide again, not to repeat the call.
    pub fn not_retryable(mut self) -> Self {
        self.0.retryable = Some(false);
        self
    }

    pub fn class(&self) -> ExitClass {
        self.0.class
    }

    /// Whether repeating the call unchanged could plausibly succeed.
    pub fn retryable(&self) -> bool {
        self.0.retryable.unwrap_or_else(|| self.0.class.retryable())
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

/// Whether a command can run on this machine right now, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// A concrete missing prerequisite, with the concrete thing to do about
    /// it. A reason without a remedy is a dead end for an agent.
    ///
    /// `code` is the domain's own stable identifier for this prerequisite —
    /// not a generic "unavailable". Dispatch refuses with it verbatim, so a
    /// caller branching on `error.code` sees the same value whether the
    /// prerequisite was caught by the gate or by the command itself.
    Unavailable {
        code: &'static str,
        reason: String,
        remedy: String,
    },
}

impl Availability {
    pub fn unavailable(
        code: &'static str,
        reason: impl Into<String>,
        remedy: impl Into<String>,
    ) -> Self {
        Self::Unavailable {
            code,
            reason: reason.into(),
            remedy: remedy.into(),
        }
    }

    pub const fn token(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable { .. } => "unavailable",
        }
    }

    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}
