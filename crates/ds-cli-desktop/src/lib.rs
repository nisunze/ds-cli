//! `ds desktop` — the paired DS GridDesign session.
//!
//! The interactive architecture is *paired desktop reuse*, not a second
//! login. When the application is running it already holds a signed-in
//! Firebase session, a selected project and a live map/design context. `ds` borrows
//! that authority over a random-loopback bridge the application publishes,
//! authenticating with the short-lived pairing secret in its private
//! descriptor file.
//!
//! Two invariants make this safe to build on, and both belong to the
//! application rather than to this crate:
//!
//!   * the bridge never returns the Firebase JWT or a refresh token, so no
//!     credential can become a CLI argument, a log line or an agent's
//!     context. Where `ds` needs authenticated work done, it asks the
//!     application to *do* it — `bridge::invoke` returns an outcome, never a
//!     credential — so the call that needs the JWT is made by the process
//!     that already holds it;
//!   * the bridge accepts only a closed set of named semantic operations, so
//!     possession of the descriptor buys the ability to ask the application
//!     to do a known thing — never the ability to run arbitrary code inside
//!     it.
//!
//! Possession of the descriptor is therefore a *transport* proof and nothing
//! more. It says a process on this machine may talk to the app. It does not
//! say who is asking, and it can never authorize a project write on its own.

pub mod bridge;
pub mod connectivity;
pub mod discover;
pub mod ops;
pub mod project;
pub mod status;

use ds_cli_contract::spec::Domain;

pub static DOMAIN: Domain = Domain {
    id: "desktop",
    summary: "Paired DS GridDesign: pairing, project, active context.",
    commands: &[
        &connectivity::STATUS,
        &connectivity::SET,
        &status::COMMAND,
        &project::LIST_COMMAND,
        &project::SWITCH_COMMAND,
    ],
};
