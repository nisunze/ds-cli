use std::path::{Component as PathComponent, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::detect::{Component, Platform};

// The component root and DS's scratch are DS-owned: directories 0700, files
// 0600. Owner rule `ds_layer_store::private`, applied inline because this
// crate does not depend on it. Unix only; an existing directory is tightened
// best effort.
pub(crate) fn private_dir_all(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        if let Ok(meta) = std::fs::symlink_metadata(path)
            && meta.is_dir()
            && meta.permissions().mode() & 0o077 != 0
        {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)
    }
}

/// `fs::create_dir` for a DS-owned directory: exclusive, created 0700.
pub(crate) fn private_dir(path: &Path) -> std::io::Result<()> {
    #[allow(unused_mut)]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

/// `OpenOptions` whose created file is 0600 on unix.
pub(crate) fn private_options() -> std::fs::OpenOptions {
    #[allow(unused_mut)]
    let mut options = std::fs::OpenOptions::new();
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

/// `fs::write` for a DS-owned file, created 0600.
pub(crate) fn private_write(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::io::Write;
    private_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?
        .write_all(bytes.as_ref())
}

#[derive(Debug, Deserialize)]
struct ReferenceReceipt {
    component: String,
    version: String,
    source: String,
    license: String,
    installed_at: String,
    task_owned: bool,
    files: Vec<ReceiptFile>,
}

#[derive(Debug, Deserialize)]
struct ReceiptFile {
    path: String,
    sha256: String,
}

pub fn component_root(platform: Platform) -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("DS_WORKSTATION_COMPONENT_ROOT") {
        return Some(PathBuf::from(value));
    }
    match platform {
        Platform::Windows => std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("Data Solutions").join("components")),
        Platform::Macos => std::env::var_os("HOME").map(PathBuf::from).map(|path| {
            path.join("Library")
                .join("Application Support")
                .join("Data Solutions")
                .join("components")
        }),
        Platform::Linux => std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join(".local").join("share"))
            })
            .map(|path| path.join("ds").join("components")),
    }
}

pub const BROWSER_SELECTION_SCHEMA: &str = "ds-workstation-browser-selection/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserSelection {
    pub schema: String,
    pub component: String,
    pub executable: String,
    pub version: String,
    pub verified_at_unix_s: u64,
    pub pdf_smoke: bool,
    pub preexisting: bool,
}

pub fn browser_selection_path(platform: Platform) -> Option<PathBuf> {
    component_root(platform).map(|root| root.join("chromium").join("browser-selection.json"))
}

pub fn read_browser_selection(platform: Platform) -> Result<Option<BrowserSelection>, String> {
    let path = browser_selection_path(platform)
        .ok_or_else(|| "the platform component root is unavailable".to_string())?;
    read_browser_selection_at(&path)
}

pub(crate) fn read_browser_selection_at(path: &Path) -> Result<Option<BrowserSelection>, String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.len() > 8 * 1024 => {
            return Err("browser selection exceeds 8 KiB".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "browser selection could not be inspected: {}",
                error.kind()
            ));
        }
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "browser selection could not be read: {}",
                error.kind()
            ));
        }
    };
    if bytes.len() > 8 * 1024 {
        return Err("browser selection exceeds 8 KiB".to_string());
    }
    let receipt: BrowserSelection = serde_json::from_slice(&bytes)
        .map_err(|error| format!("browser selection is invalid JSON: {error}"))?;
    if receipt.schema != BROWSER_SELECTION_SCHEMA
        || receipt.component != "chromium"
        || !Path::new(&receipt.executable).is_absolute()
        || receipt.version.trim().is_empty()
        || !receipt.pdf_smoke
        || !receipt.preexisting
    {
        return Err(
            "browser selection has invalid schema, path, or verification fields".to_string(),
        );
    }
    Ok(Some(receipt))
}

pub(crate) fn write_browser_selection_at(
    path: &Path,
    receipt: &BrowserSelection,
) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or("browser selection has no parent directory")?;
    private_dir_all(parent).map_err(|error| {
        format!(
            "browser settings directory could not be created: {}",
            error.kind()
        )
    })?;
    let bytes = serde_json::to_vec_pretty(receipt)
        .map_err(|error| format!("browser selection could not be encoded: {error}"))?;
    if bytes.len() > 8 * 1024 {
        return Err("browser selection exceeds 8 KiB".to_string());
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("json.tmp.{}-{nonce}", std::process::id()));
    let mut file = private_options()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("browser selection temporary write failed: {}", error.kind()))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            format!("browser selection temporary write failed: {}", error.kind())
        })?;
    drop(file);
    let moved = std::fs::rename(&temporary, path)
        .map_err(|error| format!("browser selection atomic replace failed: {}", error.kind()));
    if moved.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    moved?;
    Ok(())
}

pub fn reference_component_snapshot(component: &Component, platform: Platform) -> Value {
    let Some(root) = component_root(platform) else {
        return json!({
            "id": component.id,
            "required": component.required,
            "purpose": component.purpose,
            "state": "unknown",
            "path": null,
            "receipt": null,
            "reason": "the platform data directory could not be resolved",
        });
    };
    let directory = root.join(&component.id);
    let receipt_path = directory.join("receipt.json");
    if !receipt_path.is_file() {
        return json!({
            "id": component.id,
            "required": component.required,
            "purpose": component.purpose,
            "state": "absent",
            "path": directory.to_string_lossy(),
            "receipt": null,
        });
    }
    match verify_receipt(&directory, &receipt_path, &component.id) {
        Ok(receipt) => json!({
            "id": component.id,
            "required": component.required,
            "purpose": component.purpose,
            "state": "installed",
            "path": directory.to_string_lossy(),
            "receipt": receipt,
        }),
        Err(reason) => json!({
            "id": component.id,
            "required": component.required,
            "purpose": component.purpose,
            "state": "invalid_receipt",
            "path": directory.to_string_lossy(),
            "receipt": null,
            "reason": reason,
        }),
    }
}

fn verify_receipt(directory: &Path, receipt_path: &Path, expected: &str) -> Result<Value, String> {
    let bytes = std::fs::read(receipt_path)
        .map_err(|error| format!("receipt could not be read: {}", error.kind()))?;
    let receipt: ReferenceReceipt = serde_json::from_slice(&bytes)
        .map_err(|error| format!("receipt is not valid JSON: {error}"))?;
    if receipt.component != expected
        || receipt.version.trim().is_empty()
        || receipt.source.trim().is_empty()
        || receipt.license.trim().is_empty()
        || receipt.installed_at.trim().is_empty()
        || receipt.files.is_empty()
    {
        return Err("receipt component, version, or source is invalid".to_string());
    }
    for file in &receipt.files {
        let relative = Path::new(&file.path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| matches!(part, PathComponent::ParentDir))
        {
            return Err(format!(
                "receipt path `{}` escapes the component",
                file.path
            ));
        }
        let actual = sha256(&directory.join(relative))?;
        if !actual.eq_ignore_ascii_case(file.sha256.trim_start_matches("sha256:")) {
            return Err(format!("receipt hash mismatch for `{}`", file.path));
        }
    }
    Ok(json!({
        "component": receipt.component,
        "version": receipt.version,
        "source": receipt.source,
        "license": receipt.license,
        "installed_at": receipt.installed_at,
        "task_owned": receipt.task_owned,
        "file_count": receipt.files.len(),
        "verified": true,
    }))
}

pub const INSTALL_RECEIPT_SCHEMA: &str = "ds-workstation-install/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallReceipt {
    pub schema: String,
    pub run_id: String,
    pub component: String,
    pub package_id: String,
    pub source: String,
    pub installed_at_unix_s: u64,
    pub task_owned: bool,
    pub preexisting: bool,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub verified: bool,
    pub smoke: String,
}

pub fn install_receipt_path(platform: Platform, component: &str) -> Option<PathBuf> {
    component_root(platform).map(|root| root.join(component).join("install-receipt.json"))
}

pub fn install_ownership(platform: Platform, component: &str) -> Value {
    let Some(path) = install_receipt_path(platform, component) else {
        return json!({"state": "unrecorded", "task_owned": false, "receipt": null});
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return json!({"state": "unrecorded", "task_owned": false, "receipt": path.to_string_lossy()});
    };
    match serde_json::from_slice::<InstallReceipt>(&bytes) {
        Ok(receipt)
            if receipt.schema == INSTALL_RECEIPT_SCHEMA
                && receipt.component == component
                && receipt.task_owned
                && !receipt.preexisting =>
        {
            json!({
                "state": "task_owned",
                "task_owned": true,
                "receipt": path.to_string_lossy(),
                "run_id": receipt.run_id,
                "package_id": receipt.package_id,
                "verified": receipt.verified,
            })
        }
        Ok(_) => json!({
            "state": "non_task_owned_or_mismatched",
            "task_owned": false,
            "receipt": path.to_string_lossy(),
        }),
        Err(error) => json!({
            "state": "invalid_receipt",
            "task_owned": false,
            "receipt": path.to_string_lossy(),
            "reason": error.to_string(),
        }),
    }
}

pub fn ensure_install_receipt_slot(platform: Platform, component: &str) -> Result<PathBuf, String> {
    let path = install_receipt_path(platform, component)
        .ok_or_else(|| "the platform component root is unavailable".to_string())?;
    if path.exists() {
        return Err(format!(
            "an ownership receipt already exists at {}",
            path.display()
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| "the ownership receipt has no parent directory".to_string())?;
    private_dir_all(parent)
        .map_err(|error| format!("receipt directory is not writable: {}", error.kind()))?;
    Ok(path)
}

pub fn write_install_receipt(path: &Path, receipt: &InstallReceipt) -> Result<(), String> {
    use std::io::Write;

    let bytes = serde_json::to_vec_pretty(receipt)
        .map_err(|error| format!("receipt could not be encoded: {error}"))?;
    if bytes.len() > 8 * 1024 {
        return Err("ownership receipt exceeds 8 KiB".to_string());
    }
    let mut file = private_options()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("receipt could not be created: {}", error.kind()))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("receipt could not be persisted: {}", error.kind()))
}

pub(crate) fn sha256(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("component file could not be read: {}", error.kind()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Merge only VS Code's Windows default-profile key. Existing JSONC text,
/// comments, ordering, and unrelated settings remain byte-for-byte intact.
pub fn merge_vscode_windows_profile(text: &str, profile: &str) -> Result<String, &'static str> {
    const KEY: &str = "\"terminal.integrated.defaultProfile.windows\"";
    let value = serde_json::to_string(profile).map_err(|_| "profile cannot be encoded")?;
    if let Some(key_start) = text.find(KEY) {
        let after_key = &text[key_start + KEY.len()..];
        let colon = after_key
            .find(':')
            .ok_or("existing profile key has no value")?;
        let value_start = key_start + KEY.len() + colon + 1;
        let tail = &text[value_start..];
        let quote_start = tail
            .find('"')
            .ok_or("existing profile value is not a string")?;
        let content_start = value_start + quote_start;
        let mut escaped = false;
        let mut end = None;
        for (offset, character) in text[content_start + 1..].char_indices() {
            if character == '"' && !escaped {
                end = Some(content_start + 1 + offset + 1);
                break;
            }
            escaped = character == '\\' && !escaped;
            if character != '\\' {
                escaped = false;
            }
        }
        let end = end.ok_or("existing profile string is unterminated")?;
        return Ok(format!(
            "{}{}{}",
            &text[..content_start],
            value,
            &text[end..]
        ));
    }
    let close = text
        .rfind('}')
        .ok_or("settings document has no root object")?;
    let before = &text[..close];
    let needs_comma = before
        .chars()
        .rev()
        .find(|character| !character.is_whitespace())
        .is_some_and(|character| character != '{' && character != ',');
    let comma = if needs_comma { "," } else { "" };
    Ok(format!(
        "{before}{comma}\n  {KEY}: {value}\n{}",
        &text[close..]
    ))
}

pub fn jsonc_string(text: &str, key: &str) -> Option<String> {
    let quoted = format!("\"{key}\"");
    let start = text.find(&quoted)? + quoted.len();
    let colon = text[start..].find(':')? + start + 1;
    let quote = text[colon..].find('"')? + colon + 1;
    let end = text[quote..].find('"')? + quote;
    Some(text[quote..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_merge_preserves_comments_and_unrelated_values() {
        let original = "{\n  // keep this\n  \"editor.fontSize\": 15,\n  \"terminal.integrated.defaultProfile.windows\": \"PowerShell\"\n}\n";
        let merged = merge_vscode_windows_profile(original, "Git Bash").unwrap();
        assert!(merged.contains("// keep this"));
        assert!(merged.contains("\"editor.fontSize\": 15"));
        assert!(merged.contains("\"terminal.integrated.defaultProfile.windows\": \"Git Bash\""));
        assert_eq!(
            merge_vscode_windows_profile(&merged, "Git Bash").unwrap(),
            merged
        );
    }

    #[test]
    fn component_receipt_verifies_content_and_ownership_without_mutation() {
        let root =
            std::env::temp_dir().join(format!("ds-workstation-receipt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("villages.geojson"), b"{}").unwrap();
        let digest = sha256(&root.join("villages.geojson")).unwrap();
        std::fs::write(
            root.join("receipt.json"),
            serde_json::to_vec(&json!({
                "component": "rwanda-reference",
                "version": "fixture-v1",
                "source": "fixture",
                "license": "fixture-license",
                "installed_at": "2026-08-27T00:00:00Z",
                "task_owned": true,
                "files": [{"path": "villages.geojson", "sha256": digest}]
            }))
            .unwrap(),
        )
        .unwrap();
        let before = std::fs::read(root.join("villages.geojson")).unwrap();
        let receipt =
            verify_receipt(&root, &root.join("receipt.json"), "rwanda-reference").unwrap();
        assert_eq!(receipt["verified"], true);
        assert_eq!(receipt["task_owned"], true);
        assert_eq!(
            std::fs::read(root.join("villages.geojson")).unwrap(),
            before
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
