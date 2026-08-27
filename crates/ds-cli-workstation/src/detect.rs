use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Macos,
    Linux,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }

    pub const fn token(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Component {
    pub id: String,
    pub required: bool,
    pub purpose: String,
    pub provenance: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub linux_executables: Vec<String>,
    pub macos_executables: Vec<String>,
    pub windows_executables: Vec<String>,
    pub linux_plan: Vec<String>,
    pub macos_plan: Vec<String>,
    pub windows_plan: Vec<String>,
}

impl Component {
    pub fn executables(&self, platform: Platform) -> &[String] {
        match platform {
            Platform::Windows => &self.windows_executables,
            Platform::Macos => &self.macos_executables,
            Platform::Linux => &self.linux_executables,
        }
    }

    pub fn plan(&self, platform: Platform) -> &[String] {
        match platform {
            Platform::Windows => &self.windows_plan,
            Platform::Macos => &self.macos_plan,
            Platform::Linux => &self.linux_plan,
        }
    }
}

pub fn catalog() -> Vec<Component> {
    serde_json::from_str(include_str!("components.json"))
        .expect("the bundled workstation component catalogue is valid")
}

pub fn component(id: &str) -> Option<Component> {
    catalog().into_iter().find(|component| component.id == id)
}

pub fn path_directories(path: Option<OsString>) -> Vec<PathBuf> {
    path.map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default()
}

pub fn find_in_directories(names: &[String], directories: &[PathBuf]) -> Option<PathBuf> {
    directories.iter().find_map(|directory| {
        names
            .iter()
            .map(|name| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

pub fn find(component: &Component, platform: Platform) -> Option<PathBuf> {
    find_in_directories(
        component.executables(platform),
        &path_directories(std::env::var_os("PATH")),
    )
    .or_else(|| {
        conventional_locations(&component.id, platform)
            .into_iter()
            .find(|path| path.is_file())
    })
}

fn conventional_locations(component: &str, platform: Platform) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    match platform {
        Platform::Windows => {
            let program_files = [
                std::env::var_os("ProgramFiles"),
                std::env::var_os("ProgramFiles(x86)"),
            ]
            .into_iter()
            .flatten()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
            match component {
                "libreoffice" => {
                    candidates.extend(
                        program_files.iter().map(|root| {
                            root.join("LibreOffice").join("program").join("soffice.exe")
                        }),
                    );
                }
                "git-bash" => {
                    candidates.extend(
                        program_files
                            .iter()
                            .map(|root| root.join("Git").join("bin").join("bash.exe")),
                    );
                    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                        candidates.push(
                            PathBuf::from(local)
                                .join("Programs")
                                .join("Git")
                                .join("bin")
                                .join("bash.exe"),
                        );
                    }
                }
                "qgis" => {
                    for root in program_files {
                        if let Ok(entries) = std::fs::read_dir(root) {
                            for entry in entries.flatten() {
                                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                                if name == "qgis" || name.starts_with("qgis ") {
                                    candidates.push(entry.path().join("bin").join("qgis-bin.exe"));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Platform::Macos => match component {
            "libreoffice" => candidates.push(PathBuf::from(
                "/Applications/LibreOffice.app/Contents/MacOS/soffice",
            )),
            "qgis" => candidates.push(PathBuf::from("/Applications/QGIS.app/Contents/MacOS/QGIS")),
            _ => {}
        },
        Platform::Linux => {}
    }
    candidates
}

pub fn version(path: &Path, component: &str) -> Result<String, String> {
    let args: &[&str] = match component {
        "libreoffice" => &["--headless", "--version"],
        "qgis" | "git-bash" | "git" => &["--version"],
        _ => return Err("this component has no executable version probe".to_string()),
    };
    let mut child = ProcessCommand::new(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start version probe: {}", error.kind()))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("version probe timed out after 10 seconds".to_string());
            }
            Err(error) => {
                let _ = child.kill();
                return Err(format!(
                    "could not wait for version probe: {}",
                    error.kind()
                ));
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("could not collect version probe: {}", error.kind()))?;
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).into_owned()
    } else {
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    if !output.status.success() || line.is_empty() {
        return Err(format!(
            "version probe exited {}",
            output.status.code().unwrap_or(-1)
        ));
    }
    Ok(line.chars().take(300).collect())
}

pub fn snapshot(component: &Component, platform: Platform, probe_version: bool) -> Value {
    if component.id == "git-bash" && platform != Platform::Windows {
        return json!({
            "id": component.id,
            "required": component.required,
            "purpose": component.purpose,
            "state": "not_applicable",
            "reason": "Git Bash is a Git for Windows component; use the platform's native shell",
            "path": null,
            "version": null,
        });
    }
    if component.id == "rwanda-reference" {
        return crate::policy::reference_component_snapshot(component, platform);
    }
    let found = find(component, platform);
    let (version, probe_error) = match (&found, probe_version) {
        (Some(path), true) => match version(path, &component.id) {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(error)),
        },
        _ => (None, None),
    };
    let suitable = if component.id == "git-bash" {
        found.as_deref().is_some_and(git_bash_is_suitable)
    } else {
        true
    };
    json!({
        "id": component.id,
        "required": component.required,
        "purpose": component.purpose,
        "state": if found.is_none() { "absent" } else if suitable { "installed" } else { "variant_unverified" },
        "path": found.as_deref().map(|path| path.to_string_lossy().into_owned()),
        "version": version,
        "probe_error": probe_error,
        "suitable": found.as_ref().map(|_| suitable),
        "ownership": crate::policy::install_ownership(platform, &component.id),
    })
}

pub(crate) fn git_bash_is_suitable(path: &Path) -> bool {
    if !path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("bash.exe"))
    {
        return false;
    }
    let Some(root) = path.parent().and_then(Path::parent) else {
        return false;
    };
    root.join("cmd").join("git.exe").is_file()
        || root.join("mingw64").join("bin").join("git.exe").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_is_unique_and_complete() {
        let catalog = catalog();
        assert_eq!(catalog.len(), 4);
        for (index, component) in catalog.iter().enumerate() {
            assert!(!component.purpose.is_empty());
            assert!(!component.provenance.is_empty());
            assert!(
                catalog[..index]
                    .iter()
                    .all(|other| other.id != component.id)
            );
        }
    }

    #[test]
    fn detection_is_stable_and_does_not_mutate_an_existing_tool() {
        let root =
            std::env::temp_dir().join(format!("ds-workstation-detect-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("bash.exe");
        std::fs::write(&executable, b"pre-existing").unwrap();
        let before = std::fs::read(&executable).unwrap();
        let names = vec!["bash.exe".to_string()];
        assert_eq!(
            find_in_directories(&names, std::slice::from_ref(&root)),
            Some(executable.clone())
        );
        assert_eq!(
            find_in_directories(&names, std::slice::from_ref(&root)),
            Some(executable.clone())
        );
        assert_eq!(std::fs::read(&executable).unwrap(), before);
        std::fs::remove_dir_all(root).unwrap();
    }
}
