//! The page's one door: a capability id and a JSON request in, the DS
//! command envelope out.
//!
//! A standalone client calls the Kernel almost as it would the API gateway
//! (owner, 2026-10-24): `invoke(capability_id, request_json, host)` answers
//! the same envelope `ds <cmd> --output json` prints, serialized by the CLI's
//! own serializer ([`ds_core_contracts::envelope`]): `{v, command, contract,
//! status: "ok", data}` or `{v, command, contract, status: "error",
//! error: {class, code, message, retryable, remedy?, next?, detail?}}`.
//!
//! Admission is the capability manifest's row for this build's platform
//! ([`super::route_on`]); nothing else decides where a capability runs:
//!
//! - `kernel:<module>`: the row's typed owner, the export the row names
//!   ([`super::admit_export_on`]). The request is that owner's input: its one
//!   object ([`Kind::Whole`]) or its named inputs, admitted strictly here
//!   with the CLI's own refusals (`unknown_flag`, `missing_input`).
//! - `network_unit`: the page's governed Network unit, through the host's
//!   `unit` door, with the request as the unit's input.
//! - `core`: installed DS Core through the host's `core` door, exactly as
//!   `capabilityRun` opens it: the request is the command's arguments by
//!   input name, `project` is the door's project, and the row's `run`
//!   switches and output are added. Core's own envelope is the answer,
//!   verbatim, so its additive members reach the caller.
//! - `pending:` / `discarded:`: the row's typed refusal.
//!
//! Requests are strict; replies may grow. Every decision here is native and
//! tested without a browser ([`invoke_with`] over any [`Doors`]); the WASM
//! export only opens the doors the page passes.

#![allow(clippy::result_large_err)] // The CLI refusal body is the envelope's, unboxed.

use std::collections::BTreeSet;

use ds_core_contracts::envelope::{
    error_envelope, success_envelope, Class, CommandEnvelope, CommandRefusal,
};
use serde::Serialize;
use serde_json::{json, Map, Value};

use super::{admit_export_on, capability, Capability, Executor, Route, CODE_UNKNOWN};

/// The contract of the invoke envelope's in-page owners. Core's own envelope
/// carries Core's command contract.
pub const CONTRACT: u32 = 1;
/// The most request bytes one call admits.
pub const REQUEST_MAX_BYTES: usize = 16 * 1024 * 1024;
/// A request that is not one JSON value of the shape the row takes.
pub const CODE_REQUEST_INVALID: &str = "invalid_request";
/// A row whose typed owner this door does not carry yet.
pub const CODE_UNBOUND: &str = "capability_invoke_unbound";
/// A request that performs another capability than the one invoked.
pub const CODE_REQUEST_MISMATCH: &str = "capability_request_mismatch";
/// The page did not pass the door the row's executor needs.
pub const CODE_DOOR_UNAVAILABLE: &str = "capability_door_unavailable";
/// An owner failed without a typed code of its own.
pub const CODE_FAILED: &str = "capability_failed";
/// Core answered something that is not a command envelope.
pub const CODE_CORE_INCOMPATIBLE: &str = "core_incompatible";

/// What a typed owner reads from the host besides the request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Door {
    /// The page's `GatewayHost`: its clock, session facts and refusal sink.
    Gateway,
    /// The page's `StoreHost`: the Kernel's durable store transactions.
    Store,
}

/// How one request member is admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Text,
    Number,
    /// A whole number from 0 through 4294967295.
    Count,
    Flag,
    TextList,
    /// Any JSON value the owner decodes itself.
    Any,
    /// The whole request object is the owner's one input; the owner admits
    /// its members.
    Whole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Member {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
}

/// A request member's Rust type, which fixes its [`Kind`] and whether it is
/// required, and decodes the admitted value.
pub trait Arg: Sized {
    const KIND: Kind;
    const REQUIRED: bool = true;
    fn decode(value: Value) -> Result<Self, String>;
}

macro_rules! serde_arg {
    ($($ty:ty => $kind:ident),* $(,)?) => {$(
        impl Arg for $ty {
            const KIND: Kind = Kind::$kind;
            fn decode(value: Value) -> Result<Self, String> {
                serde_json::from_value(value).map_err(|error| error.to_string())
            }
        }
    )*};
}
serde_arg!(String => Text, f64 => Number, u32 => Count, bool => Flag, Vec<String> => TextList);

/// A member the owner decodes itself.
#[derive(Clone, Debug, PartialEq)]
pub struct Js(pub Value);

impl Arg for Js {
    const KIND: Kind = Kind::Any;
    fn decode(value: Value) -> Result<Self, String> {
        Ok(Self(value))
    }
}

/// The whole request object.
#[derive(Clone, Debug, PartialEq)]
pub struct Whole(pub Map<String, Value>);

impl Arg for Whole {
    const KIND: Kind = Kind::Whole;
    fn decode(value: Value) -> Result<Self, String> {
        match value {
            Value::Object(map) => Ok(Self(map)),
            _ => Err("the request is one object".into()),
        }
    }
}

impl<T: Arg> Arg for Option<T> {
    const KIND: Kind = T::KIND;
    const REQUIRED: bool = false;
    fn decode(value: Value) -> Result<Self, String> {
        match value {
            Value::Null => Ok(None),
            value => T::decode(value).map(Some),
        }
    }
}

/// Take one admitted member (or, for [`Whole`], the whole request).
pub fn take<T: Arg>(request: &mut Map<String, Value>, name: &str) -> Result<T, String> {
    let value = if T::KIND == Kind::Whole {
        Value::Object(std::mem::take(request))
    } else {
        request.remove(name).unwrap_or(Value::Null)
    };
    T::decode(value).map_err(|error| format!("`{name}`: {error}"))
}

/// An owner the page runs in-page through its typed WASM export.
#[derive(Debug)]
pub struct PageOwner {
    pub export: &'static str,
    pub doors: &'static [Door],
    pub members: &'static [Member],
}

/// An owner that runs here without a host door: the function the export
/// itself calls, so the native build answers exactly what the page does.
pub struct NativeOwner {
    pub export: &'static str,
    pub members: &'static [Member],
    pub run: fn(Map<String, Value>) -> Result<Value, CommandRefusal>,
}

impl std::fmt::Debug for NativeOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeOwner")
            .field("export", &self.export)
            .field("members", &self.members)
            .finish()
    }
}

/// A typed export this door does not carry, and why.
#[derive(Debug)]
pub struct Unbound {
    pub export: &'static str,
    pub reason: &'static str,
    pub message: &'static str,
}

/// How this door reaches one row's typed owner.
#[derive(Clone, Copy, Debug)]
pub enum Binding {
    Native(&'static NativeOwner),
    Page(&'static PageOwner),
    Unbound(&'static Unbound),
}

impl Binding {
    pub fn members(&self) -> &'static [Member] {
        match self {
            Self::Native(owner) => owner.members,
            Self::Page(owner) => owner.members,
            Self::Unbound(_) => &[],
        }
    }
}

const WHOLE: &[Member] = &[Member {
    name: "request",
    kind: Kind::Whole,
    required: true,
}];

const fn member<T: Arg>(name: &'static str) -> Member {
    Member {
        name,
        kind: T::KIND,
        required: T::REQUIRED,
    }
}

fn request_bytes(request: &Map<String, Value>) -> Vec<u8> {
    serde_json::to_vec(request).expect("a JSON object serializes")
}

/// An owner's untyped `Err(text)`: its `code: message` when it names one.
fn owner_failure(text: impl std::fmt::Display) -> CommandRefusal {
    let text = text.to_string();
    let (code, message) = text
        .split_once(": ")
        .filter(|(code, _)| snake(code))
        .map_or((CODE_FAILED, text.as_str()), |(code, message)| {
            (code, message)
        });
    refusal(Class::Failed, code, message)
}

fn json_answer(text: Result<String, String>) -> Result<Value, CommandRefusal> {
    let text = text.map_err(owner_failure)?;
    serde_json::from_str(&text).map_err(|_| {
        refusal(
            Class::Internal,
            CODE_FAILED,
            "The owner answered text that is not JSON.",
        )
    })
}

macro_rules! json_owners {
    ($($export:literal => $owner:path),* $(,)?) => {
        &[$(NativeOwner {
            export: $export,
            members: WHOLE,
            run: |request| json_answer($owner(&request_bytes(&request))),
        }),*]
    };
}

/// The owners that answer without a host door.
pub static NATIVE_OWNERS: &[NativeOwner] = {
    const JSON: &[NativeOwner] = json_owners! {
        "assetsCommand" => crate::assets::evaluate,
        "designActivitiesStore" => crate::design_activities_store::evaluate,
        "designConfig" => crate::design_config::evaluate,
        "designConflict" => crate::design_conflict::evaluate,
        "designDashboard" => crate::design_dashboard::evaluate,
        "designVersionCommand" => crate::design_versions::evaluate,
        "styleCatalog" => crate::style_plan::style_catalog_request,
        "stylePlan" => crate::style_plan::plan_style_request,
        "styleResolve" => crate::style_resolution::evaluate,
    };
    JSON
};

/// The door-less owners whose function is not `&[u8] -> Result<String, String>`.
pub static NATIVE_OWNERS_TYPED: &[NativeOwner] = &[
    NativeOwner {
        export: "surveyEvaluate",
        members: WHOLE,
        run: |request| {
            crate::survey::survey_evaluate(&request_bytes(&request)).map_err(owner_failure)
        },
    },
    NativeOwner {
        export: "planProjectControlArtifactDownload",
        members: WHOLE,
        run: |request| {
            json_answer(
                crate::project_control::plan_project_control_artifact_download(&request_bytes(
                    &request,
                ))
                .map_err(|error| error.to_string()),
            )
        },
    },
    NativeOwner {
        export: "decodeAreaSelection",
        members: &[member::<Js>("input"), member::<String>("reply")],
        run: |request| {
            let (input, reply) = network_reply(request)?;
            json_answer(
                crate::area_selection::decode_response_json(&input, &reply)
                    .map_err(|error| error.to_string()),
            )
        },
    },
    NativeOwner {
        export: "decodeLineExtensionDifference",
        members: &[member::<Js>("input"), member::<String>("reply")],
        run: |request| {
            let (input, reply) = network_reply(request)?;
            json_answer(
                crate::line_extension::decode_response_json(&input, &reply)
                    .map_err(|error| error.to_string()),
            )
        },
    },
];

/// A decode owner's request text and the Network unit's reply text.
fn network_reply(mut request: Map<String, Value>) -> Result<(Vec<u8>, String), CommandRefusal> {
    let invalid = |error: String| refusal(Class::InvalidInput, CODE_REQUEST_INVALID, error);
    let Js(input) = take::<Js>(&mut request, "input").map_err(invalid)?;
    let reply = take::<String>(&mut request, "reply").map_err(invalid)?;
    Ok((
        serde_json::to_vec(&input).expect("a JSON value serializes"),
        reply,
    ))
}

const SESSION: &str =
    "The page's own session lifecycle: it keeps its typed export, outside invoke.";
const SESSION_OBJECT: &str =
    "A live page state object with signals: it keeps its typed class, outside invoke.";
const BINARY: &str = "Its input or answer is bytes, which the JSON envelope does not carry yet.";
const NOT_YET: &str = "Not carried by invoke yet: it runs through its typed export meanwhile.";

/// The typed exports this door refuses, each with why.
pub static UNBOUND: &[Unbound] = &[
    Unbound {
        export: "ClientState",
        reason: "session_object",
        message: SESSION_OBJECT,
    },
    Unbound {
        export: "authSessionForceRefresh",
        reason: "session_lifecycle",
        message: SESSION,
    },
    Unbound {
        export: "authSessionSignIn",
        reason: "session_lifecycle",
        message: SESSION,
    },
    Unbound {
        export: "authSessionSignOut",
        reason: "session_lifecycle",
        message: SESSION,
    },
    Unbound {
        export: "authSessionSignUp",
        reason: "session_lifecycle",
        message: SESSION,
    },
    Unbound {
        export: "authSessionStart",
        reason: "session_lifecycle",
        message: SESSION,
    },
    Unbound {
        export: "firebaseSendPasswordReset",
        reason: "session_lifecycle",
        message: SESSION,
    },
    Unbound {
        export: "messagingCommand",
        reason: "not_yet_bound",
        message: NOT_YET,
    },
    Unbound {
        export: "messagingOutboxDrain",
        reason: "not_yet_bound",
        message: NOT_YET,
    },
    Unbound {
        export: "assetsBytes",
        reason: "binary",
        message: BINARY,
    },
    Unbound {
        export: "assetsPreview",
        reason: "binary",
        message: BINARY,
    },
    Unbound {
        export: "computeArtifactBytes",
        reason: "binary",
        message: BINARY,
    },
    Unbound {
        export: "geodataUpload",
        reason: "binary",
        message: BINARY,
    },
    Unbound {
        export: "storedObjectBytes",
        reason: "binary",
        message: BINARY,
    },
    Unbound {
        export: "surveyPhotoMediaProcess",
        reason: "binary",
        message: BINARY,
    },
    Unbound {
        export: "vectorToolEffect",
        reason: "binary",
        message: BINARY,
    },
];

/// Declares each in-page owner once: its doors and request members (the
/// native table admission reads) and, in the WASM build, the call of its
/// typed export with exactly those values.
macro_rules! page_owners {
    (capability = $capability:ident; $(
        $export:literal [$($door:ident),*] { $($name:ident : $ty:ty),* $(,)? } => $call:expr;
    )*) => {
        /// Every owner the page runs in-page through its typed export.
        pub static PAGE_OWNERS: &[PageOwner] = &[$(
            PageOwner {
                export: $export,
                doors: &[$(page_door!($door)),*],
                members: &[$(member::<$ty>(stringify!($name))),*],
            },
        )*];

        /// Call one owner's typed export with an admitted request.
        #[cfg(all(feature = "wasm", target_arch = "wasm32"))]
        #[allow(unused_variables, unused_mut, clippy::too_many_lines)]
        pub(crate) async fn call_page_owner(
            export: &str,
            id: &'static str,
            host: &page::Host,
            mut request: Map<String, Value>,
        ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
            #[allow(unused_imports)]
            use page::{answer, js, js_opt, json_text, unit};
            #[allow(unused_imports)]
            use crate::gateway_exchange::{
                account, account_capabilities, admin, architecture, assets, compute_artifacts, design,
                design_collab, feedback, forms, geocode, geodata, governance, grid_catalog, grid_models,
                held_reads, help, installations, notes, notifications, pipeline, project_work, projects,
                reference_data, report_artifacts, sre, style_catalog, styles, survey, survey_queue,
                survey_reports,
            };
            let $capability: &'static str = id;
            match export {
                $($export => {
                    $(let $door = host.$door()?;)*
                    $(let $name: $ty = take(&mut request, stringify!($name)).map_err(page::invalid)?;)*
                    answer($call)
                })*
                other => Err(page::door_error(CODE_UNBOUND, &format!("`{other}` has no in-page owner here."))),
            }
        }
    };
}

macro_rules! page_door {
    (gateway) => {
        Door::Gateway
    };
    (store) => {
        Door::Store
    };
}

page_owners! {
    capability = capability;
    "accountCapabilitiesRefresh" [gateway, store] { project: Option<String> }
        => account_capabilities::exports::refresh(gateway, store, project).await;
    "accountDelete" [gateway] { confirmed: bool }
        => account::exports::account_delete(gateway, confirmed).await;
    "accountProfile" [gateway, store] {}
        => account::exports::account_profile(gateway, store).await;
    "accountProfileWrite" [gateway, store] { fields: Js }
        => account::exports::account_profile_write(gateway, store, js(fields)?).await;
    "adminBoundary" [gateway, store] { code: String, timeout_ms: Option<u32> }
        => admin::exports::admin_boundary(gateway, store, code, timeout_ms).await;
    "adminChildrenRefresh" [gateway, store] { level: String, parent_code: Option<String>, timeout_ms: Option<u32> }
        => admin::exports::admin_children_refresh(gateway, store, level, parent_code, timeout_ms).await;
    "architectureSnapshot" [gateway] { revision: Option<u32> }
        => architecture::exports::architecture_snapshot(gateway, revision).await;
    "assetsClassify" [gateway, store] { scope: Js, asset_id: String, classify: Js }
        => assets::exports::assets_classify(gateway, store, js(scope)?, asset_id, js(classify)?).await;
    "assetsDownload" [gateway] { project: String, asset_id: String, surface: Option<String> }
        => assets::exports::assets_download(gateway, project, asset_id, surface).await;
    "assetsIndexRow" [gateway] { project: String, asset_id: String }
        => assets::exports::assets_index_row(gateway, project, asset_id).await;
    "assetsLink" [gateway] { project: String, asset_id: String, link: Js, detach: bool }
        => assets::exports::assets_link(gateway, project, asset_id, js(link)?, detach).await;
    "assetsTimelinePage" [gateway, store] { scope: Js, query: Option<Js> }
        => assets::exports::assets_timeline_page(gateway, store, js(scope)?, js_opt(query)?, wasm_bindgen::JsValue::UNDEFINED).await;
    "assetsTree" [gateway] { project: String }
        => assets::exports::assets_tree(gateway, project).await;
    "backupLedgerRefresh" [gateway, store] { days: u32, limit: Option<u32> }
        => held_reads::exports::backup_ledger_refresh(gateway, store, days, limit).await;
    "computeArtifactHeads" [gateway] { project_id: String, limit: Option<u32>, cursor: Option<String> }
        => compute_artifacts::exports::compute_artifact_heads(gateway, project_id, limit, cursor).await;
    "designCall" [gateway] { call: Whole }
        => design::exports::design_call(gateway, js(call)?).await;
    "designCollab" [gateway] { surface: Js, request: Js }
        => design_collab::exports::design_collab(gateway, js(surface)?, js(request)?).await;
    "deviceApproval" [gateway] { decision: Js, uid: String, email: String }
        => account::exports::device_approval(gateway, js(decision)?, uid, email).await;
    "feedbackBacklog" [gateway] {}
        => feedback::exports::feedback_backlog(gateway).await;
    "feedbackSubmitHuman" [gateway] { title: String, detail: String, component: String, kind: String, severity: String, project: Option<String> }
        => feedback::exports::feedback_submit_human(gateway, title, detail, component, kind, severity, project).await;
    "feedbackUpdateStatus" [gateway] { id: String, expected_version: u32, status: Js, resolution: String }
        => feedback::exports::feedback_update_status(gateway, id, expected_version, js(status)?, resolution).await;
    "formsAcquire" [gateway, store] { endpoint: Js, body: Js, mode: Js }
        => forms::exports::forms_acquire(gateway, store, js(endpoint)?, js(body)?, js(mode)?).await;
    "formsRequest" [gateway, store] { endpoint: Js, body: Js }
        => forms::exports::forms_request(gateway, store, js(endpoint)?, js(body)?).await;
    "geocodeReverse" [gateway] { lat: f64, lng: f64, language: Option<String> }
        => geocode::exports::geocode_reverse(gateway, lat, lng, language).await;
    "geocodeSearch" [gateway] { query: String, language: Option<String>, bias: Option<Js> }
        => geocode::exports::geocode_search(gateway, query, language, js_opt(bias)?).await;
    "geodataDelete" [gateway] { project: Option<String>, upload_id: String }
        => unit(geodata::exports::geodata_delete(gateway, project, upload_id).await);
    "geodataList" [gateway] { project: Option<String> }
        => geodata::exports::geodata_list(gateway, project).await;
    "geometryOperation" [] { input: Whole }
        => crate::geometry_operation::browser::run(tsify::Ts::new_unchecked(js(input)?));
    "governancePage" [gateway, store] {}
        => governance::exports::governance_page(gateway, store).await;
    "governanceRemoveFromAllProjects" [gateway] { email: String }
        => governance::exports::governance_remove_from_all_projects(gateway, email).await;
    "governanceSetAccessState" [gateway] { email: String, account_status: String, reason: String }
        => governance::exports::governance_set_access_state(gateway, email, account_status, reason).await;
    "governanceSetAppRoles" [gateway] { email: String, roles: Vec<String> }
        => unit(governance::exports::governance_set_app_roles(gateway, email, roles).await);
    "gridCatalogRead" [gateway, store] { scope: Js, mode: Js }
        => grid_catalog::exports::grid_project_catalog(gateway, store, js(scope)?, js(mode)?).await;
    "gridExportsList" [gateway] { project: String, model: String, revision: String }
        => grid_models::exports::grid_exports_list(gateway, project, model, revision).await;
    "gridModelDelete" [gateway] { project: String, model: String, expected_head_revision_id: String, expected_model_digest: String, reason: String }
        => grid_models::exports::grid_model_delete(gateway, project, model, expected_head_revision_id, expected_model_digest, reason).await;
    "gridModelGet" [gateway] { project: String, model: String }
        => grid_models::exports::grid_model_get(gateway, project, model).await;
    "gridModelsList" [gateway] { project: String, include_deleted: bool }
        => grid_models::exports::grid_models_list(gateway, project, include_deleted).await;
    "gridVersionCreate" [gateway] { input: Whole }
        => grid_models::exports::grid_version_create(gateway, js(input)?).await;
    "gridVersionResolve" [gateway] { project: String, model: String, revision: String }
        => grid_models::exports::grid_version_resolve(gateway, project, model, revision).await;
    "gridVersionsList" [gateway] { project: String, model: String }
        => grid_models::exports::grid_versions_list(gateway, project, model).await;
    "helpArticles" [gateway] { language: String }
        => help::exports::help_articles(gateway, language).await;
    "installationManagerRefresh" [gateway, store] { url: String, channel: String }
        => held_reads::exports::installation_manager_refresh(gateway, store, url, channel).await;
    "installationsCommand" [gateway, store] { value: Whole }
        => installations::exports::installations_command(gateway, store, js(value)?).await;
    "notesCreate" [gateway] { note: Whole }
        => notes::exports::notes_create(gateway, js(note)?).await;
    "notesGet" [gateway] { id: String }
        => notes::exports::notes_get(gateway, id).await;
    "notesList" [gateway] { include_archived: bool, include_trashed: bool }
        => notes::exports::notes_list(gateway, include_archived, include_trashed).await;
    "notesTrash" [gateway] { id: String, expected_version: f64, trashed: bool }
        => notes::exports::notes_trash(gateway, id, expected_version, trashed).await;
    "notesUpdate" [gateway] { id: String, expected_version: f64, note: Js }
        => notes::exports::notes_update(gateway, id, expected_version, js(note)?).await;
    "notificationsMark" [gateway] { mark: Js, ids: Vec<String> }
        => notifications::exports::notifications_mark(gateway, js(mark)?, ids).await;
    "notificationsPage" [gateway, store] { limit: Option<u32>, cursor: Option<String> }
        => notifications::exports::notifications_page(gateway, store, limit, cursor).await;
    "pipelineCall" [gateway] { call: Whole }
        => pipeline::exports::pipeline_call(gateway, js(call)?).await;
    "pipelineStatusRefresh" [gateway, store] { project: String }
        => held_reads::exports::pipeline_status_refresh(gateway, store, project).await;
    "projectConfigRefresh" [gateway, store] { project: String }
        => held_reads::exports::project_config_refresh(gateway, store, project).await;
    "projectFormsAcquire" [gateway, store] { project: String, mode: Js }
        => forms::exports::project_forms_acquire(gateway, store, project, js(mode)?).await;
    "projectWork" [gateway] { request: Whole }
        => project_work::exports::project_work(gateway, js(request)?).await;
    "projectsAction" [gateway] { endpoint: Js, action: String, data: Js }
        => projects::exports::projects_action(gateway, js(endpoint)?, action, js(data)?).await;
    "projectsDirectory" [gateway, store] { status: String }
        => projects::exports::projects_directory(gateway, store, status).await;
    "referenceCatalogue" [gateway] { project: String }
        => reference_data::exports::reference_catalogue(gateway, project).await;
    "releaseRefresh" [gateway, store] { url: String }
        => held_reads::exports::release_refresh(gateway, store, url).await;
    "reportArtifactRemove" [gateway] { project_id: String, scope: String, transformer: String, file_name: String, gcs_path: String, sha256: String }
        => unit(report_artifacts::exports::report_artifact_remove(gateway, project_id, scope, transformer, file_name, gcs_path, sha256).await);
    "sreActivity" [gateway, store] { days: u32, limit: u32 }
        => sre::exports::sre_activity(gateway, store, days, limit).await;
    "sreOverview" [gateway] {}
        => sre::exports::sre_overview(gateway).await;
    "styleCatalogLayerDocument" [gateway, store] { project: String, refresh: bool }
        => style_catalog::exports::layer_document(gateway, store, project, refresh).await;
    "storeStatus" [] { native: Option<String> }
        => json_text(crate::store_status::browser::store_status(native).await);
    "stylesPublish" [gateway] { payload: Js }
        => styles::exports::styles_publish(gateway, js(payload)?, Some(capability.to_owned())).await;
    "surveyHoldRead" [gateway, store] { request: Whole }
        => survey::exports::survey_hold_read(gateway, store, js(request)?).await;
    "surveyQueueDrain" [gateway, store] { project: Option<String> }
        => survey_queue::browser::survey_queue_drain(gateway, store, project).await;
    "surveyReportsRead" [gateway, store] { project: String, include_sync_counts: bool, refresh_stale: bool }
        => survey_reports::exports::survey_reports_read(gateway, store, project, include_sync_counts, refresh_stale).await;
    "surveyReportsRegenerate" [gateway, store] { project: String, force: bool, include_sync_counts: bool }
        => survey_reports::exports::survey_reports_regenerate(gateway, store, project, force, include_sync_counts).await;
    "surveyWorkingAreaFormsInvoke" [gateway, store] { invocation: Whole }
        => crate::survey_working_area_forms::store::browser::invoke(gateway, store, js(invocation)?).await;
    "transformerVersionDownload" [gateway, store] { project: String, transformer: String, version: String }
        => held_reads::exports::transformer_version_download(gateway, store, project, transformer, version).await;
    "transformerVersionsRefresh" [gateway, store] { project: String, transformer: String }
        => held_reads::exports::transformer_versions_refresh(gateway, store, project, transformer).await;
    "tutorialArticles" [gateway] { language: String }
        => help::exports::tutorial_articles(gateway, language).await;
    "vectorToolCatalogue" [] {}
        => crate::vector_tools::browser::catalogue();
}

/// How this door reaches `export`, or `None` for an export it has no
/// decision for (a test holds every manifest export to one decision).
pub fn binding(export: &str) -> Option<Binding> {
    NATIVE_OWNERS
        .iter()
        .chain(NATIVE_OWNERS_TYPED)
        .find(|owner| owner.export == export)
        .map(Binding::Native)
        .or_else(|| {
            PAGE_OWNERS
                .iter()
                .find(|owner| owner.export == export)
                .map(Binding::Page)
        })
        .or_else(|| {
            UNBOUND
                .iter()
                .find(|owner| owner.export == export)
                .map(Binding::Unbound)
        })
}

/// A refusal of this door, before or after its owner ran.
#[derive(Clone, Debug, PartialEq)]
pub struct Refused {
    /// The envelope's `command`: the capability id, or `ds` for a request
    /// that names no well-formed id, as the CLI answers one.
    pub command: String,
    pub refusal: CommandRefusal,
}

/// A refusal with an explicit `retryable`.
pub fn refusal(class: Class, code: &str, message: impl Into<String>) -> CommandRefusal {
    CommandRefusal {
        class: class.token().into(),
        code: code.into(),
        message: message.into(),
        retryable: false,
        remedy: None,
        next: Vec::new(),
        detail: None,
    }
}

fn with_remedy(mut refusal: CommandRefusal, remedy: impl Into<String>) -> CommandRefusal {
    refusal.remedy = Some(remedy.into());
    refusal
}

fn with_detail(mut refusal: CommandRefusal, detail: Value) -> CommandRefusal {
    refusal.detail = Some(detail);
    refusal
}

fn snake(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 128
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn well_formed_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')
        })
}

/// What one admitted call runs.
#[derive(Debug)]
pub enum Plan {
    /// An in-page typed owner, with its admitted request.
    Owner {
        capability: &'static Capability,
        binding: Binding,
        request: Map<String, Value>,
    },
    /// One Core command through the host's Core door.
    Core {
        capability: &'static Capability,
        project: String,
        args: Map<String, Value>,
    },
    /// The page's Network unit.
    Unit {
        capability: &'static Capability,
        input: Value,
    },
}

impl Plan {
    pub fn capability(&self) -> &'static Capability {
        match self {
            Self::Owner { capability, .. }
            | Self::Core { capability, .. }
            | Self::Unit { capability, .. } => capability,
        }
    }
}

/// Admit one call on a build serving `platform`: the row, its executor and
/// the request's shape, before any door opens.
pub fn plan(platform: &str, id: &str, request: &str) -> Result<Plan, Refused> {
    let unresolved = || {
        if well_formed_id(id) {
            id.to_owned()
        } else {
            "ds".to_owned()
        }
    };
    let capability = capability(id).map_err(|_| Refused {
        command: unresolved(),
        refusal: with_remedy(
            refusal(
                Class::InvalidInput,
                CODE_UNKNOWN,
                format!("`{id}` is not a capability this client's manifest declares."),
            ),
            "Choose a capability id `ds capabilities` declares.",
        ),
    })?;
    let refused = |refusal: CommandRefusal| Refused {
        command: capability.id.to_owned(),
        refusal,
    };
    let route = capability.route_on(platform).ok_or_else(|| {
        refused(refusal(
            Class::Internal,
            CODE_UNKNOWN,
            format!("No platform `{platform}` is declared."),
        ))
    })?;
    let executor = match route {
        Route::Supported { executor, .. } => executor,
        refused_route => {
            let answer = refused_route
                .refusal()
                .expect("a refused route has its refusal");
            return Err(refused(with_detail(
                with_remedy(
                    refusal(Class::Unavailable, answer.code, answer.message),
                    "Choose a capability supported by this build, or open a compatible DS client.",
                ),
                json!({ "reason": answer.reason }),
            )));
        }
    };
    if request.len() > REQUEST_MAX_BYTES {
        return Err(refused(refusal(
            Class::InvalidInput,
            CODE_REQUEST_INVALID,
            format!("A request carries at most {REQUEST_MAX_BYTES} bytes."),
        )));
    }
    let value: Value = serde_json::from_str(request).map_err(|error| {
        refused(with_remedy(
            refusal(
                Class::InvalidInput,
                CODE_REQUEST_INVALID,
                format!("The request is not one JSON value: {error}."),
            ),
            format!("Pass the request `{}` takes as JSON text.", capability.id),
        ))
    })?;
    let object = |value: Value| match value {
        Value::Object(map) => Ok(map),
        _ => Err(refused(with_remedy(
            refusal(
                Class::InvalidInput,
                CODE_REQUEST_INVALID,
                "The request is one JSON object.",
            ),
            format!(
                "Pass the inputs `{}` declares as one object.",
                capability.id
            ),
        ))),
    };
    match executor {
        Executor::NetworkUnit => Ok(Plan::Unit {
            capability,
            input: value,
        }),
        Executor::Core => {
            let mut args = object(value)?;
            let project = match args.remove("project") {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(project)) => project,
                Some(_) => {
                    return Err(refused(refusal(
                        Class::InvalidInput,
                        "invalid_value",
                        "`project` is one exact project id.",
                    )))
                }
            };
            if let Some(run) = capability.run {
                for switch in run.switches {
                    args.insert((*switch).to_owned(), Value::Bool(true));
                }
                if let Some(output) = run.output {
                    args.insert(output.to_owned(), Value::Bool(true));
                }
            }
            Ok(Plan::Core {
                capability,
                project,
                args,
            })
        }
        Executor::Kernel => {
            let (export, exchange) = match (capability.export, route) {
                (
                    Some(export),
                    Route::Supported {
                        exchange: Some(exchange),
                        ..
                    },
                ) => (export, exchange),
                _ => {
                    return Err(refused(refusal(
                        Class::Internal,
                        CODE_UNBOUND,
                        format!("`{}` names no in-page owner.", capability.id),
                    )))
                }
            };
            admit_export_on(platform, capability.id, export, exchange).map_err(|unadmitted| {
                refused(refusal(
                    Class::Unavailable,
                    unadmitted.code,
                    unadmitted.message,
                ))
            })?;
            let binding = binding(export).ok_or_else(|| {
                refused(refusal(
                    Class::Internal,
                    CODE_UNBOUND,
                    format!("`{export}` has no invoke decision in this build."),
                ))
            })?;
            if let Binding::Unbound(unbound) = binding {
                return Err(refused(with_detail(
                    with_remedy(
                        refusal(
                            Class::Unavailable,
                            CODE_UNBOUND,
                            format!(
                                "`{}` runs through its typed `{export}` export. {}",
                                capability.id, unbound.message
                            ),
                        ),
                        format!(
                            "Call `{export}` for `{}` until invoke carries it.",
                            capability.id
                        ),
                    ),
                    json!({ "export": export, "reason": unbound.reason }),
                )));
            }
            let request = object(value)?;
            admit_members(capability.id, binding.members(), &request).map_err(refused)?;
            admit_request_capability(platform, capability.id, export, &request).map_err(refused)?;
            Ok(Plan::Owner {
                capability,
                binding,
                request,
            })
        }
    }
}

/// The CLI's own refusals for a request that breaks its declared inputs:
/// `unknown_flag` for an undeclared member, `missing_input` for every
/// omitted required one at once, and a typed value refusal.
pub fn admit_members(
    id: &str,
    members: &[Member],
    request: &Map<String, Value>,
) -> Result<(), CommandRefusal> {
    if members.iter().any(|member| member.kind == Kind::Whole) {
        return Ok(());
    }
    let accepted: Vec<&str> = members.iter().map(|member| member.name).collect();
    if let Some(name) = request
        .keys()
        .find(|name| !accepted.contains(&name.as_str()))
    {
        return Err(with_detail(
            with_remedy(
                refusal(
                    Class::InvalidInput,
                    "unknown_flag",
                    format!("`{name}` is not an input of `{id}`"),
                ),
                "pass only the inputs this capability declares",
            ),
            json!({ "accepted": accepted }),
        ));
    }
    let missing: Vec<&str> = members
        .iter()
        .filter(|member| member.required && request.get(member.name).is_none_or(Value::is_null))
        .map(|member| member.name)
        .collect();
    match missing.as_slice() {
        [] => {}
        [name] => {
            return Err(with_remedy(
                refusal(
                    Class::InvalidInput,
                    "missing_input",
                    format!("`{name}` is required"),
                ),
                format!("pass `{name}`"),
            ))
        }
        [rest @ .., last] => {
            let names: Vec<String> = rest.iter().map(|name| format!("`{name}`")).collect();
            return Err(with_detail(
                with_remedy(
                    refusal(
                        Class::InvalidInput,
                        "missing_input",
                        format!("{} and `{last}` are required", names.join(", ")),
                    ),
                    format!("pass {} and `{last}`", names.join(", ")),
                ),
                json!({ "missing": missing }),
            ));
        }
    }
    for member in members {
        let Some(value) = request.get(member.name).filter(|value| !value.is_null()) else {
            continue;
        };
        let fits = match member.kind {
            Kind::Text => value.is_string(),
            Kind::Number => value.is_number(),
            Kind::Count => value
                .as_u64()
                .is_some_and(|count| count <= u64::from(u32::MAX)),
            Kind::Flag => value.is_boolean(),
            Kind::TextList => value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string)),
            Kind::Any | Kind::Whole => true,
        };
        if !fits {
            let (code, expected) = match member.kind {
                Kind::Number => ("invalid_number", "a number"),
                Kind::Count => ("invalid_number", "a whole number from 0 through 4294967295"),
                Kind::Text => ("invalid_value", "text"),
                Kind::Flag => ("invalid_value", "true or false"),
                _ => ("invalid_value", "a list of text"),
            };
            return Err(refusal(
                Class::InvalidInput,
                code,
                format!("`{}` must be {expected}", member.name),
            ));
        }
    }
    Ok(())
}

/// A shared export runs one of several rows, decided by its request: the
/// request must perform the capability invoked, not a sibling row.
fn admit_request_capability(
    platform: &str,
    id: &'static str,
    export: &str,
    request: &Map<String, Value>,
) -> Result<(), CommandRefusal> {
    use crate::gateway_exchange::{design_collab, forms, project_work, projects};
    let from_gateway =
        |refused: crate::gateway_exchange::Refusal| from_rejection(&Rejection::of(&refused));
    let decode = |name: &str| -> Result<Value, CommandRefusal> {
        Ok(request.get(name).cloned().unwrap_or(Value::Null))
    };
    let performed: Vec<&'static str> = match export {
        project_work::EXPORT => project_work::ProjectWork {
            request: request.clone(),
        }
        .admit_in_page(platform)
        .map_err(from_gateway)?,
        design_collab::EXPORT => {
            let surface = serde_json::from_value(decode("surface")?).map_err(|_| {
                refusal(
                    Class::InvalidInput,
                    "invalid_value",
                    "`surface` is not a design collaboration surface",
                )
            })?;
            let Value::Object(request) = decode("request")? else {
                return Err(refusal(
                    Class::InvalidInput,
                    "invalid_value",
                    "`request` is one object",
                ));
            };
            vec![design_collab::DesignCollab { surface, request }
                .admit_in_page(platform)
                .map_err(from_gateway)?]
        }
        forms::EXPORT => {
            let endpoint = serde_json::from_value(decode("endpoint")?).map_err(|_| {
                refusal(
                    Class::InvalidInput,
                    "invalid_value",
                    "`endpoint` is not a form endpoint",
                )
            })?;
            let Value::Object(body) = decode("body")? else {
                return Err(refusal(
                    Class::InvalidInput,
                    "invalid_value",
                    "`body` is one object",
                ));
            };
            vec![forms::admit_in_page(platform, endpoint, &body).map_err(from_gateway)?]
        }
        projects::EXPORT => {
            let endpoint = serde_json::from_value(decode("endpoint")?).map_err(|_| {
                refusal(
                    Class::InvalidInput,
                    "invalid_value",
                    "`endpoint` is not a project endpoint",
                )
            })?;
            let Value::Object(data) = decode("data")? else {
                return Err(refusal(
                    Class::InvalidInput,
                    "invalid_value",
                    "`data` is one object",
                ));
            };
            let action = request
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            vec![projects::ProjectAction {
                endpoint,
                action,
                data,
            }
            .admit_in_page(platform)
            .map_err(from_gateway)?]
        }
        "styleCatalog" => match request.get("operation").and_then(Value::as_str) {
            Some("list") => vec!["style.list"],
            Some("read") => vec!["style.read"],
            _ => return Ok(()),
        },
        _ => return Ok(()),
    };
    if performed.contains(&id) {
        return Ok(());
    }
    Err(with_detail(
        with_remedy(
            refusal(
                Class::InvalidInput,
                CODE_REQUEST_MISMATCH,
                format!(
                    "This request performs `{}`, not `{id}`.",
                    performed.join("`, `")
                ),
            ),
            format!(
                "Invoke `{}` for this request, or send the request `{id}` takes.",
                performed[0]
            ),
        ),
        json!({ "performs": performed }),
    ))
}

/// What a rejected door or owner told the page, as the Kernel's JS errors
/// carry it (`GatewayRefusal`, `CapabilityRefusal`, a CLI refusal body).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rejection {
    pub class: Option<String>,
    pub code: Option<String>,
    pub message: Option<String>,
    pub remedy: Option<String>,
    pub retryable: Option<bool>,
    pub next: Vec<String>,
    pub detail: Option<Value>,
    pub status: Option<u16>,
    pub transient: bool,
    pub signed_out: bool,
    pub outcome_unknown: bool,
    pub details: Option<Value>,
}

impl Rejection {
    /// A gateway exchange's own refusal.
    pub fn of(refused: &crate::gateway_exchange::Refusal) -> Self {
        Self {
            code: Some(refused.code.clone()),
            message: Some(refused.message.clone()),
            retryable: Some(refused.retryable),
            status: refused.status,
            transient: refused.transient,
            signed_out: refused.signed_out,
            outcome_unknown: refused.outcome_unknown,
            details: refused.details.clone(),
            ..Self::default()
        }
    }
}

/// The CLI refusal body for a rejection. A typed class is kept; otherwise
/// the class follows from what the rejection says happened, and the code is
/// the owner's own, in the envelope's snake_case.
pub fn from_rejection(rejection: &Rejection) -> CommandRefusal {
    let code = rejection
        .code
        .as_deref()
        .map(|code| {
            code.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .take(128)
                .collect::<String>()
        })
        .filter(|code| snake(code))
        .unwrap_or_else(|| CODE_FAILED.to_owned());
    let status = rejection.status.unwrap_or(0);
    let class = rejection
        .class
        .as_deref()
        .and_then(Class::from_token)
        .unwrap_or_else(|| {
            if rejection.signed_out || matches!(status, 401 | 403) || code == "signed_out" {
                Class::Unauthorized
            } else if matches!(status, 409 | 412) || code == "session_changed" {
                Class::Conflict
            } else if rejection.transient
                || rejection.outcome_unknown
                || status == 429
                || status >= 500
                || matches!(
                    code.as_str(),
                    "network_error"
                        | "network_offline"
                        | "request_timeout"
                        | "engine_timed_out"
                        | "engine_connection_lost"
                        | "callee_timed_out"
                        | "core_missing"
                        | "network_engine_missing"
                        | CODE_DOOR_UNAVAILABLE
                        | CODE_UNBOUND
                )
            {
                Class::Unavailable
            } else if (400..500).contains(&status)
                || code.ends_with("_input_invalid")
                || matches!(
                    code.as_str(),
                    "invalid_input" | "inputs_invalid" | CODE_REQUEST_INVALID | "project_invalid"
                )
            {
                Class::InvalidInput
            } else {
                Class::Failed
            }
        });
    let mut message: String = rejection
        .message
        .clone()
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| "The capability failed without saying why.".into());
    if message.len() > 4096 {
        let mut end = 4096;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    let retryable = rejection.retryable.unwrap_or(false) && !rejection.outcome_unknown;
    let detail =
        if rejection.status.is_some() || rejection.outcome_unknown || rejection.details.is_some() {
            let mut detail = Map::new();
            if let Some(status) = rejection.status {
                detail.insert("status".into(), json!(status));
            }
            if rejection.outcome_unknown {
                detail.insert("outcome_unknown".into(), Value::Bool(true));
            }
            if let Some(details) = &rejection.details {
                detail.insert("details".into(), details.clone());
            }
            Some(Value::Object(detail))
        } else {
            rejection.detail.clone()
        };
    CommandRefusal {
        class: class.token().into(),
        code,
        message,
        retryable,
        remedy: rejection.remedy.clone().filter(|remedy| !remedy.is_empty()),
        next: rejection.next.iter().take(64).cloned().collect(),
        detail: detail
            .filter(|detail| serde_json::to_vec(detail).is_ok_and(|bytes| bytes.len() <= 65536)),
    }
}

/// The doors the page passes. Each answers what its executor answered, or
/// that executor's refusal.
#[allow(async_fn_in_trait)]
pub trait Doors {
    /// One Core command; Core's own envelope as JSON text.
    async fn core(
        &self,
        id: &'static str,
        project: &str,
        args: Map<String, Value>,
    ) -> Result<String, CommandRefusal>;
    /// The page's Network unit.
    async fn unit(&self, id: &'static str, input: Value) -> Result<Value, CommandRefusal>;
    /// One in-page typed owner, with its admitted request.
    async fn owner(
        &self,
        owner: &'static PageOwner,
        id: &'static str,
        request: Map<String, Value>,
    ) -> Result<Value, CommandRefusal>;
}

/// The envelope text of one answered call.
pub fn success_text(command: &str, data: Value) -> String {
    serde_json::to_string(&success_envelope(command, CONTRACT, data))
        .expect("a success envelope serializes")
}

/// The envelope text of one refused call, through the CLI's serializer.
pub fn error_text(command: &str, refusal: &CommandRefusal) -> String {
    let refusal = match refusal.clone().into_failure() {
        Ok(failure) => failure,
        Err(_) => from_rejection(&Rejection {
            class: Some(refusal.class.clone()),
            code: Some(refusal.code.clone()),
            message: Some(refusal.message.clone()),
            remedy: refusal.remedy.clone(),
            retryable: Some(refusal.retryable),
            ..Rejection::default()
        })
        .into_failure()
        .expect("a normalized refusal is valid"),
    };
    serde_json::to_string(&error_envelope(command, CONTRACT, &refusal))
        .expect("an error envelope serializes")
}

/// Core's answer when it is a command envelope for this command: its own
/// text, unchanged, so additive members reach the caller.
pub fn core_envelope(id: &str, text: String) -> Result<String, CommandRefusal> {
    let envelope: CommandEnvelope<Value> = serde_json::from_str(&text).map_err(|_| {
        refusal(
            Class::Unavailable,
            CODE_CORE_INCOMPATIBLE,
            "DS Core answered without a compatible command envelope.",
        )
    })?;
    if envelope.command() != id {
        return Err(refusal(
            Class::Unavailable,
            CODE_CORE_INCOMPATIBLE,
            format!("DS Core answered `{}` for `{id}`.", envelope.command()),
        ));
    }
    Ok(text)
}

/// One call on a build serving `platform`, through `doors`: the envelope text.
pub async fn invoke_with<D: Doors>(doors: &D, platform: &str, id: &str, request: &str) -> String {
    let plan = match plan(platform, id, request) {
        Ok(plan) => plan,
        Err(refused) => return error_text(&refused.command, &refused.refusal),
    };
    let command = plan.capability().id;
    let answer = match plan {
        Plan::Core {
            capability,
            project,
            args,
        } => match doors.core(capability.id, &project, args).await {
            Ok(text) => match core_envelope(capability.id, text) {
                Ok(text) => return text,
                Err(refusal) => Err(refusal),
            },
            Err(refusal) => Err(refusal),
        },
        Plan::Unit { capability, input } => doors.unit(capability.id, input).await,
        Plan::Owner {
            capability,
            binding,
            request,
        } => match binding {
            Binding::Native(owner) => (owner.run)(request),
            Binding::Page(owner) => doors.owner(owner, capability.id, request).await,
            Binding::Unbound(_) => unreachable!("plan refuses an unbound owner"),
        },
    };
    match answer {
        Ok(data) => success_text(command, data),
        Err(refusal) => error_text(command, &refusal),
    }
}

/// Every export some row names for an in-page cell, for the coverage test.
pub fn manifest_exports() -> BTreeSet<&'static str> {
    super::CAPABILITIES
        .iter()
        .filter(|capability| {
            super::PLATFORMS.iter().any(|platform| {
                matches!(
                    capability.route_on(platform),
                    Some(Route::Supported {
                        executor: Executor::Kernel,
                        ..
                    })
                )
            })
        })
        .filter_map(|capability| capability.export)
        .collect()
}

/// The page's export: the doors it passes, opened as the plan names them.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod page {
    use super::{
        from_rejection, refusal, Class, CommandRefusal, Js, Map, PageOwner, Rejection, Value,
    };
    use super::{CODE_DOOR_UNAVAILABLE, CODE_REQUEST_INVALID};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::GatewayHost;
    use js_sys::{Function, Object, Promise, Reflect};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    #[wasm_bindgen(typescript_custom_section)]
    const INVOKE_HOST: &'static str = r#"
/** The doors the page passes to `invoke`; the Kernel picks the door, the page only opens it. */
export interface InvokeHost {
    /** The page's gateway host, for capabilities whose owner calls the gateway. */
    gateway?: GatewayHost;
    /** The page's Kernel store host, for owners that keep held data. */
    store?: unknown;
    /** One Core command through the host's Core door, answered by Core's own envelope. */
    core?(command: string, project: string, args: Record<string, unknown>): Promise<unknown>;
    /** The page's Network WASM unit. */
    unit?(capability: string, input: unknown): Promise<unknown>;
}
"#;

    /// The doors one call reads.
    pub(crate) struct Host {
        host: JsValue,
    }

    pub(crate) fn door_error(code: &str, message: &str) -> JsValue {
        let error = js_sys::Error::new(message);
        error.set_name("CapabilityRefusal");
        let _ = Reflect::set(&error, &"code".into(), &code.into());
        error.into()
    }

    pub(crate) fn invalid(message: String) -> JsValue {
        door_error(CODE_REQUEST_INVALID, &message)
    }

    impl Host {
        fn member(&self, name: &str) -> Result<JsValue, JsValue> {
            let value = if self.host.is_object() {
                Reflect::get(&self.host, &name.into()).unwrap_or(JsValue::UNDEFINED)
            } else {
                JsValue::UNDEFINED
            };
            if value.is_undefined() || value.is_null() {
                return Err(door_error(
                    CODE_DOOR_UNAVAILABLE,
                    &format!(
                        "This page passed no `{name}` door, which this capability's owner needs."
                    ),
                ));
            }
            Ok(value)
        }

        pub(crate) fn gateway(&self) -> Result<GatewayHost, JsValue> {
            self.member("gateway").map(JsCast::unchecked_into)
        }

        pub(crate) fn store(&self) -> Result<StoreHost, JsValue> {
            self.member("store").map(JsCast::unchecked_into)
        }

        async fn open(&self, door: &str, args: [JsValue; 3]) -> Result<JsValue, CommandRefusal> {
            let function = self
                .member(door)
                .and_then(|value| value.dyn_into::<Function>().map_err(|_| JsValue::UNDEFINED))
                .map_err(|_| {
                    refusal(
                        Class::Unavailable,
                        CODE_DOOR_UNAVAILABLE,
                        format!(
                            "This page has no `{door}` door for the executor the Kernel names."
                        ),
                    )
                })?;
            let [first, second, third] = args;
            let answer = function
                .call3(&self.host, &first, &second, &third)
                .map_err(|error| rejected(&error))?;
            let answer = JsFuture::from(Promise::resolve(&answer));
            let deadline = gloo_timers::future::TimeoutFuture::new(610_000);
            match futures_util::future::select(std::pin::pin!(answer), std::pin::pin!(deadline))
                .await
            {
                futures_util::future::Either::Left((answer, _)) => {
                    answer.map_err(|error| rejected(&error))
                }
                futures_util::future::Either::Right(_) => Err(refusal(
                    Class::Unavailable,
                    "engine_timed_out",
                    "The engine did not answer within the command deadline.",
                )),
            }
        }
    }

    fn member(value: &JsValue, key: &str) -> JsValue {
        if value.is_object() {
            Reflect::get(value, &key.into()).unwrap_or(JsValue::UNDEFINED)
        } else {
            JsValue::UNDEFINED
        }
    }

    /// What a rejection carries, read leniently: any member may be absent.
    pub(crate) fn rejected(error: &JsValue) -> CommandRefusal {
        let json = |value: JsValue| -> Option<Value> {
            if value.is_undefined() || value.is_null() {
                return None;
            }
            js_sys::JSON::stringify(&value)
                .ok()
                .and_then(|text| text.as_string())
                .and_then(|text| serde_json::from_str(&text).ok())
        };
        let next = json(member(error, "next"))
            .and_then(|next| serde_json::from_value::<Vec<String>>(next).ok())
            .unwrap_or_default();
        from_rejection(&Rejection {
            class: member(error, "class").as_string(),
            code: member(error, "code").as_string(),
            message: member(error, "message")
                .as_string()
                .or_else(|| error.as_string()),
            remedy: member(error, "remedy").as_string(),
            retryable: member(error, "retryable").as_bool(),
            next,
            detail: json(member(error, "detail")),
            status: member(error, "status").as_f64().map(|status| status as u16),
            transient: member(error, "transient").as_bool().unwrap_or(false),
            signed_out: member(error, "signedOut").as_bool().unwrap_or(false),
            outcome_unknown: member(error, "outcomeUnknown").as_bool().unwrap_or(false),
            details: json(member(error, "details")),
        })
    }

    /// An answer as the JSON the envelope carries.
    fn value(answer: &JsValue) -> Result<Value, CommandRefusal> {
        if answer.is_undefined() {
            return Ok(Value::Null);
        }
        js_sys::JSON::stringify(answer)
            .ok()
            .and_then(|text| text.as_string())
            .and_then(|text| serde_json::from_str(&text).ok())
            .ok_or_else(|| {
                refusal(
                    Class::Internal,
                    super::CODE_FAILED,
                    "The owner answered a value the JSON envelope cannot carry.",
                )
            })
    }

    pub(crate) fn js(value: impl Into<JsArg>) -> Result<JsValue, JsValue> {
        crate::gateway_exchange::browser::typed(&value.into().0)
    }

    pub(crate) fn js_opt(value: Option<Js>) -> Result<JsValue, JsValue> {
        value.map_or(Ok(JsValue::UNDEFINED), js)
    }

    pub(crate) struct JsArg(Value);
    impl From<Js> for JsArg {
        fn from(value: Js) -> Self {
            Self(value.0)
        }
    }
    impl From<super::Whole> for JsArg {
        fn from(value: super::Whole) -> Self {
            Self(Value::Object(value.0))
        }
    }

    pub(crate) fn answer<T: Into<JsValue>, E: Into<JsValue>>(
        result: Result<T, E>,
    ) -> Result<JsValue, JsValue> {
        result.map(Into::into).map_err(Into::into)
    }

    /// An owner that answers its record as JSON text: the record itself.
    pub(crate) fn json_text(result: Result<String, JsValue>) -> Result<JsValue, JsValue> {
        js_sys::JSON::parse(&result?)
    }

    pub(crate) fn unit(result: Result<(), JsValue>) -> Result<JsValue, JsValue> {
        result.map(|()| JsValue::NULL)
    }

    impl super::Doors for Host {
        async fn core(
            &self,
            id: &'static str,
            project: &str,
            args: Map<String, Value>,
        ) -> Result<String, CommandRefusal> {
            let args = crate::gateway_exchange::browser::typed(&Value::Object(args))
                .map_err(|error| rejected(&error))?;
            let envelope = self.open("core", [id.into(), project.into(), args]).await?;
            serde_json::to_string(&value(&envelope)?).map_err(|_| {
                refusal(
                    Class::Internal,
                    super::CODE_FAILED,
                    "Core's envelope did not serialize.",
                )
            })
        }

        async fn unit(&self, id: &'static str, input: Value) -> Result<Value, CommandRefusal> {
            let input = crate::gateway_exchange::browser::typed(&input)
                .map_err(|error| rejected(&error))?;
            value(
                &self
                    .open("unit", [id.into(), input, JsValue::UNDEFINED])
                    .await?,
            )
        }

        async fn owner(
            &self,
            owner: &'static PageOwner,
            id: &'static str,
            request: Map<String, Value>,
        ) -> Result<Value, CommandRefusal> {
            let answer = super::call_page_owner(owner.export, id, self, request)
                .await
                .map_err(|error| rejected(&error))?;
            value(&answer)
        }
    }

    /// One capability, by its id and JSON request, on the executor this
    /// build's manifest names; answers the DS command envelope as JSON text,
    /// the same envelope `ds <command> --output json` prints. It never
    /// rejects: every refusal is an error envelope.
    #[wasm_bindgen(js_name = invoke, unchecked_return_type = "Promise<string>")]
    pub fn invoke(
        #[wasm_bindgen(unchecked_param_type = "CapabilityId | string")] capability_id: String,
        request_json: String,
        #[wasm_bindgen(unchecked_param_type = "InvokeHost | undefined")] host: Option<Object>,
    ) -> Promise {
        let host = Host {
            host: host.map_or(JsValue::UNDEFINED, JsValue::from),
        };
        wasm_bindgen_futures::future_to_promise(async move {
            let text = super::invoke_with(
                &host,
                crate::capabilities::PLATFORM,
                &capability_id,
                &request_json,
            )
            .await;
            Ok(JsValue::from_str(&text))
        })
    }
}

#[cfg(test)]
#[path = "capability_invoke_tests.rs"]
mod tests;
