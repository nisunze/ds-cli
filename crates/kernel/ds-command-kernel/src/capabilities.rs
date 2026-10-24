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
//!
//! A row the page runs in-page names its one typed owner, the Kernel export
//! the page calls ([`Capability::export`]). An export that serves several
//! rows admits each request through [`admit_export`], so it runs only what a
//! row declares for this platform. The Desktop's Core door admits through
//! [`core_door`]: an id this build's table lacks still reaches installed
//! Core when Core itself declares it, because Core and this Kernel are
//! released independently.

use serde::Serialize;

#[path = "capability_command.rs"]
pub mod command;

/// The page's one door: a capability id and a JSON request in, the CLI's
/// command envelope out.
#[path = "capability_invoke.rs"]
pub mod invoke;

pub const SCHEMA: &str = "ds.capability-route/v1";
pub const CODE_PENDING: &str = "capability_pending";
pub const CODE_DISCARDED: &str = "capability_discarded";
pub const CODE_UNKNOWN: &str = "capability_unknown";
/// A request names a capability whose row binds another export, or none.
pub const CODE_EXPORT_MISMATCH: &str = "capability_export_mismatch";
/// A capability this platform runs elsewhere than in the page.
pub const CODE_NOT_IN_PAGE: &str = "capability_not_in_page";

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
    /// The wasm-bindgen JS name the page calls to run this capability
    /// in-page: present exactly when some platform runs it on the Kernel.
    pub export: Option<&'static str>,
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

/// The executors a declared platform has.
pub fn executors_on(platform: &str) -> Option<&'static [Executor]> {
    let index = PLATFORMS.iter().position(|known| *known == platform)?;
    Some(EXECUTORS[index])
}

/// Whether the platform is a page beside installed Core: it runs both the
/// in-page Kernel and Core (the Desktop).
pub fn beside_core(platform: &str) -> bool {
    executors_on(platform).is_some_and(|executors| {
        executors.contains(&Executor::Core) && executors.contains(&Executor::Kernel)
    })
}

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
    /// The Kernel export the page calls to run it in-page here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_lanes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

/// How this build serves `id`.
pub fn route(id: &str) -> Result<RouteAnswer, String> {
    route_on(PLATFORM, id)
}

/// How a build for `platform` serves `id`.
pub fn route_on(platform: &str, id: &str) -> Result<RouteAnswer, String> {
    let capability = capability(id)?;
    let index = PLATFORMS
        .iter()
        .position(|known| *known == platform)
        .ok_or_else(|| format!("{CODE_UNKNOWN}: no platform `{platform}` is declared"))?;
    let route = ROUTES[usize::from(capability.routes[index])];
    let (executor, exchange) = match route {
        Route::Supported { executor, exchange } => (Some(executor), exchange),
        _ => (None, None),
    };
    let in_page = match executor {
        Some(Executor::Kernel | Executor::NetworkUnit) => true,
        Some(Executor::Core) => beside_core(platform),
        _ => false,
    };
    Ok(RouteAnswer {
        schema: SCHEMA,
        capability: capability.id,
        platform: PLATFORMS[index],
        status: route.status(),
        executor,
        exchange,
        export: capability.export.filter(|_| in_page),
        page_lanes: (executor == Some(Executor::Core))
            .then(|| capability.run.and_then(|run| run.page_lanes))
            .flatten(),
        refusal: route.refusal(),
    })
}

/// Why an in-page run of a capability is not admitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unadmitted {
    pub code: &'static str,
    pub message: String,
}

/// Admit one in-page run of `id` through the Kernel export `export`, which
/// runs the gateway exchange `exchange`, on this build's platform. An export
/// that serves several rows calls this for each request, so it runs only what
/// a row declares: the row must name this export as its owner and be
/// supported here, by this exchange on the Kernel, or on Core beside the page
/// (the Desktop page runs Core rows in-page through the same owner too).
pub fn admit_export(id: &str, export: &str, exchange: &str) -> Result<&'static Capability, Unadmitted> {
    admit_export_on(PLATFORM, id, export, exchange)
}

/// [`admit_export`] for a build serving `platform`.
pub fn admit_export_on(
    platform: &str,
    id: &str,
    export: &str,
    exchange: &str,
) -> Result<&'static Capability, Unadmitted> {
    let capability = capability(id).map_err(|text| Unadmitted {
        code: CODE_UNKNOWN,
        message: text
            .split_once(": ")
            .map_or(text.clone(), |(_, message)| message.to_owned()),
    })?;
    if capability.export != Some(export) {
        return Err(Unadmitted {
            code: CODE_EXPORT_MISMATCH,
            message: match capability.export {
                Some(owner) => format!("`{id}` runs in the page through `{owner}`, not `{export}`."),
                None => format!("`{id}` does not run in the page, so `{export}` cannot run it."),
            },
        });
    }
    let route = capability.route_on(platform).ok_or_else(|| Unadmitted {
        code: CODE_UNKNOWN,
        message: format!("No platform `{platform}` is declared."),
    })?;
    match route {
        Route::Supported {
            executor: Executor::Kernel,
            exchange: Some(owner),
        } if owner == exchange => Ok(capability),
        Route::Supported {
            executor: Executor::Core,
            ..
        } if beside_core(platform) => Ok(capability),
        Route::Supported { executor, exchange: owner } => Err(Unadmitted {
            code: CODE_NOT_IN_PAGE,
            message: match (executor, owner) {
                (Executor::Kernel, Some(owner)) => format!(
                    "`{id}` runs through the Kernel's `{owner}` exchange on {platform}, not `{exchange}`."
                ),
                _ => format!("`{id}` runs on {executor:?} on {platform}, not in the page."),
            },
        }),
        refused => {
            let refusal = refused.refusal().expect("a refused route has its refusal");
            Err(Unadmitted {
                code: refusal.code,
                message: refusal.message.to_owned(),
            })
        }
    }
}

/// How a Core door answers a command before Core is asked.
#[derive(Clone, Copy, Debug)]
pub enum CoreDoor {
    /// This build's manifest runs it on Core.
    Declared(&'static Capability),
    /// This build's manifest does not know it, and the page sits beside
    /// installed Core: Core's own discovery decides, and Core authorizes and
    /// validates the run. The door forwards only what Core declares.
    Installed,
}

/// The Core door's admission on this build: [`on_core`] for every id the
/// manifest knows; an id it lacks is left to installed Core's own discovery
/// where the page runs beside Core, and refused everywhere else.
pub fn core_door(id: &str) -> Result<CoreDoor, String> {
    core_door_on(PLATFORM, id)
}

/// [`core_door`] for a build serving `platform`.
pub fn core_door_on(platform: &str, id: &str) -> Result<CoreDoor, String> {
    let Ok(capability) = capability(id) else {
        return if beside_core(platform) {
            Ok(CoreDoor::Installed)
        } else {
            Err(format!("{CODE_UNKNOWN}: DS Core has no command `{id}`"))
        };
    };
    match capability.route_on(platform) {
        Some(Route::Supported {
            executor: Executor::Core,
            ..
        }) => Ok(CoreDoor::Declared(capability)),
        Some(Route::Supported { executor, .. }) => Err(format!(
            "capability_not_on_core: `{id}` runs on {executor:?} on {platform}, not on DS Core"
        )),
        Some(route) => {
            let refusal = route.refusal().expect("a refused route has its refusal");
            Err(format!("{}: {}", refusal.code, refusal.message))
        }
        None => Err(format!("{CODE_UNKNOWN}: no platform `{platform}` is declared")),
    }
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
pub(crate) mod exports {
    use super::{capability, route, Executor, Route, Run};
    use js_sys::{Function, Object, Promise, Reflect};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    #[wasm_bindgen(typescript_custom_section)]
    const CAPABILITY_HOST: &'static str = r#"
/** The page's doors for the capabilities the Kernel routes. The Kernel picks the door; the page only opens it. */
export interface CapabilityHost {
    /** One Core command through the host's generic Core door, answered by Core's own envelope. */
    core?(command: CoreCapabilityId, project: string, args: Record<string, unknown>): Promise<unknown>;
    /** The page's Network WASM unit. */
    unit?(capability: string, input: unknown): Promise<unknown>;
}

/** How `capabilityRun` rejects. */
export interface CapabilityRefusal extends Error, CommandRefusal {}

export interface CapabilityFailure {
    class: string;
    code: string;
    message: string;
    remedy: string;
    retryable: boolean;
}
"#;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(typescript_type = "CapabilityHost")]
        pub type CapabilityHost;
    }

    fn refusal(code: &str, message: &str) -> JsValue {
        failure(super::command::connection_failure(
            Some(code),
            message,
            None,
        ))
    }

    fn failure(failure: super::command::Failure) -> JsValue {
        let error = js_sys::Error::new(&failure.message);
        error.set_name("CapabilityRefusal");
        let _ = Reflect::set(&error, &"code".into(), &failure.code.into());
        let _ = Reflect::set(&error, &"remedy".into(), &failure.remedy.into());
        let _ = Reflect::set(&error, &"retryable".into(), &failure.retryable.into());
        error.into()
    }

    fn host_failure(error: JsValue) -> JsValue {
        if let Ok(refusal) = tsify::serde_wasm_bindgen::from_value::<
            ds_core_contracts::CommandRefusal,
        >(error.clone())
        {
            if refusal.validate().is_ok() {
                return crate::design_lv::browser::failure(refusal);
            }
        }
        let code = member(&error, "code").as_string();
        let message = member(&error, "message")
            .as_string()
            .or_else(|| error.as_string())
            .unwrap_or_else(|| "The engine connection ended before it answered.".into());
        let remedy = member(&error, "remedy").as_string();
        failure(super::command::connection_failure(
            code.as_deref(),
            &message,
            remedy.as_deref(),
        ))
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
        let answer = function
            .call3(host, &first, &second, &third)
            .map_err(host_failure)?;
        let answer = JsFuture::from(Promise::resolve(&answer));
        let deadline = gloo_timers::future::TimeoutFuture::new(610_000);
        match futures_util::future::select(std::pin::pin!(answer), std::pin::pin!(deadline)).await {
            futures_util::future::Either::Left((answer, _)) => answer.map_err(host_failure),
            futures_util::future::Either::Right(_) => Err(refusal(
                "engine_timed_out",
                "The engine did not answer within the command deadline.",
            )),
        }
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
            let remedy = member(&error, "remedy").as_string();
            let mut answer =
                super::command::connection_failure(Some(&code), &message, remedy.as_deref());
            if let Some(retryable) = member(&error, "retryable").as_bool() {
                answer.retryable = retryable;
            }
            return Err(failure(answer));
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

    /// One concrete application exchange. The native command and the browser
    /// class consume the same captured input and the same owner decoder.
    pub(crate) async fn run_captured_lv(
        host: &CapabilityHost,
        captured: &crate::design_lv::CapturedLvProcess,
    ) -> Result<crate::design_lv::LvProcessRun, JsValue> {
        let capability =
            super::on_core(crate::design_lv::COMMAND).map_err(|text| refused(&text))?;
        let batch: ds_network_contracts::lv::LvProcessBatchInput =
            serde_json::from_slice(captured.input()).expect("private captured LV input");
        let input = crate::gateway_exchange::browser::typed(&batch)?;
        let args = arguments(capability.run, input)?;
        let envelope = open(
            host,
            "core",
            [capability.id.into(), captured.project().into(), args],
        )
        .await?;
        let value = tsify::serde_wasm_bindgen::from_value(envelope).map_err(|_| {
            refusal(
                "network_result_invalid",
                "Core's LV response is not a compatible command envelope.",
            )
        })?;
        captured
            .decode_core_run(value)
            .map_err(crate::design_lv::browser::failure)
    }

    /// This build's answer for `capability`.
    #[wasm_bindgen(js_name = capabilityRoute, unchecked_return_type = "{ schema: string; capability: string; platform: string; status: 'supported' | 'pending' | 'discarded'; executor?: 'core' | 'kernel' | 'network_unit'; exchange?: string; export?: keyof CapabilityExports; page_lanes?: number; refusal?: { code: 'capability_pending' | 'capability_discarded'; reason: string; message: string } }")]
    pub fn capability_route(
        #[wasm_bindgen(unchecked_param_type = "CapabilityId")] capability: String,
    ) -> Result<JsValue, JsValue> {
        let answer = route(&capability).map_err(|text| refused(&text))?;
        crate::gateway_exchange::browser::typed(&answer)
    }

    /// Run `capability` with the caller's document on the executor this build
    /// names: Core through the host's Core door, the
    /// page's Network unit, or the typed refusal. A refusal rejects as a
    /// `CapabilityRefusal`; a Kernel exchange is run through its typed export.
    #[wasm_bindgen(js_name = capabilityRun, unchecked_return_type = "Promise<any>")]
    pub fn capability_run(
        host: CapabilityHost,
        #[wasm_bindgen(unchecked_param_type = "RunnableCapabilityId")] capability_id: String,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "unknown")] input: JsValue,
    ) -> Promise {
        let capability = match capability(&capability_id) {
            Ok(capability) => capability,
            Err(text) => return Promise::reject(&refused(&text)),
        };
        // wasm-bindgen polls an async export on a later microtask. Capture the
        // LV JsValue synchronously here, before returning its Promise.
        let captured = if capability.id == crate::design_lv::COMMAND {
            if let Err(text) = super::on_core(capability.id) {
                return Promise::reject(&refused(&text));
            }
            match crate::design_lv::browser::capture_wasm(
                project.clone(),
                tsify::Ts::new_unchecked(input.clone()),
            ) {
                Ok(captured) => Some(captured),
                Err(error) => return Promise::reject(&error),
            }
        } else {
            None
        };
        wasm_bindgen_futures::future_to_promise(async move {
            if let Some(captured) = captured {
                let output = run_captured_lv(&host, &captured).await?;
                return crate::gateway_exchange::browser::typed(&output);
            }
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
        })
    }

    /// Narrow a runtime-selected id before consulting this build's routes.
    #[wasm_bindgen(js_name = capabilityIsKnown, unchecked_return_type = "value is CapabilityId")]
    pub fn capability_is_known(value: String) -> bool {
        super::capability(&value).is_ok()
    }

    /// Narrow a route-checked UI binding to this target's runnable IDs.
    #[wasm_bindgen(js_name = capabilityIsRunnable, unchecked_return_type = "value is RunnableCapabilityId")]
    pub fn capability_is_runnable(value: String) -> bool {
        super::route(&value).is_ok_and(|route| {
            matches!(route.executor, Some(Executor::Core | Executor::NetworkUnit))
        })
    }

    /// The Kernel's decision for a Core door. This includes supported
    /// capabilities that use another executor in this build.
    #[wasm_bindgen(js_name = capabilityCoreRefusal, unchecked_return_type = "CapabilityFailure | null")]
    pub fn capability_core_refusal(value: String) -> Result<JsValue, JsValue> {
        match super::command::core_refusal(&value) {
            Some(refusal) => crate::gateway_exchange::browser::typed(&refusal),
            None => Ok(JsValue::NULL),
        }
    }

    /// The same input admission used by native forwarding, before a form can run.
    #[wasm_bindgen(js_name = capabilityCommandAdmission, unchecked_return_type = "CapabilityFailure | null")]
    pub fn capability_command_admission(
        id: String,
        descriptor: JsValue,
        args: JsValue,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let descriptor: serde_json::Value = tsify::serde_wasm_bindgen::from_value(descriptor)
            .map_err(|error| refusal("capability_input_invalid", &error.to_string()))?;
        let args: serde_json::Value = tsify::serde_wasm_bindgen::from_value(args)
            .map_err(|error| refusal("capability_input_invalid", &error.to_string()))?;
        match super::command::admit(&id, &descriptor, &args, &project) {
            Some(refusal) => crate::gateway_exchange::browser::typed(&refusal),
            None => Ok(JsValue::NULL),
        }
    }

    /// Admission of a fixed application request against the installed Core's
    /// descriptor: an input or choice that Core does not declare answers
    /// `core_update_required` before anything is sent.
    #[wasm_bindgen(js_name = capabilityCoreRequestAdmission, unchecked_return_type = "CapabilityFailure | null")]
    pub fn capability_core_request_admission(
        id: String,
        descriptor: JsValue,
        args: JsValue,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let descriptor: serde_json::Value = tsify::serde_wasm_bindgen::from_value(descriptor)
            .map_err(|error| refusal("capability_input_invalid", &error.to_string()))?;
        let args: serde_json::Value = tsify::serde_wasm_bindgen::from_value(args)
            .map_err(|error| refusal("capability_input_invalid", &error.to_string()))?;
        match super::command::admit_request(&id, &descriptor, &args, &project) {
            Some(refusal) => crate::gateway_exchange::browser::typed(&refusal),
            None => Ok(JsValue::NULL),
        }
    }

    /// Bind a native/JS transport rejection to the shared failure vocabulary.
    #[wasm_bindgen(js_name = capabilityFailure, unchecked_return_type = "CapabilityFailure")]
    pub fn capability_failure(
        #[wasm_bindgen(unchecked_param_type = "unknown")] error: JsValue,
    ) -> Result<JsValue, JsValue> {
        let code = member(&error, "code").as_string();
        let message = member(&error, "message")
            .as_string()
            .or_else(|| error.as_string())
            .unwrap_or_else(|| "The engine connection ended before it answered.".into());
        let remedy = member(&error, "remedy").as_string();
        crate::gateway_exchange::browser::typed(&super::command::connection_failure(
            code.as_deref(),
            &message,
            remedy.as_deref(),
        ))
    }
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "capabilities_admission_tests.rs"]
mod admission_tests;

#[cfg(test)]
#[path = "capability_dispatch_tests.rs"]
mod dispatch_tests;

#[cfg(test)]
#[path = "capabilities_owners_tests.rs"]
mod owners_tests;

#[cfg(test)]
#[path = "capabilities_operations_tests.rs"]
mod operations_tests;
