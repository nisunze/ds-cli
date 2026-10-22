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

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The envelope version. Raised only for a breaking change to the envelope
/// itself, never for a change inside `data`.
pub const ENVELOPE_VERSION: u32 = 1;

pub use crate::{ExitClass, Failure};

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
            retryable: failure.retryable(),
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

/// The class of a refusal, in the terms a caller can act on. The token is what
/// the JSON `error.class` carries and what `ds` maps to its exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// A defect in the command's own implementation. Never expected; report it.
    Internal,
    /// The invocation was wrong: unknown command or argument, missing required
    /// input, unparseable value, input outside its bound.
    InvalidInput,
    /// The command is real but cannot run here: an engine, an external tool, a
    /// data asset or the desktop is missing. Retry after fixing the remedy.
    Unavailable,
    /// No verified principal, or the principal may not do this.
    Unauthorized,
    /// The caller's view of the world is stale, or two effects collided.
    Conflict,
    /// The command ran and failed. The work did not happen.
    Failed,
}

impl Class {
    /// Every class, in the order `ds` numbers its exit codes (1 through 6).
    pub const ALL: [Class; 6] = [
        Class::Internal,
        Class::InvalidInput,
        Class::Unavailable,
        Class::Unauthorized,
        Class::Conflict,
        Class::Failed,
    ];

    /// The stable snake_case token the envelope carries as `error.class`.
    pub const fn token(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::InvalidInput => "invalid_input",
            Self::Unavailable => "unavailable",
            Self::Unauthorized => "unauthorized",
            Self::Conflict => "conflict",
            Self::Failed => "failed",
        }
    }

    /// The class a token names, when it names one.
    pub fn from_token(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.token() == token)
    }

    /// Whether an unchanged retry could plausibly succeed. `unavailable` and
    /// `conflict` say yes because the world, not the request, is what has to
    /// change.
    pub const fn retryable(self) -> bool {
        matches!(self, Self::Unavailable | Self::Conflict)
    }
}

/// A refusal, in the terms a caller can act on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub class: Class,
    /// Stable, machine-matchable, snake_case. Never localized, never reworded.
    /// This is the field a script branches on.
    pub code: String,
    /// One sentence. No path echo beyond what the caller supplied, no
    /// credential-shaped text, no stack trace.
    pub message: String,
    /// The concrete thing that would fix it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
    /// Commands worth running next. Ordered most-useful first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
    /// Bounded structured context, when a code alone is not enough — for
    /// example the accepted values of a rejected choice, or the message key
    /// and params a localised surface renders instead of `message`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

impl Refusal {
    /// A refusal with only the three fields every refusal has.
    pub fn new(class: Class, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            class,
            code: code.into(),
            message: message.into(),
            remedy: None,
            next: Vec::new(),
            detail: None,
        }
    }

    /// Name the concrete thing that would fix it.
    pub fn remedy(mut self, remedy: impl Into<String>) -> Self {
        self.remedy = Some(remedy.into());
        self
    }

    /// Add one command worth running next, after those already named.
    pub fn next(mut self, command: impl Into<String>) -> Self {
        self.next.push(command.into());
        self
    }

    /// Attach the bounded structured context.
    pub fn detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

/// Owned wire refusal. This is the exact error body emitted by the borrowed
/// serializer above, retained without rewriting the owner's class or advice.
#[derive(Clone, Debug, serde::Deserialize, Serialize, PartialEq)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(hashmap_as_object)
)]
pub struct CommandRefusal {
    pub class: String,
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_detail"
    )]
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "unknown")
    )]
    pub detail: Option<Value>,
}

fn present_detail<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(decoder).map(Some)
}

impl CommandRefusal {
    pub fn from_failure(failure: &Failure) -> Self {
        Self {
            class: failure.class().token().into(),
            code: failure.code().into(),
            message: failure.message().into(),
            retryable: failure.retryable(),
            remedy: failure.remedy_text().map(str::to_owned),
            next: failure.next_commands().to_vec(),
            detail: failure.detail_value().cloned(),
        }
    }

    pub fn into_failure(self) -> Result<Failure, &'static str> {
        self.validate()?;
        let class = match Class::from_token(&self.class).expect("validated refusal class") {
            Class::Internal => ExitClass::Internal,
            Class::InvalidInput => ExitClass::InvalidInput,
            Class::Unavailable => ExitClass::Unavailable,
            Class::Unauthorized => ExitClass::Unauthorized,
            Class::Conflict => ExitClass::Conflict,
            Class::Failed => ExitClass::Failed,
        };
        Ok(Failure(Box::new(crate::Refusal {
            class,
            code: self.code,
            message: self.message,
            remedy: self.remedy,
            next: self.next,
            detail: self.detail,
            retryable: Some(self.retryable),
        })))
    }

    /// Bound the fields clients consume. Unknown additive fields are admitted
    /// by serde; incompatible required fields remain a decoding error.
    pub fn validate(&self) -> Result<(), &'static str> {
        if Class::from_token(&self.class).is_none()
            || self.code.is_empty()
            || self.code.len() > 128
            || !self
                .code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || self.message.is_empty()
            || self.message.len() > 4096
            || self.remedy.as_ref().is_some_and(|s| s.len() > 65536)
            || self.next.len() > 64
            || self.next.iter().any(|s| s.len() > 4096)
            || self
                .detail
                .as_ref()
                .is_some_and(|v| serde_json::to_vec(v).map_or(true, |b| b.len() > 65536))
        {
            return Err("malformed or unbounded command refusal");
        }
        Ok(())
    }
}

impl std::fmt::Display for CommandRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for CommandRefusal {}

/// The owned command response clients decode. Version/contract values remain
/// provenance; compatibility is decided from the required fields actually read.
/// `files` is the host's existing declared-file handoff, never a second command.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(hashmap_as_object)
)]
#[serde(tag = "status")]
pub enum CommandEnvelope<T> {
    #[serde(rename = "ok")]
    Ok {
        v: u64,
        command: String,
        contract: u64,
        data: T,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(
            all(feature = "bindings", target_arch = "wasm32"),
            tsify(type = "unknown")
        )]
        more: Option<Value>,
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        #[cfg_attr(
            all(feature = "bindings", target_arch = "wasm32"),
            tsify(type = "Record<string, unknown>")
        )]
        files: std::collections::BTreeMap<String, Value>,
    },
    #[serde(rename = "error")]
    Error {
        v: u64,
        command: String,
        contract: u64,
        error: CommandRefusal,
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        #[cfg_attr(
            all(feature = "bindings", target_arch = "wasm32"),
            tsify(type = "Record<string, unknown>")
        )]
        files: std::collections::BTreeMap<String, Value>,
    },
}

impl<T> CommandEnvelope<T> {
    pub fn command(&self) -> &str {
        match self {
            Self::Ok { command, .. } | Self::Error { command, .. } => command,
        }
    }
}

#[cfg(test)]
#[path = "envelope_tests.rs"]
mod tests;

/// Kernel and host producers use the same serializer as the CLI.
pub fn success(command: &str, contract: u32, data: Value) -> Value {
    serde_json::to_value(success_envelope(command, contract, data))
        .expect("command success envelope serializes")
}

pub fn error(command: &str, contract: u32, refusal: &Refusal) -> Value {
    let class = match refusal.class {
        Class::Internal => ExitClass::Internal,
        Class::InvalidInput => ExitClass::InvalidInput,
        Class::Unavailable => ExitClass::Unavailable,
        Class::Unauthorized => ExitClass::Unauthorized,
        Class::Conflict => ExitClass::Conflict,
        Class::Failed => ExitClass::Failed,
    };
    let mut failure = Failure::new(class, &refusal.code, &refusal.message);
    if let Some(remedy) = &refusal.remedy {
        failure = failure.remedy(remedy);
    }
    for next in &refusal.next {
        failure = failure.next(next);
    }
    if let Some(detail) = &refusal.detail {
        failure = failure.detail(detail.clone());
    }
    serde_json::to_value(error_envelope(command, contract, &failure))
        .expect("command refusal envelope serializes")
}
