//! Closed native module installation receipts and compatibility decisions.
//! Metadata validation alone never mints installation authority: the native
//! updater owns acquisition from the captured release authority and its files.
pub use crate::module_engine::platform_target;
use crate::module_engine::{admit_engine_build, check_build_info_size};
use crate::module_release::{
    validate_module_release, ModuleComponent, ModuleRelease, ModuleReleaseAsset,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const INSTALLED_SCHEMA: &str = "ds.module-installation/v1";
pub const SELECTION_SCHEMA: &str = "ds.module-selection/v1";
pub const REMOVED_SELECTION_SCHEMA: &str = "ds.module-selection/v2";
pub const MAX_SELECTION_BYTES: usize = 384 * 1024;
pub const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseAuthorityReceipt {
    pub gateway_origin: String,
    pub native_catalog_sha256: String,
    pub release_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstalledModule {
    pub schema: String,
    pub release: ModuleRelease,
    pub platform: String,
    pub authority: ReleaseAuthorityReceipt,
    pub engine_identity: Value,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModuleSelection {
    pub schema: String,
    pub component: ModuleComponent,
    pub channel: String,
    pub generation: u64,
    pub active: InstalledModule,
    pub previous: Option<InstalledModule>,
}
/// Removed is a new closed record shape: v1 readers refuse it rather than
/// treating it as an absent selection and discovering an unmanaged executable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemovedModuleSelection {
    pub schema: String,
    pub component: ModuleComponent,
    pub channel: String,
    pub generation: u64,
    pub state: RemovedModuleState,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovedModuleState {
    Removed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModuleSelectionRecord {
    Installed(ModuleSelection),
    Removed(RemovedModuleSelection),
}
impl ModuleSelectionRecord {
    pub fn generation(&self) -> u64 {
        match self {
            Self::Installed(s) => s.generation,
            Self::Removed(s) => s.generation,
        }
    }
    pub fn installed(self) -> Result<ModuleSelection, String> {
        match self {
            Self::Installed(s) => Ok(s),
            Self::Removed(_) => Err(module_removed()),
        }
    }
}
pub fn module_removed() -> String {
    "module_removed: this component was removed from the selected lane; explicitly reinstall it before use".into()
}
pub fn validate_selection_record(
    record: &ModuleSelectionRecord,
    component: ModuleComponent,
    channel: &str,
    platform: &str,
) -> Result<(), String> {
    match record {
        ModuleSelectionRecord::Installed(s) => validate_selection(s, component, channel, platform),
        ModuleSelectionRecord::Removed(s) => {
            platform_target(platform)?;
            if s.schema != REMOVED_SELECTION_SCHEMA
                || s.component != component
                || s.channel != channel
                || !matches!(channel, "stable" | "canary")
                || s.generation == 0
            {
                return Err("module_receipt_invalid: crossed or empty removed selection".into());
            }
            Ok(())
        }
    }
}
/// Host-owned admission facts. Already captured jobs retain their immutable
/// executable paths and do not count as an uncaptured persistent session.
#[derive(Clone, Copy, Debug, Default)]
pub struct ActivationGuard {
    pub unsaved_edits: bool,
    pub uncaptured_sessions: usize,
    pub host_restart_required: bool,
}
impl ActivationGuard {
    pub fn admit(self) -> Result<(), String> {
        if self.host_restart_required || self.unsaved_edits || self.uncaptured_sessions != 0 {
            return Err("module_activation_deferred: retain held work; finish or checkpoint the owning session before activation".into());
        }
        Ok(())
    }
}
fn hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn selected_asset(receipt: &InstalledModule) -> Result<&ModuleReleaseAsset, String> {
    receipt
        .release
        .assets
        .iter()
        .find(|asset| asset.platform == receipt.platform)
        .ok_or_else(|| "module_platform_unsupported: release has no selected native asset".into())
}
pub fn executable_name(component: ModuleComponent, platform: &str) -> Result<String, String> {
    platform_target(platform)?;
    Ok(format!(
        "{}{}",
        component.executable(),
        if platform == "windows" { ".exe" } else { "" }
    ))
}
/// Check the exact qualified probe against its producer facts. No executable
/// is started here, and unsupported future capabilities grant no operation.
pub fn validate_engine_identity(receipt: &InstalledModule) -> Result<(), String> {
    let asset = selected_asset(receipt)?;
    check_build_info_size(&receipt.engine_identity)?;
    let producer = asset
        .engines
        .first()
        .ok_or("module_engine_incompatible: absent producer identity")?;
    let admitted = admit_engine_build(
        receipt.release.component,
        &receipt.platform,
        &receipt.engine_identity,
        Some(&producer.release),
    )?;
    if admitted.source_sha != asset.source_sha
        || admitted.manifest_sha256 != producer.manifest_sha256
        || producer.release != admitted.release
    {
        return Err("module_engine_incompatible: probe source, closure or release differs from its exact producer".into());
    }
    Ok(())
}
pub fn validate_installed(
    receipt: &InstalledModule,
    component: ModuleComponent,
    channel: &str,
    platform: &str,
) -> Result<(), String> {
    if receipt.schema != INSTALLED_SCHEMA
        || receipt.release.component != component
        || receipt.release.channel != channel
        || receipt.platform != platform
        || receipt.release.status != "active"
    {
        return Err(
            "module_receipt_invalid: crossed installation identity or inactive admitted release"
                .into(),
        );
    }
    validate_module_release(
        component.as_str(),
        channel,
        &serde_json::to_value(&receipt.release).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let authority = url::Url::parse(&receipt.authority.gateway_origin)
        .map_err(|_| "module_receipt_invalid: invalid release authority")?;
    if authority.scheme() != "https"
        || !authority
            .host_str()
            .is_some_and(|h| h.ends_with(".ue.gateway.dev"))
        || authority.port().is_some()
        || !authority.username().is_empty()
        || authority.password().is_some()
        || authority.path() != "/"
        || authority.query().is_some()
        || authority.fragment().is_some()
        || !hex(&receipt.authority.native_catalog_sha256, 64)
        || !hex(&receipt.authority.release_sha256, 64)
    {
        return Err("module_receipt_invalid: incomplete native authority provenance".into());
    }
    if selected_asset(receipt)?.size_bytes as u64 > MAX_EXECUTABLE_BYTES {
        return Err("module_receipt_invalid: executable exceeds the native bound".into());
    }
    use sha2::{Digest, Sha256};
    let release_digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&receipt.release).map_err(|e| e.to_string())?)
    );
    if receipt.authority.release_sha256 != release_digest {
        return Err("module_receipt_invalid: admitted publication facts changed".into());
    }
    validate_engine_identity(receipt)
}
pub fn validate_selection(
    selection: &ModuleSelection,
    component: ModuleComponent,
    channel: &str,
    platform: &str,
) -> Result<(), String> {
    if selection.schema != SELECTION_SCHEMA
        || selection.component != component
        || selection.channel != channel
        || selection.generation == 0
    {
        return Err("module_receipt_invalid: crossed or empty native selection".into());
    }
    validate_installed(&selection.active, component, channel, platform)?;
    if let Some(previous) = &selection.previous {
        validate_installed(previous, component, channel, platform)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "module_install_tests.rs"]
mod tests;
