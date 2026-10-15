//! UI WASM unit admission. Hosts acquire and hash bytes; this owner decides
//! which complete unit and retained selection may be used. Native executable
//! receipts are deliberately not accepted here. Public metadata alone never
//! grants installation, execution or project authority.
use super::*;
use crate::module_install::ActivationGuard;
use sha2::{Digest, Sha256};

pub const PROTOCOL: &str = "ds.ui-wasm/v1";
pub const INSTALLATION_SCHEMA: &str = "ds.ui-module-installation/v1";
pub const SELECTION_SCHEMA: &str = "ds.ui-module-selection/v1";
pub const MAX_WIRE_BYTES: usize = 384 * 1024;
const MAX_UNIT_BYTES: i64 = 128 * 1024 * 1024;
const MAX_GENERATION: u64 = crate::desktop_instance::MAX_GENERATION;
/// UI components are deliberately separate from the three native engines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiComponent {
    Interactions,
    CommandKernel,
}
impl UiComponent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactions => "interactions",
            Self::CommandKernel => "command-kernel",
        }
    }
    pub fn owner(self) -> &'static str {
        match self {
            Self::Interactions => "ds-interactions",
            Self::CommandKernel => "ds-command-kernel",
        }
    }
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

/// Compiler/source facts exported by the loaded WASM, in canonical digest order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiEngineIdentity {
    pub schema: String,
    pub component: UiComponent,
    pub protocol: String,
    pub ui_contract: u32,
    pub source_sha: String,
    pub cargo_lock_sha256: String,
    pub rustc: String,
    pub target: String,
    pub profile: String,
    pub config_sha256: String,
}
impl UiEngineIdentity {
    pub fn manifest_sha256(&self) -> String {
        // Canonical flat ten-key JSON: alphabetic keys, compact UTF-8, no newline.
        let value = serde_json::to_value(self).expect("typed UI identity");
        let ordered: BTreeMap<_, _> = value.as_object().expect("identity object").iter().collect();
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&ordered).expect("ordered UI identity"))
        )
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiUnit {
    pub release: UiRelease,
    pub unit_sha256: String,
    pub total_bytes: i64,
}
pub use super::browser::{BrowserAuthority as UiAuthority, VerifiedFile};
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
    if captured.generation == 0 || captured.generation > MAX_GENERATION {
        return fail("module_receipt_invalid", "Invalid captured UI generation");
    }
    validate_installation(&captured.receipt, &captured.receipt.unit.release.channel)?;
    verify_files(&captured.receipt.unit, verified)?;
    if parse_identity(identity)? != captured.receipt.engine_identity {
        return fail(
            "module_engine_incompatible",
            "Loaded UI identity differs from the captured installation",
        );
    }
    Ok(())
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
    const KEYS: [&str; 10] = [
        "schema",
        "component",
        "protocol",
        "ui_contract",
        "source_sha",
        "cargo_lock_sha256",
        "rustc",
        "target",
        "profile",
        "config_sha256",
    ];
    let valid = value.as_object().is_some_and(|object| {
        object.len() == KEYS.len()
            && object.iter().all(|(key, value)| {
                KEYS.contains(&key.as_str())
                    && if key == "ui_contract" {
                        value.as_u64().is_some()
                    } else {
                        value.as_str().is_some_and(|text| text.len() <= 4096)
                    }
            })
    });
    if !valid {
        return fail(
            "module_engine_incompatible",
            "UI identity exceeds its closed bounded shape",
        );
    }
    serde_json::from_value(value.clone())
        .map_err(|_| refuse("module_engine_incompatible", "Invalid UI identity schema"))
}

pub fn validate_identity(
    unit: &UiUnit,
    identity: &UiEngineIdentity,
) -> Result<(), ModuleReleaseError> {
    let expected = inspect(
        unit.release.component.as_str(),
        &unit.release.channel,
        &serde_json::to_value(&unit.release).expect("typed UI release"),
    )?;
    if unit != &expected {
        return fail(
            "module_receipt_invalid",
            "UI identity requires its exact complete catalog unit",
        );
    }
    if identity != &unit.release.build_manifest {
        return fail(
            "module_engine_incompatible",
            "Loaded UI identity differs from its trusted build manifest",
        );
    }
    validate_build_manifest(&unit.release, identity)
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
    if identity.schema != "ds.ui-engine-build/v1"
        || identity.component != release.component
        || identity.protocol != PROTOCOL
        || identity.ui_contract != 1
        || !hex(&identity.source_sha, 40)
        || identity.source_sha != asset.source_sha
        || !hex(&identity.cargo_lock_sha256, 64)
        || !hex(&identity.config_sha256, 64)
        || !valid_compiler_identity(&identity.rustc)
        || identity.target != "wasm32-unknown-unknown"
        || identity.profile != "release"
        || identity.manifest_sha256() != producer.manifest_sha256
    {
        return fail(
            "module_engine_incompatible",
            "Loaded UI ABI, compiler, configuration or source differs from its producer receipt",
        );
    }
    Ok(())
}

fn valid_compiler_identity(text: &str) -> bool {
    if text.len() > 4096 || text.bytes().any(|byte| byte.is_ascii_control()) {
        return false;
    }
    let mut parts = text.split("; ");
    let Some(headline) = parts.next() else {
        return false;
    };
    let expression = Regex::new(r"^rustc ([0-9]+\.[0-9]+\.[0-9]+(?:-[a-zA-Z0-9.-]+)?) \(([0-9a-f]{7,40}) ([0-9]{4}-[0-9]{2}-[0-9]{2})\)$").unwrap();
    let Some(head) = expression.captures(headline) else {
        return false;
    };
    let mut facts = std::collections::BTreeMap::new();
    for part in parts {
        let Some((key, value)) = part.split_once(": ") else {
            return false;
        };
        if value.is_empty() || facts.insert(key, value).is_some() {
            return false;
        }
    }
    let expected = [
        "binary",
        "commit-hash",
        "commit-date",
        "host",
        "release",
        "LLVM version",
    ];
    if facts.len() != expected.len() || expected.iter().any(|key| !facts.contains_key(key)) {
        return false;
    }
    let commit = facts["commit-hash"];
    facts["binary"] == "rustc"
        && hex(commit, 40)
        && commit.starts_with(&head[2])
        && facts["commit-date"] == &head[3]
        && facts["release"] == &head[1]
        && Regex::new(r"^[a-zA-Z0-9_][a-zA-Z0-9_.-]*$")
            .unwrap()
            .is_match(facts["host"])
        && Regex::new(r"^[0-9]+\.[0-9]+(?:\.[0-9]+)?$")
            .unwrap()
            .is_match(facts["LLVM version"])
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
    Ok(UiUnit {
        unit_sha256: unit_digest(&release),
        release,
        total_bytes: total,
    })
}

fn validate_installation(
    receipt: &InstalledUiModule,
    lane: &str,
) -> Result<(), ModuleReleaseError> {
    let expected = inspect(
        receipt.unit.release.component.as_str(),
        lane,
        &serde_json::to_value(&receipt.unit.release).expect("typed release"),
    )?;
    if receipt.schema != INSTALLATION_SCHEMA
        || receipt.unit != expected
        || receipt.unit.release.status != "active"
    {
        return fail(
            "module_receipt_invalid",
            "UI installation crosses its complete unit",
        );
    }
    let origin = Url::parse(&receipt.authority.gateway_origin)
        .map_err(|_| refuse("module_receipt_invalid", "Invalid UI release authority"))?;
    if origin.scheme() != "https"
        || origin.host_str().is_none()
        || origin.path() != "/"
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.query().is_some()
        || origin.fragment().is_some()
        || !hex(&receipt.authority.route_manifest_sha256, 64)
    {
        return fail(
            "module_receipt_invalid",
            "Invalid captured UI release authority",
        );
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
    if receipt.authority.gateway_origin != gateway_origin {
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
    let expected = inspect(
        unit.release.component.as_str(),
        &unit.release.channel,
        &serde_json::to_value(&unit.release).expect("typed release"),
    )?;
    if *unit != expected {
        return fail(
            "module_receipt_invalid",
            "UI unit differs from its captured catalog",
        );
    }
    if verified.len() != 4 {
        return fail(
            "module_artifact_invalid",
            "Verify all four members before staging",
        );
    }
    for asset in &unit.release.assets {
        let name = filename(asset)?;
        let matches = verified
            .iter()
            .filter(|file| {
                file.filename == name
                    && file.sha256 == asset.sha256
                    && file.size_bytes == asset.size_bytes
            })
            .count();
        if matches != 1 {
            return fail(
                "module_artifact_invalid",
                "UI member bytes differ from the captured catalog",
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
    if unit.release.status != "active" {
        return fail(
            "module_release_withdrawn",
            "Only an active UI release may be staged",
        );
    }
    verify_files(&unit, verified)?;
    let lane = unit.release.channel.clone();
    let engine_identity = unit.release.build_manifest.clone();
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
    let expected = inspect(
        unit.release.component.as_str(),
        host_channel,
        &serde_json::to_value(&unit.release).expect("typed release"),
    )?;
    if unit != &expected || unit.unit_sha256 != expected_sha256 {
        return fail(
            "module_selection_changed",
            "The UI release differs from the confirmed unit",
        );
    }
    if unit.release.status != "active" {
        return fail("module_release_withdrawn", "The UI release is not active");
    }
    Ok(unit.release.assets.clone())
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
        if active.unit.release.component != component {
            return fail("module_receipt_invalid", "Selection crosses UI component");
        }
    }
    if let Some(previous) = previous {
        validate_installation(previous, lane)?;
        if previous.unit.release.component != component {
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
    let component = staged.unit.release.component;
    let lane = staged.unit.release.channel.clone();
    validate_installation(&staged, &lane)?;
    let generation = next_generation(current, component, &lane, expected)?;
    if let Some(current) = current.filter(|current| matches!(&current.selection, UiSelectionState::Installed { active, .. } if active.unit.unit_sha256 == staged.unit.unit_sha256)) {
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
            return fail("module_removed", "This UI component is already removed")
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
            )
        }
    };
    Ok(UiSelection {
        generation,
        selection: UiSelectionState::Installed { active, previous },
        ..current.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(component: UiComponent) -> (Value, Value) {
        let (release, identity) = match component {
            UiComponent::Interactions => (
                include_str!("fixtures/interactions-ui-wasm-canary.json"),
                include_str!("fixtures/interactions-ui-engine-build.json"),
            ),
            UiComponent::CommandKernel => (
                include_str!("fixtures/command-kernel-ui-wasm-canary.json"),
                include_str!("fixtures/command-kernel-ui-engine-build.json"),
            ),
        };
        (
            serde_json::from_str(release).unwrap(),
            serde_json::from_str(identity).unwrap(),
        )
    }
    fn files(unit: &UiUnit) -> Vec<VerifiedFile> {
        unit.release
            .assets
            .iter()
            .map(|asset| {
                let name = filename(asset).unwrap();
                let bytes = format!("fixture:{name}\n");
                VerifiedFile {
                    filename: name,
                    sha256: format!("{:x}", Sha256::digest(bytes.as_bytes())),
                    size_bytes: bytes.len() as i64,
                }
            })
            .collect()
    }
    fn ready() -> (InstalledUiModule, Vec<VerifiedFile>, Value) {
        let (release, identity) = fixture(UiComponent::Interactions);
        let unit = inspect("interactions", "canary", &release).unwrap();
        let verified = files(&unit);
        let receipt = prepare(
            unit,
            UiAuthority {
                gateway_origin: "https://gateway.example".into(),
                route_manifest_sha256: "e".repeat(64),
            },
            &verified,
            true,
        )
        .unwrap();
        (receipt, verified, identity)
    }
    fn guard() -> ActivationGuard {
        ActivationGuard {
            unsaved_edits: false,
            uncaptured_sessions: 0,
            host_restart_required: false,
        }
    }

    #[test]
    fn both_fixed_ui_units_preserve_exact_receipts_and_canonical_identity() {
        for component in [UiComponent::Interactions, UiComponent::CommandKernel] {
            let (release, identity) = fixture(component);
            let unit = inspect(component.as_str(), "canary", &release).unwrap();
            assert_eq!(serde_json::to_value(&unit.release).unwrap(), release);
            verify_files(&unit, &files(&unit)).unwrap();
            let identity = parse_identity(&identity).unwrap();
            validate_identity(&unit, &identity).unwrap();
            assert_eq!(
                identity.manifest_sha256(),
                unit.release.assets[0].engines[0].manifest_sha256
            );
            let mut reordered = release.clone();
            reordered["assets"].as_array_mut().unwrap().reverse();
            assert_eq!(
                inspect(component.as_str(), "canary", &reordered)
                    .unwrap()
                    .unit_sha256,
                unit.unit_sha256
            );
        }
        assert!(super::super::ModuleComponent::parse("interactions").is_err());
        assert!(admit_target("network", "canary").is_err());
    }

    #[test]
    fn compiler_facts_are_complete_bounded_and_consistent() {
        let (_, identity) = fixture(UiComponent::Interactions);
        let compiler = identity["rustc"].as_str().unwrap();
        assert!(valid_compiler_identity(compiler));
        for invalid in [
            compiler.split("; ").next().unwrap().to_owned(),
            compiler.replace("; host: x86_64-unknown-linux-gnu", ""),
            format!("{compiler}; binary: rustc"),
            format!("{compiler}; unknown: fact"),
            compiler.replace("binary: rustc", "binary: clang"),
            compiler.replace("commit-hash: 1159", "commit-hash: 2159"),
            compiler.replace("commit-date: 2025-09-14", "commit-date: 2025-09-15"),
            compiler.replace("release: 1.90.0", "release: 1.91.0"),
            compiler.replace("LLVM version: 20.1.8", "LLVM version: unknown"),
            compiler.replace(
                "host: x86_64-unknown-linux-gnu",
                "host: x86_64\n-unknown-linux-gnu",
            ),
            "x".repeat(4097),
        ] {
            assert!(!valid_compiler_identity(&invalid), "{invalid}");
        }
    }

    #[test]
    fn closed_ui_identity_rejects_unknown_abi_source_compiler_and_config() {
        let (receipt, _, identity) = ready();
        let mut unknown = identity.clone();
        unknown["future"] = json!(true);
        assert!(parse_identity(&unknown).is_err());
        for (key, value) in [
            ("schema", json!("ds.engine-build/v1")),
            ("component", json!("command-kernel")),
            ("protocol", json!("ds.ui-wasm/v2")),
            ("ui_contract", json!(2)),
            ("source_sha", json!("B".repeat(40))),
            ("source_sha", json!("a".repeat(40))),
            ("cargo_lock_sha256", json!("a".repeat(64))),
            ("rustc", json!("rustc different")),
            ("target", json!("x86_64-unknown-linux-gnu")),
            ("profile", json!("debug")),
            ("config_sha256", json!("a".repeat(64))),
        ] {
            let mut changed = identity.clone();
            changed[key] = value;
            assert!(
                validate_identity(&receipt.unit, &parse_identity(&changed).unwrap()).is_err(),
                "{key}"
            );
        }
        let mut oversized = identity;
        oversized["rustc"] = json!("x".repeat(4097));
        assert!(parse_identity(&oversized).is_err());
    }

    #[test]
    fn complete_catalog_refuses_crossed_lane_source_member_and_unknown_keys() {
        let (catalog, _) = fixture(UiComponent::Interactions);
        assert!(inspect("interactions", "stable", &catalog).is_err());
        assert!(inspect("command-kernel", "canary", &catalog).is_err());
        for (path, value) in [
            ("/runtime", json!("native")), ("/product_id", json!("rw.datasolutions.module.interactions")),
            ("/assets/0/platform", json!("linux")), ("/assets/0/sha256", json!("A".repeat(64))),
            ("/assets/0/source_revisions", json!({"ds-command-kernel":"b".repeat(40)})),
            ("/assets/1/source_sha", json!("c".repeat(40))),
            ("/assets/1/built_at", json!("2026-10-14T08:00:00Z")),
            ("/assets/1/engines/0/manifest_sha256", json!("f".repeat(64))),
            ("/assets/0/engines/0/release", json!("ds-interactions@0.2.0+bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")),
            ("/assets/0/url", json!("https://storage.googleapis.com/another-bucket/modules/interactions/canary/wasm/0.1.0-r1/ds_interactions_wasm.js")),
            ("/assets/0/url", json!("https://storage.googleapis.com/dedicated-public-releases/modules/interactions/stable/wasm/0.1.0-r1/ds_interactions_wasm.js")),
        ] {
            let mut changed = catalog.clone(); *changed.pointer_mut(path).unwrap() = value;
            assert!(inspect("interactions", "canary", &changed).is_err(), "{path}");
        }
        let mut missing = catalog.clone();
        missing["assets"].as_array_mut().unwrap().pop();
        assert!(inspect("interactions", "canary", &missing).is_err());
        let mut duplicate = catalog.clone();
        duplicate["assets"][3] = duplicate["assets"][0].clone();
        assert!(inspect("interactions", "canary", &duplicate).is_err());
        for path in ["", "/assets/0", "/assets/0/engines/0"] {
            let mut unknown = catalog.clone();
            unknown.pointer_mut(path).unwrap()["future"] = json!(true);
            assert!(inspect("interactions", "canary", &unknown).is_err());
        }
    }

    #[test]
    fn trusted_catalog_manifest_is_required_before_transfer_or_adapter_evaluation() {
        let (catalog, identity) = fixture(UiComponent::Interactions);
        let mut missing = catalog.clone();
        missing.as_object_mut().unwrap().remove("build_manifest");
        assert!(inspect("interactions", "canary", &missing).is_err());
        let mut unknown = catalog.clone();
        unknown["build_manifest"]["future"] = json!(true);
        assert!(inspect("interactions", "canary", &unknown).is_err());
        for (key, value) in [
            ("schema", json!("ds.engine-build/v1")),
            ("component", json!("command-kernel")),
            ("source_sha", json!("a".repeat(40))),
            ("ui_contract", json!(2)),
            ("target", json!("x86_64-unknown-linux-gnu")),
            ("profile", json!("debug")),
            ("cargo_lock_sha256", json!("a".repeat(64))),
            ("config_sha256", json!("a".repeat(64))),
            ("rustc", json!("rustc 1.91.0 (1159e78c4 2025-09-14)")),
        ] {
            let mut tampered = catalog.clone();
            tampered["build_manifest"][key] = value;
            assert!(
                inspect("interactions", "canary", &tampered).is_err(),
                "{key}"
            );
        }
        let unit = inspect("interactions", "canary", &catalog).unwrap();
        let (prepared, _, _) = ready();
        assert_eq!(prepared.engine_identity, unit.release.build_manifest);
        assert_eq!(
            serde_json::to_value(&prepared.engine_identity).unwrap(),
            identity
        );
    }

    #[test]
    fn bytes_precede_loaded_identity_and_bind_all_four_members() {
        let (receipt, verified, identity) = ready();
        let selection = activate(None, receipt.clone(), 0, true, guard()).unwrap();
        let captured = capture(
            Some(&selection),
            UiComponent::Interactions,
            "canary",
            "https://gateway.example",
        )
        .unwrap();
        admit_loaded(&captured, &verified, &identity).unwrap();
        let mut corrupted = verified.clone();
        corrupted[0].sha256 = "a".repeat(64);
        assert_eq!(
            admit_loaded(&captured, &corrupted, &Value::Null)
                .unwrap_err()
                .code,
            "module_artifact_invalid"
        );
        assert!(verify_files(&receipt.unit, &verified[..3]).is_err());
        let mut duplicate = verified.clone();
        duplicate[3] = duplicate[0].clone();
        assert!(verify_files(&receipt.unit, &duplicate).is_err());
        let mut tampered = receipt.unit.clone();
        tampered.total_bytes += 1;
        assert!(verify_files(&tampered, &verified).is_err());
        assert!(admit_loaded(&captured, &verified, &json!({})).is_err());
    }

    #[test]
    fn held_generation_lane_authority_and_guard_fence_offline_transitions() {
        let (receipt, _, _) = ready();
        assert!(prepare(
            receipt.unit.clone(),
            receipt.authority.clone(),
            &files(&receipt.unit),
            false
        )
        .is_err());
        let first = activate(None, receipt.clone(), 0, true, guard()).unwrap();
        assert!(activate(Some(&first), receipt.clone(), 0, true, guard()).is_err());
        let mut dirty = guard();
        dirty.unsaved_edits = true;
        assert_eq!(
            activate(Some(&first), receipt.clone(), 1, true, dirty)
                .unwrap_err()
                .code,
            "module_activation_deferred"
        );
        assert!(admit_retained(&receipt, "stable", "https://gateway.example").is_err());
        assert!(admit_retained(&receipt, "canary", "https://other.example").is_err());
        assert!(capture(
            Some(&first),
            UiComponent::CommandKernel,
            "canary",
            "https://gateway.example"
        )
        .is_err());
        // A second complete immutable version retains the former active unit.
        let mut next_catalog = serde_json::to_value(&receipt.unit.release).unwrap();
        next_catalog["version"] = json!("0.1.0-r2");
        for asset in next_catalog["assets"].as_array_mut().unwrap() {
            let url = asset["url"]
                .as_str()
                .unwrap()
                .replace("/0.1.0-r1/", "/0.1.0-r2/");
            asset["url"] = json!(url);
        }
        let next_unit = inspect("interactions", "canary", &next_catalog).unwrap();
        let next = prepare(
            next_unit.clone(),
            receipt.authority.clone(),
            &files(&next_unit),
            true,
        )
        .unwrap();
        let advanced = activate(Some(&first), next, 1, true, guard()).unwrap();
        let rolled_back = rollback(&advanced, 2, true, guard()).unwrap();
        assert_eq!(
            capture(
                Some(&rolled_back),
                UiComponent::Interactions,
                "canary",
                "https://gateway.example"
            )
            .unwrap()
            .receipt,
            receipt
        );
        assert_eq!(rolled_back.generation, 3);
        let removed = remove(&first, 1, true).unwrap();
        assert_eq!(
            capture(
                Some(&removed),
                UiComponent::Interactions,
                "canary",
                "https://gateway.example"
            )
            .unwrap_err()
            .code,
            "module_removed"
        );
        let restored = rollback(&removed, 2, true, guard()).unwrap();
        assert_eq!(restored.generation, 3);
        assert_eq!(
            capture(
                Some(&restored),
                UiComponent::Interactions,
                "canary",
                "https://gateway.example"
            )
            .unwrap()
            .receipt,
            receipt
        );
        let mut corrupt = restored.clone();
        if let UiSelectionState::Installed { active, .. } = &mut corrupt.selection {
            active.engine_identity.config_sha256 = "f".repeat(64);
        }
        assert!(validate_selection(&corrupt, UiComponent::Interactions, "canary").is_err());
    }

    #[test]
    fn acquisition_plan_is_bounded_and_withdrawn_catalogs_stay_visible() {
        let (receipt, _, _) = ready();
        let unit = &receipt.unit;
        assert_eq!(
            acquisition_plan(unit, "canary", &unit.unit_sha256, true)
                .unwrap()
                .len(),
            4
        );
        assert!(acquisition_plan(unit, "canary", &unit.unit_sha256, false).is_err());
        assert!(acquisition_plan(unit, "stable", &unit.unit_sha256, true).is_err());
        assert!(acquisition_plan(unit, "canary", &"f".repeat(64), true).is_err());
        for status in ["blocked", "stopped", "rolled_back"] {
            let mut release = serde_json::to_value(&unit.release).unwrap();
            release["status"] = json!(status);
            let withdrawn = inspect("interactions", "canary", &release).unwrap();
            assert_eq!(withdrawn.release.status, status);
            assert!(acquisition_plan(&withdrawn, "canary", &withdrawn.unit_sha256, true).is_err());
        }
    }
}
