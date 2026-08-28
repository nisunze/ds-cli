//! `ds design comment` — append-only threads on a design object.
//!
//! ```text
//!   list → read → post | resolve | promote
//! ```
//!
//! Comments are append-only. There is no `edit`, because there is no edit
//! action: removing text is a moderator's audited redaction, which stays in the
//! application where the moderator can read what they are removing first. A
//! redacted comment keeps its author, its place in the sequence and its time;
//! `read` reports it as redacted rather than showing an empty body.

pub mod list;
pub mod post;
pub mod promote;
pub mod read;
pub mod resolve;
