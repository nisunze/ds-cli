//! What `report project export` prints from: this machine's copy of the
//! project's print inputs and transformer rooms, refreshed from the service
//! where it answers.
//!
//! The Server is offline first. Every input is read from the service when it
//! answers and then held (`ds_project_data::room_hold`); when the service
//! cannot be reached, the held copy is the input and the receipt says so.
//! Which rooms are read again is the kernel's decision
//! (`ds_command_kernel::pinned_context::plan`): a held room is reused, and
//! only a missing room or one whose head revision moved is read — so a warm
//! run reads no room, and a run with no link prints every room it holds and
//! names the ones it does not.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ds_cli_auth::{
    DEVICE_AUTH_TRANSIENT_REFUSAL, ProviderIdentity, TRANSFORMER_READ_FAILED, TransformerInventory,
    TransformerKind, TransformerLifecycle, TransformerSet, WeakNetwork,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Refusal;
use ds_command_kernel::pinned_context;
use ds_command_kernel::project_dataset_cache::Scope;
use ds_project_data::room_hold::{self, Room};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// What a read answers when the service could not be reached at all — the
/// device authorization endpoint or the service itself. Any other refusal is
/// the service's own answer and ends the run as it always did.
const UNREACHABLE: [&str; 2] = [
    DEVICE_AUTH_TRANSIENT_REFUSAL.code,
    super::AUTH_TRANSIENT.code,
];

/// Whether `failure` says the service could not be reached. The transformer
/// context route shares the transient code when it answers that it could
/// not read one transformer; that is a deterministic answer about one room,
/// never evidence of an outage.
fn unreachable(failure: &Failure) -> bool {
    UNREACHABLE.contains(&failure.code())
        && failure
            .detail_value()
            .and_then(|detail| detail.get("service_code"))
            .and_then(Value::as_str)
            != Some(TRANSFORMER_READ_FAILED)
}

pub(super) const INPUTS_NOT_HELD: Refusal = Refusal {
    code: "report_inputs_not_held",
    when: "the service is unreachable and no print inputs are held for the project",
    remedy: "export the project once while connected",
};
pub(super) const ROOM_NOT_HELD: Refusal = Refusal {
    code: "report_room_not_held",
    when: "the service is unreachable and a room is unheld, older than its head, or its held saved content digest does not match its layers (batch row)",
    remedy: "export that transformer once while connected",
};

/// Whether the service answered this run. Every read is asked under the
/// shared weak-network curve first; once one read still found the service
/// unreachable after it, every later read answers from the hold at once
/// instead of spending the curve again.
pub(super) struct Link {
    schedule: WeakNetwork,
    unreachable: Option<String>,
}

impl Default for Link {
    fn default() -> Self {
        Self::with_schedule(WeakNetwork::FIELD)
    }
}

impl Link {
    pub(super) fn with_schedule(schedule: WeakNetwork) -> Self {
        Self {
            schedule,
            unreachable: None,
        }
    }

    /// The service's answer, or `None` when it could not be reached (now,
    /// after the whole retry curve, or earlier in this run). Any other
    /// refusal is returned as it came — including the service's own answer
    /// that it could not read one transformer, which says it WAS reached.
    pub(super) fn read<T>(
        &mut self,
        read: impl FnMut() -> Result<T, Failure>,
    ) -> Result<Option<T>, Failure> {
        if self.unreachable.is_some() {
            return Ok(None);
        }
        match super::export::with_weak_network(&self.schedule, read) {
            Ok(value) => Ok(Some(value)),
            Err(failure) if unreachable(&failure) => {
                self.unreachable = Some(format!("{}: {}", failure.code(), failure.message()));
                Ok(None)
            }
            Err(failure) => Err(failure),
        }
    }

    /// Why the service was not used, when it was not.
    pub(super) fn unreachable(&self) -> Option<&str> {
        self.unreachable.as_deref()
    }
}

/// One inventory row as a batch reads it, and as it is held.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub name: String,
    /// `transformer` or `project_level`.
    pub kind: String,
    /// The lifecycle token: `active`, `retired`, `deleted` or `missing`.
    pub state: String,
    /// True when ds-brain keeps a retirement record, whose reason may be absent.
    #[serde(default)]
    pub retired: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Row {
    pub(super) fn is_transformer(&self) -> bool {
        self.kind == TransformerKind::Transformer.token()
    }
    pub(super) fn is_active(&self) -> bool {
        self.state == TransformerLifecycle::Active.token()
    }
}

/// The service's inventory, as rows.
pub fn rows(inventory: &TransformerInventory) -> Vec<Row> {
    inventory
        .rows()
        .iter()
        .map(|row| Row {
            name: row.name().to_owned(),
            kind: row.kind().token().to_owned(),
            state: row.lifecycle().token().to_owned(),
            retired: row.retirement().is_some(),
            reason: row
                .retirement()
                .and_then(|record| record.reason())
                .map(str::to_owned),
        })
        .collect()
}

/// The rows a batch over `requested` reads: the whole project, or one row per
/// requested name in the order given, where a name the project does not have
/// reads `missing` exactly as the service answers it.
pub(super) fn select(rows: &[Row], requested: &TransformerSet) -> Vec<Row> {
    if requested.is_empty() {
        return rows.to_vec();
    }
    requested
        .names()
        .iter()
        .map(|name| {
            rows.iter()
                .find(|row| &row.name == name)
                .cloned()
                .unwrap_or_else(|| Row {
                    name: name.clone(),
                    kind: TransformerKind::Transformer.token().to_owned(),
                    state: TransformerLifecycle::Missing.token().to_owned(),
                    retired: false,
                    reason: None,
                })
        })
        .collect()
}

/// The project-level inputs a batch prints with besides its rooms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_crs: Option<ds_command_kernel::printing::project_crs::Capture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printing_style_capture: Option<ds_command_kernel::printing::style_capture::Capture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transformer_natures: Option<ds_command_kernel::report_export::held::TransformerNatures>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub saved_analyses: BTreeMap<String, ds_command_kernel::report_export::held::SavedAnalysis>,
    /// The whole project's lifecycle inventory.
    pub rows: Vec<Row>,
    /// The project configuration's Network Reporter receipt members.
    pub configuration: Value,
    /// The printing setups the output selection names, as read.
    pub setups: Vec<Value>,
    /// When this copy was read from the service.
    pub read_at: String,
}

/// This machine's copy for one account and project.
#[derive(Clone)]
pub(super) struct Hold {
    root: PathBuf,
    scope: Scope,
}

impl Hold {
    /// The copy under the layer root, beside the survey hold and the dataset
    /// rooms, for the identity observed without the network.
    pub(super) fn open(identity: &ProviderIdentity, project: &str) -> Result<Self, Failure> {
        let root = ds_layer_store::default_root().map_err(|error| {
            Failure::failed(INPUTS_NOT_HELD.code, error).remedy(INPUTS_NOT_HELD.remedy)
        })?;
        Ok(Self::at(root, identity.uid(), project))
    }

    pub(super) fn at(root: PathBuf, principal: &str, project: &str) -> Self {
        Self {
            root,
            scope: Scope {
                principal: principal.to_owned(),
                project: project.to_owned(),
            },
        }
    }

    /// Reuse one checksum-verified saved room for print-only project context.
    /// The same scoped hold feeds the normal report room plan.
    pub(super) fn room(&self, transformer: &str) -> Result<Option<Room>, String> {
        room_hold::room(&self.root, &self.scope, transformer)
    }

    pub(super) fn hold_room(&self, room: &Room) -> Result<(), String> {
        room_hold::hold_room(&self.root, &self.scope, room)
    }

    /// The inputs as last read from the service, or the refusal that names
    /// why neither the service nor this machine can supply them.
    pub(super) fn inputs(&self, unreachable: &str) -> Result<Inputs, Failure> {
        let not_held = |detail: String| {
            Failure::unavailable(
                INPUTS_NOT_HELD.code,
                format!("the service could not be reached ({unreachable}) and {detail}"),
            )
            .remedy(INPUTS_NOT_HELD.remedy)
        };
        let document = room_hold::inputs(&self.root, &self.scope)
            .map_err(|error| not_held(format!("the held copy is unusable: {error}")))?
            .ok_or_else(|| not_held("this machine holds no print inputs for the project".into()))?;
        serde_json::from_value(document)
            .map_err(|error| not_held(format!("the held copy is unusable: {error}")))
    }

    /// Keep the inputs just read. A copy that cannot be kept never stops a
    /// print; the receipt names it.
    pub(super) fn hold_inputs(&self, inputs: &Inputs) -> Result<(), String> {
        let document = serde_json::to_value(inputs).map_err(|error| error.to_string())?;
        room_hold::hold_inputs(&self.root, &self.scope, &document)
    }
}

/// Metadata-only inventory of this principal's complete saved rooms. Consumers
/// use this owner instead of inventing a second native pin cache.
pub fn transformer_room_inventory(
    identity: &ProviderIdentity,
    project: &str,
) -> Result<Vec<Value>, Failure> {
    let hold = Hold::open(identity, project)?;
    room_hold::held(&hold.root, &hold.scope).map_err(|error| {
        Failure::failed(INPUTS_NOT_HELD.code, error).remedy(INPUTS_NOT_HELD.remedy)
    })
}

/// Acquire through the same saved-room owner project exports use. `heads` is
/// the consumer's existing authenticated status result; there is no new probe.
pub fn acquire_transformer_room(
    lane: &str,
    project: &str,
    identity: &ProviderIdentity,
    name: &str,
    heads: &[Value],
    force: bool,
) -> Result<(Room, &'static str), Failure> {
    let hold = Hold::open(identity, project)?;
    let mut rooms = Rooms::plan(hold, &[name.to_owned()], Heads::Read(heads.to_vec()))?;
    rooms.force = force;
    let mut link = Link::default();
    let room = rooms.room(name, &mut link, || {
        let context = ds_cli_auth::saved_transformer_context_for_project(lane, project, name)?;
        if context.identity().uid() != identity.uid() {
            return Err(Failure::conflict(
                ROOM_NOT_HELD.code,
                "account changed during transformer acquisition",
            ));
        }
        let snapshot = context.snapshot();
        Ok(Room {
            transformer: snapshot.transformer_name().to_owned(),
            version: snapshot
                .metadata()
                .version()
                .and_then(|value| i64::try_from(value).ok()),
            content_digest: snapshot.metadata().content_digest().map(str::to_owned),
            layers: snapshot.layers().clone(),
        })
    })?;
    if !rooms.unkept.is_empty() {
        return Err(Failure::failed(
            INPUTS_NOT_HELD.code,
            "the transformer was read but its complete local room could not be retained",
        ));
    }
    let source = rooms.source(name).expect("successful room has a source");
    Ok((room, source))
}

/// What one batched room acquisition used and could not supply.
#[derive(Debug)]
pub struct RoomsAcquisition {
    /// The plan's receipt: rooms reused and fetched, the heads read, and the
    /// rooms neither this machine nor the service could supply.
    pub receipt: Value,
    /// Per-room failures, in request order. One unreadable room never ends
    /// the batch.
    pub failures: Vec<(String, Failure)>,
    /// Why the service was not used, when it was not.
    pub unreachable: Option<String>,
}

/// Acquire many complete saved rooms through ONE plan and ONE weak-network
/// link, so a project-wide consumer spends the retry curve at most once: after
/// an outage latches, every later room answers from this machine's hold at
/// once. `heads` is the consumer's existing status result; `None` reads no
/// service at all and uses only rooms already held (an explicit held read, or
/// a service already found unreachable). Each room is handed to `visit` as it
/// is acquired, so the caller never holds the whole project in memory.
pub fn acquire_transformer_rooms(
    lane: &str,
    project: &str,
    identity: &ProviderIdentity,
    names: &[String],
    heads: Option<&[Value]>,
    visit: impl FnMut(Room, &'static str) -> Result<(), Failure>,
) -> Result<RoomsAcquisition, Failure> {
    let hold = Hold::open(identity, project)?;
    let read = |name: &str| {
        let context = ds_cli_auth::saved_transformer_context_for_project(lane, project, name)?;
        if context.identity().uid() != identity.uid() {
            return Err(Failure::conflict(
                ROOM_NOT_HELD.code,
                "account changed during transformer acquisition",
            ));
        }
        let snapshot = context.snapshot();
        Ok(Room {
            transformer: snapshot.transformer_name().to_owned(),
            version: snapshot
                .metadata()
                .version()
                .and_then(|value| i64::try_from(value).ok()),
            content_digest: snapshot.metadata().content_digest().map(str::to_owned),
            layers: snapshot.layers().clone(),
        })
    };
    acquire_rooms(hold, names, heads, Link::default(), read, visit)
}

fn acquire_rooms(
    hold: Hold,
    names: &[String],
    heads: Option<&[Value]>,
    mut link: Link,
    mut read: impl FnMut(&str) -> Result<Room, Failure>,
    mut visit: impl FnMut(Room, &'static str) -> Result<(), Failure>,
) -> Result<RoomsAcquisition, Failure> {
    let plan_heads = match heads {
        Some(rows) => Heads::Read(rows.to_vec()),
        None => Heads::Unreachable,
    };
    let mut rooms = Rooms::plan(hold, names, plan_heads)?;
    if heads.is_none() {
        link.unreachable = Some("held read: no service request is made".into());
    }
    let mut failures = Vec::new();
    for name in names {
        match rooms.room(name, &mut link, || read(name)) {
            Ok(room) => {
                let source = rooms.source(name).expect("acquired room has a source");
                visit(room, source)?;
            }
            Err(failure) => failures.push((name.clone(), failure)),
        }
    }
    Ok(RoomsAcquisition {
        receipt: rooms.receipt(),
        failures,
        unreachable: link.unreachable().map(str::to_owned),
    })
}

/// Read only the governed nature projection needed by an authored view. This
/// metadata request is batched; it never re-downloads transformer geometry.
pub fn transformer_nature_view(
    lane: &str,
    project: &str,
    identity: &ProviderIdentity,
    names: &[String],
) -> Result<Option<BTreeMap<String, String>>, Failure> {
    let command = ds_cli_auth::DesignTagsCommand::Projection {
        transformers: names.to_vec(),
        definitions: vec!["transformer_nature".into()],
    };
    let projection = match ds_cli_auth::design_tags(lane, project, &command) {
        Ok(result) => result,
        Err(error) if error.code() == ds_cli_auth::TAG_DEFINITION_UNKNOWN_REFUSAL.code => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    if projection.identity() != identity || projection.project_id() != project {
        return Err(Failure::conflict(
            "project_context_stale",
            "the nature projection belongs to another project or account",
        ));
    }
    let document = projection.result()["document"].as_str().ok_or_else(|| {
        Failure::invalid(
            "report_inputs_invalid",
            "the nature projection has no document",
        )
    })?;
    ds_command_kernel::report_export::held::transformer_natures_from_projection(
        document, project, names,
    )
    .map(Some)
    .map_err(|error| Failure::invalid("report_inputs_invalid", error))
}

/// What the service said about the head revisions of a batch's rooms.
pub(super) enum Heads {
    /// Each room's head revision, as the service reported it.
    Read(Vec<Value>),
    /// The service could not be reached. An unknown head is no evidence that
    /// a held room moved, so every held room is reused.
    Unreachable,
    /// The service refused the read. Nothing says a held room is current, so
    /// every room is read again, as before rooms were held.
    Refused(String),
}

/// The rooms one batch prints from: the kernel's plan over what is held and
/// the heads the service reported, and where each room actually came from.
pub(super) struct Rooms {
    hold: Hold,
    /// `read`, `unread` (no link), or `refused: <why>`.
    heads: String,
    observed_heads: Vec<Value>,
    force: bool,
    sources: BTreeMap<String, &'static str>,
    not_held: Vec<String>,
    unkept: Vec<String>,
}

impl Rooms {
    /// Plan the batch's rooms over what this machine holds and what the
    /// service said about their heads.
    pub(super) fn plan(hold: Hold, names: &[String], heads: Heads) -> Result<Self, Failure> {
        let refused = |error: String| {
            Failure::failed(
                INPUTS_NOT_HELD.code,
                format!("the room plan was refused: {error}"),
            )
            .remedy(INPUTS_NOT_HELD.remedy)
        };
        let held = room_hold::held(&hold.root, &hold.scope).map_err(refused)?;
        let (force, read, heads) = match heads {
            Heads::Read(rows) => (false, "read".to_owned(), rows),
            Heads::Unreachable => (false, "unread".to_owned(), Vec::new()),
            Heads::Refused(reason) => (true, format!("refused: {reason}"), Vec::new()),
        };
        let request = serde_json::from_value::<pinned_context::PlanRequest>(json!({
            "schema": pinned_context::SCHEMA,
            "pins": names,
            "force": force,
            "held": held,
            "heads": heads,
        }))
        .map_err(|error| refused(error.to_string()))?;
        pinned_context::plan(request).map_err(refused)?;
        Ok(Self {
            hold,
            heads: read,
            observed_heads: heads,
            force,
            sources: BTreeMap::new(),
            not_held: Vec::new(),
            unkept: Vec::new(),
        })
    }

    /// One transformer's room: the held copy the plan reuses, or the
    /// service's, held as it is read. `read` is never called for a room the
    /// plan reuses and this machine can still read; it is asked again under
    /// the link's retry curve while the service blinks.
    pub(super) fn room(
        &mut self,
        name: &str,
        link: &mut Link,
        read: impl FnMut() -> Result<Room, Failure>,
    ) -> Result<Room, Failure> {
        use room_hold::{AcquisitionError as Error, RoomSource};
        match room_hold::acquire(&self.hold.root, &self.hold.scope, name,
            &self.observed_heads, self.force, || link.read(read)) {
            Ok(acquired) => {
                if acquired.persistence_error.is_some() { self.unkept.push(name.to_owned()); }
                self.sources.insert(name.to_owned(), match acquired.source {
                    RoomSource::Held => "held", RoomSource::Fetched => "fetched",
                });
                Ok(acquired.room)
            }
            Err(Error::Read(error)) => Err(error),
            Err(Error::Unavailable) => {
                self.not_held.push(name.to_owned());
                Err(Failure::unavailable(ROOM_NOT_HELD.code, format!(
                    "this machine has no current room with a verified saved content digest for {name}, and the service could not be reached ({})",
                    link.unreachable().unwrap_or("unreachable"),
                )).remedy(ROOM_NOT_HELD.remedy))
            }
            Err(Error::Invalid(error)) => Err(Failure::invalid(super::export::INPUTS_INVALID.code, format!("{name}: {error}"))
                .remedy(super::export::INPUTS_INVALID.remedy)),
            Err(Error::Superseded) => Err(Failure::conflict(ROOM_NOT_HELD.code,
                format!("the saved room of {name} changed during acquisition; retry with its current head"))
                .remedy(ROOM_NOT_HELD.remedy)),
            Err(Error::Store(error)) => Err(Failure::failed(INPUTS_NOT_HELD.code, error)
                .remedy(INPUTS_NOT_HELD.remedy)),
        }
    }

    /// Where one room came from: `held` or `fetched`.
    pub(super) fn source(&self, name: &str) -> Option<&'static str> {
        self.sources.get(name).copied()
    }

    /// What the batch read: rooms reused from this machine, rooms read from
    /// the service, and the ones neither could supply.
    pub(super) fn receipt(&self) -> Value {
        let count = |source: &str| self.sources.values().filter(|s| **s == source).count();
        let mut receipt = json!({
            "reused": count("held"),
            "rooms_fetched": count("fetched"),
            "not_held": self.not_held,
            "heads": self.heads,
        });
        if !self.unkept.is_empty() {
            receipt["unkept"] = json!(self.unkept);
        }
        receipt
    }
}

/// The head revision a status row carries for one transformer, or null. The
/// kernel reads an unknown revision as absence of evidence, never staleness.
pub(super) fn head_version(row: &Value) -> Value {
    row.get("metadata")
        .and_then(|metadata| metadata.get("version"))
        .and_then(|version| {
            version
                .as_i64()
                .or_else(|| version.as_f64().map(|n| n as i64))
        })
        .map_or(Value::Null, Value::from)
}

#[cfg(test)]
#[path = "hold_tests.rs"]
mod tests;
