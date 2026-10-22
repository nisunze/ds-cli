//! UI WASM unit admission. Hosts acquire and hash bytes; this owner decides
//! which complete unit and retained selection may be used. Native executable
//! receipts are deliberately not accepted here. Public metadata alone never
//! grants installation, execution or project authority.
use super::*;
// The UI component catalog and identity rule are shared with release
// packaging (module_engine); these paths remain the public names.
pub use crate::module_engine::{UiComponent, UiEngineIdentity};
use crate::module_engine::{admit_ui_identity, parse_ui_identity};
use crate::module_install::ActivationGuard;
use sha2::{Digest, Sha256};

pub const PROTOCOL: &str = "ds.ui-wasm/v1";
pub const INSTALLATION_SCHEMA: &str = "ds.ui-module-installation/v1";
pub const SELECTION_SCHEMA: &str = "ds.ui-module-selection/v1";
pub const MAX_WIRE_BYTES: usize = 384 * 1024;
const MAX_UNIT_BYTES: i64 = 128 * 1024 * 1024;
const MAX_GENERATION: u64 = crate::desktop_instance::MAX_GENERATION;
impl UiComponent {
    pub fn parse(value: &str) -> Result<Self, ModuleReleaseError> {
        match value {
            "interactions" => Ok(Self::Interactions),
            "command-kernel" => Ok(Self::CommandKernel),
            _ => fail(
                "module_release_component_invalid",
                "Expected interactions or command-kernel",
            ),
        }
    }
    pub fn files(self) -> [(&'static str, &'static str); 4] {
        match self {
            Self::Interactions => [
                ("ds_interactions_wasm.js", "module-javascript"),
                ("ds_interactions_wasm_bg.wasm", "module-wasm"),
                ("ds_interactions_wasm.d.ts", "module-types"),
                ("ds_interactions_wasm_bg.wasm.d.ts", "module-types"),
            ],
            Self::CommandKernel => [
                ("ds_command_kernel.js", "module-javascript"),
                ("ds_command_kernel_bg.wasm", "module-wasm"),
                ("ds_command_kernel.d.ts", "module-types"),
                ("ds_command_kernel_bg.wasm.d.ts", "module-types"),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiRelease {
    pub schema: String,
    pub component: UiComponent,
    pub runtime: String,
    pub channel: String,
    pub product_id: String,
    pub version: String,
    pub protocol: String,
    pub build_manifest: UiEngineIdentity,
    pub released_at: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub assets: Vec<ModuleReleaseAsset>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "kebab-case")]
pub enum UiScope {
    #[default]
    Release,
    LocalDev,
}
impl UiScope {
    pub fn parse(value: &str) -> Result<Self, ModuleReleaseError> {
        match value {
            "release" => Ok(Self::Release),
            "local-dev" => Ok(Self::LocalDev),
            _ => fail(
                "module_scope_invalid",
                "Select release or local-dev UI scope",
            ),
        }
    }
}
pub use super::browser::{BrowserAuthority as PublishedUiAuthority, VerifiedFile};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalUiBuild {
    pub schema: String,
    pub scope: UiScope,
    pub component: UiComponent,
    pub channel: String,
    pub identity: UiEngineIdentity,
    pub files: Vec<VerifiedFile>,
}
/// Published JSON retains its original shape. A local build has no release,
/// publication URL, governance status or gateway acquisition assertion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum UiUnit {
    Published {
        release: UiRelease,
        unit_sha256: String,
        total_bytes: i64,
    },
    LocalBuild {
        local_build: LocalUiBuild,
        unit_sha256: String,
        total_bytes: i64,
    },
}
impl UiUnit {
    pub fn published(&self) -> Option<&UiRelease> {
        match self {
            Self::Published { release, .. } => Some(release),
            _ => None,
        }
    }
    pub fn component(&self) -> UiComponent {
        match self {
            Self::Published { release, .. } => release.component,
            Self::LocalBuild { local_build, .. } => local_build.component,
        }
    }
    pub fn channel(&self) -> &str {
        match self {
            Self::Published { release, .. } => &release.channel,
            Self::LocalBuild { local_build, .. } => &local_build.channel,
        }
    }
    pub fn scope(&self) -> UiScope {
        match self {
            Self::Published { .. } => UiScope::Release,
            Self::LocalBuild { .. } => UiScope::LocalDev,
        }
    }
    pub fn identity(&self) -> &UiEngineIdentity {
        match self {
            Self::Published { release, .. } => &release.build_manifest,
            Self::LocalBuild { local_build, .. } => &local_build.identity,
        }
    }
    pub fn unit_sha256(&self) -> &str {
        match self {
            Self::Published { unit_sha256, .. } | Self::LocalBuild { unit_sha256, .. } => {
                unit_sha256
            }
        }
    }
    pub fn total_bytes(&self) -> i64 {
        match self {
            Self::Published { total_bytes, .. } | Self::LocalBuild { total_bytes, .. } => {
                *total_bytes
            }
        }
    }
    pub fn version(&self) -> String {
        match self {
            Self::Published { release, .. } => release.version.clone(),
            Self::LocalBuild { local_build, .. } => {
                format!("local-{}", local_build.identity.source_sha)
            }
        }
    }
    pub fn files(&self) -> Vec<VerifiedFile> {
        match self {
            Self::Published { release, .. } => release
                .assets
                .iter()
                .map(|asset| VerifiedFile {
                    filename: asset.url.rsplit('/').next().unwrap_or_default().into(),
                    sha256: asset.sha256.clone(),
                    size_bytes: asset.size_bytes,
                })
                .collect(),
            Self::LocalBuild { local_build, .. } => local_build.files.clone(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalUiProvenance {
    pub scope: UiScope,
    pub source_sha: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum UiAuthority {
    Published(PublishedUiAuthority),
    LocalBuild { local_build: LocalUiProvenance },
}
impl UiAuthority {
    pub fn gateway_origin(&self) -> &str {
        match self {
            Self::Published(authority) => &authority.gateway_origin,
            Self::LocalBuild { .. } => "",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledUiModule {
    pub schema: String,
    pub unit: UiUnit,
    pub authority: UiAuthority,
    pub engine_identity: UiEngineIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiSelectionState {
    Installed {
        active: Box<InstalledUiModule>,
        previous: Option<Box<InstalledUiModule>>,
    },
    Removed {
        previous: Option<Box<InstalledUiModule>>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSelection {
    pub schema: String,
    pub component: UiComponent,
    pub channel: String,
    pub generation: u64,
    pub selection: UiSelectionState,
}

/// A job keeps this exact unit even if the host's selection changes later.
/// Capturing a component does not grant principal/project or write authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCapture {
    pub generation: u64,
    pub receipt: InstalledUiModule,
}

pub const CAPTURE_BYTES_SCHEMA: &str = "ds.ui-capture-bytes/v1";

/// Bootstrap projection of an admitted Core capture. Receipt provenance stays
/// opaque; this descriptor never grants installation or execution authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct UiCaptureByteDescriptor {
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(type = "'ds.ui-capture-bytes/v1'"))]
    pub schema: String,
    pub component: UiComponent,
    pub channel: String,
    pub scope: UiScope,
    pub unit_sha256: String,
    pub generation: u64,
    pub files: Vec<VerifiedFile>,
}

/// Core has already verified its retained bytes. Re-admit the complete capture
/// before projecting its bounded facts for a loader that has not loaded Kernel.
pub fn capture_byte_descriptor(
    captured: &UiCapture,
) -> Result<UiCaptureByteDescriptor, ModuleReleaseError> {
    validate_capture(captured)?;
    let unit = &captured.receipt.unit;
    Ok(UiCaptureByteDescriptor {
        schema: CAPTURE_BYTES_SCHEMA.into(),
        component: unit.component(),
        channel: unit.channel().into(),
        scope: unit.scope(),
        unit_sha256: unit.unit_sha256().into(),
        generation: captured.generation,
        files: unit.files(),
    })
}

pub fn capture(
    current: Option<&UiSelection>,
    component: UiComponent,
    lane: &str,
    gateway_origin: &str,
) -> Result<UiCapture, ModuleReleaseError> {
    channel(lane)?;
    let selected = current.ok_or_else(|| {
        refuse(
            "module_not_installed",
            "Install and activate UI component before processing",
        )
    })?;
    validate_selection(selected, component, lane)?;
    let UiSelectionState::Installed { active, .. } = &selected.selection else {
        return fail(
            "module_removed",
            "UI component was explicitly removed; restore it before processing",
        );
    };
    let receipt = admit_retained(active, lane, gateway_origin)?;
    Ok(UiCapture {
        generation: selected.generation,
        receipt,
    })
}

/// Re-admit the bytes and identity loaded inside a captured worker. Selection
/// changes cannot retarget a captured job, and this does not grant write authority.
pub fn admit_loaded(
    captured: &UiCapture,
    verified: &[VerifiedFile],
    identity: &Value,
) -> Result<(), ModuleReleaseError> {
    validate_capture(captured)?;
    verify_files(&captured.receipt.unit, verified)?;
    if parse_identity(identity)? != captured.receipt.engine_identity {
        return fail(
            "module_engine_incompatible",
            "Loaded UI identity differs from the captured installation",
        );
    }
    Ok(())
}

fn validate_capture(captured: &UiCapture) -> Result<(), ModuleReleaseError> {
    if captured.generation == 0 || captured.generation > MAX_GENERATION {
        return fail("module_receipt_invalid", "Invalid captured UI generation");
    }
    validate_installation(&captured.receipt, captured.receipt.unit.channel())
}

fn refuse(code: &'static str, message: &str) -> ModuleReleaseError {
    ModuleReleaseError {
        code,
        message: message.into(),
    }
}
fn channel(value: &str) -> Result<(), ModuleReleaseError> {
    if matches!(value, "stable" | "canary") {
        Ok(())
    } else {
        fail(
            "module_release_channel_invalid",
            "Channel must be stable or canary",
        )
    }
}
pub fn admit_target(component: &str, lane: &str) -> Result<(), ModuleReleaseError> {
    channel(lane)?;
    UiComponent::parse(component).map(|_| ())
}
fn validate_provenance(
    component: UiComponent,
    asset: &ModuleReleaseAsset,
) -> Result<(), ModuleReleaseError> {
    let repositories = Regex::new(r"^ds-[a-z][a-z0-9-]{0,60}$").unwrap();
    let [engine] = asset.engines.as_slice() else {
        return fail(
            "module_release_provenance_invalid",
            "Expected exactly one UI producer identity",
        );
    };
    if !hex(&asset.source_sha, 40)
        || asset.source_revisions.get(component.owner()) != Some(&asset.source_sha)
        || asset
            .source_revisions
            .iter()
            .any(|(owner, sha)| !repositories.is_match(owner) || !hex(sha, 40))
        || OffsetDateTime::parse(&asset.built_at, &Rfc3339).is_err()
        || engine.engine != component.owner()
        || engine.release.is_empty()
        || engine.release.len() > 200
        || !hex(&engine.manifest_sha256, 64)
    {
        return fail(
            "module_release_provenance_invalid",
            "UI asset requires its exact producer source closure and build identity",
        );
    }
    Ok(())
}
/// Decode the closed identity before admitting compiler/source facts.
pub fn parse_identity(value: &Value) -> Result<UiEngineIdentity, ModuleReleaseError> {
    parse_ui_identity(value).map_err(|error| {
        refuse(
            "module_engine_incompatible",
            error
                .strip_prefix("module_engine_incompatible: ")
                .unwrap_or(&error),
        )
    })
}

pub fn validate_identity(
    unit: &UiUnit,
    identity: &UiEngineIdentity,
) -> Result<(), ModuleReleaseError> {
    validate_unit(unit)?;
    if identity != unit.identity() {
        return fail(
            "module_engine_incompatible",
            "Loaded UI identity differs from its producer manifest",
        );
    }
    if let Some(release) = unit.published() {
        validate_build_manifest(release, identity)?;
    }
    Ok(())
}

/// Construct a local unit from acquired producer facts. No publication or
/// gateway authority is inferred; only the canary local-dev namespace admits it.
pub fn local_unit(
    component: UiComponent,
    lane: &str,
    source_sha: &str,
    identity: &Value,
    files: &[VerifiedFile],
) -> Result<UiUnit, ModuleReleaseError> {
    if lane != "canary" || !hex(source_sha, 40) {
        return fail(
            "module_scope_invalid",
            "Local UI builds require canary local-dev scope and an exact source snapshot",
        );
    }
    let identity = parse_identity(identity)?;
    admit_ui_identity(component, &identity, Some(source_sha))
        .map_err(|error| refuse("module_engine_incompatible", &error))?;
    if files.len() != 4 {
        return fail(
            "module_artifact_invalid",
            "Local UI build requires all four fixed members",
        );
    }
    let mut total = 0_i64;
    for (name, _) in component.files() {
        let matches: Vec<_> = files.iter().filter(|file| file.filename == name).collect();
        let [file] = matches.as_slice() else {
            return fail(
                "module_artifact_invalid",
                "Local UI members are missing, repeated or foreign",
            );
        };
        if !hex(&file.sha256, 64) || file.size_bytes <= 0 || file.size_bytes > MAX_UNIT_BYTES {
            return fail(
                "module_artifact_invalid",
                "Local UI member size or digest is invalid",
            );
        }
        total = total
            .checked_add(file.size_bytes)
            .ok_or_else(|| refuse("module_artifact_invalid", "Local UI size overflow"))?;
    }
    if total > MAX_UNIT_BYTES {
        return fail("module_artifact_invalid", "Local UI unit exceeds 128 MiB");
    }
    let mut files = files.to_vec();
    files.sort_by(|a, b| a.filename.cmp(&b.filename));
    let local_build = LocalUiBuild {
        schema: "ds.ui-local-build/v1".into(),
        scope: UiScope::LocalDev,
        component,
        channel: lane.into(),
        identity,
        files,
    };
    let unit_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&local_build).expect("typed local UI build"))
    );
    Ok(UiUnit::LocalBuild {
        local_build,
        unit_sha256,
        total_bytes: total,
    })
}

fn validate_unit(unit: &UiUnit) -> Result<(), ModuleReleaseError> {
    let expected = match unit {
        UiUnit::Published { release, .. } => inspect(
            release.component.as_str(),
            &release.channel,
            &serde_json::to_value(release).expect("typed release"),
        )?,
        UiUnit::LocalBuild { local_build, .. } => {
            if local_build.schema != "ds.ui-local-build/v1"
                || local_build.scope != UiScope::LocalDev
            {
                return fail(
                    "module_receipt_invalid",
                    "Local UI build provenance is invalid",
                );
            }
            local_unit(
                local_build.component,
                &local_build.channel,
                &local_build.identity.source_sha,
                &serde_json::to_value(&local_build.identity).expect("typed identity"),
                &local_build.files,
            )?
        }
    };
    if unit != &expected {
        return fail(
            "module_receipt_invalid",
            "UI unit differs from its complete producer facts",
        );
    }
    Ok(())
}

fn validate_build_manifest(
    release: &UiRelease,
    identity: &UiEngineIdentity,
) -> Result<(), ModuleReleaseError> {
    // The typed struct cannot carry unknown fields; validate its exact values.
    let asset = release.assets.first().ok_or_else(|| {
        refuse(
            "module_engine_incompatible",
            "UI producer receipt is absent",
        )
    })?;
    let producer = asset
        .engines
        .first()
        .ok_or_else(|| refuse("module_engine_incompatible", "UI build receipt is absent"))?;
    if admit_ui_identity(release.component, identity, Some(&asset.source_sha)).is_err()
        || identity.manifest_sha256() != producer.manifest_sha256
    {
        return fail(
            "module_engine_incompatible",
            "Loaded UI ABI, compiler, configuration or source differs from its producer receipt",
        );
    }
    Ok(())
}

fn filename(asset: &ModuleReleaseAsset) -> Result<String, ModuleReleaseError> {
    let url = Url::parse(&asset.url)
        .map_err(|_| refuse("module_release_url_invalid", "Invalid UI member URL"))?;
    Ok(url.path().rsplit('/').next().unwrap_or_default().to_owned())
}
fn unit_digest(release: &UiRelease) -> String {
    // Struct fields and BTreeMap provenance have the same order on native/WASM.
    // Catalog member order is irrelevant, but every member fact is bound.
    let mut ordered = release.clone();
    ordered.assets.sort_by(|a, b| a.url.cmp(&b.url));
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&ordered).expect("typed release"))
    )
}

pub fn inspect(
    component_name: &str,
    channel_name: &str,
    value: &Value,
) -> Result<UiUnit, ModuleReleaseError> {
    let component = UiComponent::parse(component_name)?;
    channel(channel_name)?;
    if !value
        .get("assets")
        .and_then(Value::as_array)
        .is_some_and(|assets| assets.len() == 4 && assets.iter().all(bounded_asset_shape))
    {
        return fail(
            "module_release_invalid",
            "UI members exceed their closed bounded shape",
        );
    }
    // Bound the required nested manifest before cloning the catalog.
    parse_identity(value.get("build_manifest").ok_or_else(|| {
        refuse(
            "module_release_invalid",
            "UI release requires its exact build manifest",
        )
    })?)?;
    if !value.as_object().is_some_and(|object| {
        object.iter().all(|(key, field)| {
            key == "build_manifest"
                || key == "assets"
                || ([
                    "schema",
                    "component",
                    "runtime",
                    "channel",
                    "product_id",
                    "version",
                    "protocol",
                    "released_at",
                    "status",
                    "notes",
                ]
                .contains(&key.as_str())
                    && field.as_str().is_some_and(|text| text.len() <= 8000))
        })
    }) || serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > MAX_WIRE_BYTES)
    {
        return fail(
            "module_release_invalid",
            "UI release exceeds its closed bounded shape",
        );
    }
    let release: UiRelease = serde_json::from_value(value.clone())
        .map_err(|_| refuse("module_release_invalid", "Invalid UI release schema"))?;
    let expected_product = format!(
        "rw.datasolutions.module.{}{}",
        component.as_str(),
        if channel_name == "canary" {
            ".canary"
        } else {
            ""
        }
    );
    if release.schema != MODULE_RELEASE_SCHEMA
        || release.runtime != "wasm"
        || release.component != component
        || release.channel != channel_name
        || release.product_id != expected_product
        || release.protocol != PROTOCOL
    {
        return fail(
            "module_release_identity_mismatch",
            "Expected this component's lane-bound UI catalog",
        );
    }
    if !release_folder(&release.version)
        || OffsetDateTime::parse(&release.released_at, &Rfc3339).is_err()
        || !matches!(
            release.status.as_str(),
            "active" | "blocked" | "stopped" | "rolled_back"
        )
        || release.assets.len() != 4
    {
        return fail(
            "module_release_invalid",
            "Invalid UI version, status or complete unit",
        );
    }
    let first = &release.assets[0];
    let first_url = Url::parse(&first.url)
        .map_err(|_| refuse("module_release_url_invalid", "Invalid UI member URL"))?;
    let bucket = first_url.path().split('/').nth(1).unwrap_or_default();
    let producer_release = format!(
        "{}@{}+{}",
        component.owner(),
        release.version.split("-r").next().unwrap_or_default(),
        first.source_sha
    );
    let mut seen = Vec::new();
    let mut total = 0_i64;
    for asset in &release.assets {
        if asset.platform != "browser"
            || asset.arch != "wasm32"
            || asset.protocol != PROTOCOL
            || asset.size_bytes <= 0
            || asset.size_bytes > MAX_UNIT_BYTES
            || !hex(&asset.sha256, 64)
            || asset.source_revisions.is_empty()
            || asset.source_revisions.len() > 16
        {
            return fail(
                "module_release_asset_invalid",
                "Invalid UI target, size, digest or closure",
            );
        }
        validate_provenance(component, asset)?;
        if asset.engines[0].release != producer_release {
            return fail(
                "module_release_provenance_invalid",
                "UI producer release must match version and source",
            );
        }
        if asset.source_sha != first.source_sha
            || asset.built_at != first.built_at
            || asset.source_revisions != first.source_revisions
            || asset.engines != first.engines
        {
            return fail(
                "module_release_provenance_invalid",
                "UI members cross producer closures",
            );
        }
        let url = Url::parse(&asset.url)
            .map_err(|_| refuse("module_release_url_invalid", "Invalid UI member URL"))?;
        let parts: Vec<_> = url.path().split('/').collect();
        let name = filename(asset)?;
        if url.scheme() != "https"
            || url.host_str() != Some("storage.googleapis.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || asset.url != url.as_str()
            || parts.len() != 8
            || parts[1].is_empty()
            || parts[1] != bucket
            || parts[2] != "modules"
            || parts[3] != component.as_str()
            || parts[4] != channel_name
            || parts[5] != "wasm"
            || parts[6] != release.version
            || !release
                .component
                .files()
                .iter()
                .any(|(file, kind)| *file == name && *kind == asset.kind)
            || seen.contains(&name)
        {
            return fail(
                "module_release_url_invalid",
                "Expected a unique immutable member of this complete UI unit",
            );
        }
        seen.push(name);
        total = total
            .checked_add(asset.size_bytes)
            .ok_or_else(|| refuse("module_release_asset_invalid", "UI unit size overflow"))?;
    }
    if total > MAX_UNIT_BYTES {
        return fail("module_release_asset_invalid", "UI unit exceeds 128 MiB");
    }
    validate_build_manifest(&release, &release.build_manifest)?;
    Ok(UiUnit::Published {
        unit_sha256: unit_digest(&release),
        release,
        total_bytes: total,
    })
}

fn validate_installation(
    receipt: &InstalledUiModule,
    lane: &str,
) -> Result<(), ModuleReleaseError> {
    validate_unit(&receipt.unit)?;
    if receipt.schema != INSTALLATION_SCHEMA || receipt.unit.channel() != lane {
        return fail(
            "module_receipt_invalid",
            "UI installation crosses its complete unit",
        );
    }
    match (&receipt.unit, &receipt.authority) {
        (UiUnit::Published { release, .. }, UiAuthority::Published(authority)) => {
            if release.status != "active" {
                return fail("module_receipt_invalid", "UI release is not active");
            }
            let origin = Url::parse(&authority.gateway_origin)
                .map_err(|_| refuse("module_receipt_invalid", "Invalid UI release authority"))?;
            if origin.scheme() != "https"
                || origin.host_str().is_none()
                || origin.path() != "/"
                || !origin.username().is_empty()
                || origin.password().is_some()
                || origin.query().is_some()
                || origin.fragment().is_some()
                || !hex(&authority.route_manifest_sha256, 64)
            {
                return fail(
                    "module_receipt_invalid",
                    "Invalid captured UI release authority",
                );
            }
        }
        (
            UiUnit::LocalBuild { local_build, .. },
            UiAuthority::LocalBuild {
                local_build: authority,
            },
        ) if authority.scope == UiScope::LocalDev
            && authority.source_sha == local_build.identity.source_sha => {}
        _ => {
            return fail(
                "module_receipt_invalid",
                "UI installation crosses publication and local-build provenance",
            );
        }
    }
    validate_identity(&receipt.unit, &receipt.engine_identity)?;
    Ok(())
}

/// Admit a retained receipt against this host's declared lane and authority.
/// This is offline validation; it grants no acquisition or computation authority.
pub fn admit_retained(
    receipt: &InstalledUiModule,
    channel: &str,
    gateway_origin: &str,
) -> Result<InstalledUiModule, ModuleReleaseError> {
    validate_installation(receipt, channel)?;
    if matches!(&receipt.authority, UiAuthority::Published(authority) if authority.gateway_origin != gateway_origin)
    {
        return fail(
            "module_receipt_invalid",
            "Retained UI authority differs from this host",
        );
    }
    Ok(receipt.clone())
}

/// Verify every member before executing even the JavaScript identity adapter.
/// Hosts supply hashes and sizes computed from acquired bytes, never catalog facts.
pub fn verify_files(unit: &UiUnit, verified: &[VerifiedFile]) -> Result<(), ModuleReleaseError> {
    validate_unit(unit)?;
    if verified.len() != 4 {
        return fail(
            "module_artifact_invalid",
            "Verify all four members before staging",
        );
    }
    for file in unit.files() {
        if verified.iter().filter(|actual| **actual == file).count() != 1 {
            return fail(
                "module_artifact_invalid",
                "UI member bytes differ from the captured unit",
            );
        }
    }
    Ok(())
}

pub fn prepare(
    unit: UiUnit,
    authority: UiAuthority,
    verified: &[VerifiedFile],
    consent: bool,
) -> Result<InstalledUiModule, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Deliberate consent is required before UI component acquisition",
        );
    }
    let Some(release) = unit.published() else {
        return fail(
            "module_scope_invalid",
            "Published staging cannot install a local UI build",
        );
    };
    if release.status != "active" {
        return fail(
            "module_release_withdrawn",
            "Only an active UI release may be staged",
        );
    }
    if !matches!(authority, UiAuthority::Published(_)) {
        return fail(
            "module_receipt_invalid",
            "Published staging requires its captured release authority",
        );
    }
    prepare_receipt(unit, authority, verified)
}

pub fn prepare_local(
    unit: UiUnit,
    source_sha: &str,
    verified: &[VerifiedFile],
    consent: bool,
) -> Result<InstalledUiModule, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Confirm local UI component staging",
        );
    }
    if unit.scope() != UiScope::LocalDev || unit.identity().source_sha != source_sha {
        return fail(
            "module_scope_invalid",
            "Local UI staging requires its exact producer source snapshot",
        );
    }
    prepare_receipt(
        unit,
        UiAuthority::LocalBuild {
            local_build: LocalUiProvenance {
                scope: UiScope::LocalDev,
                source_sha: source_sha.into(),
            },
        },
        verified,
    )
}

fn prepare_receipt(
    unit: UiUnit,
    authority: UiAuthority,
    verified: &[VerifiedFile],
) -> Result<InstalledUiModule, ModuleReleaseError> {
    verify_files(&unit, verified)?;
    let lane = unit.channel().to_owned();
    let engine_identity = unit.identity().clone();
    let receipt = InstalledUiModule {
        schema: INSTALLATION_SCHEMA.into(),
        unit,
        authority,
        engine_identity,
    };
    validate_installation(&receipt, &lane)?;
    Ok(receipt)
}

/// The host may issue asset requests only after this decision. A public catalog
/// can be inspected across lanes, but installation belongs to this compiled
/// application's lane and the exact unit the person confirmed.
pub fn acquisition_plan(
    unit: &UiUnit,
    host_channel: &str,
    expected_sha256: &str,
    consent: bool,
) -> Result<Vec<ModuleReleaseAsset>, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Confirm this UI component before acquisition",
        );
    }
    channel(host_channel)?;
    let Some(release) = unit.published() else {
        return fail(
            "module_scope_invalid",
            "Local UI builds cannot acquire published release authority",
        );
    };
    let expected = inspect(
        release.component.as_str(),
        host_channel,
        &serde_json::to_value(release).expect("typed release"),
    )?;
    if unit != &expected || unit.unit_sha256() != expected_sha256 {
        return fail(
            "module_selection_changed",
            "The UI release differs from the confirmed unit",
        );
    }
    if release.status != "active" {
        return fail("module_release_withdrawn", "The UI release is not active");
    }
    Ok(release.assets.clone())
}

pub fn validate_selection(
    selection: &UiSelection,
    component: UiComponent,
    lane: &str,
) -> Result<(), ModuleReleaseError> {
    channel(lane)?;
    if selection.schema != SELECTION_SCHEMA
        || selection.component != component
        || selection.channel != lane
        || selection.generation == 0
        || selection.generation > MAX_GENERATION
    {
        return fail(
            "module_receipt_invalid",
            "Invalid UI selection lane or generation",
        );
    }
    let (active, previous) = match &selection.selection {
        UiSelectionState::Installed { active, previous } => (Some(active), previous),
        UiSelectionState::Removed { previous } => (None, previous),
    };
    if let Some(active) = active {
        validate_installation(active, lane)?;
        if active.unit.component() != component {
            return fail("module_receipt_invalid", "Selection crosses UI component");
        }
    }
    if let Some(previous) = previous {
        validate_installation(previous, lane)?;
        if previous.unit.component() != component {
            return fail(
                "module_receipt_invalid",
                "Retained selection crosses UI component",
            );
        }
    }
    Ok(())
}
fn next_generation(
    current: Option<&UiSelection>,
    component: UiComponent,
    lane: &str,
    expected: u64,
) -> Result<u64, ModuleReleaseError> {
    if let Some(current) = current {
        validate_selection(current, component, lane)?;
    }
    let generation = current.map_or(0, |selection| selection.generation);
    if generation != expected || generation >= MAX_GENERATION {
        return fail(
            "module_selection_changed",
            "UI selection changed; inspect it before retrying",
        );
    }
    Ok(generation + 1)
}
pub fn activate(
    current: Option<&UiSelection>,
    staged: InstalledUiModule,
    expected: u64,
    consent: bool,
    guard: ActivationGuard,
) -> Result<UiSelection, ModuleReleaseError> {
    if !consent {
        return fail("module_consent_required", "Confirm UI component activation");
    }
    guard.admit().map_err(|_| {
        refuse(
            "module_activation_deferred",
            "Retain held work before changing the UI selection",
        )
    })?;
    let component = staged.unit.component();
    let lane = staged.unit.channel().to_owned();
    validate_installation(&staged, &lane)?;
    if current.is_some_and(|selection| match &selection.selection {
        UiSelectionState::Installed { active, previous } => {
            active.unit.scope() != staged.unit.scope()
                || previous
                    .as_ref()
                    .is_some_and(|previous| previous.unit.scope() != staged.unit.scope())
        }
        UiSelectionState::Removed { previous } => previous
            .as_ref()
            .is_some_and(|previous| previous.unit.scope() != staged.unit.scope()),
    }) {
        return fail(
            "module_scope_invalid",
            "UI activation cannot cross release and local-dev selections",
        );
    }
    let generation = next_generation(current, component, &lane, expected)?;
    if let Some(current) = current.filter(|current| matches!(&current.selection, UiSelectionState::Installed { active, .. } if active.unit.unit_sha256() == staged.unit.unit_sha256())) {
        return Ok(current.clone());
    }
    let previous = current.and_then(|selection| match &selection.selection {
        UiSelectionState::Installed { active, .. } => Some(active.clone()),
        UiSelectionState::Removed { previous } => previous.clone(),
    });
    Ok(UiSelection {
        schema: SELECTION_SCHEMA.into(),
        component,
        channel: lane,
        generation,
        selection: UiSelectionState::Installed {
            active: Box::new(staged),
            previous,
        },
    })
}
pub fn remove(
    current: &UiSelection,
    expected: u64,
    consent: bool,
) -> Result<UiSelection, ModuleReleaseError> {
    if !consent {
        return fail("module_consent_required", "Confirm UI component removal");
    }
    let generation = next_generation(Some(current), current.component, &current.channel, expected)?;
    let previous = match &current.selection {
        UiSelectionState::Installed { active, .. } => Some(active.clone()),
        UiSelectionState::Removed { .. } => {
            return fail("module_removed", "This UI component is already removed");
        }
    };
    Ok(UiSelection {
        generation,
        selection: UiSelectionState::Removed { previous },
        ..current.clone()
    })
}
pub fn rollback(
    current: &UiSelection,
    expected: u64,
    consent: bool,
    guard: ActivationGuard,
) -> Result<UiSelection, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Confirm restoration of retained UI bytes",
        );
    }
    guard.admit().map_err(|_| {
        refuse(
            "module_activation_deferred",
            "Retain held work before rollback",
        )
    })?;
    let generation = next_generation(Some(current), current.component, &current.channel, expected)?;
    let (active, previous) = match &current.selection {
        UiSelectionState::Installed {
            active,
            previous: Some(previous),
        } => (previous.clone(), Some(active.clone())),
        UiSelectionState::Removed {
            previous: Some(previous),
        } => (previous.clone(), None),
        _ => {
            return fail(
                "module_rollback_unavailable",
                "No retained UI installation is available",
            );
        }
    };
    Ok(UiSelection {
        generation,
        selection: UiSelectionState::Installed { active, previous },
        ..current.clone()
    })
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
