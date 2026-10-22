//! Native Solar workspace authority.
//!
//! Server and Desktop share operator-approved directories and opaque workspace
//! receipts. Resolving a receipt never consults a runtime installer, sidecar
//! launch, browser or MCP registration. The native Rust owner retains it.

use ds_solar_contracts::is_solar_id as valid_solar_data_id;
use std::io::{Read as _, Write};
use std::path::{Path, PathBuf};

use crate::NativeHost as AppHandle;
use serde::{Deserialize, Serialize};

const WORKSPACE_RECEIPT_SCHEMA: u32 = 1;
const WORKSPACE_HANDLE_PREFIX: &str = "solar-workspace-";
const MAX_RECEIPT_BYTES: u64 = 8 * 1024;

/// A durable, app-owned declaration that a directory was approved by the
/// operator.  The frontend receives only `handle`; every compute command
/// reopens this receipt before it touches the filesystem.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SolarWorkspaceReceipt {
    schema: u32,
    handle: String,
    canonical_root: String,
}

/// Minimal result of a native workspace approval.
///
/// Do not add a temp path, briefing path, sidecar state, or MCP state here.
/// Those were properties of the retired Python launch path, not properties of
/// a local Rust workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSolarWorkspace {
    pub workspace_handle: String,
    pub dir: String,
}

/// Stable app-owned default paths the frontend may display.  They carry no
/// authority: writes still require a `NativeSolarWorkspace` handle.
#[derive(Debug, Clone, Serialize)]
pub struct NativeSolarWorkspacePaths {
    pub root_dir: String,
    pub working_dir: String,
    pub cache_dir: String,
}

/// Returns whether a string is a safe single filesystem segment on every
/// desktop platform we support.
pub fn valid_solar_segment(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed != value || value.ends_with('.') {
        return false;
    }
    let windows_stem = value.split('.').next().unwrap_or_default();
    let windows_stem_upper = windows_stem.to_ascii_uppercase();
    let windows_stem_bytes = windows_stem_upper.as_bytes();
    let windows_reserved = ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|name| windows_stem.eq_ignore_ascii_case(name))
        || (windows_stem_bytes.len() == 4
            && (&windows_stem_bytes[..3] == b"COM" || &windows_stem_bytes[..3] == b"LPT")
            && matches!(windows_stem_bytes[3], b'1'..=b'9'));
    !value.is_empty()
        && value.len() <= 80
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && value != "."
        && value != ".."
        && !windows_reserved
}

/// Checks the collection root used by native Solar artifacts.
pub fn valid_solar_root(root: &str) -> bool {
    if root == "eds_solar" {
        return true;
    }
    let parts: Vec<&str> = root.split('/').collect();
    parts.len() == 3
        && parts[0] == "eds_project"
        && valid_solar_data_id(parts[1])
        && parts[2] == "eds_solar"
}

/// Whether a string is even shaped like an approved workspace authority.
///
/// Structural only — [`resolve_workspace_handle`] still has to find and match
/// the receipt. It is `pub(crate)` so a door that resolves a run directory some
/// other way can still refuse a publication that carries no workspace at all,
/// rather than defaulting one.
pub fn valid_workspace_handle(handle: &str) -> bool {
    handle.len() == WORKSPACE_HANDLE_PREFIX.len() + 32
        && handle.starts_with(WORKSPACE_HANDLE_PREFIX)
        && handle[WORKSPACE_HANDLE_PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(windows)]
fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_type().is_symlink() || metadata.file_attributes() & 0x0000_0400 != 0
}

#[cfg(not(windows))]
fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Windows canonicalization returns extended-length (`\\?\`) spelling while
/// dialogs generally return DOS/UNC spelling.  This preserves identity checks
/// across those spellings without weakening the reparse-point boundary.
#[cfg(windows)]
fn same_resolved_path(left: &Path, right: &Path) -> bool {
    fn identity(path: &Path) -> String {
        let value = path.to_string_lossy();
        let without_prefix = value
            .strip_prefix(r"\\?\UNC\")
            .map(|rest| format!(r"\\{rest}"))
            .or_else(|| value.strip_prefix(r"\\?\").map(str::to_string))
            .unwrap_or_else(|| value.into_owned());
        without_prefix.replace('/', r"\").to_lowercase()
    }

    identity(left) == identity(right)
}

#[cfg(not(windows))]
fn same_resolved_path(left: &Path, right: &Path) -> bool {
    left == right
}

/// Creates or validates an app-private directory without following links in
/// any owned ancestor.  Native Solar uses it for its receipt store and for all
/// engine-owned children inside an approved workspace.
pub fn secure_private_directory(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("private Solar directory must be absolute".to_string());
    }
    let mut ancestors: Vec<PathBuf> = path
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();
    ancestors.reverse();
    // Inspect the entire existing prefix before the first mkdir.  A planted
    // link is authority, not a harmless filesystem shortcut.
    for ancestor in &ancestors {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                    return Err(
                        "private Solar directory has a linked or non-directory ancestor"
                            .to_string(),
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(format!(
                    "could not inspect private Solar directory ancestor: {error}"
                ));
            }
        }
    }
    for ancestor in &ancestors {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                    return Err(
                        "private Solar directory has a linked or non-directory ancestor"
                            .to_string(),
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                #[cfg_attr(not(unix), allow(unused_mut))]
                let mut builder = std::fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(ancestor).map_err(|error| {
                    format!("could not create private Solar directory: {error}")
                })?;
                let metadata = std::fs::symlink_metadata(ancestor).map_err(|error| {
                    format!("could not inspect created private Solar directory: {error}")
                })?;
                if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
                    return Err("created private Solar directory changed identity".to_string());
                }
            }
            Err(error) => {
                return Err(format!(
                    "could not inspect private Solar directory: {error}"
                ));
            }
        }
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect private Solar directory: {error}"))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err("private Solar directory cannot be a link or non-directory".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("could not protect private Solar directory: {error}"))?;
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("could not resolve private Solar directory: {error}"))?;
    if !same_resolved_path(&canonical, path) {
        return Err("private Solar directory must use its exact canonical path".to_string());
    }
    Ok(canonical)
}

/// Reopen an engine-owned directory without creating anything and without
/// following a linked/reparse ancestor. Read selectors use this counterpart to
/// the creation helper so a run directory cannot be swapped for an external
/// tree after it was committed.
pub fn secure_existing_private_directory(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("private Solar directory must be absolute".to_string());
    }
    let mut ancestors: Vec<PathBuf> = path
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();
    ancestors.reverse();
    for ancestor in ancestors {
        let metadata = std::fs::symlink_metadata(&ancestor).map_err(|error| {
            format!("could not inspect private Solar directory ancestor: {error}")
        })?;
        if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(
                "private Solar directory has a linked or non-directory ancestor".to_string(),
            );
        }
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("could not resolve private Solar directory: {error}"))?;
    if !same_resolved_path(&canonical, path) {
        return Err("private Solar directory must use its exact canonical path".to_string());
    }
    Ok(canonical)
}

fn app_workspace_root(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app.data_dir()?;
    app_workspace_root_at(&app_data)
}

fn app_workspace_root_at(app_data: &Path) -> Result<PathBuf, String> {
    secure_private_directory(&app_data.join("solar-workspaces"))
}

/// Receipts intentionally share the legacy app-owned directory and schema.
/// Existing handles remain resolvable during migration, while new native
/// defaults no longer derive any path from the legacy runtime.
fn workspace_receipts_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app_workspace_root(app)
}

fn default_workspace_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app_workspace_root(app)?;
    secure_private_directory(&root.join("workspace"))
}

fn default_cache_dir(workspace: &Path) -> PathBuf {
    workspace
        .join("solar-rust")
        .join("v1")
        .join("weather-cache")
}

fn write_workspace_receipt(app: &AppHandle, canonical_root: &Path) -> Result<String, String> {
    let receipt_dir = workspace_receipts_dir(app)?;
    let handle = format!(
        "{WORKSPACE_HANDLE_PREFIX}{:016x}{:016x}",
        rand::random::<u64>(),
        rand::random::<u64>()
    );
    let receipt = SolarWorkspaceReceipt {
        schema: WORKSPACE_RECEIPT_SCHEMA,
        handle: handle.clone(),
        canonical_root: canonical_root.to_string_lossy().into_owned(),
    };
    let mut body = serde_json::to_vec_pretty(&receipt)
        .map_err(|error| format!("could not encode Solar workspace receipt: {error}"))?;
    body.push(b'\n');

    let path = receipt_dir.join(format!("{handle}.json"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|error| format!("could not persist Solar workspace receipt: {error}"))?;
    file.write_all(&body)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("could not persist Solar workspace receipt: {error}"))?;
    Ok(handle)
}

/// Read one regular file through an opened-handle size check and a hard
/// max-plus-one ceiling. The pathname check refuses links/reparse points, while
/// the handle metadata prevents a larger replacement from driving allocation.
fn read_bounded_regular_file(path: &Path, label: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    let path_metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("{label} is unavailable: {error}"))?;
    if metadata_is_link_or_reparse(&path_metadata) || !path_metadata.is_file() {
        return Err(format!("{label} is not a regular file"));
    }
    let file =
        std::fs::File::open(path).map_err(|error| format!("{label} is unreadable: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("{label} is unreadable: {error}"))?;
    if !metadata.is_file() {
        return Err(format!("{label} is not a regular file"));
    }
    if metadata.len() > max_bytes {
        return Err(format!("{label} exceeds its read bound"));
    }
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| format!("{label} does not fit this platform's address space"))?;
    let read_limit = max_bytes
        .checked_add(1)
        .ok_or_else(|| format!("{label} read bound overflowed"))?;
    let mut body = Vec::with_capacity(capacity);
    file.take(read_limit)
        .read_to_end(&mut body)
        .map_err(|error| format!("{label} is unreadable: {error}"))?;
    if body.len() as u64 > max_bytes || body.len() as u64 != metadata.len() {
        return Err(format!("{label} changed during its bounded read"));
    }
    Ok(body)
}

fn read_workspace_receipt(path: &Path, root: &Path) -> Result<SolarWorkspaceReceipt, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("Solar workspace receipt is unavailable: {error}"))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err("Solar workspace receipt is not a regular file".to_string());
    }
    if metadata.len() == 0 || metadata.len() > MAX_RECEIPT_BYTES {
        return Err("Solar workspace receipt has an invalid size".to_string());
    }
    let canonical_root = secure_existing_private_directory(root)
        .map_err(|error| format!("Solar workspace receipt root is unavailable: {error}"))?;
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("Solar workspace receipt is unavailable: {error}"))?;
    if !canonical.starts_with(&canonical_root) || !canonical.is_file() {
        return Err("Solar workspace receipt leaves its approved directory".to_string());
    }
    let body = read_bounded_regular_file(&canonical, "Solar workspace receipt", MAX_RECEIPT_BYTES)?;
    serde_json::from_slice(&body)
        .map_err(|error| format!("could not parse Solar workspace receipt: {error}"))
}

/// Reopens one opaque workspace authority.  The client can never nominate a
/// path here; changing or replacing a selected directory invalidates its
/// receipt instead of silently retargeting native computation.
pub fn resolve_workspace_handle(app: &AppHandle, handle: &str) -> Result<PathBuf, String> {
    if !valid_workspace_handle(handle) {
        return Err("Solar workspace handle is invalid".to_string());
    }
    let receipt_dir = secure_existing_private_directory(&app.data_dir()?.join("solar-workspaces"))?;
    let receipt =
        read_workspace_receipt(&receipt_dir.join(format!("{handle}.json")), &receipt_dir)?;
    if receipt.schema != WORKSPACE_RECEIPT_SCHEMA
        || receipt.handle != handle
        || !valid_workspace_handle(&receipt.handle)
    {
        return Err("Solar workspace receipt does not match its handle".to_string());
    }
    let recorded = PathBuf::from(&receipt.canonical_root);
    if !recorded.is_absolute() {
        return Err("Solar workspace receipt root is not absolute".to_string());
    }
    secure_existing_private_directory(&recorded)
        .map_err(|_| "Solar workspace identity changed after it was approved".to_string())
}

fn selected_workspace(path: &Path) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect selected Solar workspace: {error}"))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err("selected Solar workspace cannot be a link or non-directory".to_string());
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("could not resolve selected Solar workspace: {error}"))?;
    if !same_resolved_path(&canonical, path) {
        return Err("selected Solar workspace must use its exact canonical path".to_string());
    }
    Ok(canonical)
}

fn prepare_workspace_selection(
    app: &AppHandle,
    canonical: &Path,
) -> Result<NativeSolarWorkspace, String> {
    Ok(NativeSolarWorkspace {
        workspace_handle: write_workspace_receipt(app, canonical)?,
        dir: canonical.to_string_lossy().into_owned(),
    })
}

/// App-owned default locations for the linked Rust engine.  This never calls
/// the retired launcher, a component installer, or a sidecar.
pub fn solar_native_workspace_paths(app: AppHandle) -> Result<NativeSolarWorkspacePaths, String> {
    let root = app_workspace_root(&app)?;
    let workspace = default_workspace_root(&app)?;
    Ok(NativeSolarWorkspacePaths {
        root_dir: root.to_string_lossy().into_owned(),
        working_dir: workspace.to_string_lossy().into_owned(),
        cache_dir: default_cache_dir(&workspace).to_string_lossy().into_owned(),
    })
}

/// Opens a native folder picker and records the selected directory behind an
/// opaque handle.  There is no sidecar setup step after selection.
pub fn solar_native_choose_workspace(
    app: AppHandle,
    selected: PathBuf,
) -> Result<NativeSolarWorkspace, String> {
    let canonical = selected_workspace(&selected)?;
    prepare_workspace_selection(&app, &canonical)
}

/// Creates and approves the source-backed app-data workspace used on first
/// native run.  The location is deterministic; the receipt remains opaque.
pub fn solar_native_prepare_default_workspace(
    app: AppHandle,
) -> Result<NativeSolarWorkspace, String> {
    let state = app.state();
    let _lock = state
        .workspaces
        .lock()
        .map_err(|_| "Solar workspace state is poisoned")?;
    let canonical = default_workspace_root(&app)?;
    let pointer = workspace_receipts_dir(&app)?.join("default-workspace.json");
    if std::fs::symlink_metadata(&pointer).is_ok() {
        let bytes =
            read_bounded_regular_file(&pointer, "Default Solar workspace", MAX_RECEIPT_BYTES)?;
        let workspace: NativeSolarWorkspace = serde_json::from_slice(&bytes)
            .map_err(|_| "Default Solar workspace receipt is invalid")?;
        let resolved = resolve_workspace_handle(&app, &workspace.workspace_handle)?;
        if !same_resolved_path(&resolved, &canonical)
            || workspace.dir != canonical.to_string_lossy()
        {
            return Err("Default Solar workspace has changed".into());
        }
        return Ok(workspace);
    }
    let workspace = prepare_workspace_selection(&app, &canonical)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(pointer)
        .map_err(|e| format!("Cannot record default Solar workspace: {e}"))?;
    file.write_all(&serde_json::to_vec(&workspace).map_err(|e| e.to_string())?)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("Cannot persist default Solar workspace: {e}"))?;
    Ok(workspace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_workspace_result_exposes_only_handle_and_directory() {
        let value = serde_json::to_value(NativeSolarWorkspace {
            workspace_handle: format!("solar-workspace-{}", "a".repeat(32)),
            dir: "/approved/solar".to_string(),
        })
        .expect("workspace result serializes");
        let object = value.as_object().expect("workspace result is an object");
        assert_eq!(object.len(), 2);
        assert!(object.contains_key("workspace_handle"));
        assert!(object.contains_key("dir"));
        assert!(!object.contains_key("mcp_registered"));
        assert!(!object.contains_key("briefing_path"));
        assert!(!object.contains_key("temp_dir"));
    }

    #[test]
    fn safe_native_workspace_segments_and_roots_remain_cross_platform() {
        assert!(valid_solar_segment("aderm_bere"));
        for hostile in ["../escape", "a/b", "", ".", "..", "CON", " trailing "] {
            assert!(!valid_solar_segment(hostile), "{hostile:?}");
        }
        assert!(valid_solar_root("eds_solar"));
        assert!(valid_solar_root("eds_project/project-1/eds_solar"));
        assert!(!valid_solar_root("eds_project/../eds_solar"));
    }

    #[test]
    fn native_default_workspace_is_under_the_app_owned_solar_workspaces_root() {
        let app_data = std::env::temp_dir().join(format!(
            "ds-native-solar-workspace-root-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = app_workspace_root_at(&app_data).expect("app workspace root");
        let workspace =
            secure_private_directory(&root.join("workspace")).expect("default workspace");
        assert_eq!(workspace, root.join("workspace"));
        assert_eq!(
            default_cache_dir(&workspace),
            workspace.join("solar-rust/v1/weather-cache")
        );
        std::fs::remove_dir_all(app_data).expect("remove scratch app data");
    }

    #[cfg(unix)]
    #[test]
    fn private_workspace_directory_refuses_linked_ancestors() {
        let root = std::env::temp_dir().join(format!(
            "ds-native-solar-workspace-link-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let outside = root.join("outside");
        let planted = root.join("planted");
        std::fs::create_dir_all(&outside).expect("outside directory");
        std::os::unix::fs::symlink(&outside, &planted).expect("planted link");
        assert!(secure_private_directory(&planted.join("workspace")).is_err());
        assert!(!outside.join("workspace").exists());
        std::fs::remove_dir_all(root).expect("remove scratch root");
    }

    #[cfg(unix)]
    #[test]
    fn existing_private_directory_refuses_linked_ancestors() {
        let root = std::env::temp_dir().join(format!(
            "ds-native-solar-existing-workspace-link-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let outside = root.join("outside");
        let existing = outside.join("run");
        let planted = root.join("planted");
        std::fs::create_dir_all(&existing).expect("existing outside directory");
        std::os::unix::fs::symlink(&outside, &planted).expect("planted link");

        let error = secure_existing_private_directory(&planted.join("run"))
            .expect_err("a linked existing ancestor must be refused");
        assert!(
            error.contains("linked or non-directory ancestor"),
            "{error}"
        );
        std::fs::remove_dir_all(root).expect("remove scratch root");
    }

    #[test]
    fn oversized_workspace_receipt_is_refused_before_deserialization() {
        let root = std::env::temp_dir().join(format!(
            "ds-native-solar-workspace-receipt-bound-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = secure_private_directory(&root).expect("receipt root");
        let receipt_path = root.join("oversized.json");
        let receipt = std::fs::File::create(&receipt_path).expect("sparse receipt");
        receipt
            .set_len(MAX_RECEIPT_BYTES + 1)
            .expect("oversized sparse receipt");

        let error = read_workspace_receipt(&receipt_path, &root)
            .expect_err("an oversized receipt must be refused");
        assert!(error.contains("invalid size"), "{error}");
        std::fs::remove_dir_all(root).expect("remove receipt root");
    }
}
