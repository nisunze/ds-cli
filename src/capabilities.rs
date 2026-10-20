//! Where every Core capability runs on every platform, decided when the
//! Kernel is built.
//!
//! The Kernel is the entry point for every client (owner, 2026-10-20), so the
//! one capability manifest lives here and never in a client framework.
//! build.rs compiles `routing/capabilities.json` into [`CAPABILITIES`]: one
//! row per Core command id, with each declared platform's answer, which is
//! the executor that serves it (`core`, `kernel`, `network_unit`) or a typed
//! refusal (`pending` or `discarded`, with its reason). The platforms are the
//! manifest's data. A Kernel build serves the one platform whose cargo
//! feature it enables (none: the manifest's default), so the answer is a
//! constant of the binary: no host passes, reads or chooses a platform.
//!
//! Hosts execute the answer: Core and the Desktop shell through [`route`]
//! and [`on_core`], the page through the `capabilityRoute` and
//! `capabilityRun` WASM exports. ds-cli's coverage test holds the table to
//! Core's command registry, so no Core command ships without a decision for
//! every platform.

use serde::Serialize;

pub const SCHEMA: &str = "ds.capability-route/v1";
pub const CODE_PENDING: &str = "capability_pending";
pub const CODE_DISCARDED: &str = "capability_discarded";
pub const CODE_UNKNOWN: &str = "capability_unknown";

/// Who runs a supported capability.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Executor {
    /// Installed DS Core, through the host's Core door.
    Core,
    /// The in-page Kernel's typed gateway exchange.
    Kernel,
    /// The governed Network WASM unit in the page.
    NetworkUnit,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Supported,
    /// Planned here; refused until it lands.
    Pending,
    /// Not offered here.
    Discarded,
}

/// Why a platform does not serve a capability: a stable key and the message
/// a person reads.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Reason {
    pub key: &'static str,
    pub message: &'static str,
}

/// One platform's answer for one capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Supported {
        executor: Executor,
        /// The `src/gateway_exchange` module a `kernel` executor runs.
        exchange: Option<&'static str>,
    },
    Pending(Reason),
    Discarded(Reason),
}

impl Route {
    pub const fn status(&self) -> Status {
        match self {
            Self::Supported { .. } => Status::Supported,
            Self::Pending(_) => Status::Pending,
            Self::Discarded(_) => Status::Discarded,
        }
    }

    /// The typed refusal of a pending or discarded route.
    pub fn refusal(&self) -> Option<Refusal> {
        match *self {
            Self::Supported { .. } => None,
            Self::Pending(reason) => Some(Refusal::new(CODE_PENDING, reason)),
            Self::Discarded(reason) => Some(Refusal::new(CODE_DISCARDED, reason)),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub reason: &'static str,
    pub message: &'static str,
}

impl Refusal {
    const fn new(code: &'static str, reason: Reason) -> Self {
        Self {
            code,
            reason: reason.key,
            message: reason.message,
        }
    }
}

/// How the page calls a capability that runs on Core, when it is not the
/// command with the caller's document as its arguments.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Run {
    /// The command input the caller's document is passed as.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<&'static str>,
    /// The section of that input the document becomes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrap: Option<&'static str>,
    /// The member of Core's answer the caller receives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<&'static str>,
    /// The file Core writes that the caller receives, from the envelope's
    /// `files`; the command is asked to write it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<&'static str>,
    /// Switches the command always runs with.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub switches: &'static [&'static str],
    /// How many jobs a page sends to Core at once; absent, its worker width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_lanes: Option<u32>,
}

/// One Core command and every platform's answer for it.
#[derive(Debug)]
pub struct Capability {
    pub id: &'static str,
    routes: [u8; PLATFORMS.len()],
    pub run: Option<Run>,
}

impl Capability {
    /// This build's answer.
    pub fn route(&self) -> Route {
        ROUTES[usize::from(self.routes[PLATFORM_INDEX])]
    }

    /// The answer on a declared platform.
    pub fn route_on(&self, platform: &str) -> Option<Route> {
        let index = PLATFORMS.iter().position(|known| *known == platform)?;
        Some(ROUTES[usize::from(self.routes[index])])
    }
}

include!(concat!(env!("OUT_DIR"), "/capabilities_table.rs"));

/// The compiled capability, or the typed refusal for an id Core does not have.
pub fn capability(id: &str) -> Result<&'static Capability, String> {
    CAPABILITIES
        .binary_search_by(|capability| capability.id.cmp(id))
        .map(|index| &CAPABILITIES[index])
        .map_err(|_| format!("{CODE_UNKNOWN}: DS Core has no command `{id}`"))
}

/// The Kernel's answer: how this build serves `capability`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RouteAnswer {
    pub schema: &'static str,
    pub capability: &'static str,
    pub platform: &'static str,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor: Option<Executor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exchange: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_lanes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

/// How this build serves `id`.
pub fn route(id: &str) -> Result<RouteAnswer, String> {
    let capability = capability(id)?;
    let route = capability.route();
    let (executor, exchange) = match route {
        Route::Supported { executor, exchange } => (Some(executor), exchange),
        _ => (None, None),
    };
    Ok(RouteAnswer {
        schema: SCHEMA,
        capability: capability.id,
        platform: PLATFORM,
        status: route.status(),
        executor,
        exchange,
        page_lanes: (executor == Some(Executor::Core))
            .then(|| capability.run.and_then(|run| run.page_lanes))
            .flatten(),
        refusal: route.refusal(),
    })
}

/// The capability, when this build runs it on installed Core; otherwise its
/// typed `code: message` refusal. A host door admits a Core command by this.
pub fn on_core(id: &str) -> Result<&'static Capability, String> {
    let capability = capability(id)?;
    match capability.route() {
        Route::Supported {
            executor: Executor::Core,
            ..
        } => Ok(capability),
        Route::Supported { executor, .. } => Err(format!(
            "capability_not_on_core: `{id}` runs on {executor:?} on {PLATFORM}, not on DS Core"
        )),
        route => {
            let refusal = route.refusal().expect("a refused route has its refusal");
            Err(format!("{}: {}", refusal.code, refusal.message))
        }
    }
}

/// The page's exports: this build's answer for a capability, and one run of
/// it on the executor that answer names.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{capability, route, Executor, Route, Run};
    use js_sys::{Function, Object, Promise, Reflect};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    #[wasm_bindgen(typescript_custom_section)]
    const CAPABILITY_HOST: &'static str = r#"
/** The page's doors for the capabilities the Kernel routes. The Kernel picks the door; the page only opens it. */
export interface CapabilityHost {
    /** One Core command through the host's generic Core door, answered by Core's own envelope. */
    core?(command: string, project: string, args: Record<string, unknown>): Promise<unknown>;
    /** The page's Network WASM unit. */
    unit?(capability: string, input: unknown): Promise<unknown>;
}

/** How `capabilityRun` rejects. */
export interface CapabilityRefusal extends Error {
    code: string;
}
"#;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(typescript_type = "CapabilityHost")]
        pub type CapabilityHost;
    }

    fn refusal(code: &str, message: &str) -> JsValue {
        let error = js_sys::Error::new(message);
        error.set_name("CapabilityRefusal");
        let _ = Reflect::set(&error, &"code".into(), &code.into());
        error.into()
    }

    /// The `code: message` refusal the Kernel answered.
    fn refused(text: &str) -> JsValue {
        match text.split_once(": ") {
            Some((code, message)) => refusal(code, message),
            None => refusal("capability_refused", text),
        }
    }

    /// Call one host door by name; an absent door is the typed refusal.
    async fn open(
        host: &CapabilityHost,
        door: &str,
        args: [JsValue; 3],
    ) -> Result<JsValue, JsValue> {
        let function = Reflect::get(host, &door.into())?
            .dyn_into::<Function>()
            .map_err(|_| {
                refusal(
                    "capability_door_unavailable",
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
    fn admit(envelope: &JsValue, run: Option<Run>) -> Result<JsValue, JsValue> {
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
        if let Some(output) = run.and_then(|run| run.output) {
            let file = member(&member(envelope, "files"), output);
            if file.is_undefined() || file.is_null() {
                return Err(refusal(
                    "core_incompatible",
                    "DS Core answered without the file this capability reads.",
                ));
            }
            return Ok(file);
        }
        let data = member(envelope, "data");
        let Some(key) = run.and_then(|run| run.answer) else {
            return Ok(data);
        };
        let answer = member(&data, key);
        if answer.is_undefined() || answer.is_null() {
            return Err(refusal(
                "core_incompatible",
                "DS Core answered without the result this capability reads.",
            ));
        }
        Ok(answer)
    }

    /// Core's arguments: the caller's document as the declared input (or as
    /// the arguments themselves), plus the declared switches.
    fn arguments(run: Option<Run>, input: JsValue) -> Result<JsValue, JsValue> {
        let args = match run.and_then(|run| run.input) {
            Some(name) => {
                let document = match run.and_then(|run| run.wrap) {
                    Some(section) => {
                        let wrapped = Object::new();
                        Reflect::set(&wrapped, &section.into(), &input)?;
                        wrapped.into()
                    }
                    None => input,
                };
                let args = Object::new();
                Reflect::set(&args, &name.into(), &document)?;
                args
            }
            None if input.is_undefined() || input.is_null() => Object::new(),
            None if input.is_object() && !js_sys::Array::is_array(&input) => {
                Object::assign(&Object::new(), input.unchecked_ref())
            }
            None => {
                return Err(refusal(
                    "capability_input_invalid",
                    "A Core command takes its arguments as one object.",
                ))
            }
        };
        for switch in run.map_or(&[][..], |run| run.switches) {
            Reflect::set(&args, &(*switch).into(), &JsValue::TRUE)?;
        }
        if let Some(output) = run.and_then(|run| run.output) {
            Reflect::set(&args, &output.into(), &JsValue::TRUE)?;
        }
        Ok(args.into())
    }

    /// This build's answer for `capability`.
    #[wasm_bindgen(js_name = capabilityRoute, unchecked_return_type = "{ schema: string; capability: string; platform: string; status: 'supported' | 'pending' | 'discarded'; executor?: 'core' | 'kernel' | 'network_unit'; exchange?: string; page_lanes?: number; refusal?: { code: 'capability_pending' | 'capability_discarded'; reason: string; message: string } }")]
    pub fn capability_route(capability: String) -> Result<JsValue, JsValue> {
        let answer = route(&capability).map_err(|text| refused(&text))?;
        crate::gateway_exchange::browser::typed(&answer)
    }

    /// Run `capability` with the caller's document on the executor this build
    /// names: Core through the host's Core door, the
    /// page's Network unit, or the typed refusal. A refusal rejects as a
    /// `CapabilityRefusal`; a Kernel exchange is run through its typed export.
    #[wasm_bindgen(js_name = capabilityRun, unchecked_return_type = "any")]
    pub async fn capability_run(
        host: CapabilityHost,
        capability_id: String,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "unknown")] input: JsValue,
    ) -> Result<JsValue, JsValue> {
        let capability = capability(&capability_id).map_err(|text| refused(&text))?;
        match capability.route() {
            Route::Supported {
                executor: Executor::Core,
                ..
            } => {
                let args = arguments(capability.run, input)?;
                let envelope =
                    open(&host, "core", [capability.id.into(), project.into(), args]).await?;
                admit(&envelope, capability.run)
            }
            Route::Supported {
                executor: Executor::NetworkUnit,
                ..
            } => {
                open(
                    &host,
                    "unit",
                    [capability.id.into(), input, JsValue::UNDEFINED],
                )
                .await
            }
            Route::Supported {
                executor: Executor::Kernel,
                exchange,
            } => Err(refusal(
                "capability_typed_export",
                &format!(
                    "`{}` runs through the Kernel's typed `{}` exchange here, not capabilityRun.",
                    capability.id,
                    exchange.unwrap_or_default()
                ),
            )),
            route => {
                let answer = route.refusal().expect("a refused route has its refusal");
                Err(refusal(answer.code, answer.message))
            }
        }
    }
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
