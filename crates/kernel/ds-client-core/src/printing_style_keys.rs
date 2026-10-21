//! The governed printing-template migration from legacy `style_refs` to exact
//! `style_keys`, computed here in Core.
//!
//! A plan reads one project library (or one setup in it) and the project's
//! governed style-resolution table through their existing authorized read
//! routes, then lets ds-command-kernel's `style_migration` decide every setup
//! locally. It writes nothing. An apply re-plans the same way, refuses unless
//! the fresh plan still has the reviewed `plan_sha256`, and hands ds-brain
//! exactly the migrated layouts, each fenced by the revision it was planned
//! from; ds-brain saves them in one transaction beside an immutable copy of
//! every setup it replaces. A platform administrator's census reads one page
//! of library ids from ds-brain and plans each library here.
//!
//! ds-brain never computes a plan (owner ruling 2026-10-19: no hybrid
//! compute). Its `/api/v1/printing/style_keys` route only lists libraries and
//! saves a reviewed plan's layouts.
use crate::authorized::Authority;
use crate::profile::ClientProfile;
use crate::transport::{CallAuthority, SecretRequestBody, CORRELATION_HEADERS};
use crate::{AuthorizedTransport, ClientError, ErrorKind, ServiceRefusal, TransportResponse};
use ds_command_kernel::printing::identifier;
use ds_command_kernel::printing::style_migration::{self, LibraryPlan, StoredSetup};
use ds_command_kernel::style_resolution::{Ink, Snapshot, Target};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const PRINT_STYLE_KEYS_PATH: &str = "/api/v1/printing/style_keys";
pub const PRINT_STYLE_KEYS_RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
pub const PRINT_STYLE_KEYS_PLAN_SCHEMA: &str = style_migration::LIBRARY_PLAN_SCHEMA;
pub const PRINT_STYLE_KEYS_RECEIPT_SCHEMA: &str = "ds.print-style-key-migration-receipt/v1";
pub const PRINT_STYLE_KEYS_CENSUS_SCHEMA: &str = "ds.print-style-key-census/v1";
/// ds-brain's answer to a save: the setups it replaced and their copies.
pub const PRINT_STYLE_KEYS_SAVE_SCHEMA: &str = "ds.print-style-key-save/v1";
/// ds-brain's answer to a census page: library ids only.
pub const PRINT_STYLE_KEYS_LIBRARIES_SCHEMA: &str = "ds.print-style-key-libraries/v1";
/// The migrated layout bytes one save carries; past it, apply per setup id.
pub const PRINT_STYLE_KEYS_SAVE_BYTES: usize = 4 * 1024 * 1024;
/// The decider every plan records as provenance (never part of its digest).
pub const PRINT_STYLE_KEYS_DECIDER: &str = concat!(
    "ds-client-core@",
    env!("CARGO_PKG_VERSION"),
    "/style_migration"
);
const PRINT_STYLE_KEYS_TIMEOUT_SECONDS: u64 = 120;
const LAYOUT_LIMIT: usize = 750_000;
const LIBRARY_SETUPS: usize = 200;

/// An owner's exact governed key for a layer the Kernel named, per setup id.
pub type PrintStyleKeyChoices = style_migration::LibraryChoices;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrintStyleKeysRequest {
    /// The Kernel's plan for one library, or one setup in it. Writes nothing.
    Plan {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        choices: PrintStyleKeyChoices,
    },
    /// One page of every project library. Writes nothing; each library
    /// carries the digest its own apply checks.
    Census {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    /// Apply exactly the reviewed plan; ds-brain keeps every replaced setup.
    Apply {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        choices: PrintStyleKeyChoices,
        expected_plan_sha256: String,
    },
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

/// A refusal under one of the codes `ds report layout style-keys` documents.
fn named(
    kind: ErrorKind,
    message: &'static str,
    status: u16,
    code: &str,
    detail: &str,
) -> ClientError {
    ClientError::with_service_refusal(
        kind,
        message,
        ServiceRefusal::new(status, Some(code), Some(detail)),
    )
}

fn valid_setup(id: &Option<String>) -> Result<(), ClientError> {
    match id {
        Some(id) if !identifier(id) => Err(invalid("Invalid print setup id")),
        _ => Ok(()),
    }
}

/// Bounded, and each choice names one exact colour print key; the Kernel
/// decides whether the catalogue declares it.
fn valid_choices(choices: &PrintStyleKeyChoices) -> Result<(), ClientError> {
    let refused = || {
        invalid("Choices name at most 200 setups, each with 1-100 layers and one exact colour print key")
    };
    if choices.len() > 200 {
        return Err(refused());
    }
    for (setup, layers) in choices {
        if !identifier(setup) || layers.is_empty() || layers.len() > 100 {
            return Err(refused());
        }
        for (layer, key) in layers {
            if !identifier(layer)
                || key.target != Target::Print
                || key.ink != Ink::Colour
                || key.validate().is_err()
            {
                return Err(refused());
            }
        }
    }
    Ok(())
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn library(project: &str) -> Result<(), ClientError> {
    if project.is_empty() {
        return Err(invalid(
            "A template migration names one project library; the global printing library is retired",
        ));
    }
    crate::tiles::validate_project(project)
}

impl PrintStyleKeysRequest {
    /// `project` is the library. A census names none.
    pub fn validate_for(&self, project: &str) -> Result<(), ClientError> {
        match self {
            Self::Census { cursor, limit } => {
                if !project.is_empty() {
                    return Err(invalid(
                        "A template census covers every library; it names no project",
                    ));
                }
                if limit.is_some_and(|limit| limit == 0 || limit > 25) {
                    return Err(invalid("A template census page holds 1-25 libraries"));
                }
                if cursor
                    .as_ref()
                    .is_some_and(|cursor| cursor.is_empty() || cursor.len() > 512)
                {
                    return Err(invalid("Invalid template census cursor"));
                }
                Ok(())
            }
            Self::Plan { id, choices } => {
                library(project)?;
                valid_setup(id)?;
                valid_choices(choices)
            }
            Self::Apply {
                id,
                choices,
                expected_plan_sha256,
            } => {
                library(project)?;
                valid_setup(id)?;
                valid_choices(choices)?;
                if !digest(expected_plan_sha256) {
                    return Err(invalid("Apply names the reviewed plan's exact sha256"));
                }
                Ok(())
            }
        }
    }
}

/// One authorized call to ds-brain's save/libraries route. A transport can
/// inspect it but cannot redirect it.
pub struct PrintStyleKeysCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) body: Vec<u8>,
    pub(crate) project: &'a str,
    pub(crate) authority: CallAuthority<'a>,
}

impl PrintStyleKeysCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        PRINT_STYLE_KEYS_PATH
    }
    pub const fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        PRINT_STYLE_KEYS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        PRINT_STYLE_KEYS_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn ds_project(&self) -> &str {
        self.project
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
    }
    pub fn canonical_uid(&self) -> &str {
        self.authority.canonical_uid
    }
    pub fn device_id(&self) -> Option<&str> {
        self.authority.device_id
    }
    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody::new(self.body.clone())
    }
}

pub(crate) fn call<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    request: &PrintStyleKeysRequest,
) -> Result<Value, ClientError> {
    request.validate_for(project)?;
    let mut core = Core {
        profile,
        transport,
        authority,
    };
    match request {
        PrintStyleKeysRequest::Plan { id, choices } => {
            let plan = core.plan(project, id.as_deref(), choices)?;
            Ok(serde_json::to_value(&plan).expect("closed template migration plan"))
        }
        PrintStyleKeysRequest::Apply {
            id,
            choices,
            expected_plan_sha256,
        } => core.apply(project, id.as_deref(), choices, expected_plan_sha256),
        PrintStyleKeysRequest::Census { cursor, limit } => core.census(cursor.as_deref(), *limit),
    }
}

struct Core<'p, 'a, T> {
    profile: &'p ClientProfile,
    transport: &'p mut T,
    authority: Authority<'a>,
}

impl<T: AuthorizedTransport> Core<'_, '_, T> {
    /// The project's governed key → document declarations, from the same
    /// resolution table every print resolves against.
    fn catalogue(&mut self, project: &str) -> Result<style_migration::Catalogue, ClientError> {
        let table = crate::styles::governance(
            self.profile,
            self.transport,
            self.authority,
            project,
            &ds_command_kernel::style_governance::Command::Table,
        )
        .map_err(unseeded)?;
        let snapshot: Snapshot = serde_json::from_value(table).map_err(|_| {
            ClientError::new(
                ErrorKind::UnreadableResponse,
                "Invalid governed style table",
            )
        })?;
        Ok(style_migration::Catalogue::of(&snapshot))
    }

    /// One setup's stored identity and raw layout through the printing
    /// library route; the Kernel decides whether the layout is readable.
    fn setup(&mut self, project: &str, id: &str) -> Result<StoredSetup, ClientError> {
        let request = crate::PrintingRequest::Get { id: id.into() };
        let response = self.transport.printing(crate::PrintingCall {
            profile: self.profile,
            ds_project: project,
            operation: &request,
            authority: self.authority.call(),
        })?;
        let unreadable = || {
            ClientError::new(
                ErrorKind::UnreadableResponse,
                "Invalid printing setup receipt",
            )
        };
        if response.status != 200 {
            // The library route's own refusal classes (not found, denied…).
            return Err(crate::printing::decode(response, &request)
                .err()
                .unwrap_or_else(unreadable));
        }
        if response.body.len() > crate::PRINTING_RESPONSE_LIMIT {
            return Err(unreadable());
        }
        let value: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
        let data = &value["data"];
        let revision = data["revision"].as_str().unwrap_or_default();
        if value["success"] != true || data["id"] != id || !digest(revision) {
            return Err(unreadable());
        }
        Ok(StoredSetup {
            id: id.into(),
            name: data["name"].as_str().unwrap_or_default().into(),
            revision: revision.into(),
            layout: data["layout"].clone(),
        })
    }

    fn setups(&mut self, project: &str, id: Option<&str>) -> Result<Vec<StoredSetup>, ClientError> {
        let ids = match id {
            Some(id) => vec![id.to_owned()],
            None => {
                let listed = crate::authorized::printing(
                    self.profile,
                    self.transport,
                    self.authority,
                    project,
                    &crate::PrintingRequest::List {},
                )?;
                let mut ids: Vec<String> = listed["setups"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|row| row["id"].as_str().map(str::to_owned))
                    .collect();
                ids.sort();
                ids.dedup();
                ids
            }
        };
        if ids.len() > LIBRARY_SETUPS {
            return Err(invalid("A printing library holds at most 200 setups"));
        }
        ids.iter().map(|id| self.setup(project, id)).collect()
    }

    /// Governed reads here; the decision is ds-command-kernel's
    /// `style_migration::plan_library`, the same one a WASM host runs.
    fn plan(
        &mut self,
        project: &str,
        id: Option<&str>,
        choices: &PrintStyleKeyChoices,
    ) -> Result<LibraryPlan, ClientError> {
        let catalogue = self.catalogue(project)?;
        let stored = self.setups(project, id)?;
        style_migration::plan_library(
            project,
            id,
            &catalogue,
            stored,
            choices,
            PRINT_STYLE_KEYS_DECIDER,
        )
        .map_err(|refusal| match refusal.split_once(':') {
            Some(("print_style_keys_choice_unplanned", detail)) => named(
                ErrorKind::InvalidInput,
                "Choices name a setup this plan does not cover",
                400,
                "print_style_keys_choice_unplanned",
                detail.trim(),
            ),
            _ => ClientError::new(
                ErrorKind::UnreadableResponse,
                "The governed style table is not a valid migration catalogue",
            ),
        })
    }

    fn apply(
        &mut self,
        project: &str,
        id: Option<&str>,
        choices: &PrintStyleKeyChoices,
        expected: &str,
    ) -> Result<Value, ClientError> {
        let plan = self.plan(project, id, choices)?;
        if plan.plan_sha256 != expected {
            return Err(named(
                ErrorKind::InvalidInput,
                "The reviewed plan changed; review the new plan before applying",
                409,
                "print_style_keys_plan_changed",
                &format!(
                    "the plan is now {}, not the reviewed {expected}",
                    plan.plan_sha256
                ),
            ));
        }
        let (writes, unchanged, not_migrated) = plan.writes();
        let mut total = 0;
        for write in &writes {
            let bytes = serde_json::to_vec(&write.layout)
                .expect("validated layout")
                .len();
            total += bytes;
            if bytes > LAYOUT_LIMIT || total > PRINT_STYLE_KEYS_SAVE_BYTES {
                return Err(named(
                    ErrorKind::InvalidInput,
                    "The migrated templates exceed one save; apply each setup with its id",
                    400,
                    "print_style_keys_bound",
                    &format!("one save carries at most {PRINT_STYLE_KEYS_SAVE_BYTES} bytes and {LAYOUT_LIMIT} per layout"),
                ));
            }
        }
        let migrated = if writes.is_empty() {
            Value::Array(Vec::new())
        } else {
            let body = json!({
                "action": "save", "scope": "project", "project_id": project,
                "plan_sha256": plan.plan_sha256, "catalogue_revision": plan.catalogue_revision,
                "kernel_version": plan.kernel_version, "writes": writes,
            });
            let saved = self.post(project, &body)?;
            let migrated = &saved["migrated"];
            let answered = migrated.as_array().is_some_and(|rows| {
                rows.len() == writes.len()
                    && rows.iter().zip(&writes).all(|(row, write)| {
                        row["id"] == write.id.as_str()
                            && row["from_revision"] == write.from_revision.as_str()
                            && row["to_revision"].as_str().is_some_and(digest)
                            && row["retained"].is_string()
                    })
            });
            if saved["schema"] != PRINT_STYLE_KEYS_SAVE_SCHEMA
                || saved["plan_sha256"] != plan.plan_sha256
                || !answered
            {
                return Err(ClientError::new(
                    ErrorKind::UnreadableResponse,
                    "Invalid template migration receipt",
                ));
            }
            migrated.clone()
        };
        Ok(json!({
            "schema": PRINT_STYLE_KEYS_RECEIPT_SCHEMA, "scope": plan.scope, "project_id": plan.project_id,
            "plan_sha256": plan.plan_sha256, "catalogue_revision": plan.catalogue_revision,
            "kernel_version": plan.kernel_version, "migrated": migrated,
            "unchanged": unchanged, "not_migrated": not_migrated,
        }))
    }

    /// One page of library ids from ds-brain, each planned here. A library
    /// this identity cannot read is named and the census goes on; a
    /// transient failure stops the page so it can be retried whole.
    fn census(&mut self, cursor: Option<&str>, limit: Option<u32>) -> Result<Value, ClientError> {
        let mut body = json!({"action": "libraries"});
        if let Some(cursor) = cursor {
            body["cursor"] = json!(cursor);
        }
        if let Some(limit) = limit {
            body["limit"] = json!(limit);
        }
        let page = self.post("", &body)?;
        let libraries: Option<Vec<String>> = page["libraries"].as_array().and_then(|rows| {
            rows.iter()
                .map(|row| row.as_str().map(str::to_owned))
                .collect()
        });
        let readable = page["schema"] == PRINT_STYLE_KEYS_LIBRARIES_SCHEMA
            && page["total_libraries"].is_u64()
            && libraries.as_ref().is_some_and(|rows| {
                rows.len() <= 25 && rows.iter().all(|row| library(row).is_ok())
            });
        let Some(libraries) = libraries.filter(|_| readable) else {
            return Err(ClientError::new(
                ErrorKind::UnreadableResponse,
                "Invalid template census page",
            ));
        };
        let mut planned = Vec::new();
        for project in libraries {
            match self.plan(&project, None, &PrintStyleKeyChoices::new()) {
                Ok(plan) => planned.push(json!({
                    "scope": "project", "project_id": project,
                    "catalogue_revision": plan.catalogue_revision,
                    "plan_sha256": plan.plan_sha256, "counts": plan.counts,
                    "setups": plan.census_setups(),
                })),
                Err(error) if error.kind() == ErrorKind::Transient => return Err(error),
                Err(error) => planned.push(json!({
                    "scope": "project", "project_id": project,
                    "error": {"code": "library_unreadable", "cause": cause(&error)},
                })),
            }
        }
        let mut census = json!({
            "schema": PRINT_STYLE_KEYS_CENSUS_SCHEMA, "kernel_version": PRINT_STYLE_KEYS_DECIDER,
            "total_libraries": page["total_libraries"], "libraries": planned,
        });
        if let Some(next) = page["next_cursor"].as_str().filter(|next| !next.is_empty()) {
            census["next_cursor"] = json!(next);
        }
        Ok(census)
    }

    /// One call to ds-brain's save/libraries route; answers its `data`.
    fn post(&mut self, project: &str, body: &Value) -> Result<Value, ClientError> {
        let response = self.transport.print_style_keys(PrintStyleKeysCall {
            profile: self.profile,
            body: serde_json::to_vec(body).expect("closed template migration request"),
            project,
            authority: self.authority.call(),
        })?;
        decode(response)
    }
}

fn cause(error: &ClientError) -> String {
    error
        .service_refusal()
        .and_then(ServiceRefusal::code)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{:?}", error.kind()).to_lowercase())
}

/// The governed style table's own "not seeded" answers, under the code the
/// template migration documents.
fn unseeded(error: ClientError) -> ClientError {
    let refusal = error.service_refusal();
    let said = |needle: &str| {
        refusal.is_some_and(|r| {
            r.code().is_some_and(|code| code.contains(needle))
                || r.message().is_some_and(|message| message.contains(needle))
        })
    };
    if said("style_resolution_unseeded") || said("style_resolution_migration_required") {
        let detail = refusal
            .and_then(ServiceRefusal::message)
            .unwrap_or("the governed style catalogue is not seeded")
            .to_owned();
        return named(
            ErrorKind::InvalidInput,
            "The governed style catalogue is not seeded on this deployment",
            412,
            "print_style_catalogue_unseeded",
            &detail,
        );
    }
    error
}

fn decode(response: TransportResponse) -> Result<Value, ClientError> {
    if response.body.len() > PRINT_STYLE_KEYS_RESPONSE_LIMIT {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Template migration response exceeds limit",
        ));
    }
    let parsed: Option<Value> = serde_json::from_slice(&response.body).ok();
    match (response.status, parsed) {
        (200, Some(value)) if value["success"] == true && value["data"].is_object() => {
            Ok(value["data"].clone())
        }
        (200, _) => Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Invalid template migration response",
        )),
        (status, parsed) => Err(refusal(status, &parsed.unwrap_or(Value::Null))),
    }
}

/// The two sentences a ds-brain released before Core computed template
/// migrations answers to a `save` or `libraries` body. Nothing else ever
/// answers them for a well-formed request from this client.
const PREDATES_CORE_PLANS: [&str; 2] = [
    "Invalid printing template migration request",
    "Unknown template migration action",
];

fn refusal(status: u16, value: &Value) -> ClientError {
    // ds-brain answers its own envelope; a gateway that does not publish this
    // route answers a bare 404, which says nothing about any setup.
    if status == 404 && value.get("success").is_none() {
        return ClientError::new(
            ErrorKind::RouteUnavailable,
            "this deployment does not serve the printing template migration",
        );
    }
    let code = value["error"]["code"].as_str();
    let message = value["error"]["message"]
        .as_str()
        .or_else(|| value["message"].as_str());
    if status == 400
        && code == Some("VALIDATION_FAILED")
        && message.is_some_and(|message| PREDATES_CORE_PLANS.contains(&message))
    {
        return ClientError::new(
            ErrorKind::RouteUnavailable,
            "this lane's ds-brain predates Core-computed template migration saves",
        );
    }
    let envelope = ServiceRefusal::new(status, code, message);
    let (kind, message) = match status {
        400 => (
            ErrorKind::InvalidInput,
            "Template migration request refused",
        ),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "Template migration rejected for this identity",
        ),
        404 => (
            ErrorKind::ResourceNotFound,
            "The printing setup does not exist",
        ),
        409 => (
            ErrorKind::InvalidInput,
            "The reviewed plan or a setup changed; review the plan again",
        ),
        429 | 500..=599 => (
            ErrorKind::Transient,
            "The template migration is temporarily unavailable",
        ),
        _ => (
            ErrorKind::UnreadableResponse,
            "Template migration refused by ds-brain",
        ),
    };
    if envelope.code().is_none() && envelope.message().is_none() {
        return ClientError::new(kind, message);
    }
    ClientError::with_service_refusal(kind, message, envelope)
}

#[cfg(test)]
mod tests;
