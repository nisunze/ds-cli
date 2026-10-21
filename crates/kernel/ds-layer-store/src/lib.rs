//! Shared desktop/CLI persistence adapter. Domain transitions live in the kernel.
pub use ds_command_kernel::overlays::{Overlay, OverlayEdit, OverlayRegistry};
use fs2::FileExt;
use serde_json::Value;
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// The machine-local layer root. The first call in a process also tightens an
/// install from before the privacy rule ([`private`]): `DS_LAYER_HOME` and
/// all it holds, or — for the default location — the whole `ds` namespace
/// under the platform's local data directory (the layer root beside the
/// workstation components and other DS-owned data kept there).
pub fn default_root() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("DS_LAYER_HOME") {
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            return Err("DS_LAYER_HOME must be absolute".into());
        }
        private::tighten_root_once(&root);
        return Ok(root);
    }
    let namespace = dirs::data_local_dir()
        .map(|p| p.join("ds"))
        .ok_or_else(|| "local data directory is unavailable".to_string())?;
    private::tighten_root_once(&namespace);
    Ok(namespace.join("layers"))
}
pub fn new_id() -> String {
    format!("tile-{}", uuid::Uuid::new_v4())
}
pub fn execute(edit: OverlayEdit) -> Result<Value, String> {
    execute_at(&default_root()?, edit)
}
fn io_error(_: impl std::fmt::Display) -> String {
    "local overlay store could not be read or persisted".into()
}
pub fn execute_at(root: &Path, edit: OverlayEdit) -> Result<Value, String> {
    let path = root.join("overlays.json");
    let reading = matches!(edit, OverlayEdit::List);
    if reading && !path.try_exists().map_err(io_error)? {
        let mut result = OverlayRegistry::default().apply(edit)?;
        result["persisted"] = serde_json::json!("native_local");
        return Ok(result);
    }
    // Existing stores serialize reads with writers. An absent read creates nothing.
    if !reading {
        private::create_dir_all(root).map_err(io_error)?;
    }
    let lock = private::open_options()
        .create(!reading)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("overlays.lock"))
        .map_err(io_error)?;
    lock.lock_exclusive().map_err(io_error)?;
    let mut registry = match File::open(&path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            if bytes.len() > 1024 * 1024 {
                return Err("local overlay store exceeds 1 MiB".into());
            }
            serde_json::from_slice::<OverlayRegistry>(&bytes)
                .map_err(|_| "local overlay store is malformed; repair it before editing")?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => OverlayRegistry::default(),
        Err(e) => return Err(io_error(e)),
    };
    let mut result = registry.apply(edit)?;
    if result["changed"] == true {
        let mut staged = tempfile::NamedTempFile::new_in(root).map_err(io_error)?;
        serde_json::to_writer(&mut staged, &registry).map_err(io_error)?;
        staged.flush().map_err(io_error)?;
        staged.as_file().sync_all().map_err(io_error)?;
        staged.persist(&path).map_err(io_error)?;
        #[cfg(unix)]
        File::open(root)
            .and_then(|f| f.sync_all())
            .map_err(io_error)?;
    }
    result["persisted"] = serde_json::json!("native_local");
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn row(id: &str) -> Overlay {
        Overlay {
            id: id.into(),
            name: id.into(),
            kind: "xyz".into(),
            url: "https://example.test/{z}/{x}/{y}".into(),
            tile_size: 256,
            attribution: None,
            visible: true,
        }
    }
    #[test]
    fn concurrent_clients_keep_all_edits_and_desktop_import() {
        let tmp = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            for n in 0..12 {
                let root = tmp.path();
                scope.spawn(move || {
                    execute_at(
                        root,
                        OverlayEdit::Add {
                            layer: row(&format!("r{n}")),
                        },
                    )
                    .unwrap();
                });
            }
        });
        let read = execute_at(
            tmp.path(),
            OverlayEdit::Import {
                layers: vec![row("legacy")],
            },
        )
        .unwrap();
        assert_eq!(read["layers"].as_array().unwrap().len(), 13);
        execute_at(
            tmp.path(),
            OverlayEdit::Remove {
                id: "legacy".into(),
            },
        )
        .unwrap();
        let read = execute_at(
            tmp.path(),
            OverlayEdit::Import {
                layers: vec![row("legacy")],
            },
        )
        .unwrap();
        assert_eq!(read["layers"].as_array().unwrap().len(), 12);
        #[cfg(unix)]
        crate::private::assert_private_tree(tmp.path());
    }
    #[test]
    fn corrupt_store_is_never_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("overlays.json");
        fs::write(&file, b"broken").unwrap();
        assert!(execute_at(tmp.path(), OverlayEdit::Add { layer: row("a") }).is_err());
        assert_eq!(fs::read(file).unwrap(), b"broken");
    }
}

#[cfg(test)]
mod read_test {
    #[test]
    fn absent_catalogue_read_creates_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("not-created");
        let result = super::execute_at(&root, super::OverlayEdit::List).unwrap();
        assert_eq!(result["layers"], serde_json::json!([]));
        assert!(!root.exists());
    }
}

/// The path a SQLite connection is opened with.
///
/// `std::fs` reaches a Windows path of any length because Rust converts an
/// absolute path to the `\\?\` extended form before it calls Win32. SQLite
/// does not: rusqlite hands the path to the C library, which calls
/// `CreateFileW` with whatever string it was given, so a path over Windows'
/// 260-character limit fails to open a database in a directory `fs` has just
/// created successfully.
///
/// A staged reference install lands exactly there. Its directory is
/// `<local app data>/<product>/reference-bundle-staging/reference-bundle-<64
/// hex>-<64 hex>/<temp>/reference-query-cache/v2/<64 hex>/<64 hex>/`, which is
/// past 400 characters before the file name — so every index a Windows install
/// builds is on the wrong side of that limit, which is why installing one
/// failed there and nowhere else.
///
/// The conversion is lexical (`std::path::absolute`), never `canonicalize`:
/// resolving links is what this store spends `symlink_metadata` calls refusing
/// to do, and it must not be reintroduced by a path helper. A relative path or
/// one that already carries the prefix is returned unchanged, and non-Windows
/// targets are untouched.
pub fn sqlite_path(path: &Path) -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        const VERBATIM: &str = r"\\?\";
        const DEVICE: &str = r"\\.\";
        let absolute = std::path::absolute(path).map_err(|error| error.to_string())?;
        let text = absolute.to_string_lossy().into_owned();
        if text.starts_with(VERBATIM) || text.starts_with(DEVICE) {
            return Ok(PathBuf::from(text));
        }
        if let Some(share) = text.strip_prefix(r"\\") {
            // A UNC path takes the `\\?\UNC\server\share` spelling.
            return Ok(PathBuf::from(format!("{VERBATIM}UNC\\{share}")));
        }
        Ok(PathBuf::from(format!("{VERBATIM}{text}")))
    }
    #[cfg(not(windows))]
    {
        Ok(path.to_path_buf())
    }
}

pub mod asset_catalogue;
pub mod grid_project_catalogue;
pub mod local_models;
pub mod pm_graph_catalogue;
pub mod prepared;
pub mod private;
pub mod project_dataset_cache;
pub mod project_snapshot;
pub mod reference_cache;
pub mod scoped;
pub mod snapshot_privacy;
pub mod visibility;
pub mod working_area_forms;

// The prepared local layer catalogue answers with the kernel's own types, so a
// host names them once and never re-declares them.
pub use ds_command_kernel::local_layers::{
    GeometryType, Host as PreparedHost, LocalLayer, Registry as PreparedRegistry,
    Scope as PreparedScope,
};
pub use prepared::{Op as PreparedOp, Register as PreparedRegister, StoreError as PreparedError};

/// An exclusive advisory lock on `path`, held until the returned file drops.
/// For a store beside these (the survey hold) that keeps its bytes under the
/// same root and must not take a locking dependency of its own.
pub fn exclusive_lock(path: &Path) -> Result<File, String> {
    if let Some(parent) = path.parent() {
        private::create_dir_all(parent).map_err(io_error)?;
    }
    let lock = private::open_options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(io_error)?;
    lock.lock_exclusive().map_err(io_error)?;
    Ok(lock)
}
