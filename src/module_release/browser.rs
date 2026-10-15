//! Browser component admission. Hosts acquire and hash bytes; this owner decides
//! which complete unit and retained selection may be used. Native executable
//! receipts are deliberately not accepted here. Public metadata alone never
//! grants installation, execution or project authority.
use super::*;
use crate::module_install::ActivationGuard;
use sha2::{Digest, Sha256};

pub const PROTOCOL: &str = "ds.network-wasm/v1";
pub const INSTALLATION_SCHEMA: &str = "ds.browser-module-installation/v1";
pub const SELECTION_SCHEMA: &str = "ds.browser-module-selection/v1";
pub const MAX_WIRE_BYTES: usize = 384 * 1024;
const MAX_UNIT_BYTES: i64 = 128 * 1024 * 1024;
const MAX_GENERATION: u64 = crate::desktop_instance::MAX_GENERATION;
const FILES: [(&str, &str); 4] = [
    ("ds_network_wasm.js", "module-javascript"),
    ("ds_network_wasm_bg.wasm", "module-wasm"),
    ("ds_network_wasm.d.ts", "module-types"),
    ("ds_network_wasm_bg.wasm.d.ts", "module-types"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    if ModuleComponent::parse(component)? != ModuleComponent::Network {
        return fail(
            "module_release_target_unsupported",
            "Only Network has an admitted browser publication contract",
        );
    }
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
        .is_some_and(|assets| assets.len() == FILES.len() && assets.iter().all(bounded_asset_shape))
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
    let expected_product = format!(
        "rw.datasolutions.module.network{}",
        if channel_name == "canary" {
            ".canary"
        } else {
            ""
        }
    );
    if release.schema != MODULE_RELEASE_SCHEMA
        || release.runtime != "wasm"
        || release.component != ModuleComponent::Network
        || release.channel != channel_name
        || release.product_id != expected_product
        || release.protocol != PROTOCOL
    {
        return fail(
            "module_release_identity_mismatch",
            "Expected this lane's Network browser catalog",
        );
    }
    if !release_folder(&release.version)
        || OffsetDateTime::parse(&release.released_at, &Rfc3339).is_err()
        || !matches!(
            release.status.as_str(),
            "active" | "blocked" | "stopped" | "rolled_back"
        )
        || release.assets.len() != FILES.len()
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
            || asset.protocol != PROTOCOL
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
        validate_asset_provenance(ModuleComponent::Network, asset)?;
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
            || parts[3] != "network"
            || parts[4] != channel_name
            || parts[5] != "wasm"
            || parts[6] != release.version
            || !FILES
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
    let asset = &receipt.unit.release.assets[0];
    let version = receipt
        .unit
        .release
        .version
        .split("-r")
        .next()
        .unwrap_or_default();
    if identity["schema"] != "ds.network-engine/v1"
        || identity["protocolMajor"] != 1
        || identity["environment"] != "wasm"
        || identity["runtime"]["environment"] != "wasm"
        || identity["coreVersion"] != version
        || identity["sourceSha"] != asset.source_sha
        || identity["dependencyClosureSha256"] != asset.engines[0].manifest_sha256
        || !matches!(
            identity["jsonObjectOrder"].as_str(),
            Some("sorted" | "insertion")
        )
        || !identity["capabilities"]
            .as_array()
            .is_some_and(|capabilities| {
                capabilities
                    .iter()
                    .any(|capability| capability == "lv_processing_v1")
            })
    {
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
    if verified.len() != FILES.len() {
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
    if selection.schema != SELECTION_SCHEMA
        || selection.component != ModuleComponent::Network
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
        validate_installation(active, lane)?;
    }
    if let Some(previous) = previous {
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
        component: ModuleComponent::Network,
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
    Installation {
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
            Request::Installation {
                receipt,
                channel,
                gateway_origin,
            } => serde_json::to_value(admit_retained(&receipt, &channel, &gateway_origin)?)
                .expect("typed retained receipt"),
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
            Request::Inspect { channel, release } => {
                serde_json::to_value(inspect(&channel, &release)?).expect("typed unit")
            }
            Request::Prepare {
                unit,
                authority,
                verified,
                identity,
                consent,
            } => serde_json::to_value(prepare(*unit, authority, &verified, identity, consent)?)
                .expect("typed receipt"),
            Request::Selection { channel, current } => {
                self::channel(&channel)?;
                if let Some(selection) = &current {
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
mod tests {
    use super::*;
    use serde_json::json;
    fn catalog() -> Value {
        serde_json::from_str::<Value>(include_str!("fixtures/network-wasm-canary.json")).unwrap()
            ["release"]
            .clone()
    }
    fn ready() -> InstalledBrowserModule {
        let unit = inspect("canary", &catalog()).unwrap();
        let verified = unit
            .release
            .assets
            .iter()
            .map(|asset| VerifiedFile {
                filename: filename(asset).unwrap(),
                sha256: asset.sha256.clone(),
                size_bytes: asset.size_bytes,
            })
            .collect::<Vec<_>>();
        let identity = json!({"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"environment":"wasm","runtime":{"environment":"wasm","logicalCpus":1,"internalWorkerThreads":1},"coreVersion":"0.1.0","sourceSha":unit.release.assets[0].source_sha,"dependencyClosureSha256":unit.release.assets[0].engines[0].manifest_sha256,"jsonObjectOrder":"sorted","capabilities":["lv_processing_v1"]});
        prepare(
            unit,
            BrowserAuthority {
                gateway_origin: "https://gateway.example".into(),
                route_manifest_sha256: "a".repeat(64),
            },
            &verified,
            identity,
            true,
        )
        .unwrap()
    }
    #[test]
    fn published_complete_unit_has_stable_identity_without_reinterpreting_native_catalogs() {
        let value = catalog();
        let unit = inspect("canary", &value).unwrap();
        assert_eq!(unit.total_bytes, 26_795_337);
        let mut reversed = value.clone();
        reversed["assets"].as_array_mut().unwrap().reverse();
        assert_eq!(
            unit.unit_sha256,
            inspect("canary", &reversed).unwrap().unit_sha256
        );
        assert!(validate_module_release("network", "canary", &value).is_err());
        assert!(inspect("stable", &value).is_err());
        let mut native = value.clone();
        native["runtime"] = json!("native");
        assert!(inspect("canary", &native).is_err());
    }
    #[test]
    fn acquisition_requires_exact_consent_lane_and_current_complete_catalog() {
        let unit = inspect("canary", &catalog()).unwrap();
        assert_eq!(
            acquisition_plan(&unit, "canary", &unit.unit_sha256, true)
                .unwrap()
                .len(),
            4
        );
        assert_eq!(
            acquisition_plan(&unit, "canary", &unit.unit_sha256, false)
                .unwrap_err()
                .code,
            "module_consent_required"
        );
        assert!(acquisition_plan(&unit, "stable", &unit.unit_sha256, true).is_err());
        assert_eq!(
            acquisition_plan(&unit, "canary", &"a".repeat(64), true)
                .unwrap_err()
                .code,
            "module_selection_changed"
        );
        for component in ["solar", "reporter"] {
            assert!(admit_target(component, "canary").is_err());
        }
        let mut withdrawn = catalog();
        withdrawn["status"] = json!("blocked");
        let unit = inspect("canary", &withdrawn).unwrap();
        assert_eq!(
            acquisition_plan(&unit, "canary", &unit.unit_sha256, true)
                .unwrap_err()
                .code,
            "module_release_withdrawn"
        );
    }
    #[test]
    fn incomplete_crossed_mixed_oversized_and_unknown_members_refuse() {
        let base = catalog();
        let mut cases = Vec::new();
        let mut v = base.clone();
        v["assets"].as_array_mut().unwrap().pop();
        cases.push(v);
        let mut v = base.clone();
        v["assets"][1] = v["assets"][0].clone();
        cases.push(v);
        let mut v = base.clone();
        v["assets"][0]["source_revisions"]["ds-command-kernel"] = json!("a".repeat(40));
        cases.push(v);
        let mut v = base.clone();
        v["assets"][0]["url"] = json!("https://storage.googleapis.com/bucket/modules/network/stable/wasm/0.1.0-r1/ds_network_wasm.d.ts");
        cases.push(v);
        let mut v = base.clone();
        v["assets"][0]["size_bytes"] = json!(MAX_UNIT_BYTES);
        cases.push(v);
        let mut v = base.clone();
        v["assets"][0]["built_at"] = json!("bad");
        cases.push(v);
        let mut v = base.clone();
        v["assets"][0]["url"] = json!(format!(
            "{}?redirect=bad",
            v["assets"][0]["url"].as_str().unwrap()
        ));
        cases.push(v);
        let mut v = base.clone();
        v["assets"][0]["executable"] = json!("evil");
        cases.push(v);
        let mut v = base;
        v["extra"] = json!(true);
        cases.push(v);
        for value in cases {
            assert!(inspect("canary", &value).is_err(), "{value}");
        }
    }
    #[test]
    fn staging_requires_consent_all_verified_bytes_and_exact_loaded_identity() {
        let receipt = ready();
        let files = receipt
            .unit
            .release
            .assets
            .iter()
            .map(|asset| VerifiedFile {
                filename: filename(asset).unwrap(),
                sha256: asset.sha256.clone(),
                size_bytes: asset.size_bytes,
            })
            .collect::<Vec<_>>();
        let stage = |unit, verified: &[VerifiedFile], identity, consent| {
            prepare(unit, receipt.authority.clone(), verified, identity, consent)
        };
        assert_eq!(
            stage(
                receipt.unit.clone(),
                &files,
                receipt.engine_identity.clone(),
                false
            )
            .unwrap_err()
            .code,
            "module_consent_required"
        );
        assert!(stage(
            receipt.unit.clone(),
            &files[..3],
            receipt.engine_identity.clone(),
            true
        )
        .is_err());
        let mut corrupt = files.clone();
        corrupt[0].sha256 = "b".repeat(64);
        assert!(stage(
            receipt.unit.clone(),
            &corrupt,
            receipt.engine_identity.clone(),
            true
        )
        .is_err());
        for (key, value) in [
            ("environment", json!("native")),
            ("sourceSha", json!("c".repeat(40))),
            ("dependencyClosureSha256", json!("c".repeat(64))),
            ("coreVersion", json!("0.2.0")),
            ("jsonObjectOrder", json!("unknown")),
            ("capabilities", json!([])),
        ] {
            let mut identity = receipt.engine_identity.clone();
            identity[key] = value;
            assert!(stage(receipt.unit.clone(), &files, identity, true).is_err());
        }
        let mut unit = receipt.unit.clone();
        unit.unit_sha256 = "d".repeat(64);
        assert!(stage(unit, &files, receipt.engine_identity.clone(), true).is_err());
        let mut unit = receipt.unit.clone();
        unit.release.status = "blocked".into();
        assert_eq!(
            stage(unit, &files, receipt.engine_identity.clone(), true)
                .unwrap_err()
                .code,
            "module_release_withdrawn"
        );
    }
    #[test]
    fn retained_receipt_requires_this_hosts_exact_lane_and_authority() {
        let receipt = ready();
        assert_eq!(
            admit_retained(&receipt, "canary", "https://gateway.example").unwrap(),
            receipt
        );
        assert!(admit_retained(&receipt, "stable", "https://gateway.example").is_err());
        assert!(admit_retained(&receipt, "canary", "https://other.example").is_err());
        let wire = serde_json::to_vec(&json!({"kind":"installation","receipt":receipt,"channel":"canary","gateway_origin":"https://gateway.example"})).unwrap();
        let result: InstalledBrowserModule =
            serde_json::from_str(&evaluate(&wire).unwrap()).unwrap();
        assert_eq!(result, receipt);
    }
    #[test]
    fn byte_verification_precedes_loaded_identity_and_rejects_tampered_units() {
        let receipt = ready();
        let mut files = receipt
            .unit
            .release
            .assets
            .iter()
            .map(|asset| VerifiedFile {
                filename: filename(asset).unwrap(),
                sha256: asset.sha256.clone(),
                size_bytes: asset.size_bytes,
            })
            .collect::<Vec<_>>();
        let wire = |unit: &BrowserUnit, files: &[VerifiedFile]| {
            serde_json::to_vec(&json!({"kind":"verify_files","unit":unit,"verified":files}))
                .unwrap()
        };
        assert_eq!(evaluate(&wire(&receipt.unit, &files)).unwrap(), "null");
        files[0].size_bytes += 1;
        assert!(evaluate(&wire(&receipt.unit, &files))
            .unwrap_err()
            .starts_with("module_artifact_invalid"));
        files[0].size_bytes -= 1;
        files[0].filename = files[1].filename.clone();
        assert!(verify_files(&receipt.unit, &files).is_err());
        let mut unit = receipt.unit.clone();
        unit.total_bytes += 1;
        assert!(verify_files(&unit, &[]).is_err());
        let mut withdrawn = receipt;
        withdrawn.unit.release.status = "blocked".into();
        withdrawn.unit = inspect(
            "canary",
            &serde_json::to_value(&withdrawn.unit.release).unwrap(),
        )
        .unwrap();
        assert!(activate(None, withdrawn, 0, true, ActivationGuard::default()).is_err());
    }
    #[test]
    fn selection_fences_held_work_and_supports_explicit_offline_remove_restore() {
        let receipt = ready();
        let selected =
            activate(None, receipt.clone(), 0, true, ActivationGuard::default()).unwrap();
        assert_eq!(selected.generation, 1);
        assert_eq!(
            activate(
                Some(&selected),
                receipt.clone(),
                1,
                true,
                ActivationGuard::default()
            )
            .unwrap(),
            selected
        );
        assert!(activate(
            Some(&selected),
            receipt.clone(),
            0,
            true,
            ActivationGuard::default()
        )
        .is_err());
        assert!(activate(
            None,
            receipt,
            0,
            true,
            ActivationGuard {
                unsaved_edits: true,
                ..Default::default()
            }
        )
        .is_err());
        assert!(remove(&selected, 1, false).is_err());
        let removed = remove(&selected, 1, true).unwrap();
        assert_eq!(removed.generation, 2);
        assert!(matches!(
            removed.selection,
            BrowserSelectionState::Removed { .. }
        ));
        assert!(remove(&removed, 1, true).is_err());
        assert!(rollback(&removed, 2, false, ActivationGuard::default()).is_err());
        let restored = rollback(&removed, 2, true, ActivationGuard::default()).unwrap();
        assert_eq!(restored.generation, 3);
        assert!(matches!(
            restored.selection,
            BrowserSelectionState::Installed { .. }
        ));
        assert!(validate_selection(&restored, "stable").is_err());
        let mut corrupt = restored;
        corrupt.generation = MAX_GENERATION;
        assert!(remove(&corrupt, MAX_GENERATION, true).is_err());
    }
    #[test]
    fn wire_is_closed_bounded_and_exposes_the_same_inspection_owner() {
        let wire =
            serde_json::to_vec(&json!({"kind":"inspect","channel":"canary","release":catalog()}))
                .unwrap();
        let unit: BrowserUnit = serde_json::from_str(&evaluate(&wire).unwrap()).unwrap();
        assert_eq!(unit, inspect("canary", &catalog()).unwrap());
        for input in [
            b"{}".as_slice(),
            b"{\"kind\":\"selection\",\"channel\":\"canary\",\"current\":null,\"unknown\":true}",
        ] {
            assert!(evaluate(input).is_err());
        }
        assert!(evaluate(&vec![b' '; MAX_WIRE_BYTES + 1]).is_err());
    }
}
