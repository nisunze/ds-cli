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

fn named_candidates(name: &str, directories: &[PathBuf]) -> Vec<PathBuf> {
    directories
        .iter()
        .map(|directory| directory.join(name))
        .filter(|candidate| candidate.is_file())
        .collect()
}

fn executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn named_or_path(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.contains('/') || value.contains('\\') {
        let path = PathBuf::from(value);
        return executable(&path).then_some(path);
    }
    path_directories(std::env::var_os("PATH"))
        .into_iter()
        .map(|directory| directory.join(value))
        .find(|path| executable(path))
}

fn playwright_headless_shell(cache: &Path) -> Option<PathBuf> {
    let mut found = std::fs::read_dir(cache)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let revision = entry
                .file_name()
                .to_string_lossy()
                .strip_prefix("chromium_headless_shell-")?
                .parse::<u64>()
                .ok()?;
            let root = entry.path();
            if !root.join("INSTALLATION_COMPLETE").is_file() {
                return None;
            }
            let path = root.join("chrome-headless-shell-linux64/chrome-headless-shell");
            executable(&path).then_some((revision, path))
        })
        .collect::<Vec<_>>();
    found.sort_by_key(|(revision, _)| *revision);
    found.pop().map(|(_, path)| path)
}

fn chromium_location(platform: Platform) -> (Option<PathBuf>, String, Option<String>) {
    for key in ["DS_VD_CHROME", "CHROME"] {
        if let Some(value) = std::env::var_os(key) {
            let value = value.to_string_lossy();
            let path = named_or_path(&value);
            let reason = path
                .is_none()
                .then(|| format!("{key} is set but does not name an executable browser"));
            return (path, format!("environment:{key}"), reason);
        }
    }
    match crate::policy::read_browser_selection(platform) {
        Ok(Some(selection)) => {
            let path = PathBuf::from(selection.executable);
            if executable(&path) {
                return (Some(path), "ds_verified_selection".to_string(), None);
            }
            return (
                None,
                "ds_verified_selection".to_string(),
                Some("the saved report browser is no longer executable; set DS_VD_CHROME to an existing browser and reconfigure".to_string()),
            );
        }
        Err(reason) => {
            return (
                None,
                "ds_verified_selection".to_string(),
                Some(format!(
                    "the saved report browser selection is invalid: {reason}; set DS_VD_CHROME to an existing browser and reconfigure"
                )),
            );
        }
        Ok(None) => {}
    }
    if platform == Platform::Windows {
        for path in windows_edge_paths(
            std::env::var_os("ProgramFiles(x86)"),
            std::env::var_os("ProgramFiles"),
        ) {
            if executable(&path) {
                return (Some(path), "windows_edge".to_string(), None);
            }
        }
    }
    let names: &[&str] = match platform {
        Platform::Linux => &[
            "chromium",
            "chromium-browser",
            "google-chrome",
            "microsoft-edge",
            "chrome-headless-shell",
        ],
        Platform::Macos => &["chromium", "google-chrome", "msedge"],
        Platform::Windows => &["chrome.exe", "chromium.exe", "msedge.exe"],
    };
    for name in names {
        if let Some(path) = named_or_path(name) {
            return (Some(path), "system_path".to_string(), None);
        }
    }
    let conventional = match platform {
        Platform::Windows => {
            let mut paths = Vec::new();
            for root in [
                std::env::var_os("ProgramFiles"),
                std::env::var_os("ProgramFiles(x86)"),
                std::env::var_os("LOCALAPPDATA"),
            ]
            .into_iter()
            .flatten()
            {
                let root = PathBuf::from(root);
                paths.push(root.join("Google/Chrome/Application/chrome.exe"));
                paths.push(root.join("Chromium/Application/chrome.exe"));
                paths.push(root.join("Microsoft/Edge/Application/msedge.exe"));
            }
            paths
        }
        Platform::Macos => vec![
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
            PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
        ],
        Platform::Linux => Vec::new(),
    };
    if let Some(path) = conventional.into_iter().find(|path| executable(path)) {
        return (Some(path), "conventional_path".to_string(), None);
    }
    if platform == Platform::Linux {
        let cache = match std::env::var_os("PLAYWRIGHT_BROWSERS_PATH") {
            Some(value) if value == "0" => {
                return (
                    None,
                    "playwright_hermetic".to_string(),
                    Some("PLAYWRIGHT_BROWSERS_PATH=0 uses a project-local browser cache; set DS_VD_CHROME to the exact headless-shell executable".to_string()),
                );
            }
            Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
            _ => std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".cache/ms-playwright")),
        };
        if let Some(cache) = cache
            && let Some(path) = playwright_headless_shell(&cache)
        {
            return (Some(path), "playwright_headless_shell".to_string(), None);
        }
    }
    (None, "none".to_string(), None)
}

fn windows_edge_paths(x86: Option<OsString>, native: Option<OsString>) -> Vec<PathBuf> {
    [x86, native]
        .into_iter()
        .flatten()
        .map(|root| PathBuf::from(root).join("Microsoft/Edge/Application/msedge.exe"))
        .collect()
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
                _ => {}
            }
        }
        Platform::Macos => {
            if component == "libreoffice" {
                candidates.push(PathBuf::from(
                    "/Applications/LibreOffice.app/Contents/MacOS/soffice",
                ));
            }
        }
        Platform::Linux => {}
    }
    candidates
}

pub fn version(path: &Path, component: &str) -> Result<String, String> {
    let args: &[&str] = match component {
        "chromium" => return browser_identity(path),
        "libreoffice" => &["--headless", "--version"],
        "git-bash" | "git" | "pandoc" | "tippecanoe" => &["--version"],
        "pmtiles" => &["version"],
        _ => return Err("this component has no executable version probe".to_string()),
    };
    run_version_probe(path, args)
}

/// The one browser identity probe: configure, verify, status and an explicit
/// `DS_VD_CHROME` selection all read it. Windows browsers are GUI-subsystem
/// executables that print nothing for `--version`, so there the identity comes
/// from the file's version resource and the executable is never run.
#[cfg(windows)]
fn browser_identity(path: &Path) -> Result<String, String> {
    crate::windows_version::product_identity(path)
}

#[cfg(not(windows))]
fn browser_identity(path: &Path) -> Result<String, String> {
    run_version_probe(path, &["--version"])
}

/// `<ProductName> <ProductVersion>`, the shape [`chromium_version_is_supported`]
/// reads. Which product names are acceptable is that check's decision, so a
/// foreign executable gets the same refusal as on Linux.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn compose_product_identity(name: &str, version: &str) -> Result<String, String> {
    let name = name.trim();
    let version = version.split_whitespace().next().unwrap_or("");
    if name.is_empty() {
        return Err("the executable's version resource has an empty ProductName".to_string());
    }
    if !version.starts_with(|c: char| c.is_ascii_digit())
        || !version.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        return Err("the executable's version resource has no numeric ProductVersion".to_string());
    }
    Ok(format!("{name} {version}"))
}

fn run_version_probe(path: &Path, args: &[&str]) -> Result<String, String> {
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

/// A supported product name followed by a version that starts with a digit, so
/// `Microsoft Edge WebView2 154.0` and similar siblings are not a browser.
pub(crate) fn chromium_version_is_supported(version: &str) -> bool {
    [
        "Chromium ",
        "Google Chrome for Testing ",
        "Google Chrome ",
        "Microsoft Edge ",
    ]
    .iter()
    .any(|prefix| {
        version
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    })
}

pub fn snapshot(component: &Component, platform: Platform, probe_version: bool) -> Value {
    if component.id == "chromium" {
        let (path, source, location_error) = chromium_location(platform);
        let probe = path
            .as_deref()
            .filter(|_| probe_version)
            .map(|path| version(path, "chromium"));
        let (version, probe_error) = match probe {
            Some(Ok(value)) if chromium_version_is_supported(&value) => (Some(value), None),
            Some(Ok(_)) => (
                None,
                Some("the executable did not identify itself as Chromium".to_string()),
            ),
            Some(Err(error)) => (None, Some(error)),
            None => (None, location_error),
        };
        return json!({
            "id": component.id,
            "required": component.required,
            "purpose": component.purpose,
            "state": if path.is_none() { if probe_error.is_some() { "variant_unverified" } else { "absent" } } else if probe_version && version.is_none() { "variant_unverified" } else { "installed" },
            "path": path.as_deref().map(|path| path.to_string_lossy().into_owned()),
            "version": version,
            "probe_error": probe_error,
            "source": source,
            "suitable": if probe_version { version.is_some() } else { path.is_some() },
            "ownership": crate::policy::install_ownership(platform, &component.id),
        });
    }
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
    if component.id == "tippecanoe" && platform == Platform::Linux {
        return tiling_snapshot(component);
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

fn tiling_snapshot(component: &Component) -> Value {
    const TIP_VERSION: &str = "tippecanoe v2.82.0";
    const PM_VERSION_PREFIX: &str = "pmtiles 1.20.0,";
    let directories = path_directories(std::env::var_os("PATH"));
    let tip_candidates = named_candidates("tippecanoe", &directories);
    let pm_candidates = named_candidates("pmtiles", &directories);
    let tip = tip_candidates.iter().find_map(|path| {
        version(path, "tippecanoe")
            .ok()
            .filter(|found| found.trim() == TIP_VERSION)
            .map(|found| (path, found))
    });
    let pm = pm_candidates.iter().find_map(|path| {
        version(path, "pmtiles")
            .ok()
            .filter(|found| found.starts_with(PM_VERSION_PREFIX))
            .map(|found| (path, found))
    });
    let suitable = tip.is_some() && pm.is_some();
    let any_found = !tip_candidates.is_empty() || !pm_candidates.is_empty();
    json!({
        "id": component.id,
        "required": component.required,
        "purpose": component.purpose,
        "state": if suitable { "installed" } else if any_found { "variant_unverified" } else { "absent" },
        "path": tip.as_ref().map(|(path, _)| path.to_string_lossy().into_owned()),
        "version": tip.as_ref().map(|(_, version)| version),
        "probe_error": if suitable { Value::Null } else { json!("the kernel requires Tippecanoe 2.82.0 and PMTiles 1.20.0") },
        "suitable": suitable,
        "tools": {
            "tippecanoe": tip.map(|(path, version)| json!({"path": path.to_string_lossy(), "version": version})),
            "pmtiles": pm.map(|(path, version)| json!({"path": path.to_string_lossy(), "version": version})),
        },
        "ownership": crate::policy::install_ownership(Platform::Linux, &component.id),
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
        // Name the components rather than counting them. `qgis` was dropped
        // from components.json in 436fc98 and only a bare `len() == 4` caught
        // it, which says a number changed but not which component left — the
        // question an operator actually has when a probe stops finding a tool.
        let ids: Vec<&str> = catalog.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "libreoffice",
                "git-bash",
                "rwanda-reference",
                // Local tiling and local document conversion: a Linux desktop
                // or server owns these rather than calling a cloud service.
                "tippecanoe",
                "pandoc",
                "chromium",
            ]
        );
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

    #[test]
    fn playwright_shell_selection_requires_completion_and_uses_numeric_revision() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ds-workstation-playwright-detect-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        for (revision, complete) in [(99, true), (100, true), (999, false)] {
            let browser = root
                .join(format!("chromium_headless_shell-{revision}"))
                .join("chrome-headless-shell-linux64/chrome-headless-shell");
            std::fs::create_dir_all(browser.parent().unwrap()).unwrap();
            std::fs::write(&browser, b"existing browser").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            if complete {
                std::fs::write(
                    browser
                        .parent()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .join("INSTALLATION_COMPLETE"),
                    b"",
                )
                .unwrap();
            }
        }
        let found = playwright_headless_shell(&root).unwrap();
        assert!(
            found
                .to_string_lossy()
                .contains("chromium_headless_shell-100/")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accepts_chrome_chromium_and_edge_browser_versions() {
        for version in [
            "Google Chrome 153.0.8010.12",
            "Google Chrome for Testing 153.0.8010.12",
            "Chromium 153.0.8010.12",
            "Microsoft Edge 153.0.8010.12",
        ] {
            assert!(chromium_version_is_supported(version), "{version}");
        }
        assert!(!chromium_version_is_supported("Firefox 153.0"));
        assert!(!chromium_version_is_supported(
            "Microsoft Edge WebView2 154.0.4258.53"
        ));
        assert!(!chromium_version_is_supported("Google Chrome"));
    }

    #[test]
    fn windows_version_resource_maps_to_the_supported_variant_shape() {
        for (name, version, expected) in [
            (
                "Microsoft Edge",
                "154.0.4258.53",
                "Microsoft Edge 154.0.4258.53",
            ),
            (
                "Google Chrome",
                " 154.0.7000.0 ",
                "Google Chrome 154.0.7000.0",
            ),
            (
                "Chromium",
                "154.0.1.2 (Official Build)",
                "Chromium 154.0.1.2",
            ),
            (
                "Google Chrome for Testing",
                "154.0.7000.0",
                "Google Chrome for Testing 154.0.7000.0",
            ),
        ] {
            let identity = compose_product_identity(name, version).unwrap();
            assert_eq!(identity, expected);
            assert!(chromium_version_is_supported(&identity), "{identity}");
        }
    }

    #[test]
    fn windows_version_resource_of_a_wrong_executable_is_refused() {
        for (name, version) in [
            ("Notepad", "10.0.19041.1"),
            ("Microsoft Edge WebView2", "154.0.4258.53"),
            ("Firefox", "153.0"),
        ] {
            let identity = compose_product_identity(name, version).unwrap();
            assert!(!chromium_version_is_supported(&identity), "{identity}");
        }
        assert!(compose_product_identity("", "154.0.1.2").is_err());
        assert!(compose_product_identity("Microsoft Edge", "").is_err());
        assert!(compose_product_identity("Microsoft Edge", "latest").is_err());
    }

    #[test]
    fn windows_edge_is_probed_under_x86_program_files_first() {
        let paths = windows_edge_paths(
            Some(OsString::from(r"C:\Program Files (x86)")),
            Some(OsString::from(r"C:\Program Files")),
        );
        assert_eq!(paths.len(), 2);
        assert!(
            paths[0]
                .to_string_lossy()
                .starts_with(r"C:\Program Files (x86)")
        );
        assert!(paths[1].to_string_lossy().starts_with(r"C:\Program Files"));
        assert!(
            paths
                .iter()
                .all(|path| path.to_string_lossy().ends_with("msedge.exe"))
        );
    }
}
