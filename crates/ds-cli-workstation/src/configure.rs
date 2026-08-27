//! `ds workstation configure` — one narrow, conservative settings mutation.

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::detect::{self, Platform};

const TARGET_ARG: Arg = Arg::value(
    "target",
    "<vscode>",
    "Select the one proven Git Bash integration target.",
)
.choices(&["vscode"]);

const SETTINGS_WRITE_FAILED: Refusal = Refusal {
    code: "workstation_settings_write_failed",
    when: "the conservatively merged VS Code settings cannot be persisted",
    remedy: "repair permissions for the reported settings file and retry",
};

pub static COMMAND: Command = Command {
    id: "workstation.configure",
    path: &["workstation", "configure"],
    contract: 1,
    chapter: Chapter::Workstation,
    summary: "Select an existing suitable Git Bash profile in VS Code.",
    purpose: "On native Windows, changes only VS Code's Windows default-profile key after proving that an existing Git Bash profile names the discovered suitable Git for Windows Bash with login/interactive arguments. It preserves unrelated JSONC text and never changes Remote-SSH, Windows Terminal, or DS subprocess behavior.",
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::COMPONENT_ARG, TARGET_ARG],
    output: "A bounded before/after settings receipt, discovered Git Bash path, idempotence result, and explicit untouched integration boundaries.",
    examples: &[Example {
        command: "ds workstation configure --component git-bash --target vscode --yes --output json",
        note: "Native Windows only; the suitable Git Bash profile must already exist.",
        runnable: false,
    }],
    refusals: &[
        crate::COMPONENT_UNKNOWN,
        crate::MUTATION_UNSUPPORTED,
        crate::SETTINGS_UNSAFE,
        SETTINGS_WRITE_FAILED,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a machine settings change",
            remedy: "review `ds workstation plan`, then re-run with --yes",
        },
    ],
    reference: Some("docs/reference/workstation.md"),
    availability: crate::always,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let component_id = inputs.require("component")?;
    if detect::component(component_id).is_none() {
        return Err(Failure::invalid(
            "workstation_component_unknown",
            format!("`{component_id}` is not a governed workstation component"),
        )
        .remedy(crate::COMPONENT_UNKNOWN.remedy));
    }
    if Platform::current() != Platform::Windows
        || component_id != "git-bash"
        || inputs.require("target")? != "vscode"
    {
        return Err(Failure::unavailable(
            "workstation_mutation_unsupported",
            "only existing Git Bash to VS Code configuration is proven on native Windows",
        )
        .remedy(crate::MUTATION_UNSUPPORTED.remedy));
    }

    let component = detect::component("git-bash").expect("validated catalogue component");
    let discovery = detect::snapshot(&component, Platform::Windows, true);
    let git_bash = discovery["path"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| {
            Failure::unavailable(
                "workstation_settings_unsafe",
                "a suitable existing Git for Windows Bash was not found",
            )
            .remedy(crate::SETTINGS_UNSAFE.remedy)
        })?;
    if discovery["state"] != "installed" || !detect::git_bash_is_suitable(&git_bash) {
        return Err(Failure::unavailable(
            "workstation_settings_unsafe",
            "the discovered bash.exe is not proven to belong to Git for Windows",
        )
        .remedy(crate::SETTINGS_UNSAFE.remedy));
    }
    let settings_path = vscode_settings_path().ok_or_else(|| {
        Failure::unavailable(
            "workstation_settings_unsafe",
            "APPDATA is unavailable, so VS Code settings cannot be resolved",
        )
        .remedy(crate::SETTINGS_UNSAFE.remedy)
    })?;
    let original = std::fs::read_to_string(&settings_path).map_err(|error| {
        Failure::unavailable(
            "workstation_settings_unsafe",
            format!("VS Code settings cannot be read: {}", error.kind()),
        )
        .remedy(crate::SETTINGS_UNSAFE.remedy)
    })?;
    if !profile_is_suitable(&original, &git_bash) {
        return Err(Failure::unavailable(
            "workstation_settings_unsafe",
            "the existing VS Code `Git Bash` profile does not name the discovered executable with `--login -i`",
        )
        .remedy(crate::SETTINGS_UNSAFE.remedy));
    }
    let before =
        crate::policy::jsonc_string(&original, "terminal.integrated.defaultProfile.windows");
    let merged =
        crate::policy::merge_vscode_windows_profile(&original, "Git Bash").map_err(|reason| {
            Failure::unavailable("workstation_settings_unsafe", reason)
                .remedy(crate::SETTINGS_UNSAFE.remedy)
        })?;
    let changed = merged != original;
    if changed {
        std::fs::write(&settings_path, merged.as_bytes()).map_err(|error| {
            Failure::failed(
                "workstation_settings_write_failed",
                format!("VS Code settings write failed: {}", error.kind()),
            )
            .remedy(SETTINGS_WRITE_FAILED.remedy)
        })?;
    }
    Ok(json!({
        "component": component_id,
        "target": "vscode",
        "platform": "windows",
        "changed": changed,
        "settings": settings_path.to_string_lossy(),
        "git_bash": git_bash.to_string_lossy(),
        "before": before,
        "after": "Git Bash",
        "preserved": ["unrelated_jsonc", "remote_ssh", "windows_terminal", "ds_subprocess"],
        "temporary_cleanup": [],
    }))
}

fn vscode_settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|root| root.join("Code").join("User").join("settings.json"))
}

fn profile_is_suitable(text: &str, executable: &Path) -> bool {
    let encoded = serde_json::to_string(&executable.to_string_lossy()).unwrap_or_default();
    let escaped_path = encoded
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'));
    profile_object(text, "Git Bash").is_some_and(|profile| {
        escaped_path.is_some_and(|path| profile.contains(path))
            && profile.contains("\"--login\"")
            && profile.contains("\"-i\"")
    })
}

fn profile_object<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let key = serde_json::to_string(name).ok()?;
    let mut offset = 0;
    while let Some(relative) = text[offset..].find(&key) {
        let start = offset + relative + key.len();
        let colon = text[start..].find(':')? + start + 1;
        let object_start = text[colon..]
            .char_indices()
            .find(|(_, character)| !character.is_whitespace())
            .map(|(index, _)| colon + index)?;
        if text.as_bytes().get(object_start) != Some(&b'{') {
            offset = start;
            continue;
        }
        let mut depth = 0_u32;
        let mut in_string = false;
        let mut escaped = false;
        for (index, character) in text[object_start..].char_indices() {
            if in_string {
                if character == '"' && !escaped {
                    in_string = false;
                }
                escaped = character == '\\' && !escaped;
                if character != '\\' {
                    escaped = false;
                }
                continue;
            }
            match character {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(&text[object_start..=object_start + index]);
                    }
                }
                _ => {}
            }
        }
        return None;
    }
    None
}

pub fn render(data: &Value) -> String {
    format!(
        "Git Bash → VS Code · {}\n",
        if data["changed"].as_bool().unwrap_or(false) {
            "configured"
        } else {
            "already configured"
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suitable_profile_requires_exact_executable_and_login_arguments() {
        let path = Path::new(r"C:\Program Files\Git\bin\bash.exe");
        let text = r#"{
          "terminal.integrated.profiles.windows": {
            "Git Bash": {"path": "C:\\Program Files\\Git\\bin\\bash.exe", "args": ["--login", "-i"]}
          }
        }"#;
        assert!(profile_is_suitable(text, path));
        assert!(!profile_is_suitable(
            &text.replace("--login", "--noprofile"),
            path
        ));
        assert!(!profile_is_suitable(text, Path::new(r"C:\Other\bash.exe")));
        let unrelated = r#"{
          "Git Bash": {"path": "C:\\Other\\bash.exe"},
          "Other": {"path": "C:\\Program Files\\Git\\bin\\bash.exe", "args": ["--login", "-i"]}
        }"#;
        assert!(!profile_is_suitable(unrelated, path));
    }
}
