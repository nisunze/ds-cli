//! `ds design comment` — append-only threads on a design object.
//!
//! ```text
//!   list → read → post | resolve | promote | redact
//! ```
//!
//! Comments are append-only. There is no `edit`, because there is no edit
//! action: removing text is a moderator's audited redaction. `redact` is fenced
//! on the thread version the moderator read the comment at, so what is removed
//! is what was read — the reason redaction once stayed in the application. A
//! redacted comment keeps its author, its place in the sequence and its time;
//! `read` reports it as redacted rather than showing an empty body.

pub mod list;
pub mod post;
pub mod promote;
pub mod read;
pub mod redact;
pub mod resolve;
