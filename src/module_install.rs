//! Closed native module installation receipts and compatibility decisions.
//! Metadata validation alone never mints installation authority: the native
//! updater owns acquisition from the captured release authority and its files.
use crate::module_release::{
    validate_module_release, ModuleComponent, ModuleRelease, ModuleReleaseAsset,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const INSTALLED_SCHEMA: &str = "ds.module-installation/v1";
pub const SELECTION_SCHEMA: &str = "ds.module-selection/v1";
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
fn field<'a>(v: &'a Value, name: &str) -> Result<&'a str, String> {
    v[name]
        .as_str()
        .ok_or_else(|| format!("module_engine_incompatible: missing {name}"))
}
pub fn platform_target(platform: &str) -> Result<&'static str, String> {
    match platform {
        "linux" => Ok("x86_64-unknown-linux-gnu"),
        "windows" => Ok("x86_64-pc-windows-msvc"),
        _ => Err(
            "module_platform_unsupported: use the admitted Linux or Windows x86_64 build".into(),
        ),
    }
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
    let v = &receipt.engine_identity;
    if serde_json::to_vec(v).map_err(|e| e.to_string())?.len() > 16 * 1024 {
        return Err("module_engine_incompatible: excessive build-info".into());
    }
    let component = receipt.release.component;
    let producer = asset
        .engines
        .first()
        .ok_or("module_engine_incompatible: absent producer identity")?;
    let (version, source, manifest) = if component == ModuleComponent::Network {
        if v["schema"] != "ds.engine-build/v1"
            || v["module"] != "network"
            || v["engineIdentity"]["schema"] != component.protocol()
            || v["engineIdentity"]["protocolMajor"] != 1
            || v["engineIdentity"]["environment"] != "native"
            || v["engineIdentity"]["runtime"]["environment"] != "native"
            || v["build"]["profile"] != "release"
            || v["build"]["target"] != platform_target(&receipt.platform)?
        {
            return Err(
                "module_engine_incompatible: Network native protocol or target differs".into(),
            );
        }
        if !hex(field(&v["build"], "cargoLockSha256")?, 64)
            || field(&v["build"], "rustc")?.is_empty()
            || field(&v["build"], "rustc")?.len() > 4096
        {
            return Err(
                "module_engine_incompatible: incomplete Network compiler provenance".into(),
            );
        }
        let engine = &v["engineIdentity"];
        let cpus = engine["runtime"]["logicalCpus"].as_u64().unwrap_or(0);
        let workers = engine["runtime"]["internalWorkerThreads"]
            .as_u64()
            .unwrap_or(0);
        if cpus == 0
            || cpus > 65536
            || workers == 0
            || workers > cpus
            || !matches!(
                engine["jsonObjectOrder"].as_str(),
                Some("sorted" | "insertion")
            )
        {
            return Err(
                "module_engine_incompatible: invalid Network capacity or JSON order".into(),
            );
        }
        require_tokens(
            &engine["capabilities"],
            &[
                "lv_processing_v1",
                "lv_publication_v1",
                "lv_analysis_validation_v1",
                "grid_query_v1",
                "grid_inspect_v1",
                "grid_apply_v1",
                "grid_publication_v1",
                "profile_session_v1",
                "report_engineering_v1",
                "report_sizing_v1",
                "report_feeder_selection_v1",
            ],
        )?;
        (
            field(engine, "coreVersion")?,
            field(engine, "sourceSha")?,
            field(engine, "dependencyClosureSha256")?,
        )
    } else {
        if v["schema"] != "ds.engine-build/v1"
            || v["profile"] != "release"
            || v["target"] != platform_target(&receipt.platform)?
            || !hex(field(v, "cargo_lock_sha256")?, 64)
            || field(v, "rustc")?.is_empty()
            || field(v, "rustc")?.len() > 4096
        {
            return Err("module_engine_incompatible: unqualified module build or target".into());
        }
        if component == ModuleComponent::Solar {
            if v["name"] != "ds-solar-engine" {
                return Err("module_engine_incompatible: different Solar owner".into());
            }
            require_tokens(
                &v["schemas"],
                &[
                    "ds-solar.prepared-calculation-result/v1",
                    "ds-solar.result/v1",
                    "ds-solar.prepared-city-input/v1",
                    "ds-solar.worker-budget/v1",
                ],
            )?;
            (
                field(v, "version")?,
                field(v, "source_sha")?,
                field(v, "build_manifest_sha256")?,
            )
        } else {
            if v["service"] != "ds-network-reporter"
                || v["binary"] != "ds-report"
                || v["features"] != serde_json::json!([])
                || v["engine_version"] != producer.release
            {
                return Err(
                    "module_engine_incompatible: different Reporter owner or non-compute build"
                        .into(),
                );
            }
            require_tokens(
                &v["schemas"],
                &["ds.engine-build/v1", "ds-network-reporter.task-result/v1"],
            )?;
            (
                field(v, "package_version")?,
                field(v, "source_sha")?,
                field(v, "build_manifest_sha256")?,
            )
        }
    };
    if source != asset.source_sha
        || !hex(source, 40)
        || manifest != producer.manifest_sha256
        || !hex(manifest, 64)
        || producer.release != format!("{}@{}+{}", component.engine_owner(), version, source)
    {
        return Err("module_engine_incompatible: probe source, closure or release differs from its exact producer".into());
    }
    Ok(())
}
fn require_tokens(value: &Value, required: &[&str]) -> Result<(), String> {
    let tokens = value
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .ok_or_else(|| "module_engine_incompatible: missing bounded capabilities".to_string())?;
    let mut seen = std::collections::BTreeSet::new();
    for token in tokens {
        let token = token
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .ok_or_else(|| "module_engine_incompatible: invalid capability token".to_string())?;
        if !seen.insert(token) {
            return Err("module_engine_incompatible: repeated capability".into());
        }
    }
    if required.iter().any(|token| !seen.contains(token)) {
        return Err(
            "module_engine_incompatible: required capability is absent; retain the installed unit"
                .into(),
        );
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
