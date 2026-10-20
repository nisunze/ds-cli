//! Where each Network capability executes on each platform.
//!
//! The Kernel is the entry point for every client (owner, 2026-10-20), so the
//! route manifest lives here and never in a client framework:
//! `routing/network-routes.json` names, for each capability and platform, the
//! executor that serves it and any page-lane bound. Platforms and where each
//! executor exists are data, so a new platform adds its manifest column and
//! its adapter, never Kernel code. Hosts execute the answer: the Desktop's
//! Network adapter and Core natively through [`route`], the page through the
//! `networkRoute` WASM export.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "ds.network-route/v1";
const MANIFEST_SCHEMA: &str = "ds.network-routes/v1";
const MANIFEST: &str = include_str!("../routing/network-routes.json");
const MAX_INPUT_BYTES: usize = 1024;
const MAX_PAGE_LANES: u32 = 64;

/// Who runs a routed capability. Each platform's adapter implements the
/// executors the manifest says exist there.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Executor {
    /// Installed DS Core with its Network addon.
    Core,
    /// The governed Network WASM unit in the page.
    NetworkUnit,
    /// The typed refusal the route names.
    Refuse,
}

const EXECUTORS: [Executor; 3] = [Executor::Core, Executor::NetworkUnit, Executor::Refuse];

impl Executor {
    fn name(self) -> &'static str {
        match self {
            Executor::Core => "core",
            Executor::NetworkUnit => "network_unit",
            Executor::Refuse => "refuse",
        }
    }
}

/// An engineering job runs on Core wherever Core is installed; an in-page
/// operation belongs to the page's Network unit where that unit is admitted.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    EngineeringJob,
    InPage,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RouteRefusal {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub executor: Executor,
    /// How many jobs a page runs at once; absent, the page's own worker width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_lanes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<RouteRefusal>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutorSpec {
    platforms: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capability {
    kind: CapabilityKind,
    #[serde(rename = "doc")]
    _doc: String,
    routes: BTreeMap<String, Route>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    #[serde(rename = "doc")]
    _doc: Vec<String>,
    platforms: Vec<String>,
    executors: BTreeMap<Executor, ExecutorSpec>,
    capabilities: BTreeMap<String, Capability>,
}

/// The Kernel's answer: the executor that serves `capability` on `platform`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RouteAnswer {
    pub schema: &'static str,
    pub capability: String,
    pub platform: String,
    pub kind: CapabilityKind,
    #[serde(flatten)]
    pub route: Route,
}

/// Which executor serves `capability` on `platform`, from the one manifest.
pub fn route(capability: &str, platform: &str) -> Result<RouteAnswer, String> {
    let manifest = manifest()?;
    if !manifest.platforms.iter().any(|known| known == platform) {
        return Err(format!(
            "network_route_platform_unknown: `{platform}` is not a platform the Kernel routes"
        ));
    }
    let entry = manifest.capabilities.get(capability).ok_or_else(|| {
        format!("network_route_unknown: the Kernel has no Network route for `{capability}`")
    })?;
    let route =
        entry.routes.get(platform).cloned().ok_or_else(|| {
            format!("network_routes_invalid: `{capability}` has no {platform} route")
        })?;
    Ok(RouteAnswer {
        schema: SCHEMA,
        capability: capability.to_owned(),
        platform: platform.to_owned(),
        kind: entry.kind,
        route,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    capability: String,
    platform: String,
}

/// The JSON door the page's WASM export serves: `{schema, capability,
/// platform}` in, the [`RouteAnswer`] out.
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("network_route_request_invalid: the request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("network_route_request_invalid: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!(
            "network_route_request_invalid: expected schema {SCHEMA}"
        ));
    }
    let answer = route(&request.capability, &request.platform)?;
    serde_json::to_string(&answer).map_err(|error| error.to_string())
}

fn manifest() -> Result<&'static Manifest, String> {
    static LOADED: OnceLock<Result<Manifest, String>> = OnceLock::new();
    LOADED
        .get_or_init(|| parse(MANIFEST))
        .as_ref()
        .map_err(Clone::clone)
}

fn parse(json: &str) -> Result<Manifest, String> {
    let manifest: Manifest =
        serde_json::from_str(json).map_err(|error| format!("network_routes_invalid: {error}"))?;
    validate(&manifest).map_err(|message| format!("network_routes_invalid: {message}"))?;
    Ok(manifest)
}

/// The gates every manifest passes before any host may read a route.
fn validate(manifest: &Manifest) -> Result<(), String> {
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(format!("expected schema {MANIFEST_SCHEMA}"));
    }
    let platforms: BTreeSet<&str> = manifest.platforms.iter().map(String::as_str).collect();
    if platforms.is_empty()
        || platforms.len() != manifest.platforms.len()
        || !platforms.iter().all(|platform| is_name(platform))
    {
        return Err("platforms must be distinct lowercase names".into());
    }
    for executor in EXECUTORS {
        let spec = manifest
            .executors
            .get(&executor)
            .ok_or_else(|| format!("executor `{}` is not declared", executor.name()))?;
        if let Some(unknown) = spec
            .platforms
            .iter()
            .find(|platform| !platforms.contains(platform.as_str()))
        {
            return Err(format!(
                "executor `{}` names the undeclared platform `{unknown}`",
                executor.name()
            ));
        }
    }
    let exists = |executor: Executor, platform: &str| {
        manifest.executors[&executor]
            .platforms
            .iter()
            .any(|known| known == platform)
    };
    if manifest.capabilities.is_empty() {
        return Err("no capability is routed".into());
    }
    for (name, capability) in &manifest.capabilities {
        if !name.split('.').all(is_name) || !name.contains('.') {
            return Err(format!(
                "capability `{name}` is not a dotted lowercase name"
            ));
        }
        let routed: BTreeSet<&str> = capability.routes.keys().map(String::as_str).collect();
        if routed != platforms {
            return Err(format!(
                "`{name}` must route every declared platform, and only those"
            ));
        }
        for (platform, route) in &capability.routes {
            let executor = route.executor;
            if !exists(executor, platform) {
                return Err(format!(
                    "`{name}` on {platform} names `{}`, which does not exist there",
                    executor.name()
                ));
            }
            match (executor, &route.refusal) {
                (Executor::Refuse, Some(refusal))
                    if is_name(&refusal.code) && !refusal.message.trim().is_empty() => {}
                (Executor::Refuse, _) => {
                    return Err(format!(
                        "`{name}` on {platform} refuses without a typed code and message"
                    ))
                }
                (_, Some(_)) => {
                    return Err(format!(
                        "`{name}` on {platform} carries a refusal but does not refuse"
                    ))
                }
                (_, None) => {}
            }
            if route
                .page_lanes
                .is_some_and(|lanes| !(1..=MAX_PAGE_LANES).contains(&lanes))
            {
                return Err(format!(
                    "`{name}` on {platform} bounds its page lanes outside 1..={MAX_PAGE_LANES}"
                ));
            }
            if capability.kind == CapabilityKind::EngineeringJob
                && executor == Executor::NetworkUnit
                && exists(Executor::Core, platform)
            {
                return Err(format!(
                    "engineering job `{name}` routes to the Network unit on {platform}, where Core is installed"
                ));
            }
        }
    }
    Ok(())
}

fn is_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
#[path = "network_routes_tests.rs"]
mod tests;
