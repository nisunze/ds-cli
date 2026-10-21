//! The DS envelope, version 1: the one answer shape every DS command gives
//! its external clients, success or refusal.
//!
//! `ds` prints it on stdout under `--output json` and the desktop shell hands
//! it to the webview; both are external clients issuing the same command ids
//! with the same argument names, so they receive the same envelope. The shape
//! is the one `ds-cli-contract/src/outcome.rs` serialises
//! (`SuccessEnvelope`/`ErrorEnvelope`), stated here in plain `serde_json`
//! because the shell's release graph cannot link ds-cli and the answer shape
//! belongs to the core, not to one client. `tests/envelope_cases.rs` replays
//! the corpus `tests/fixtures/ds-envelope-cases.json` through this module and
//! ds-cli replays the same corpus through its own definition, so the two
//! cannot drift.
//!
//! An error is short on purpose: what failed, whether an unchanged retry could
//! help, what would fix it and which command to run now. Nothing here
//! exports to WASM; the envelope is spoken between processes, not inside the
//! browser.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The envelope version. Raised only for a breaking change to the envelope
/// itself, never for a change inside `data`.
pub const VERSION: u32 = 1;

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

/// The head every envelope opens with: `v`, `command`, `contract`, `status`.
fn head(command: &str, contract: u32, status: &str) -> Map<String, Value> {
    let mut envelope = Map::new();
    envelope.insert("v".to_owned(), Value::from(VERSION));
    envelope.insert("command".to_owned(), Value::from(command));
    envelope.insert("contract".to_owned(), Value::from(contract));
    envelope.insert("status".to_owned(), Value::from(status));
    envelope
}

/// The success envelope: `{v, command, contract, status: "ok", data}`.
/// `command` is the dotted id, `contract` that command's own contract version.
pub fn success(command: &str, contract: u32, data: Value) -> Value {
    let mut envelope = head(command, contract, "ok");
    envelope.insert("data".to_owned(), data);
    Value::Object(envelope)
}

/// The error envelope: `{v, command, contract, status: "error", error: {class,
/// code, message, retryable, remedy?, next?, detail?}}`. `remedy` and `detail`
/// are present only when the refusal carries them and `next` only when it
/// names at least one command, so an absent member means "nothing to say",
/// which is itself information. `command` is the dotted id when one was
/// resolved, or the closest thing to it.
pub fn error(command: &str, contract: u32, refusal: &Refusal) -> Value {
    let mut body = Map::new();
    body.insert("class".to_owned(), Value::from(refusal.class.token()));
    body.insert("code".to_owned(), Value::from(refusal.code.as_str()));
    body.insert("message".to_owned(), Value::from(refusal.message.as_str()));
    body.insert(
        "retryable".to_owned(),
        Value::from(refusal.class.retryable()),
    );
    if let Some(remedy) = &refusal.remedy {
        body.insert("remedy".to_owned(), Value::from(remedy.as_str()));
    }
    if !refusal.next.is_empty() {
        body.insert("next".to_owned(), Value::from(refusal.next.clone()));
    }
    if let Some(detail) = &refusal.detail {
        body.insert("detail".to_owned(), detail.clone());
    }
    let mut envelope = head(command, contract, "error");
    envelope.insert("error".to_owned(), Value::Object(body));
    Value::Object(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_class_has_one_token_and_the_token_names_it_back() {
        let mut tokens = std::collections::BTreeSet::new();
        for class in Class::ALL {
            let token = class.token();
            assert!(tokens.insert(token), "{token} names two classes");
            assert_eq!(Class::from_token(token), Some(class));
            assert_eq!(serde_json::to_value(class).expect("encodes"), json!(token));
        }
        assert_eq!(
            tokens.into_iter().collect::<Vec<_>>(),
            [
                "conflict",
                "failed",
                "internal",
                "invalid_input",
                "unauthorized",
                "unavailable",
            ]
        );
        assert_eq!(Class::from_token("ok"), None);
        assert_eq!(Class::from_token("InvalidInput"), None);
    }

    #[test]
    fn only_unavailable_and_conflict_are_retryable() {
        let retryable: Vec<_> = Class::ALL
            .into_iter()
            .filter(|class| class.retryable())
            .collect();
        assert_eq!(retryable, [Class::Unavailable, Class::Conflict]);
    }

    #[test]
    fn a_success_envelope_carries_exactly_five_members_in_order() {
        let envelope = success("report.project.export", 1, json!({"results": []}));
        assert_eq!(
            serde_json::to_string(&envelope).expect("encodes"),
            r#"{"v":1,"command":"report.project.export","contract":1,"status":"ok","data":{"results":[]}}"#
        );
    }

    #[test]
    fn an_error_envelope_omits_what_the_refusal_does_not_carry() {
        let bare = Refusal::new(Class::Internal, "kernel_defect", "the kernel panicked");
        assert_eq!(
            serde_json::to_string(&error("ds", 0, &bare)).expect("encodes"),
            r#"{"v":1,"command":"ds","contract":0,"status":"error","error":{"class":"internal","code":"kernel_defect","message":"the kernel panicked","retryable":false}}"#
        );
        let full = Refusal::new(
            Class::Unavailable,
            "reporter_engine_missing",
            "the ds-report engine is not installed",
        )
        .remedy("install the native profile")
        .next("ds doctor")
        .next("ds report project export --transformer T --preview-layout draft.json")
        .detail(json!({"engine": "ds-report"}));
        assert_eq!(
            serde_json::to_string(&error("report.project.export", 1, &full)).expect("encodes"),
            r#"{"v":1,"command":"report.project.export","contract":1,"status":"error","error":{"class":"unavailable","code":"reporter_engine_missing","message":"the ds-report engine is not installed","retryable":true,"remedy":"install the native profile","next":["ds doctor","ds report project export --transformer T --preview-layout draft.json"],"detail":{"engine":"ds-report"}}}"#
        );
    }

    #[test]
    fn a_refusal_reads_back_from_its_own_json_and_refuses_a_stranger_member() {
        let refusal: Refusal = serde_json::from_value(json!({
            "class": "conflict",
            "code": "desktop_project_not_open",
            "message": "the desktop has another project open",
            "next": ["ds project select P"]
        }))
        .expect("decodes");
        assert_eq!(refusal.class, Class::Conflict);
        assert_eq!(refusal.remedy, None);
        assert_eq!(refusal.next, ["ds project select P"]);
        assert_eq!(refusal.detail, None);
        assert!(serde_json::from_value::<Refusal>(json!({
            "class": "failed",
            "code": "x",
            "message": "y",
            "retryable": true
        }))
        .is_err());
    }
}
