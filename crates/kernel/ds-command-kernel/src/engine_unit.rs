//! Page engine units held by installed Core (Desktop and Local Dev).
//!
//! The Network (and Reporter) browser WASM unit is released separately and
//! never bundled into a page. On a Desktop, Core's component system installs,
//! verifies and selects it the way it holds the UI units; Local Dev selects a
//! unit built from the working tree in Core's separate local-dev scope. Core
//! moves opaque bytes only and evaluates nothing, so a published installation
//! carries the producer facts its catalog published, and the page's Kernel
//! admits the identity the loaded unit actually exports against them before
//! the page calls the unit. A local build carries the identity its tested
//! bytes exported. Public metadata alone grants no installation, execution or
//! project authority. The hosted Web keeps its own retained browser selection
//! (`module_release::browser`).
use crate::module_engine::{admit_browser_engine_build, BrowserEngineProducer};
use crate::module_install::ActivationGuard;
use crate::module_release::browser::{self, BrowserAuthority, BrowserUnit, VerifiedFile};
use crate::module_release::ui::{LocalUiProvenance, UiAuthority, UiScope};
use crate::module_release::{ModuleComponent, ModuleReleaseError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const INSTALLATION_SCHEMA: &str = "ds.engine-unit-installation/v1";
pub const SELECTION_SCHEMA: &str = "ds.engine-unit-selection/v1";
pub const LOCAL_BUILD_SCHEMA: &str = "ds.engine-unit-local-build/v1";
pub const CAPTURE_BYTES_SCHEMA: &str = "ds.engine-unit-capture-bytes/v1";
pub const MAX_WIRE_BYTES: usize = 384 * 1024;
const MAX_UNIT_BYTES: i64 = 128 * 1024 * 1024;
const MAX_GENERATION: u64 = crate::desktop_instance::MAX_GENERATION;
const MAX_IDENTITY_BYTES: usize = 16 * 1024;

const NETWORK_FILES: [&str; 4] = [
    "ds_network_wasm.js",
    "ds_network_wasm_bg.wasm",
    "ds_network_wasm.d.ts",
    "ds_network_wasm_bg.wasm.d.ts",
];
const REPORTER_FILES: [&str; 4] = [
    "ds_reporter_wasm.js",
    "ds_reporter_wasm_bg.wasm",
    "ds_reporter_wasm.d.ts",
    "ds_reporter_wasm_bg.wasm.d.ts",
];

fn refuse(code: &'static str, message: &str) -> ModuleReleaseError {
    ModuleReleaseError {
        code,
        message: message.into(),
    }
}
fn fail<T>(code: &'static str, message: &str) -> Result<T, ModuleReleaseError> {
    Err(refuse(code, message))
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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

/// The four fixed members of a component's unit, JavaScript and WASM first.
pub fn member_names(component: ModuleComponent) -> Result<[&'static str; 4], ModuleReleaseError> {
    match component {
        ModuleComponent::Network => Ok(NETWORK_FILES),
        ModuleComponent::Reporter => Ok(REPORTER_FILES),
        ModuleComponent::Solar => fail(
            "module_release_target_unsupported",
            "Solar has no page engine unit",
        ),
    }
}

/// Admit an engine-unit target before any store, catalog or byte effect.
pub fn admit_target(component: &str, lane: &str) -> Result<ModuleComponent, ModuleReleaseError> {
    channel(lane)?;
    let component = ModuleComponent::from_name(component).ok_or_else(|| {
        refuse(
            "module_release_component_invalid",
            "Expected the network or reporter engine unit",
        )
    })?;
    member_names(component)?;
    Ok(component)
}

/// A unit built from a working tree for the Linux canary local-dev scope.
/// Core and the page are released independently: the capture types accept
/// members a newer Core adds; the unit digest still binds every typed fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalEngineBuild {
    pub schema: String,
    pub component: ModuleComponent,
    pub channel: String,
    /// The identity the tested bytes exported, admitted on construction.
    pub identity: Value,
    pub files: Vec<VerifiedFile>,
}

/// A published unit keeps its exact catalog shape; a local build has no
/// release, publication URL, governance status or gateway assertion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EngineUnit {
    Published(BrowserUnit),
    LocalBuild {
        local_build: LocalEngineBuild,
        unit_sha256: String,
        total_bytes: i64,
    },
}
impl EngineUnit {
    pub fn component(&self) -> ModuleComponent {
        match self {
            Self::Published(unit) => unit.release.component,
            Self::LocalBuild { local_build, .. } => local_build.component,
        }
    }
    pub fn channel(&self) -> &str {
        match self {
            Self::Published(unit) => &unit.release.channel,
            Self::LocalBuild { local_build, .. } => &local_build.channel,
        }
    }
    pub fn scope(&self) -> UiScope {
        match self {
            Self::Published(_) => UiScope::Release,
            Self::LocalBuild { .. } => UiScope::LocalDev,
        }
    }
    pub fn unit_sha256(&self) -> &str {
        match self {
            Self::Published(unit) => &unit.unit_sha256,
            Self::LocalBuild { unit_sha256, .. } => unit_sha256,
        }
    }
    pub fn total_bytes(&self) -> i64 {
        match self {
            Self::Published(unit) => unit.total_bytes,
            Self::LocalBuild { total_bytes, .. } => *total_bytes,
        }
    }
    pub fn version(&self) -> String {
        match self {
            Self::Published(unit) => unit.release.version.clone(),
            Self::LocalBuild { local_build, .. } => format!(
                "local-{}",
                local_build.identity["sourceSha"]
                    .as_str()
                    .unwrap_or_default()
            ),
        }
    }
    /// The four members' published or locally tested facts.
    pub fn files(&self) -> Vec<VerifiedFile> {
        match self {
            Self::Published(unit) => unit
                .release
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
pub struct InstalledEngineUnit {
    pub schema: String,
    pub unit: EngineUnit,
    pub authority: UiAuthority,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum EngineSelectionState {
    Installed {
        active: Box<InstalledEngineUnit>,
        previous: Option<Box<InstalledEngineUnit>>,
    },
    Removed {
        previous: Option<Box<InstalledEngineUnit>>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineUnitSelection {
    pub schema: String,
    pub component: ModuleComponent,
    pub channel: String,
    pub generation: u64,
    pub selection: EngineSelectionState,
}
/// A page load keeps this exact unit even if Core's selection changes later.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineUnitCapture {
    pub generation: u64,
    pub receipt: InstalledEngineUnit,
}
/// The bounded facts a page needs to read and verify a captured unit's bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineUnitByteDescriptor {
    pub schema: String,
    pub component: ModuleComponent,
    pub channel: String,
    pub scope: UiScope,
    pub unit_sha256: String,
    pub generation: u64,
    pub version: String,
    pub files: Vec<VerifiedFile>,
}

/// Inspect one published catalog for exactly this component and lane.
pub fn inspect(
    component: ModuleComponent,
    lane: &str,
    release: &Value,
) -> Result<EngineUnit, ModuleReleaseError> {
    member_names(component)?;
    let unit = browser::inspect(lane, release)?;
    if unit.release.component != component {
        return fail(
            "module_release_identity_mismatch",
            "The catalog names another engine unit",
        );
    }
    Ok(EngineUnit::Published(unit))
}

/// Asset requests are admitted only for the exact confirmed active unit.
pub fn acquisition_plan(
    unit: &EngineUnit,
    lane: &str,
    expected_sha256: &str,
    consent: bool,
) -> Result<Vec<crate::module_release::ModuleReleaseAsset>, ModuleReleaseError> {
    let EngineUnit::Published(unit) = unit else {
        return fail(
            "module_scope_invalid",
            "A local build is never acquired from a catalog",
        );
    };
    browser::acquisition_plan(unit, lane, expected_sha256, consent)
}

/// Construct a local unit from a working-tree build's tested files and the
/// identity those bytes exported. Only the canary local-dev scope admits it.
pub fn local_unit(
    component: ModuleComponent,
    lane: &str,
    identity: &Value,
    files: &[VerifiedFile],
) -> Result<EngineUnit, ModuleReleaseError> {
    let names = member_names(component)?;
    if lane != "canary" {
        return fail(
            "module_scope_invalid",
            "Local engine builds enter only the canary local-dev scope",
        );
    }
    if serde_json::to_vec(identity).map_or(true, |bytes| bytes.len() > MAX_IDENTITY_BYTES) {
        return fail(
            "module_engine_incompatible",
            "Local engine identity exceeds 16 KiB",
        );
    }
    admit_browser_engine_build(component, identity, None)
        .map_err(|error| refuse("module_engine_incompatible", strip(&error)))?;
    if files.len() != names.len() {
        return fail(
            "module_artifact_invalid",
            "A local engine build requires all four fixed members",
        );
    }
    let mut total = 0_i64;
    for name in names {
        let matches: Vec<_> = files.iter().filter(|file| file.filename == name).collect();
        let [file] = matches.as_slice() else {
            return fail(
                "module_artifact_invalid",
                "Local engine members are missing, repeated or foreign",
            );
        };
        if !hex(&file.sha256, 64) || file.size_bytes <= 0 || file.size_bytes > MAX_UNIT_BYTES {
            return fail(
                "module_artifact_invalid",
                "Local engine member size or digest is invalid",
            );
        }
        total = total
            .checked_add(file.size_bytes)
            .ok_or_else(|| refuse("module_artifact_invalid", "Local engine size overflow"))?;
    }
    if total > MAX_UNIT_BYTES {
        return fail(
            "module_artifact_invalid",
            "Local engine unit exceeds 128 MiB",
        );
    }
    let mut files = files.to_vec();
    files.sort_by(|a, b| a.filename.cmp(&b.filename));
    let local_build = LocalEngineBuild {
        schema: LOCAL_BUILD_SCHEMA.into(),
        component,
        channel: lane.into(),
        identity: identity.clone(),
        files,
    };
    let unit_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&local_build).expect("typed local engine build"))
    );
    Ok(EngineUnit::LocalBuild {
        local_build,
        unit_sha256,
        total_bytes: total,
    })
}
fn strip(error: &str) -> &str {
    error
        .strip_prefix("module_engine_incompatible: ")
        .unwrap_or(error)
}

/// A unit equals what its complete producer facts construct.
fn validate_unit(unit: &EngineUnit) -> Result<(), ModuleReleaseError> {
    let expected = match unit {
        EngineUnit::Published(published) => inspect(
            published.release.component,
            &published.release.channel,
            &serde_json::to_value(&published.release).expect("typed release"),
        )?,
        EngineUnit::LocalBuild { local_build, .. } => {
            if local_build.schema != LOCAL_BUILD_SCHEMA {
                return fail(
                    "module_receipt_invalid",
                    "Local engine build provenance is invalid",
                );
            }
            local_unit(
                local_build.component,
                &local_build.channel,
                &local_build.identity,
                &local_build.files,
            )?
        }
    };
    if *unit != expected {
        return fail(
            "module_receipt_invalid",
            "Engine unit differs from its complete producer facts",
        );
    }
    Ok(())
}

/// Hosts supply digests and sizes computed from held bytes, never catalog facts.
pub fn verify_files(unit: &EngineUnit, verified: &[VerifiedFile]) -> Result<(), ModuleReleaseError> {
    validate_unit(unit)?;
    let expected = unit.files();
    if verified.len() != expected.len() {
        return fail(
            "module_artifact_invalid",
            "Verify all four engine members",
        );
    }
    for file in &expected {
        if verified.iter().filter(|held| *held == file).count() != 1 {
            return fail(
                "module_artifact_invalid",
                "Engine member bytes differ from the installed unit",
            );
        }
    }
    Ok(())
}

fn validate_authority(
    unit: &EngineUnit,
    authority: &UiAuthority,
) -> Result<(), ModuleReleaseError> {
    match (unit, authority) {
        (EngineUnit::Published(_), UiAuthority::Published(authority)) => {
            let origin = url::Url::parse(&authority.gateway_origin).map_err(|_| {
                refuse("module_receipt_invalid", "Invalid engine release authority")
            })?;
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
                    "Invalid captured engine release authority",
                );
            }
            Ok(())
        }
        (EngineUnit::LocalBuild { local_build, .. }, UiAuthority::LocalBuild { local_build: provenance })
            if provenance.scope == UiScope::LocalDev
                && hex(&provenance.source_sha, 40)
                && local_build.identity["sourceSha"].as_str() == Some(provenance.source_sha.as_str()) =>
        {
            Ok(())
        }
        _ => fail(
            "module_receipt_invalid",
            "Engine authority differs from its unit's scope",
        ),
    }
}

/// The installed receipt for verified bytes, after deliberate consent.
pub fn prepare(
    unit: EngineUnit,
    authority: UiAuthority,
    verified: &[VerifiedFile],
    consent: bool,
) -> Result<InstalledEngineUnit, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Deliberate consent is required before engine unit installation",
        );
    }
    verify_files(&unit, verified)?;
    let lane = unit.channel().to_owned();
    let receipt = InstalledEngineUnit {
        schema: INSTALLATION_SCHEMA.into(),
        unit,
        authority,
    };
    validate_installation(&receipt, &lane)?;
    Ok(receipt)
}

/// A local build's authority is the source snapshot its identity names.
pub fn local_authority(unit: &EngineUnit) -> Result<UiAuthority, ModuleReleaseError> {
    let EngineUnit::LocalBuild { local_build, .. } = unit else {
        return fail(
            "module_scope_invalid",
            "A published unit carries its release authority",
        );
    };
    Ok(UiAuthority::LocalBuild {
        local_build: LocalUiProvenance {
            scope: UiScope::LocalDev,
            source_sha: local_build.identity["sourceSha"]
                .as_str()
                .unwrap_or_default()
                .into(),
        },
    })
}
/// A published unit's authority is the release gateway Core acquired it from.
pub fn published_authority(gateway_origin: &str, catalog_sha256: &str) -> UiAuthority {
    UiAuthority::Published(BrowserAuthority {
        gateway_origin: gateway_origin.into(),
        route_manifest_sha256: catalog_sha256.into(),
    })
}

fn validate_installation(
    receipt: &InstalledEngineUnit,
    lane: &str,
) -> Result<(), ModuleReleaseError> {
    channel(lane)?;
    if receipt.schema != INSTALLATION_SCHEMA || receipt.unit.channel() != lane {
        return fail(
            "module_receipt_invalid",
            "Engine installation crosses its lane or schema",
        );
    }
    validate_unit(&receipt.unit)?;
    if let EngineUnit::Published(unit) = &receipt.unit {
        if unit.release.status != "active" {
            return fail(
                "module_release_withdrawn",
                "Only an active engine release may be installed",
            );
        }
    }
    validate_authority(&receipt.unit, &receipt.authority)
}

/// Admit a retained receipt for this lane; offline, granting nothing.
pub fn admit_retained(
    receipt: &InstalledEngineUnit,
    lane: &str,
) -> Result<InstalledEngineUnit, ModuleReleaseError> {
    validate_installation(receipt, lane)?;
    Ok(receipt.clone())
}

pub fn validate_selection(
    selection: &EngineUnitSelection,
    component: ModuleComponent,
    lane: &str,
) -> Result<(), ModuleReleaseError> {
    channel(lane)?;
    member_names(component)?;
    if selection.schema != SELECTION_SCHEMA
        || selection.component != component
        || selection.channel != lane
        || selection.generation == 0
        || selection.generation > MAX_GENERATION
    {
        return fail(
            "module_receipt_invalid",
            "Invalid engine selection component, lane or generation",
        );
    }
    let (active, previous) = match &selection.selection {
        EngineSelectionState::Installed { active, previous } => (Some(active), previous),
        EngineSelectionState::Removed { previous } => (None, previous),
    };
    let scope = active.or(previous.as_ref()).map(|held| held.unit.scope());
    for held in active.into_iter().chain(previous) {
        if held.unit.component() != component || Some(held.unit.scope()) != scope {
            return fail(
                "module_receipt_invalid",
                "Engine selection crosses its component or scope",
            );
        }
        validate_installation(held, lane)?;
    }
    Ok(())
}
fn next_generation(
    current: Option<&EngineUnitSelection>,
    expected: u64,
) -> Result<u64, ModuleReleaseError> {
    let generation = current.map_or(0, |selection| selection.generation);
    if generation != expected || generation >= MAX_GENERATION {
        return fail(
            "module_selection_changed",
            "Engine selection changed; read it before retrying",
        );
    }
    Ok(generation + 1)
}
pub fn activate(
    current: Option<&EngineUnitSelection>,
    staged: InstalledEngineUnit,
    expected: u64,
    consent: bool,
    guard: ActivationGuard,
) -> Result<EngineUnitSelection, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Confirm engine unit activation",
        );
    }
    guard
        .admit()
        .map_err(|_| refuse("module_activation_deferred", "Retain held work before changing the engine selection"))?;
    let component = staged.unit.component();
    let lane = staged.unit.channel().to_owned();
    validate_installation(&staged, &lane)?;
    if let Some(current) = current {
        validate_selection(current, component, &lane)?;
    }
    let generation = next_generation(current, expected)?;
    if let Some(current) = current.filter(|current| matches!(&current.selection, EngineSelectionState::Installed { active, .. } if active.unit.unit_sha256() == staged.unit.unit_sha256())) {
        return Ok(current.clone());
    }
    let previous = current.and_then(|selection| match &selection.selection {
        EngineSelectionState::Installed { active, .. } => Some(active.clone()),
        EngineSelectionState::Removed { previous } => previous.clone(),
    });
    let selected = EngineUnitSelection {
        schema: SELECTION_SCHEMA.into(),
        component,
        channel: lane,
        generation,
        selection: EngineSelectionState::Installed {
            active: Box::new(staged),
            previous,
        },
    };
    validate_selection(&selected, component, &selected.channel)?;
    Ok(selected)
}
pub fn remove(
    current: &EngineUnitSelection,
    expected: u64,
    consent: bool,
) -> Result<EngineUnitSelection, ModuleReleaseError> {
    if !consent {
        return fail("module_consent_required", "Confirm engine unit removal");
    }
    validate_selection(current, current.component, &current.channel)?;
    let generation = next_generation(Some(current), expected)?;
    let EngineSelectionState::Installed { active, .. } = &current.selection else {
        return fail("module_removed", "This engine unit is already removed");
    };
    Ok(EngineUnitSelection {
        generation,
        selection: EngineSelectionState::Removed {
            previous: Some(active.clone()),
        },
        ..current.clone()
    })
}
pub fn rollback(
    current: &EngineUnitSelection,
    expected: u64,
    consent: bool,
    guard: ActivationGuard,
) -> Result<EngineUnitSelection, ModuleReleaseError> {
    if !consent {
        return fail(
            "module_consent_required",
            "Confirm restoration of the retained engine unit",
        );
    }
    guard
        .admit()
        .map_err(|_| refuse("module_activation_deferred", "Retain held work before rollback"))?;
    validate_selection(current, current.component, &current.channel)?;
    let generation = next_generation(Some(current), expected)?;
    let (active, previous) = match &current.selection {
        EngineSelectionState::Installed {
            active,
            previous: Some(previous),
        } => (previous.clone(), Some(active.clone())),
        EngineSelectionState::Removed {
            previous: Some(previous),
        } => (previous.clone(), None),
        _ => {
            return fail(
                "module_rollback_unavailable",
                "No retained engine unit is available",
            )
        }
    };
    Ok(EngineUnitSelection {
        generation,
        selection: EngineSelectionState::Installed { active, previous },
        ..current.clone()
    })
}

/// Capture the active unit offline for one page load.
pub fn capture(
    current: Option<&EngineUnitSelection>,
    component: ModuleComponent,
    lane: &str,
) -> Result<EngineUnitCapture, ModuleReleaseError> {
    let selected = current.ok_or_else(|| {
        refuse(
            "module_not_installed",
            "Install and activate this engine unit before using it",
        )
    })?;
    validate_selection(selected, component, lane)?;
    let EngineSelectionState::Installed { active, .. } = &selected.selection else {
        return fail(
            "module_removed",
            "This engine unit was explicitly removed; restore it before using it",
        );
    };
    Ok(EngineUnitCapture {
        generation: selected.generation,
        receipt: admit_retained(active, lane)?,
    })
}
fn validate_capture(captured: &EngineUnitCapture) -> Result<(), ModuleReleaseError> {
    if captured.generation == 0 || captured.generation > MAX_GENERATION {
        return fail("module_receipt_invalid", "Invalid captured engine generation");
    }
    validate_installation(&captured.receipt, captured.receipt.unit.channel())
}

/// Re-admit a capture for the lane and scope this page serves and project the
/// facts its member reads need.
pub fn capture_byte_descriptor(
    captured: &EngineUnitCapture,
    component: ModuleComponent,
    lane: &str,
    scope: UiScope,
) -> Result<EngineUnitByteDescriptor, ModuleReleaseError> {
    validate_capture(captured)?;
    let unit = &captured.receipt.unit;
    if unit.component() != component || unit.channel() != lane || unit.scope() != scope {
        return fail(
            "module_receipt_invalid",
            "The capture names another engine unit, lane or scope",
        );
    }
    Ok(EngineUnitByteDescriptor {
        schema: CAPTURE_BYTES_SCHEMA.into(),
        component,
        channel: lane.into(),
        scope,
        unit_sha256: unit.unit_sha256().into(),
        generation: captured.generation,
        version: unit.version(),
        files: unit.files(),
    })
}

/// Admit the bytes a realm loaded and the identity they export against the
/// captured installation. A selection change cannot retarget a capture.
pub fn admit_loaded(
    captured: &EngineUnitCapture,
    verified: &[VerifiedFile],
    identity: &Value,
) -> Result<(), ModuleReleaseError> {
    validate_capture(captured)?;
    let unit = &captured.receipt.unit;
    verify_files(unit, verified)?;
    if serde_json::to_vec(identity).map_or(true, |bytes| bytes.len() > MAX_IDENTITY_BYTES) {
        return fail(
            "module_engine_incompatible",
            "Loaded engine identity exceeds 16 KiB",
        );
    }
    match unit {
        EngineUnit::Published(published) => {
            let asset = &published.release.assets[0];
            let producer = BrowserEngineProducer {
                version: published
                    .release
                    .version
                    .split("-r")
                    .next()
                    .unwrap_or_default(),
                source_sha: &asset.source_sha,
                manifest_sha256: &asset.engines[0].manifest_sha256,
            };
            admit_browser_engine_build(published.release.component, identity, Some(producer))
                .map_err(|_| {
                    refuse(
                        "module_engine_incompatible",
                        "Loaded engine identity differs from its published producer facts",
                    )
                })?;
        }
        EngineUnit::LocalBuild { local_build, .. } => {
            if *identity != local_build.identity {
                return fail(
                    "module_engine_incompatible",
                    "Loaded engine identity differs from the tested local build",
                );
            }
        }
    }
    Ok(())
}

/// How this compiled page obtains an engine unit: installed Core's component
/// commands through the Desktop door (`core`) or, on the Local Dev page,
/// through its development host (`development`); or its own retained browser
/// selection (`browser`, hosted Web). The lane and scope are this build's,
/// never the caller's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EngineUnitHost {
    pub host: &'static str,
    pub component: ModuleComponent,
    pub channel: &'static str,
    pub scope: UiScope,
    /// The Core command and arguments that capture the selected unit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture: Option<CoreRead>,
    /// The Core command and fixed arguments of one bounded member read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<CoreRead>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CoreRead {
    pub command: &'static str,
    pub args: Value,
}

/// The declared host effect for `component` in a build serving `lane`
/// (`canary`/`stable`), local or deployed, on a Core-attached placement or not.
pub fn host_for(
    component: ModuleComponent,
    lane: &'static str,
    local: bool,
    core_attached: bool,
) -> Result<EngineUnitHost, ModuleReleaseError> {
    member_names(component)?;
    channel(lane)?;
    let scope = if local {
        UiScope::LocalDev
    } else {
        UiScope::Release
    };
    if local && lane != "canary" {
        return fail(
            "module_scope_invalid",
            "A local build selects the canary local-dev engine unit",
        );
    }
    if !core_attached && !local {
        return Ok(EngineUnitHost {
            host: "browser",
            component,
            channel: lane,
            scope,
            capture: None,
            member: None,
        });
    }
    let args = serde_json::json!({
        "component": component.as_str(),
        "channel": lane,
        "runtime": "wasm",
        "scope": if local { "local-dev" } else { "release" },
    });
    Ok(EngineUnitHost {
        host: if core_attached { "core" } else { "development" },
        component,
        channel: lane,
        scope,
        capture: Some(CoreRead {
            command: "core.component.capture",
            args: args.clone(),
        }),
        member: Some(CoreRead {
            command: "core.component.member",
            args,
        }),
    })
}

/// This build's declared host effect. Desktop builds reach Core; a local page
/// build reaches Core through its development host; a deployed page keeps its
/// own browser selection.
pub fn host(component: ModuleComponent) -> Result<EngineUnitHost, ModuleReleaseError> {
    let target = crate::gateway_target::target();
    let lane = match target.lane {
        Some("canary") => "canary",
        Some("production") => "stable",
        _ => {
            return fail(
                "module_host_unavailable",
                "This build names no release lane for engine units",
            )
        }
    };
    host_for(
        component,
        lane,
        target.local,
        cfg!(feature = "platform-desktop"),
    )
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Host {
        component: ModuleComponent,
    },
    Captured {
        component: ModuleComponent,
        capture: Box<EngineUnitCapture>,
    },
    Loaded {
        component: ModuleComponent,
        capture: Box<EngineUnitCapture>,
        verified: Vec<VerifiedFile>,
        identity: Value,
    },
}

/// One closed page request: the declared host effect, a capture's byte facts
/// for this build, or the loaded-identity admission.
pub fn evaluate(input: &[u8]) -> Result<Value, ModuleReleaseError> {
    if input.is_empty() || input.len() > MAX_WIRE_BYTES {
        return fail(
            "module_request_invalid",
            "Engine unit request exceeds 384 KiB",
        );
    }
    let request: Request = serde_json::from_slice(input).map_err(|_| {
        refuse(
            "module_request_invalid",
            "Malformed closed engine unit request",
        )
    })?;
    Ok(match request {
        Request::Host { component } => serde_json::to_value(host(component)?).expect("typed host"),
        Request::Captured { component, capture } => {
            let declared = host(component)?;
            serde_json::to_value(capture_byte_descriptor(
                &capture,
                component,
                declared.channel,
                declared.scope,
            )?)
            .expect("typed descriptor")
        }
        Request::Loaded {
            component,
            capture,
            verified,
            identity,
        } => {
            let declared = host(component)?;
            capture_byte_descriptor(&capture, component, declared.channel, declared.scope)?;
            admit_loaded(&capture, &verified, &identity)?;
            Value::Null
        }
    })
}

#[cfg(feature = "wasm")]
mod wasm {
    use wasm_bindgen::prelude::*;

    /// `{kind:'host'|'captured'|'loaded', component, ...}` as JSON bytes; the
    /// answer is JSON text, a refusal is `code: message`.
    #[wasm_bindgen(js_name = engineUnit)]
    pub fn engine_unit(input: &[u8]) -> Result<String, JsValue> {
        let value = super::evaluate(input).map_err(|error| JsValue::from_str(&error.to_string()))?;
        serde_json::to_string(&value).map_err(|error| JsValue::from_str(&error.to_string()))
    }
}

/// The Local Dev page's development host serves Core's capture and members of
/// one unit as plain reads on the page's own origin:
/// `<route>/<component>/capture` (Core's capture envelope) and
/// `<route>/<component>/<unit_sha256>/<member>` (the member's bytes).
pub const DEVELOPMENT_ROUTE: &str = "/__ds_local/engine-units";

/// The development host's capture and member addresses for `component`, on a
/// page `origin`, in a build whose declared host is `development`.
pub fn development_reads(
    declared: &EngineUnitHost,
    origin: &str,
) -> Result<String, ModuleReleaseError> {
    if declared.host != "development" {
        return fail(
            "module_host_unavailable",
            "This build reads engine units through its Core door, not a development host",
        );
    }
    let parsed = url::Url::parse(origin)
        .map_err(|_| refuse("module_host_unavailable", "The page origin is not a URL"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || parsed.path() != "/"
        || parsed.query().is_some()
    {
        return fail("module_host_unavailable", "The page origin is not a web origin");
    }
    Ok(format!(
        "{}{DEVELOPMENT_ROUTE}/{}",
        origin.trim_end_matches('/'),
        declared.component.as_str()
    ))
}

/// Admit the development host's answer to a capture read: Core's envelope,
/// whose refusal stays Core's own code, or a capture for this build's lane
/// and scope, projected with its byte facts.
pub fn development_capture(
    declared: &EngineUnitHost,
    answer: &[u8],
) -> Result<Value, (String, String)> {
    let owned = |error: ModuleReleaseError| (error.code.to_owned(), error.message);
    if answer.len() > MAX_WIRE_BYTES {
        return Err(owned(refuse(
            "module_host_unavailable",
            "The development host's capture exceeds 384 KiB",
        )));
    }
    let envelope: Value = serde_json::from_slice(answer).map_err(|_| {
        owned(refuse(
            "module_host_unavailable",
            "The development host answered no Core envelope",
        ))
    })?;
    match envelope["status"].as_str() {
        Some("ok") => {}
        Some("error") => {
            let code = envelope["error"]["code"].as_str().unwrap_or_default();
            let message = envelope["error"]["message"].as_str().unwrap_or_default();
            let valid = !code.is_empty()
                && code.len() <= 64
                && code
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            return Err(if valid {
                (code.to_owned(), message.chars().take(1024).collect())
            } else {
                owned(refuse("module_host_unavailable", "Core answered an unnamed refusal"))
            });
        }
        _ => {
            return Err(owned(refuse(
                "module_host_unavailable",
                "The development host answered no Core envelope",
            )))
        }
    }
    let capture: EngineUnitCapture = serde_json::from_value(envelope["data"].clone())
        .map_err(|_| owned(refuse("module_receipt_invalid", "Malformed engine unit capture")))?;
    let descriptor =
        capture_byte_descriptor(&capture, declared.component, declared.channel, declared.scope)
            .map_err(owned)?;
    Ok(serde_json::json!({ "capture": capture, "descriptor": descriptor }))
}

/// One member read: a fixed member of the captured unit, with the size and
/// digest its capture declares.
pub fn development_member(
    declared: &EngineUnitHost,
    base: &str,
    unit_sha256: &str,
    filename: &str,
    size_bytes: f64,
    sha256: &str,
) -> Result<(String, u64), ModuleReleaseError> {
    let names = member_names(declared.component)?;
    if !names.contains(&filename)
        || !hex(unit_sha256, 64)
        || !hex(sha256, 64)
        || !(size_bytes.is_finite()
            && size_bytes >= 1.0
            && size_bytes <= MAX_UNIT_BYTES as f64
            && size_bytes.fract() == 0.0)
    {
        return fail(
            "module_artifact_invalid",
            "Name a fixed member of the captured unit with its declared size and digest",
        );
    }
    Ok((format!("{base}/{unit_sha256}/{filename}"), size_bytes as u64))
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod development {
    use super::*;
    use futures_util::StreamExt;
    use wasm_bindgen::prelude::*;

    fn refused(code: &str, message: &str) -> JsValue {
        JsValue::from_str(&format!("{code}: {message}"))
    }
    fn declared(component: &str) -> Result<(EngineUnitHost, String), JsValue> {
        let component = ModuleComponent::from_name(component)
            .ok_or_else(|| refused("module_release_component_invalid", "Unknown engine unit"))?;
        let declared = host(component).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let origin = crate::gateway_exchange::browser::gateway_origin()?;
        let base = development_reads(&declared, &origin).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok((declared, base))
    }

    /// Read Core's capture of this build's local-dev unit through the Local
    /// Dev development host; answers `{ capture, descriptor }` as JSON.
    #[wasm_bindgen(js_name = engineUnitDevelopmentCapture)]
    pub async fn development_capture_read(component: String) -> Result<String, JsValue> {
        let (declared, base) = declared(&component)?;
        let failed = || refused("module_host_unavailable", "The development host could not be read");
        let response = reqwest::Client::new()
            .get(format!("{base}/capture"))
            .send()
            .await
            .map_err(|_| failed())?;
        if !response.status().is_success() {
            return Err(refused(
                "module_host_unavailable",
                &format!("The development host answered HTTP {}", response.status().as_u16()),
            ));
        }
        let mut stream = response.bytes_stream();
        let mut answer = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| failed())?;
            if answer.len() + chunk.len() > MAX_WIRE_BYTES {
                return Err(refused("module_host_unavailable", "The development host's capture exceeds 384 KiB"));
            }
            answer.extend_from_slice(&chunk);
        }
        let admitted = development_capture(&declared, &answer)
            .map_err(|(code, message)| refused(&code, &message))?;
        Ok(admitted.to_string())
    }

    /// Read one member of a captured unit through the development host,
    /// verified against the size and digest the capture declares.
    #[wasm_bindgen(js_name = engineUnitDevelopmentMember)]
    pub async fn development_member_read(
        component: String,
        unit_sha256: String,
        filename: String,
        size_bytes: f64,
        sha256: String,
    ) -> Result<Vec<u8>, JsValue> {
        let (declared, base) = declared(&component)?;
        let (url, size) =
            development_member(&declared, &base, &unit_sha256, &filename, size_bytes, &sha256)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
        crate::gateway_exchange::storage::fetch_verified(&url, size, &sha256)
            .await
            .map_err(|refusal| refused("module_artifact_invalid", &refusal.message))
    }
}

#[cfg(test)]
#[path = "engine_unit_tests.rs"]
mod tests;
