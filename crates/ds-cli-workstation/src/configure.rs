//! `ds workstation configure` — one narrow, conservative settings mutation.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::detect::{self, Platform};

const TARGET_ARG: Arg = Arg::value(
    "target",
    "<vscode|reporter>",
    "Select the existing Git Bash VS Code profile or a verified browser for local report printing.",
)
.choices(&["vscode", "reporter"]);

const SETTINGS_WRITE_FAILED: Refusal = Refusal {
    code: "workstation_settings_write_failed",
    when: "the verified browser selection or conservatively merged VS Code settings cannot be persisted",
    remedy: "repair permissions for the reported settings file and retry",
};

pub static COMMAND: Command = Command {
    id: "workstation.configure",
    path: &["workstation", "configure"],
    contract: 1,
    chapter: Chapter::Workstation,
    summary: "Persist one verified existing workstation integration.",
    purpose: "Selects an existing suitable Git Bash profile in VS Code on Windows, or verifies an existing Chrome/Edge/Chromium browser by printing a task-owned PDF and persists its exact executable path for local reports. It installs no browser and preserves unrelated settings.",
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::COMPONENT_ARG, TARGET_ARG],
    output: "A bounded before/after settings receipt, verified executable path, functional smoke, and idempotence result.",
    examples: &[
        Example {
            command: "ds workstation configure --component git-bash --target vscode --yes --output json",
            note: "Native Windows only; the suitable Git Bash profile must already exist.",
            runnable: false,
        },
        Example {
            command: "ds workstation configure --component chromium --target reporter --yes --output json",
            note: "Verify an existing Chrome/Edge/Chromium and record its path; no browser installation.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::COMPONENT_UNKNOWN,
        crate::MUTATION_UNSUPPORTED,
        crate::SETTINGS_UNSAFE,
        crate::VERIFICATION_FAILED,
        SETTINGS_WRITE_FAILED,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a machine settings change",
            remedy: "review `ds workstation plan`, then re-run with --yes",
        },
    ],
    reference: Some("docs/reference/workstation.md"),
    search: &[],
    requires: Requires::Server,
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
    let target = inputs.require("target")?;
    if component_id == "chromium" && target == "reporter" {
        let platform = Platform::current();
        let component = detect::component("chromium").expect("validated catalogue component");
        let discovery = detect::snapshot(&component, platform, true);
        let path = discovery["path"].as_str().ok_or_else(|| {
            Failure::unavailable(
                "workstation_settings_unsafe",
                "no existing Chrome, Edge, or Chromium executable was found",
            )
            .remedy("expose an existing browser on this workstation, or set DS_VD_CHROME to its executable path; then retry this configure command")
            .detail(json!({"discovery": discovery}))
        })?;
        if discovery["state"] != "installed" {
            return Err(Failure::unavailable(
                "workstation_settings_unsafe",
                "the discovered browser did not pass executable/version inspection",
            )
            .remedy("run `ds workstation verify --component chromium --output json`, repair the browser, and retry")
            .detail(json!({"discovery": discovery})));
        }
        let settings = crate::policy::browser_selection_path(platform).ok_or_else(|| {
            Failure::unavailable(
                "workstation_settings_unsafe",
                "the local DS component directory cannot be resolved",
            )
            .remedy("set the local user data directory or DS_WORKSTATION_COMPONENT_ROOT and retry")
        })?;
        return configure_chromium_path(platform, Path::new(path), &settings);
    }
    if Platform::current() != Platform::Windows || component_id != "git-bash" || target != "vscode"
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

fn configure_chromium_path(
    platform: Platform,
    path: &Path,
    settings: &Path,
) -> Result<Value, Failure> {
    if !path.is_absolute() || !path.is_file() {
        return Err(Failure::invalid(
            "workstation_settings_unsafe",
            "browser path must name an existing absolute executable",
        )
        .remedy("set DS_VD_CHROME to the exact installed browser executable and retry"));
    }
    let version = detect::version(path, "chromium").map_err(|reason| {
        Failure::failed("workstation_verification_failed", reason)
            .remedy("repair the browser executable and retry")
    })?;
    if !detect::chromium_version_is_supported(&version) {
        return Err(Failure::failed(
            "workstation_verification_failed",
            "the executable did not identify itself as Chrome, Edge, or Chromium",
        )
        .remedy("set DS_VD_CHROME to an existing Chrome, Edge, or Chromium executable"));
    }
    let smoke = crate::verify::chromium_smoke(path, platform).map_err(|reason| {
        Failure::failed("workstation_verification_failed", reason)
            .remedy("repair or choose another installed browser, then rerun this configure command")
    })?;
    let before = crate::policy::read_browser_selection_at(settings);
    let previous = before.as_ref().ok().and_then(|value| value.as_ref());
    let executable = path.to_string_lossy().into_owned();
    let changed = !previous.is_some_and(|value| {
        value.executable == executable && value.version == version && value.pdf_smoke
    });
    if changed {
        let receipt = crate::policy::BrowserSelection {
            schema: crate::policy::BROWSER_SELECTION_SCHEMA.to_string(),
            component: "chromium".to_string(),
            executable: executable.clone(),
            version: version.clone(),
            verified_at_unix_s: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            pdf_smoke: true,
            preexisting: true,
        };
        crate::policy::write_browser_selection_at(settings, &receipt).map_err(|reason| {
            Failure::failed("workstation_settings_write_failed", reason)
                .remedy("repair permissions for the reported DS browser selection file and retry")
        })?;
    }
    Ok(json!({
        "component": "chromium",
        "target": "reporter",
        "platform": platform.token(),
        "changed": changed,
        "settings": settings.to_string_lossy(),
        "executable": executable,
        "version": version,
        "verification": smoke,
        "before": previous,
        "after": crate::policy::read_browser_selection_at(settings).ok().flatten(),
        "browser_preexisting": true,
        "browser_task_owned": false,
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
    if data["component"] == "chromium" {
        return format!(
            "report browser · {} · {}\n",
            if data["changed"].as_bool().unwrap_or(false) {
                "configured"
            } else {
                "already configured"
            },
            data["executable"].as_str().unwrap_or("?")
        );
    }
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

    #[cfg(unix)]
    fn fake_browser(script: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ds-workstation-browser-configure-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let browser = root.join("fake-browser");
        std::fs::write(&browser, script).unwrap();
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o700)).unwrap();
        (root, browser)
    }

    #[cfg(unix)]
    #[test]
    fn existing_browser_is_pdf_proven_persisted_once_and_never_owned() {
        let (root, browser) = fake_browser(
            "#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo 'Microsoft Edge 153.0.8010.12'; exit 0; fi\nfor arg in \"$@\"; do case \"$arg\" in --headless) headless=1;; --user-data-dir=*) private_profile=1;; --print-to-pdf=*) pdf=${arg#--print-to-pdf=};; esac; done\n[ \"$headless\" = 1 ] && [ \"$private_profile\" = 1 ] && [ -n \"$pdf\" ] || exit 2\nprintf '%%PDF-1.4\\n' > \"$pdf\"\nprintf '%01000d' 0 >> \"$pdf\"\n",
        );
        let settings = root.join("ds/chromium/browser-selection.json");
        let first = configure_chromium_path(Platform::Linux, &browser, &settings).unwrap();
        assert_eq!(first["changed"], true);
        assert_eq!(first["verification"]["passed"], true);
        assert_eq!(first["browser_preexisting"], true);
        assert_eq!(first["browser_task_owned"], false);
        let receipt = crate::policy::read_browser_selection_at(&settings)
            .unwrap()
            .unwrap();
        assert_eq!(receipt.schema, crate::policy::BROWSER_SELECTION_SCHEMA);
        assert_eq!(receipt.executable, browser.to_string_lossy());
        assert!(receipt.pdf_smoke && receipt.preexisting);
        let bytes_before = std::fs::read(&settings).unwrap();
        let second = configure_chromium_path(Platform::Linux, &browser, &settings).unwrap();
        assert_eq!(second["changed"], false);
        assert_eq!(std::fs::read(&settings).unwrap(), bytes_before);
        let updated_script = std::fs::read_to_string(&browser)
            .unwrap()
            .replace("153.0.8010.12", "154.0.8037.0");
        std::fs::write(&browser, updated_script).unwrap();
        let updated = configure_chromium_path(Platform::Linux, &browser, &settings).unwrap();
        assert_eq!(updated["changed"], true);
        let updated_receipt = crate::policy::read_browser_selection_at(&settings)
            .unwrap()
            .unwrap();
        assert_eq!(updated_receipt.version, "Microsoft Edge 154.0.8037.0");
        std::fs::write(&settings, b"{broken").unwrap();
        assert!(crate::policy::read_browser_selection_at(&settings).is_err());
        let repaired = configure_chromium_path(Platform::Linux, &browser, &settings).unwrap();
        assert_eq!(repaired["changed"], true);
        assert!(
            crate::policy::read_browser_selection_at(&settings)
                .unwrap()
                .is_some()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn failed_pdf_smoke_cannot_persist_a_browser_selection() {
        let (root, browser) = fake_browser(
            "#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo 'Google Chrome 153.0.8010.12'; fi\n",
        );
        let settings = root.join("ds/chromium/browser-selection.json");
        assert!(configure_chromium_path(Platform::Linux, &browser, &settings).is_err());
        assert!(!settings.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

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
