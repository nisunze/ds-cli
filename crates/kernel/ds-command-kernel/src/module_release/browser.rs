//! Browser component admission. Hosts acquire and hash bytes; this owner decides
//! which complete unit and retained selection may be used. Native executable
//! receipts are deliberately not accepted here. Public metadata alone never
//! grants installation, execution or project authority.
use super::*;
use crate::module_engine::{
    admit_browser_engine_build, browser_engine_profile, BrowserEngineProducer,
};
use crate::module_install::ActivationGuard;
use sha2::{Digest, Sha256};

pub const PROTOCOL: &str = "ds.network-wasm/v1";
pub const INSTALLATION_SCHEMA: &str = "ds.browser-module-installation/v1";
pub const SELECTION_SCHEMA: &str = "ds.browser-module-selection/v1";
pub const MAX_WIRE_BYTES: usize = 384 * 1024;
const MAX_UNIT_BYTES: i64 = 128 * 1024 * 1024;
const MAX_GENERATION: u64 = crate::desktop_instance::MAX_GENERATION;
const NETWORK_FILES: [(&str, &str); 4] = [
    ("ds_network_wasm.js", "module-javascript"),
    ("ds_network_wasm_bg.wasm", "module-wasm"),
    ("ds_network_wasm.d.ts", "module-types"),
    ("ds_network_wasm_bg.wasm.d.ts", "module-types"),
];

const REPORTER_FILES: [(&str, &str); 4] = [
    ("ds_reporter_wasm.js", "module-javascript"),
    ("ds_reporter_wasm_bg.wasm", "module-wasm"),
    ("ds_reporter_wasm.d.ts", "module-types"),
    ("ds_reporter_wasm_bg.wasm.d.ts", "module-types"),
];
struct ComponentProfile {
    protocol: &'static str,
    files: &'static [(&'static str, &'static str); 4],
    owner: &'static str,
}
fn profile(component: ModuleComponent) -> Result<ComponentProfile, ModuleReleaseError> {
    let Some(engine) = browser_engine_profile(component) else {
        return fail(
            "module_release_target_unsupported",
            "Solar has no admitted browser publication contract",
        );
    };
    Ok(ComponentProfile {
        protocol: engine.protocol,
        files: if component == ModuleComponent::Network {
            &NETWORK_FILES
        } else {
            &REPORTER_FILES
        },
        owner: component.engine_owner(),
    })
}
fn network() -> ModuleComponent {
    ModuleComponent::Network
}
fn bind_component(
    expected: ModuleComponent,
    actual: ModuleComponent,
) -> Result<(), ModuleReleaseError> {
    profile(expected)?;
    if expected != actual {
        return fail(
            "module_receipt_invalid",
            "Browser caller and receipt components differ",
        );
    }
    Ok(())
}

/// Brain's published catalog member: additive members from a newer publisher
/// are accepted within the shared bounds (owner 22 Oct); the receipts below are
/// this Kernel's own and stay closed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserRelease {
    pub schema: String,
    pub component: ModuleComponent,
    pub runtime: String,
    pub channel: String,
    pub product_id: String,
    pub version: String,
    pub protocol: String,
    pub released_at: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub assets: Vec<ModuleReleaseAsset>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserUnit {
    pub release: BrowserRelease,
    pub unit_sha256: String,
    pub total_bytes: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAuthority {
    pub gateway_origin: String,
    pub route_manifest_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct VerifiedFile {
    pub filename: String,
    pub sha256: String,
    pub size_bytes: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledBrowserModule {
    pub schema: String,
    pub unit: BrowserUnit,
    pub authority: BrowserAuthority,
    pub engine_identity: Value,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrowserSelectionState {
    Installed {
        active: Box<InstalledBrowserModule>,
        previous: Option<Box<InstalledBrowserModule>>,
    },
    Removed {
        previous: Option<Box<InstalledBrowserModule>>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserSelection {
    pub schema: String,
    pub component: ModuleComponent,
    pub channel: String,
    pub generation: u64,
    pub selection: BrowserSelectionState,
}

/// A job keeps this exact unit even if the host's selection changes later.
/// Capturing a component does not grant principal/project or write authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserCapture {
    pub generation: u64,
    pub receipt: InstalledBrowserModule,
}

pub fn capture(
    current: Option<&BrowserSelection>,
    lane: &str,
    gateway_origin: &str,
) -> Result<BrowserCapture, ModuleReleaseError> {
    channel(lane)?;
    let selected = current.ok_or_else(|| {
        refuse(
            "module_not_installed",
            "Install and activate this browser component before processing",
        )
    })?;
    validate_selection(selected, lane)?;
    let BrowserSelectionState::Installed { active, .. } = &selected.selection else {
        return fail(
            "module_removed",
            "This browser component was explicitly removed; restore it before processing",
        );
    };
    let receipt = admit_retained(active, lane, gateway_origin)?;
    Ok(BrowserCapture {
        generation: selected.generation,
        receipt,
    })
}

/// Re-admit the bytes and identity loaded inside a captured worker. Selection
/// changes cannot retarget a captured job, and this does not grant write authority.
pub fn admit_loaded(
    captured: &BrowserCapture,
    verified: &[VerifiedFile],
    identity: &Value,
) -> Result<(), ModuleReleaseError> {
    if captured.generation == 0 || captured.generation > MAX_GENERATION {
        return fail(
            "module_receipt_invalid",
            "Invalid captured browser generation",
        );
    }
    validate_installation(&captured.receipt, &captured.receipt.unit.release.channel)?;
    verify_files(&captured.receipt.unit, verified)?;
    if *identity != captured.receipt.engine_identity {
        return fail(
            "module_engine_incompatible",
            "Loaded browser identity differs from the captured installation",
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
pub fn admit_target(component: &str, channel_name: &str) -> Result<(), ModuleReleaseError> {
    channel(channel_name)?;
    profile(ModuleComponent::parse(component)?)?;
    Ok(())
}
fn filename(asset: &ModuleReleaseAsset) -> Result<String, ModuleReleaseError> {
    let url = Url::parse(&asset.url)
        .map_err(|_| refuse("module_release_url_invalid", "Invalid browser member URL"))?;
    Ok(url.path().rsplit('/').next().unwrap_or_default().to_owned())
}
fn unit_digest(release: &BrowserRelease) -> String {
    // Struct fields and BTreeMap provenance have the same order on native/WASM.
    // Catalog member order is irrelevant, but every member fact is bound.
    let mut ordered = release.clone();
    ordered.assets.sort_by(|a, b| a.url.cmp(&b.url));
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&ordered).expect("typed release"))
    )
}

pub fn inspect(channel_name: &str, value: &Value) -> Result<BrowserUnit, ModuleReleaseError> {
    channel(channel_name)?;
    if !value
        .get("assets")
        .and_then(Value::as_array)
        .is_some_and(|assets| {
            assets.len() == NETWORK_FILES.len() && assets.iter().all(bounded_asset_shape)
        })
    {
        return fail(
            "module_release_invalid",
            "Browser members exceed their closed bounded shape",
        );
    }
    if !bounded_fields(
        value,
        &[
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
            "assets",
        ],
        8000,
    ) || serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > MAX_WIRE_BYTES)
    {
        return fail(
            "module_release_invalid",
            "Browser release exceeds its closed bounded shape",
        );
    }
    let release: BrowserRelease = serde_json::from_value(value.clone())
        .map_err(|_| refuse("module_release_invalid", "Invalid browser release schema"))?;
    let component = release.component;
    let profile = profile(component)?;
    let expected_product = format!(
        "rw.datasolutions.module.{}{}",
        component.as_str(),
        if channel_name == "canary" {
            ".canary"
        } else {
            ""
        }
    );
    if !same_family(&release.schema, MODULE_RELEASE_SCHEMA)
        || release.runtime != "wasm"
        || release.channel != channel_name
        || release.product_id != expected_product
        || !same_family(&release.protocol, profile.protocol)
    {
        return fail(
            "module_release_identity_mismatch",
            "Expected this lane's declared browser component catalog",
        );
    }
    if !release_folder(&release.version)
        || OffsetDateTime::parse(&release.released_at, &Rfc3339).is_err()
        || !matches!(
            release.status.as_str(),
            "active" | "blocked" | "stopped" | "rolled_back"
        )
        || release.assets.len() != profile.files.len()
    {
        return fail(
            "module_release_invalid",
            "Invalid browser version, status or complete unit",
        );
    }
    let first = &release.assets[0];
    let mut seen = Vec::new();
    let mut total = 0_i64;
    for asset in &release.assets {
        if asset.platform != "browser"
            || asset.arch != "wasm32"
            || !same_family(&asset.protocol, profile.protocol)
            || asset.size_bytes <= 0
            || asset.size_bytes > MAX_UNIT_BYTES
            || !hex(&asset.sha256, 64)
            || asset.source_revisions.is_empty()
            || asset.source_revisions.len() > 16
        {
            return fail(
                "module_release_asset_invalid",
                "Invalid browser target, size, digest or closure",
            );
        }
        validate_asset_provenance(component, asset)?;
        if asset.engines[0].engine != profile.owner {
            return fail(
                "module_release_provenance_invalid",
                "Browser producer differs from its component",
            );
        }
        if asset.source_sha != first.source_sha
            || asset.built_at != first.built_at
            || asset.source_revisions != first.source_revisions
            || asset.engines != first.engines
        {
            return fail(
                "module_release_provenance_invalid",
                "Browser members cross producer closures",
            );
        }
        let url = Url::parse(&asset.url)
            .map_err(|_| refuse("module_release_url_invalid", "Invalid browser member URL"))?;
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
            || parts[2] != "modules"
            || parts[3] != component.as_str()
            || parts[4] != channel_name
            || parts[5] != "wasm"
            || parts[6] != release.version
            || !profile
                .files
                .iter()
                .any(|(file, kind)| *file == name && *kind == asset.kind)
            || seen.contains(&name)
        {
            return fail(
                "module_release_url_invalid",
                "Expected a unique immutable member of this complete browser unit",
            );
        }
        seen.push(name);
        total = total
            .checked_add(asset.size_bytes)
            .ok_or_else(|| refuse("module_release_asset_invalid", "Browser unit size overflow"))?;
    }
    if total > MAX_UNIT_BYTES {
        return fail(
            "module_release_asset_invalid",
            "Browser unit exceeds 128 MiB",
        );
    }
    Ok(BrowserUnit {
        unit_sha256: unit_digest(&release),
        release,
        total_bytes: total,
    })
}

fn validate_installation(
    receipt: &InstalledBrowserModule,
    lane: &str,
) -> Result<(), ModuleReleaseError> {
    let expected = inspect(
        lane,
        &serde_json::to_value(&receipt.unit.release).expect("typed release"),
    )?;
    if receipt.schema != INSTALLATION_SCHEMA
        || receipt.unit != expected
        || receipt.unit.release.status != "active"
    {
        return fail(
            "module_receipt_invalid",
            "Browser installation crosses its complete unit",
        );
    }
    let origin = Url::parse(&receipt.authority.gateway_origin).map_err(|_| {
        refuse(
            "module_receipt_invalid",
            "Invalid browser release authority",
        )
    })?;
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
            "Invalid captured browser release authority",
        );
    }
    let identity = &receipt.engine_identity;
    if serde_json::to_vec(identity).map_or(true, |bytes| bytes.len() > 16 * 1024) {
        return fail(
            "module_engine_incompatible",
            "Browser identity exceeds 16 KiB",
        );
    }
    let component = receipt.unit.release.component;
    profile(component)?;
    let asset = &receipt.unit.release.assets[0];
    let version = receipt
        .unit
        .release
        .version
        .split("-r")
        .next()
        .unwrap_or_default();
    // The identity rule is shared with release packaging (module_engine).
    let producer = BrowserEngineProducer {
        version,
        source_sha: &asset.source_sha,
        manifest_sha256: &asset.engines[0].manifest_sha256,
    };
    if admit_browser_engine_build(component, identity, Some(producer)).is_err() {
        return fail(
            "module_engine_incompatible",
            "Browser engine identity differs from its producer receipt",
        );
    }
    Ok(())
}

/// Admit a retained receipt against this host's declared lane and authority.
/// This is offline validation; it grants no acquisition or computation authority.
pub fn admit_retained(
    receipt: &InstalledBrowserModule,
    channel: &str,
    gateway_origin: &str,
) -> Result<InstalledBrowserModule, ModuleReleaseError> {
    validate_installation(receipt, channel)?;
    if receipt.authority.gateway_origin != gateway_origin {
        return fail(
            "module_receipt_invalid",
            "Retained browser authority differs from this host",
        );
    }
    Ok(receipt.clone())
}

/// Verify every member before executing even the JavaScript identity adapter.
/// Hosts supply hashes and sizes computed from acquired bytes, never catalog facts.
pub fn verify_files(
    unit: &BrowserUnit,
    verified: &[VerifiedFile],
) -> Result<(), ModuleReleaseError> {
    let expected = inspect(
        &unit.release.channel,
        &serde_json::to_value(&unit.release).expect("typed release"),
    )?;
    if *unit != expected {
        return fail(
            "module_receipt_invalid",
            "Browser unit differs from its captured catalog",
        );
    }
    if verified.len() != NETWORK_FILES.len() {
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
                "Browser member bytes differ from the captured catalog",
            );
        }
    }
    Ok(())
}

pub fn prepare(
    unit: BrowserUnit,
    authority: BrowserAuthority,
    verified: &[VerifiedFile],
    identity: Value,
    consent: bool,
) -> Result<InstalledBrowserModule, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Deliberate consent is required before browser component acquisition",
        );
    }
    if unit.release.status != "active" {
        return fail(
            "module_release_withdrawn",
            "Only an active browser release may be staged",
        );
    }
    verify_files(&unit, verified)?;
    let lane = unit.release.channel.clone();
    let receipt = InstalledBrowserModule {
        schema: INSTALLATION_SCHEMA.into(),
        unit,
        authority,
        engine_identity: identity,
    };
    validate_installation(&receipt, &lane)?;
    Ok(receipt)
}

/// The host may issue asset requests only after this decision. A public catalog
/// can be inspected across lanes, but installation belongs to this compiled
/// application's lane and the exact unit the person confirmed.
pub fn acquisition_plan(
    unit: &BrowserUnit,
    host_channel: &str,
    expected_sha256: &str,
    consent: bool,
) -> Result<Vec<ModuleReleaseAsset>, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Confirm this browser component before acquisition",
        );
    }
    channel(host_channel)?;
    let expected = inspect(
        host_channel,
        &serde_json::to_value(&unit.release).expect("typed release"),
    )?;
    if unit != &expected || unit.unit_sha256 != expected_sha256 {
        return fail(
            "module_selection_changed",
            "The browser release differs from the confirmed unit",
        );
    }
    if unit.release.status != "active" {
        return fail(
            "module_release_withdrawn",
            "The browser release is not active",
        );
    }
    Ok(unit.release.assets.clone())
}

pub fn validate_selection(
    selection: &BrowserSelection,
    lane: &str,
) -> Result<(), ModuleReleaseError> {
    channel(lane)?;
    profile(selection.component)?;
    if selection.schema != SELECTION_SCHEMA
        || selection.channel != lane
        || selection.generation == 0
        || selection.generation > MAX_GENERATION
    {
        return fail(
            "module_receipt_invalid",
            "Invalid browser selection lane or generation",
        );
    }
    let (active, previous) = match &selection.selection {
        BrowserSelectionState::Installed { active, previous } => (Some(active), previous),
        BrowserSelectionState::Removed { previous } => (None, previous),
    };
    if let Some(active) = active {
        bind_component(selection.component, active.unit.release.component)?;
        validate_installation(active, lane)?;
    }
    if let Some(previous) = previous {
        bind_component(selection.component, previous.unit.release.component)?;
        validate_installation(previous, lane)?;
    }
    Ok(())
}
fn next_generation(
    current: Option<&BrowserSelection>,
    lane: &str,
    expected: u64,
) -> Result<u64, ModuleReleaseError> {
    if let Some(current) = current {
        validate_selection(current, lane)?;
    }
    let generation = current.map_or(0, |selection| selection.generation);
    if generation != expected || generation >= MAX_GENERATION {
        return fail(
            "module_selection_changed",
            "Browser selection changed; inspect it before retrying",
        );
    }
    Ok(generation + 1)
}
pub fn activate(
    current: Option<&BrowserSelection>,
    staged: InstalledBrowserModule,
    expected: u64,
    consent: bool,
    guard: ActivationGuard,
) -> Result<BrowserSelection, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Confirm browser component activation",
        );
    }
    guard.admit().map_err(|_| {
        refuse(
            "module_activation_deferred",
            "Retain held work before changing the browser selection",
        )
    })?;
    let lane = staged.unit.release.channel.clone();
    validate_installation(&staged, &lane)?;
    let component = staged.unit.release.component;
    if let Some(current) = current {
        bind_component(component, current.component)?;
    }
    let generation = next_generation(current, &lane, expected)?;
    if let Some(current) = current.filter(|current| matches!(&current.selection, BrowserSelectionState::Installed { active, .. } if active.unit.unit_sha256 == staged.unit.unit_sha256)) {
        return Ok(current.clone());
    }
    let previous = current.and_then(|selection| match &selection.selection {
        BrowserSelectionState::Installed { active, .. } => Some(active.clone()),
        BrowserSelectionState::Removed { previous } => previous.clone(),
    });
    Ok(BrowserSelection {
        schema: SELECTION_SCHEMA.into(),
        component,
        channel: lane,
        generation,
        selection: BrowserSelectionState::Installed {
            active: Box::new(staged),
            previous,
        },
    })
}
pub fn remove(
    current: &BrowserSelection,
    expected: u64,
    consent: bool,
) -> Result<BrowserSelection, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Confirm browser component removal",
        );
    }
    let generation = next_generation(Some(current), &current.channel, expected)?;
    let previous = match &current.selection {
        BrowserSelectionState::Installed { active, .. } => Some(active.clone()),
        BrowserSelectionState::Removed { .. } => {
            return fail(
                "module_removed",
                "This browser component is already removed",
            )
        }
    };
    Ok(BrowserSelection {
        generation,
        selection: BrowserSelectionState::Removed { previous },
        ..current.clone()
    })
}
pub fn rollback(
    current: &BrowserSelection,
    expected: u64,
    consent: bool,
    guard: ActivationGuard,
) -> Result<BrowserSelection, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Confirm restoration of retained browser bytes",
        );
    }
    guard.admit().map_err(|_| {
        refuse(
            "module_activation_deferred",
            "Retain held work before rollback",
        )
    })?;
    let generation = next_generation(Some(current), &current.channel, expected)?;
    let (active, previous) = match &current.selection {
        BrowserSelectionState::Installed {
            active,
            previous: Some(previous),
        } => (previous.clone(), Some(active.clone())),
        BrowserSelectionState::Removed {
            previous: Some(previous),
        } => (previous.clone(), None),
        _ => {
            return fail(
                "module_rollback_unavailable",
                "No retained browser installation is available",
            )
        }
    };
    Ok(BrowserSelection {
        generation,
        selection: BrowserSelectionState::Installed { active, previous },
        ..current.clone()
    })
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Loaded {
        captured: Box<BrowserCapture>,
        verified: Vec<VerifiedFile>,
        identity: Value,
    },
    Capture {
        #[serde(default = "network")]
        component: ModuleComponent,
        current: Option<Box<BrowserSelection>>,
        channel: String,
        gateway_origin: String,
    },
    Installation {
        #[serde(default = "network")]
        component: ModuleComponent,
        receipt: Box<InstalledBrowserModule>,
        channel: String,
        gateway_origin: String,
    },
    VerifyFiles {
        unit: Box<BrowserUnit>,
        verified: Vec<VerifiedFile>,
    },
    AcquisitionPlan {
        unit: Box<BrowserUnit>,
        host_channel: String,
        expected_sha256: String,
        consent: bool,
    },
    Inspect {
        #[serde(default = "network")]
        component: ModuleComponent,
        channel: String,
        release: Value,
    },
    Prepare {
        unit: Box<BrowserUnit>,
        authority: BrowserAuthority,
        verified: Vec<VerifiedFile>,
        identity: Value,
        consent: bool,
    },
    Selection {
        #[serde(default = "network")]
        component: ModuleComponent,
        channel: String,
        current: Option<Box<BrowserSelection>>,
    },
    Activate {
        current: Option<Box<BrowserSelection>>,
        staged: Box<InstalledBrowserModule>,
        expected_generation: u64,
        consent: bool,
        unsaved_edits: bool,
        uncaptured_sessions: usize,
        host_restart_required: bool,
    },
    Remove {
        current: Box<BrowserSelection>,
        expected_generation: u64,
        consent: bool,
    },
    Rollback {
        current: Box<BrowserSelection>,
        expected_generation: u64,
        consent: bool,
        unsaved_edits: bool,
        uncaptured_sessions: usize,
        host_restart_required: bool,
    },
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.is_empty() || input.len() > MAX_WIRE_BYTES {
        return Err("module_request_invalid: browser module request exceeds 384 KiB".into());
    }
    let request: Request = serde_json::from_slice(input).map_err(|_| {
        "module_request_invalid: malformed closed browser module request".to_string()
    })?;
    let result: Result<Value, ModuleReleaseError> = (|| {
        Ok(match request {
            Request::Loaded {
                captured,
                verified,
                identity,
            } => {
                admit_loaded(&captured, &verified, &identity)?;
                Value::Null
            }
            Request::Capture {
                component,
                current,
                channel,
                gateway_origin,
            } => {
                profile(component)?;
                if let Some(current) = &current {
                    bind_component(component, current.component)?;
                }
                serde_json::to_value(capture(current.as_deref(), &channel, &gateway_origin)?)
                    .expect("typed browser capture")
            }
            Request::Installation {
                component,
                receipt,
                channel,
                gateway_origin,
            } => {
                bind_component(component, receipt.unit.release.component)?;
                serde_json::to_value(admit_retained(&receipt, &channel, &gateway_origin)?)
                    .expect("typed retained receipt")
            }
            Request::VerifyFiles { unit, verified } => {
                verify_files(&unit, &verified)?;
                Value::Null
            }
            Request::AcquisitionPlan {
                unit,
                host_channel,
                expected_sha256,
                consent,
            } => serde_json::to_value(acquisition_plan(
                &unit,
                &host_channel,
                &expected_sha256,
                consent,
            )?)
            .expect("typed acquisition plan"),
            Request::Inspect {
                component,
                channel,
                release,
            } => {
                let unit = inspect(&channel, &release)?;
                bind_component(component, unit.release.component)?;
                serde_json::to_value(unit).expect("typed unit")
            }
            Request::Prepare {
                unit,
                authority,
                verified,
                identity,
                consent,
            } => serde_json::to_value(prepare(*unit, authority, &verified, identity, consent)?)
                .expect("typed receipt"),
            Request::Selection {
                component,
                channel,
                current,
            } => {
                self::channel(&channel)?;
                profile(component)?;
                if let Some(selection) = &current {
                    bind_component(component, selection.component)?;
                    validate_selection(selection, &channel)?;
                }
                serde_json::to_value(current).expect("typed selection")
            }
            Request::Activate {
                current,
                staged,
                expected_generation,
                consent,
                unsaved_edits,
                uncaptured_sessions,
                host_restart_required,
            } => serde_json::to_value(activate(
                current.as_deref(),
                *staged,
                expected_generation,
                consent,
                ActivationGuard {
                    unsaved_edits,
                    uncaptured_sessions,
                    host_restart_required,
                },
            )?)
            .expect("typed selection"),
            Request::Remove {
                current,
                expected_generation,
                consent,
            } => serde_json::to_value(remove(&current, expected_generation, consent)?)
                .expect("typed selection"),
            Request::Rollback {
                current,
                expected_generation,
                consent,
                unsaved_edits,
                uncaptured_sessions,
                host_restart_required,
            } => serde_json::to_value(rollback(
                &current,
                expected_generation,
                consent,
                ActivationGuard {
                    unsaved_edits,
                    uncaptured_sessions,
                    host_restart_required,
                },
            )?)
            .expect("typed selection"),
        })
    })();
    serde_json::to_string(&result.map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}
#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = browserModule)]
pub fn browser_module_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    evaluate(input).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;
