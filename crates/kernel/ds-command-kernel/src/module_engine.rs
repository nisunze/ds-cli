//! Module components and their engine build admission: native executables,
//! browser engine units and UI units.
//!
//! One decision serves the installed updaters (`module_install`,
//! `module_release::{browser, ui}`) and release packaging.
//! `crates/ds-module-admission` compiles this same file through `#[path]`, so
//! packaging admits a freshly built engine with exactly the rules an
//! installation applies, without compiling the whole Kernel or adding a
//! dependency edge to it. Keep this file free of `crate::` paths and of
//! dependencies beyond serde, serde_json, sha2 and regex.
use std::collections::BTreeMap;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const MODULE_PROTOCOL: &str = "ds.module/v1";
pub const NETWORK_PROTOCOL: &str = "ds.network-engine/v1";
pub const ENGINE_BUILD_SCHEMA: &str = "ds.engine-build/v1";

/// The document family a schema tag names, without its `/vN` revision.
///
/// Owner rule (22 Oct 2026): independently released components are compatible
/// by the required typed facts and capabilities they exchange, never by equal
/// versions, schema revisions or protocol integers. The family still says what
/// a document is (a build-info is not an engine identity); the revision is
/// provenance only.
pub fn schema_family(schema: &str) -> &str {
    match schema.rsplit_once('/') {
        Some((family, revision))
            if revision.len() > 1
                && revision.starts_with('v')
                && revision[1..].bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            family
        }
        _ => schema,
    }
}

/// Does this JSON string name a document of the same family as `expected`?
fn same_family(value: &Value, expected: &str) -> bool {
    value
        .as_str()
        .is_some_and(|schema| schema_family(schema) == schema_family(expected))
}

/// A protocol integer is provenance: present and typed, never compared.
fn protocol_integer(value: &Value) -> bool {
    value
        .as_u64()
        .is_some_and(|major| major <= u64::from(u32::MAX))
}
/// Build-info beyond this bound is refused before any field is read.
pub const MAX_BUILD_INFO_BYTES: usize = 16 * 1024;

const NETWORK_CAPABILITIES: [&str; 11] = [
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
];
const SOLAR_SCHEMAS: [&str; 4] = [
    "ds-solar.prepared-calculation-result/v1",
    "ds-solar.result/v1",
    "ds-solar.prepared-city-input/v1",
    "ds-solar.worker-budget/v1",
];
const REPORTER_SCHEMAS: [&str; 2] = [ENGINE_BUILD_SCHEMA, "ds-network-reporter.task-result/v1"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleComponent {
    Solar,
    Reporter,
    Network,
}

impl ModuleComponent {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "solar" => Some(Self::Solar),
            "reporter" => Some(Self::Reporter),
            "network" => Some(Self::Network),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solar => "solar",
            Self::Reporter => "reporter",
            Self::Network => "network",
        }
    }

    pub fn engine_owner(self) -> &'static str {
        match self {
            Self::Solar => "ds-solar",
            Self::Reporter => "ds-network-reporter",
            Self::Network => "ds-network",
        }
    }

    pub fn protocol(self) -> &'static str {
        match self {
            Self::Solar | Self::Reporter => MODULE_PROTOCOL,
            Self::Network => NETWORK_PROTOCOL,
        }
    }

    pub fn executable(self) -> &'static str {
        match self {
            Self::Solar => "solar",
            Self::Reporter => "reporter",
            Self::Network => "ds-network",
        }
    }
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

/// Producer facts an admitted engine build carries into its release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmittedEngineBuild {
    pub version: String,
    pub source_sha: String,
    pub manifest_sha256: String,
    pub release: String,
}

pub fn check_build_info_size(build_info: &Value) -> Result<(), String> {
    if serde_json::to_vec(build_info)
        .map_err(|e| e.to_string())?
        .len()
        > MAX_BUILD_INFO_BYTES
    {
        return Err("module_engine_incompatible: excessive build-info".into());
    }
    Ok(())
}

/// Admit one engine's own release build-info for a component and platform.
///
/// `producer_release` is the release an existing publication names; without
/// one, the release is derived from the admitted owner, version and source.
/// No executable is started here, and an unknown future capability grants
/// no operation.
pub fn admit_engine_build(
    component: ModuleComponent,
    platform: &str,
    v: &Value,
    producer_release: Option<&str>,
) -> Result<AdmittedEngineBuild, String> {
    check_build_info_size(v)?;
    let target = platform_target(platform)?;
    let (version, source, manifest) = if component == ModuleComponent::Network {
        // Document families and typed facts decide; schema revisions and the
        // protocol integer are provenance (owner 22 Oct). The required
        // capabilities below decide whether this build can serve Core.
        if !same_family(&v["schema"], ENGINE_BUILD_SCHEMA)
            || v["module"] != "network"
            || !same_family(&v["engineIdentity"]["schema"], component.protocol())
            || !protocol_integer(&v["engineIdentity"]["protocolMajor"])
            || v["engineIdentity"]["environment"] != "native"
            || v["engineIdentity"]["runtime"]["environment"] != "native"
            || v["build"]["profile"] != "release"
            || v["build"]["target"] != target
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
        require_tokens(&engine["capabilities"], &NETWORK_CAPABILITIES)?;
        (
            field(engine, "coreVersion")?,
            field(engine, "sourceSha")?,
            field(engine, "dependencyClosureSha256")?,
        )
    } else {
        if !same_family(&v["schema"], ENGINE_BUILD_SCHEMA)
            || v["profile"] != "release"
            || v["target"] != target
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
            require_tokens(&v["schemas"], &SOLAR_SCHEMAS)?;
            (
                field(v, "version")?,
                field(v, "source_sha")?,
                field(v, "build_manifest_sha256")?,
            )
        } else {
            let derived;
            let expected = match producer_release {
                Some(release) => release,
                None => {
                    derived = release(
                        component,
                        v["package_version"].as_str().unwrap_or_default(),
                        v["source_sha"].as_str().unwrap_or_default(),
                    );
                    derived.as_str()
                }
            };
            if v["service"] != "ds-network-reporter"
                || v["binary"] != "ds-report"
                || v["features"] != serde_json::json!([])
                || v["engine_version"] != expected
            {
                return Err(
                    "module_engine_incompatible: different Reporter owner or non-compute build"
                        .into(),
                );
            }
            require_tokens(&v["schemas"], &REPORTER_SCHEMAS)?;
            (
                field(v, "package_version")?,
                field(v, "source_sha")?,
                field(v, "build_manifest_sha256")?,
            )
        }
    };
    if !hex(source, 40) || !hex(manifest, 64) || !semantic_version(version) {
        return Err("module_engine_incompatible: probe source, closure or release differs from its exact producer".into());
    }
    Ok(AdmittedEngineBuild {
        version: version.into(),
        source_sha: source.into(),
        manifest_sha256: manifest.into(),
        release: release(component, version, source),
    })
}

fn release(component: ModuleComponent, version: &str, source: &str) -> String {
    format!("{}@{}+{}", component.engine_owner(), version, source)
}

fn semantic_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 20 && part.bytes().all(|b| b.is_ascii_digit())
        })
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

/// A browser engine unit (Network, Reporter) and the identity it exports.
pub struct BrowserEngineProfile {
    pub protocol: &'static str,
    pub identity_schema: &'static str,
    pub capability: &'static str,
    pub stem: &'static str,
}

pub fn browser_engine_profile(component: ModuleComponent) -> Option<BrowserEngineProfile> {
    match component {
        ModuleComponent::Network => Some(BrowserEngineProfile {
            protocol: "ds.network-wasm/v1",
            identity_schema: NETWORK_PROTOCOL,
            capability: "lv_processing_v1",
            stem: "ds_network_wasm",
        }),
        ModuleComponent::Reporter => Some(BrowserEngineProfile {
            protocol: "ds.reporter-wasm/v1",
            identity_schema: "ds.reporter-engine/v1",
            capability: "render_held_svg_pdf_v1",
            stem: "ds_reporter_wasm",
        }),
        ModuleComponent::Solar => None,
    }
}

/// Release facts an installed browser engine identity must repeat exactly.
pub struct BrowserEngineProducer<'a> {
    pub version: &'a str,
    pub source_sha: &'a str,
    pub manifest_sha256: &'a str,
}

/// Admit a browser engine unit's exported identity. An installation passes its
/// release facts; packaging passes none and receives the identity's own.
pub fn admit_browser_engine_build(
    component: ModuleComponent,
    identity: &Value,
    producer: Option<BrowserEngineProducer>,
) -> Result<AdmittedEngineBuild, String> {
    if serde_json::to_vec(identity).map_or(true, |bytes| bytes.len() > MAX_BUILD_INFO_BYTES) {
        return Err("module_engine_incompatible: Browser identity exceeds 16 KiB".into());
    }
    let profile = browser_engine_profile(component).ok_or_else(|| {
        "module_release_target_unsupported: Solar has no admitted browser publication contract"
            .to_string()
    })?;
    let core_version = identity["coreVersion"].as_str().unwrap_or_default();
    let source = identity["sourceSha"].as_str().unwrap_or_default();
    let closure = identity["dependencyClosureSha256"]
        .as_str()
        .unwrap_or_default();
    // The one capability the Kernel calls this unit for must be declared;
    // additional capabilities never refuse a unit.
    require_tokens(&identity["capabilities"], &[profile.capability])?;
    let crossed = producer.as_ref().is_some_and(|p| {
        identity["coreVersion"] != p.version
            || identity["sourceSha"] != p.source_sha
            || identity["dependencyClosureSha256"] != p.manifest_sha256
    });
    // The unit is admitted by the capability its consumer requires, never by
    // its version, an exact capability list, a schema revision or the protocol
    // integer (owner 22 Oct): a later unit that adds capabilities or bumps its
    // release still serves the operation the Kernel calls. Producer facts
    // (`crossed`) remain this artifact's own integrity check.
    if !same_family(&identity["schema"], profile.identity_schema)
        || !protocol_integer(&identity["protocolMajor"])
        || identity["environment"] != "wasm"
        || (component == ModuleComponent::Network && identity["runtime"]["environment"] != "wasm")
        || crossed
        || (component == ModuleComponent::Network
            && !matches!(
                identity["jsonObjectOrder"].as_str(),
                Some("sorted" | "insertion")
            ))
        || !semantic_version(core_version)
        || !hex(source, 40)
        || !hex(closure, 64)
    {
        return Err(
            "module_engine_incompatible: Browser engine identity differs from its producer receipt"
                .into(),
        );
    }
    Ok(AdmittedEngineBuild {
        version: core_version.into(),
        source_sha: source.into(),
        manifest_sha256: closure.into(),
        release: release(component, core_version, source),
    })
}

pub const UI_PROTOCOL: &str = "ds.ui-wasm/v1";

/// UI components are deliberately separate from the three native engines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "kebab-case")]
pub enum UiComponent {
    Interactions,
    CommandKernel,
}
impl UiComponent {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "interactions" => Some(Self::Interactions),
            "command-kernel" => Some(Self::CommandKernel),
            _ => None,
        }
    }
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
    pub fn stem(self) -> &'static str {
        match self {
            Self::Interactions => "ds_interactions_wasm",
            Self::CommandKernel => "ds_command_kernel",
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

/// Decode the closed identity before admitting compiler/source facts.
pub fn parse_ui_identity(value: &Value) -> Result<UiEngineIdentity, String> {
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
        return Err(
            "module_engine_incompatible: UI identity exceeds its closed bounded shape".into(),
        );
    }
    serde_json::from_value(value.clone())
        .map_err(|_| "module_engine_incompatible: Invalid UI identity schema".into())
}

/// Admit a UI unit's ABI, compiler, configuration and source facts. An
/// installation passes its release source; packaging passes none.
pub fn admit_ui_identity(
    component: UiComponent,
    identity: &UiEngineIdentity,
    source_sha: Option<&str>,
) -> Result<(), String> {
    if identity.schema != "ds.ui-engine-build/v1"
        || identity.component != component
        || identity.protocol != UI_PROTOCOL
        || !hex(&identity.source_sha, 40)
        || source_sha.is_some_and(|source| identity.source_sha != source)
        || !hex(&identity.cargo_lock_sha256, 64)
        || !hex(&identity.config_sha256, 64)
        || !valid_compiler_identity(&identity.rustc)
        || identity.target != "wasm32-unknown-unknown"
        || identity.profile != "release"
    {
        return Err("module_engine_incompatible: Loaded UI ABI, compiler, configuration or source differs from its producer receipt".into());
    }
    Ok(())
}

pub fn valid_compiler_identity(text: &str) -> bool {
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
    let mut facts = BTreeMap::new();
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

#[cfg(test)]
#[path = "module_engine_tests.rs"]
mod tests;
