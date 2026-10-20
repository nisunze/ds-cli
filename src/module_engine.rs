//! Native module components and their engine build admission.
//!
//! One decision serves the installed updater (`module_install`) and release
//! packaging. `crates/ds-module-admission` compiles this same file through
//! `#[path]`, so packaging admits a freshly built engine with exactly the
//! rules an installation applies, without compiling the whole Kernel or
//! adding a dependency edge to it. Keep this file free of `crate::` paths and
//! of dependencies beyond serde and serde_json.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MODULE_PROTOCOL: &str = "ds.module/v1";
pub const NETWORK_PROTOCOL: &str = "ds.network-engine/v1";
pub const ENGINE_BUILD_SCHEMA: &str = "ds.engine-build/v1";
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
        if v["schema"] != ENGINE_BUILD_SCHEMA
            || v["module"] != "network"
            || v["engineIdentity"]["schema"] != component.protocol()
            || v["engineIdentity"]["protocolMajor"] != 1
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
        if v["schema"] != ENGINE_BUILD_SCHEMA
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

#[cfg(test)]
#[path = "module_engine_tests.rs"]
mod tests;
