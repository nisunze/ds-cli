//! Where one account's machine-local DS state lives.
//!
//! An explicit `XDG_STATE_HOME` wins on every platform. Otherwise Unix uses
//! `$HOME/.local/state`, and Windows uses `%LOCALAPPDATA%` and never `HOME`:
//! a POSIX shell such as Git Bash exports `HOME`, while the background Core,
//! the Desktop and PowerShell do not, so honouring it would split one
//! account's Core state across two directories ("no Core answers"). The host
//! reads the environment and passes it in; this decision stays pure.

use std::{ffi::OsString, path::PathBuf};

/// The per-user state base, or `None` when no usable root is available.
pub fn state_base(
    xdg_state_home: Option<OsString>,
    home: Option<OsString>,
    local_app_data: Option<OsString>,
    windows: bool,
) -> Option<PathBuf> {
    let set = |value: Option<OsString>| value.filter(|value| !value.is_empty());
    if let Some(explicit) = set(xdg_state_home) {
        return Some(PathBuf::from(explicit));
    }
    if windows {
        set(local_app_data).map(PathBuf::from)
    } else {
        set(home).map(|home| PathBuf::from(home).join(".local").join("state"))
    }
}

#[cfg(test)]
mod tests {
    use super::state_base;
    use std::path::PathBuf;

    #[test]
    fn windows_ignores_a_posix_shell_home_but_keeps_an_explicit_override() {
        let home = Some("/c/Users/eng".into());
        let local = Some(r"C:\Users\eng\AppData\Local".into());
        assert_eq!(
            state_base(None, home.clone(), local.clone(), true),
            Some(PathBuf::from(r"C:\Users\eng\AppData\Local"))
        );
        assert_eq!(
            state_base(Some(r"D:\isolated".into()), home, local, true),
            Some(PathBuf::from(r"D:\isolated"))
        );
        assert_eq!(state_base(None, Some("/c/Users/eng".into()), None, true), None);
    }

    #[test]
    fn unix_resolution_is_unchanged() {
        assert_eq!(
            state_base(None, Some("/home/eng".into()), None, false),
            Some(PathBuf::from("/home/eng/.local/state"))
        );
        assert_eq!(
            state_base(Some("/state".into()), Some("/home/eng".into()), None, false),
            Some(PathBuf::from("/state"))
        );
        assert_eq!(state_base(Some("".into()), None, None, false), None);
    }
}
