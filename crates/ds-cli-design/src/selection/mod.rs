//! `ds design selection` — a named Transformer Status selection.
//!
//! ```text
//!   list → read → save | archive | assign
//! ```
//!
//! `read` is the load-bearing step. It EVALUATES membership server-side and
//! reports each member as `present`, `changed` or `missing` — a member whose
//! transformer no longer exists is named under the label it was saved with,
//! never substituted. It also returns the member digest, which `assign` echoes
//! back: that echo is what proves the operator saw the exact set being assigned,
//! and a promotion whose membership moved in between is refused rather than
//! quietly assigning a different set of work.

pub mod archive;
pub mod assign;
pub mod list;
pub mod read;
pub mod save;
