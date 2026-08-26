//! `ds shell` — reaching `ds` from the shells on this machine.
//!
//! The desktop application installs `ds` beside itself, and on Windows that
//! directory is on nobody's PATH: `ds` answers from the app's own command-line
//! launcher and from nowhere else until something registers it. This domain
//! is that something, and it is also how a person finds out what a freshly
//! opened PowerShell, cmd, Bash or Git Bash window will resolve `ds` to.
//!
//! It owns no engineering logic. It reads this process's own PATH, reads the
//! user's durable PATH registration, and writes exactly one entry: the
//! directory this executable lives in. Nothing here needs a project, a
//! session or a network, so every command is `Authority::None`.
//!
//! Two questions are kept apart because they have different answers:
//!
//! * **this shell** — does `ds` on the PATH of the process that ran this
//!   command resolve to this executable? That is what a script running right
//!   now sees.
//! * **new shells** — is this executable's directory registered where a
//!   freshly opened terminal will pick it up? On Windows that is the user's
//!   `HKCU\Environment\Path`; elsewhere it is a `~/.local/bin/ds` link, or a
//!   system directory the package already installed into.
//!
//! The desktop installer runs `ds shell register` after copying files and
//! `ds shell unregister` before removing them, so an ordinary install needs
//! nothing by hand. The commands exist so a source build, a copied binary, or
//! a machine where that installer step failed can be repaired with one typed
//! call — and so `ds doctor` can say which of the two questions is the problem.

pub mod platform;
pub mod reach;
pub mod register;
pub mod status;
pub mod unregister;

use ds_cli_contract::spec::{Availability, Domain, Refusal};
use serde_json::{Value, json};

pub static DOMAIN: Domain = Domain {
    id: "shell",
    summary: "Reach `ds` from any shell: status, register, unregister.",
    commands: &[&status::COMMAND, &register::COMMAND, &unregister::COMMAND],
};

/// Every command here reads the environment and the user's own profile, so
/// there is no prerequisite that could be absent.
pub(crate) fn always() -> Availability {
    Availability::Available
}

pub const EXECUTABLE_UNRESOLVED: Refusal = Refusal {
    code: "executable_unresolved",
    when: "the running ds executable cannot be located on disk",
    remedy: "run ds by its full path, from a directory that still exists",
};

pub const REGISTRATION_UNREADABLE: Refusal = Refusal {
    code: "registration_unreadable",
    when: "the user's PATH registration cannot be read",
    remedy: "check that HKCU\\Environment (Windows) or the home directory is readable by this user",
};

pub const REGISTRATION_UNWRITABLE: Refusal = Refusal {
    code: "registration_unwritable",
    when: "the user's PATH registration cannot be written",
    remedy: "check that HKCU\\Environment (Windows) or ~/.local/bin is writable, then run the command again",
};

pub const LINK_FOREIGN: Refusal = Refusal {
    code: "link_foreign",
    when: "~/.local/bin/ds exists and is not a link to this executable",
    remedy: "move that file aside, then run `ds shell register` again",
};

/// The compact view `ds doctor` embeds: one status word, the executable, and
/// the remedy when a new terminal would not find it. Never a failure — doctor
/// must keep answering when this probe cannot — so an error becomes a
/// `status` of `unknown` with its reason.
pub fn report() -> Value {
    match status::snapshot() {
        Ok(snapshot) => json!({
            "status": status::word(&snapshot),
            "executable": reach::display(&snapshot.reach.executable),
            "remedy": status::remedy(&snapshot),
        }),
        Err(failure) => json!({
            "status": "unknown",
            "reason": failure.message(),
            "remedy": failure.remedy_text(),
        }),
    }
}
