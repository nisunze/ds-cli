//! Shared test helpers.
//!
//! Each integration target compiles this module independently, so a helper
//! used by one target is intentionally unused in another.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// Root-level commands are part of the public surface even though they do not
/// belong to a domain. Keep this one shared list so a fourth command cannot be
/// added to only one of the budget/contract/refusal walkers.
pub const META_COMMANDS: &[&str] = &["capabilities", "doctor", "version"];

pub struct Invocation {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

pub fn invoke(args: &[&str]) -> Invocation {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    Invocation {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

pub fn json(args: &[&str]) -> (Value, i32) {
    let invocation = invoke(args);
    let value = serde_json::from_str(&invocation.stdout).unwrap_or_else(|error| {
        panic!(
            "`ds {}` did not emit JSON ({error}): {}",
            args.join(" "),
            invocation.stdout
        )
    });
    (value, invocation.code)
}

/// The real `.dsgrid` fixture, in the authoritative repository that owns the
/// format.
///
/// It is referenced rather than copied on purpose. A vendored copy would keep
/// passing after the format moved on, which is worse than no test: it would
/// report parity that no longer exists. `ds-cli` already cannot build without
/// `ds-network` on disk — it links its crates by path — so depending on it
/// here adds no new requirement.
pub fn fixture() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
    let path = path.canonicalize().unwrap_or(path);
    assert!(
        path.is_file(),
        "the ds-network fixture is missing at {}. ds-cli links ds-network by \
         path, so the sibling repository is expected to be present.",
        path.display()
    );
    path.display().to_string()
}
