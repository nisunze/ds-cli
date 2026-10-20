//! Where each Network capability executes, decided when the Kernel is built.
//!
//! The Kernel is the entry point for every client (owner, 2026-10-20), so the
//! route manifest lives here and never in a client framework. build.rs
//! compiles `routing/network-routes.json` into [`CAPABILITIES`]; a malformed
//! or incomplete manifest fails the build. Each Kernel build serves one
//! platform, chosen by a cargo feature (`platform-core`, the native default,
//! `platform-desktop` or `platform-web`), so the executor of a capability is
//! a constant of the binary: no host passes, reads or chooses a platform.
//! Hosts execute the answer: Core and native shells through [`route`], the
//! page through the `networkRoute` and `networkRun` WASM exports, which
//! dispatch to the host's Core door, the page's Network unit, or a typed
//! refusal.

use serde::Serialize;

#[cfg(all(feature = "platform-desktop", feature = "platform-web"))]
compile_error!(
    "a Kernel build serves exactly one platform: platform-desktop or platform-web, not both"
);
#[cfg(all(
    feature = "platform-core",
    any(feature = "platform-desktop", feature = "platform-web")
))]
compile_error!("a Kernel build serves exactly one platform: platform-core excludes platform-desktop and platform-web");

pub const SCHEMA: &str = "ds.network-route/v1";
const MAX_INPUT_BYTES: usize = 1024;

/// The platforms a Kernel build can serve.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Desktop,
    Web,
    Core,
}

/// The platform this build serves.
#[cfg(feature = "platform-desktop")]
pub const PLATFORM: Platform = Platform::Desktop;
#[cfg(feature = "platform-web")]
pub const PLATFORM: Platform = Platform::Web;
#[cfg(not(any(feature = "platform-desktop", feature = "platform-web")))]
pub const PLATFORM: Platform = Platform::Core;

/// Who runs a routed capability.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Executor {
    /// Installed DS Core with its Network addon.
    Core,
    /// The governed Network WASM unit in the page.
    NetworkUnit,
    /// The typed refusal the route names.
    Refuse,
}

/// An engineering job runs on Core wherever Core is installed; an in-page
/// operation belongs to the page's Network unit where that unit is admitted.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    EngineeringJob,
    InPage,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Route {
    pub executor: Executor,
    /// How many jobs a page runs at once; absent, the page's own worker width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_lanes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

/// How Core serves a capability: one Core command through the host's generic
/// Core door, or one named host door until Core carries it as a command.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct CoreDoor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub door: Option<&'static str>,
    /// The command input the caller's document is passed as.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<&'static str>,
    /// The section of that input the document becomes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrap: Option<&'static str>,
    /// The member of Core's answer the caller receives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<&'static str>,
    /// Switches the command always runs with.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub switches: &'static [&'static str],
}

/// One capability, as the manifest compiled it: every platform's route.
#[derive(Debug)]
pub struct Capability {
    pub name: &'static str,
    pub kind: CapabilityKind,
    pub core: Option<CoreDoor>,
    desktop: Route,
    web: Route,
    core_route: Route,
}

impl Capability {
    pub const fn route(&self, platform: Platform) -> Route {
        match platform {
            Platform::Desktop => self.desktop,
            Platform::Web => self.web,
            Platform::Core => self.core_route,
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/network_routes_table.rs"));

/// The Kernel's answer: the executor that serves `capability` in this build.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RouteAnswer {
    pub schema: &'static str,
    pub capability: &'static str,
    pub platform: Platform,
    pub kind: CapabilityKind,
    #[serde(flatten)]
    pub route: Route,
}

/// The compiled capability, or the typed refusal for an unknown name.
pub fn capability(name: &str) -> Result<&'static Capability, String> {
    CAPABILITIES
        .iter()
        .find(|capability| capability.name == name)
        .ok_or_else(|| {
            format!("network_route_unknown: the Kernel has no Network route for `{name}`")
        })
}

/// Which executor serves `capability` in this build.
pub fn route(capability_name: &str) -> Result<RouteAnswer, String> {
    let capability = capability(capability_name)?;
    Ok(RouteAnswer {
        schema: SCHEMA,
        capability: capability.name,
        platform: PLATFORM,
        kind: capability.kind,
        route: capability.route(PLATFORM),
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    capability: String,
}

/// The JSON door: `{schema, capability}` in, the [`RouteAnswer`] out.
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
    let answer = route(&request.capability)?;
    serde_json::to_string(&answer).map_err(|error| error.to_string())
}

/// The page's exports: the compiled route, and one run of a capability on the
/// executor that route names.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{capability, route, CoreDoor, Executor};
    use js_sys::{Function, Object, Promise, Reflect};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    #[wasm_bindgen(typescript_custom_section)]
    const NETWORK_HOST: &'static str = r#"
/** The page's doors for the Network capabilities the Kernel routes. The Kernel picks the door; the page only opens it. */
export interface NetworkHost {
    /** One Core command through the host's generic Core door, answered by Core's own envelope. */
    core?(command: string, project: string, args: Record<string, unknown>): Promise<unknown>;
    /** One named host door, for a capability Core does not yet carry as a command. */
    door?(door: string, project: string, input: unknown): Promise<unknown>;
    /** The page's Network WASM unit. */
    unit?(capability: string, input: unknown): Promise<unknown>;
}

/** How `networkRun` rejects. */
export interface NetworkRefusal extends Error {
    code: string;
}
"#;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(typescript_type = "NetworkHost")]
        pub type NetworkHost;
    }

    fn refusal(code: &str, message: &str) -> JsValue {
        let error = js_sys::Error::new(message);
        error.set_name("NetworkRefusal");
        let _ = Reflect::set(&error, &"code".into(), &code.into());
        error.into()
    }

    /// The `code: message` refusal a route or a host answered.
    fn refused(text: &str) -> JsValue {
        match text.split_once(": ") {
            Some((code, message)) => refusal(code, message),
            None => refusal("network_route_refused", text),
        }
    }

    /// Call one host door by name; an absent door is the typed refusal.
    async fn open(host: &NetworkHost, door: &str, args: [JsValue; 3]) -> Result<JsValue, JsValue> {
        let function = Reflect::get(host, &door.into())?
            .dyn_into::<Function>()
            .map_err(|_| {
                refusal(
                    "network_door_unavailable",
                    &format!("This page has no `{door}` door for the executor the Kernel names."),
                )
            })?;
        let [first, second, third] = args;
        let answer = function.call3(host, &first, &second, &third)?;
        JsFuture::from(Promise::resolve(&answer)).await
    }

    fn member(value: &JsValue, key: &str) -> JsValue {
        Reflect::get(value, &key.into()).unwrap_or(JsValue::UNDEFINED)
    }

    /// Core's envelope: its data (or the declared member of it), or its own
    /// typed refusal.
    fn admit(envelope: &JsValue, spec: &CoreDoor) -> Result<JsValue, JsValue> {
        if member(envelope, "status").as_string().as_deref() != Some("ok") {
            let error = member(envelope, "error");
            let code = member(&error, "code")
                .as_string()
                .unwrap_or_else(|| "core_refused".into());
            let message = member(&error, "message")
                .as_string()
                .unwrap_or_else(|| "DS Core refused the command.".into());
            return Err(refusal(&code, &message));
        }
        let data = member(envelope, "data");
        let answer = match spec.answer {
            Some(key) => member(&data, key),
            None => data,
        };
        if answer.is_undefined() || answer.is_null() {
            return Err(refusal(
                "core_incompatible",
                "DS Core answered without the result this capability reads.",
            ));
        }
        Ok(answer)
    }

    /// The executor this build names for `capability`.
    #[wasm_bindgen(js_name = networkRoute, unchecked_return_type = "{ schema: string; capability: string; platform: 'desktop' | 'web' | 'core'; kind: 'engineering_job' | 'in_page'; executor: 'core' | 'network_unit' | 'refuse'; page_lanes?: number; refusal?: { code: string; message: string } }")]
    pub fn network_route(capability: String) -> Result<JsValue, JsValue> {
        let answer = route(&capability).map_err(|text| refused(&text))?;
        crate::gateway_exchange::browser::typed(&answer)
    }

    /// Run `capability` with the caller's document on the executor this build
    /// names: Core through the host's Core door, the page's Network unit, or
    /// the route's typed refusal. A refusal rejects as a `NetworkRefusal`.
    #[wasm_bindgen(js_name = networkRun, unchecked_return_type = "any")]
    pub async fn network_run(
        host: NetworkHost,
        capability_name: String,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "unknown")] input: JsValue,
    ) -> Result<JsValue, JsValue> {
        let capability = capability(&capability_name).map_err(|text| refused(&text))?;
        let route = capability.route(super::PLATFORM);
        match route.executor {
            Executor::Refuse => {
                let refusal_text = route
                    .refusal
                    .expect("build.rs requires a refusal to name its code");
                Err(refusal(refusal_text.code, refusal_text.message))
            }
            Executor::NetworkUnit => {
                open(
                    &host,
                    "unit",
                    [capability.name.into(), input, JsValue::UNDEFINED],
                )
                .await
            }
            Executor::Core => {
                let spec = capability
                    .core
                    .expect("build.rs requires a Core route to name its Core door");
                if let Some(door) = spec.door {
                    return open(&host, "door", [door.into(), project.into(), input]).await;
                }
                let args = Object::new();
                if let Some(name) = spec.input {
                    let document = match spec.wrap {
                        Some(section) => {
                            let wrapped = Object::new();
                            Reflect::set(&wrapped, &section.into(), &input)?;
                            wrapped.into()
                        }
                        None => input,
                    };
                    Reflect::set(&args, &name.into(), &document)?;
                }
                for switch in spec.switches {
                    Reflect::set(&args, &(*switch).into(), &JsValue::TRUE)?;
                }
                let command = spec.command.expect("build.rs requires a command or a door");
                let envelope =
                    open(&host, "core", [command.into(), project.into(), args.into()]).await?;
                admit(&envelope, &spec)
            }
        }
    }
}

#[cfg(test)]
#[path = "network_routes_tests.rs"]
mod tests;
