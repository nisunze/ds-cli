//! Lane-explicit lookup of native-owned immutable addon selection receipts.
use super::*;
use ds_command_kernel::module_install::{self, ModuleSelection};
use ds_command_kernel::module_release::ModuleComponent;
use sha2::{Digest, Sha256};
use std::io::Read;

pub(crate) struct ManagedIdentity {
    pub size: u64,
    pub sha256: String,
}
fn invalid(message: impl Into<String>) -> Failure {
    Failure::unavailable("module_receipt_invalid", message)
        .remedy("retain the selected lane; repair or roll back its verified module installation")
}
/// One machine-owned base shared by the native installer and explicit resolver.
/// It does not choose an account, project or release lane.
pub fn module_base() -> Result<PathBuf, Failure> {
    #[cfg(windows)]
    let base =
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("DataSolutions/addons"));
    #[cfg(not(windows))]
    let base = std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/lib/ds-addons"));
    base.filter(|p| p.is_absolute())
        .ok_or_else(|| invalid("the native addon base is unavailable"))
}
fn component(owner: &External) -> Result<ModuleComponent, Failure> {
    [
        ModuleComponent::Solar,
        ModuleComponent::Reporter,
        ModuleComponent::Network,
    ]
    .into_iter()
    .find(|c| c.engine_owner() == owner.owner && c.executable() == owner.name)
    .ok_or_else(|| {
        Failure::invalid(
            "module_component_invalid",
            "only the three declared addons have managed selection",
        )
    })
}
fn metadata(path: &Path, directory: bool) -> Result<(), Failure> {
    let m = std::fs::symlink_metadata(path)
        .map_err(|_| invalid("managed module path is absent or unavailable"))?;
    #[cfg(windows)]
    let link = {
        use std::os::windows::fs::MetadataExt;
        m.file_type().is_symlink() || m.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let link = m.file_type().is_symlink();
    if link || (directory && !m.is_dir()) || (!directory && !m.is_file()) {
        Err(invalid(
            "managed module path is linked, reparsed or the wrong type",
        ))
    } else {
        Ok(())
    }
}
fn read_file(path: &Path) -> Result<std::fs::File, Failure> {
    metadata(path, false)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(1);
    }
    options
        .open(path)
        .map_err(|_| invalid("managed module file could not be opened safely"))
}
pub(crate) fn verify(path: &Path, identity: &ManagedIdentity) -> Result<(), Failure> {
    let mut file = read_file(path)?;
    if file
        .metadata()
        .map_err(|_| invalid("managed executable is unavailable"))?
        .len()
        != identity.size
        || identity.size == 0
        || identity.size > module_install::MAX_EXECUTABLE_BYTES
    {
        return Err(invalid(
            "managed module size differs from its retained receipt",
        ));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut count = 0u64;
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| invalid("managed executable could not be verified"))?;
        if n == 0 {
            break;
        }
        count = count.saturating_add(n as u64);
        if count > identity.size {
            return Err(invalid("managed module grew beyond its retained receipt"));
        }
        hash.update(&buffer[..n]);
    }
    if count != identity.size || format!("{:x}", hash.finalize()) != identity.sha256 {
        return Err(invalid(
            "managed module bytes differ from their retained receipt",
        ));
    }
    Ok(())
}
pub(crate) fn selected(
    owner: &External,
    base: &Path,
    lane: &str,
) -> Result<Option<(PathBuf, ManagedIdentity)>, Failure> {
    if !matches!(lane, "stable" | "canary") || !base.is_absolute() {
        return Err(Failure::invalid(
            "module_lane_invalid",
            "capture exactly stable or canary from an absolute native addon base",
        ));
    }
    let component = component(owner)?;
    let root = base.join(component.as_str()).join(lane);
    let selection = root.join("selection.json");
    match std::fs::symlink_metadata(&selection) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(invalid("managed selection is unavailable")),
        Ok(_) => {}
    }
    for dir in [
        base.to_path_buf(),
        base.join(component.as_str()),
        root.clone(),
        root.join("versions"),
    ] {
        metadata(&dir, true)?;
    }
    let mut bytes = Vec::new();
    let file = read_file(&selection)?;
    if file
        .metadata()
        .map_err(|_| invalid("selection is unavailable"))?
        .len()
        > module_install::MAX_SELECTION_BYTES as u64
    {
        return Err(invalid("managed selection exceeds its bound"));
    }
    file.take(module_install::MAX_SELECTION_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("selection cannot be read"))?;
    if bytes.len() > module_install::MAX_SELECTION_BYTES {
        return Err(invalid("managed selection grew beyond its bound"));
    }
    let selected: ModuleSelection =
        serde_json::from_slice(&bytes).map_err(|_| invalid("managed selection is malformed"))?;
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return Err(invalid("managed platform is unsupported"));
    };
    module_install::validate_selection(&selected, component, lane, platform).map_err(invalid)?;
    let asset = module_install::selected_asset(&selected.active).map_err(invalid)?;
    let directory = root.join("versions").join(&asset.sha256);
    metadata(&directory, true)?;
    let path =
        directory.join(module_install::executable_name(component, platform).map_err(invalid)?);
    let identity = ManagedIdentity {
        size: asset.size_bytes as u64,
        sha256: asset.sha256.clone(),
    };
    verify(&path, &identity)?;
    Ok(Some((path, identity)))
}
