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
    DEVICE_AUTH_TRANSIENT_REFUSAL, ProviderIdentity, TransformerInventory, TransformerKind,
    TransformerLifecycle, TransformerSet,
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

pub(super) const INPUTS_NOT_HELD: Refusal = Refusal {
    code: "report_inputs_not_held",
    when: "the service is unreachable and no print inputs are held for the project",
    remedy: "export the project once while connected",
};
pub(super) const ROOM_NOT_HELD: Refusal = Refusal {
    code: "report_room_not_held",
    when: "the service is unreachable and a room is unheld or older than its head (batch row)",
    remedy: "export that transformer once while connected",
};

/// Whether the service answered this run. Once one read found it
/// unreachable, every later read answers from the hold at once instead of
/// spending the weak-network schedule again.
#[derive(Default)]
pub(super) struct Link {
    unreachable: Option<String>,
}

impl Link {
    /// The service's answer, or `None` when it could not be reached (now or
    /// earlier in this run). Any other refusal is returned as it came.
    pub(super) fn read<T>(
        &mut self,
        read: impl FnOnce() -> Result<T, Failure>,
    ) -> Result<Option<T>, Failure> {
        if self.unreachable.is_some() {
            return Ok(None);
        }
        match read() {
            Ok(value) => Ok(Some(value)),
            Err(failure) if UNREACHABLE.contains(&failure.code()) => {
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
    /// Planned reads: name → the kernel's reason.
    planned: BTreeMap<String, &'static str>,
    /// `read`, `unread` (no link), or `refused: <why>`.
    heads: String,
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
        let plan = pinned_context::plan(request).map_err(refused)?;
        Ok(Self {
            hold,
            planned: plan
                .fetch
                .iter()
                .map(|item| (item.name.clone(), item.reason))
                .collect(),
            heads: read,
            sources: BTreeMap::new(),
            not_held: Vec::new(),
            unkept: Vec::new(),
        })
    }

    /// One transformer's room: the held copy the plan reuses, or the
    /// service's, held as it is read. `read` is never called for a room the
    /// plan reuses and this machine can still read.
    pub(super) fn room(
        &mut self,
        name: &str,
        link: &mut Link,
        read: impl FnOnce() -> Result<Room, Failure>,
    ) -> Result<Room, Failure> {
        if !self.planned.contains_key(name)
            && let Ok(Some(room)) = room_hold::room(&self.hold.root, &self.hold.scope, name)
        {
            self.sources.insert(name.to_owned(), "held");
            return Ok(room);
        }
        match link.read(read)? {
            Some(room) => {
                if room_hold::hold_room(&self.hold.root, &self.hold.scope, &room).is_err() {
                    self.unkept.push(name.to_owned());
                }
                self.sources.insert(name.to_owned(), "fetched");
                Ok(room)
            }
            None => {
                self.not_held.push(name.to_owned());
                let held = if self.planned.get(name) == Some(&"head_moved") {
                    "holds an older revision of"
                } else {
                    "does not hold"
                };
                Err(Failure::unavailable(
                    ROOM_NOT_HELD.code,
                    format!(
                        "this machine {held} the room of {name}, and the service could not be reached ({})",
                        link.unreachable().unwrap_or("unreachable"),
                    ),
                )
                .remedy(ROOM_NOT_HELD.remedy))
            }
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
mod tests {
    use super::*;

    fn room(name: &str, version: i64) -> Room {
        Room {
            transformer: name.into(),
            version: Some(version),
            content_digest: None,
            layers: BTreeMap::from([(
                "tr".to_string(),
                json!({"type": "FeatureCollection", "features": [{"type": "Feature"}]}),
            )]),
        }
    }

    fn unreachable() -> Failure {
        Failure::unavailable(
            "device_auth_transient",
            "the fixed DS device authorization endpoint is temporarily unreachable",
        )
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    /// The acceptance: once every room is held and the heads have not moved,
    /// a run reads no room from the service at all.
    #[test]
    fn a_warm_run_fetches_no_room() {
        let dir = tempfile::tempdir().unwrap();
        let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
        let batch = names(&["t1", "t2"]);
        let mut link = Link::default();

        let mut cold = Rooms::plan(hold(), &batch, Heads::Read(Vec::new())).unwrap();
        for name in &batch {
            cold.room(name, &mut link, || Ok(room(name, 3))).unwrap();
        }
        assert_eq!(cold.receipt()["rooms_fetched"], 2);

        let heads = vec![
            json!({"name": "t1", "version": 3}),
            json!({"name": "t2", "version": 3}),
        ];
        let mut warm = Rooms::plan(hold(), &batch, Heads::Read(heads)).unwrap();
        for name in &batch {
            let held = warm
                .room(name, &mut link, || panic!("a warm run read {name}"))
                .unwrap();
            assert_eq!(held, room(name, 3));
            assert_eq!(warm.source(name), Some("held"));
        }
        assert_eq!(warm.receipt()["rooms_fetched"], 0);
        assert_eq!(warm.receipt()["reused"], 2);
    }

    /// A moved head is read again; the service's answer replaces the copy.
    #[test]
    fn a_moved_head_reads_that_room_again_and_holds_it() {
        let dir = tempfile::tempdir().unwrap();
        let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
        let mut link = Link::default();
        let mut cold = Rooms::plan(hold(), &names(&["t1"]), Heads::Unreachable).unwrap();
        cold.room("t1", &mut link, || Ok(room("t1", 3))).unwrap();

        let heads = vec![json!({"name": "t1", "version": 4})];
        let mut moved = Rooms::plan(hold(), &names(&["t1"]), Heads::Read(heads)).unwrap();
        let read = moved.room("t1", &mut link, || Ok(room("t1", 4))).unwrap();
        assert_eq!(read.version, Some(4));
        assert_eq!(moved.receipt()["rooms_fetched"], 1);
        let again = Rooms::plan(
            hold(),
            &names(&["t1"]),
            Heads::Read(vec![json!({"name": "t1", "version": 4})]),
        )
        .unwrap()
        .room("t1", &mut link, || panic!("settled"))
        .unwrap();
        assert_eq!(again.version, Some(4));
    }

    /// A refused heads read proves nothing current: every room is read again,
    /// exactly as before rooms were held.
    #[test]
    fn refused_heads_read_every_room_again() {
        let dir = tempfile::tempdir().unwrap();
        let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
        let mut link = Link::default();
        let mut cold = Rooms::plan(hold(), &names(&["t1"]), Heads::Unreachable).unwrap();
        cold.room("t1", &mut link, || Ok(room("t1", 3))).unwrap();
        let mut refused = Rooms::plan(
            hold(),
            &names(&["t1"]),
            Heads::Refused("auth_response_unreadable: bad".into()),
        )
        .unwrap();
        refused.room("t1", &mut link, || Ok(room("t1", 3))).unwrap();
        let receipt = refused.receipt();
        assert_eq!(receipt["rooms_fetched"], 1);
        assert_eq!(receipt["heads"], "refused: auth_response_unreadable: bad");
    }

    /// With no link, every held room prints and each one that is not held is
    /// named; the service is asked once, never once per room.
    #[test]
    fn with_no_link_held_rooms_print_and_the_rest_are_named() {
        let dir = tempfile::tempdir().unwrap();
        let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
        let mut link = Link::default();
        let mut cold = Rooms::plan(hold(), &names(&["t1"]), Heads::Unreachable).unwrap();
        cold.room("t1", &mut link, || Ok(room("t1", 3))).unwrap();

        let mut offline = Link::default();
        assert!(
            offline
                .read(|| Err::<(), _>(unreachable()))
                .unwrap()
                .is_none()
        );
        let mut rooms =
            Rooms::plan(hold(), &names(&["t1", "t2", "t3"]), Heads::Unreachable).unwrap();
        assert_eq!(
            rooms.room("t1", &mut offline, || panic!("held")).unwrap(),
            room("t1", 3)
        );
        for name in ["t2", "t3"] {
            let failure = rooms
                .room(name, &mut offline, || {
                    panic!("the link is known to be down")
                })
                .unwrap_err();
            assert_eq!(failure.code(), ROOM_NOT_HELD.code);
            assert!(failure.message().contains(name), "{}", failure.message());
            assert!(
                failure
                    .message()
                    .contains(DEVICE_AUTH_TRANSIENT_REFUSAL.code)
            );
        }
        let receipt = rooms.receipt();
        assert_eq!(receipt["reused"], 1);
        assert_eq!(receipt["rooms_fetched"], 0);
        assert_eq!(receipt["not_held"], json!(["t2", "t3"]));
        assert_eq!(receipt["heads"], "unread");
    }

    /// Only an unreachable service falls back to the hold; the service's own
    /// refusal is still the answer.
    #[test]
    fn a_refusal_is_not_an_outage() {
        let mut link = Link::default();
        let refused = link
            .read(|| Err::<(), _>(Failure::unauthorized("headless_signed_out", "signed out")))
            .unwrap_err();
        assert_eq!(refused.code(), "headless_signed_out");
        assert_eq!(link.unreachable(), None);
        assert_eq!(link.read(|| Ok(1)).unwrap(), Some(1));
    }

    #[test]
    fn inputs_are_held_whole_and_refused_by_name_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let hold = Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
        let failure = hold
            .inputs(&format!("{}: down", DEVICE_AUTH_TRANSIENT_REFUSAL.code))
            .unwrap_err();
        assert_eq!(failure.code(), INPUTS_NOT_HELD.code);
        assert!(
            failure
                .message()
                .contains(DEVICE_AUTH_TRANSIENT_REFUSAL.code)
        );
        let inputs = Inputs {
            rows: vec![Row {
                name: "t1".into(),
                kind: "transformer".into(),
                state: "active".into(),
                retired: false,
                reason: None,
            }],
            configuration: json!({"network_reporter_input_receipt": {"schema": "x"}}),
            setups: vec![json!({"id": "a3", "revision": 2, "layout": {}})],
            read_at: "2026-09-27T05:00:00Z".into(),
        };
        hold.hold_inputs(&inputs).unwrap();
        assert_eq!(hold.inputs("unused").unwrap(), inputs);
    }

    #[test]
    fn a_requested_name_the_project_lacks_reads_missing() {
        let rows = vec![Row {
            name: "t1".into(),
            kind: "transformer".into(),
            state: "active".into(),
            retired: false,
            reason: None,
        }];
        let requested = TransformerSet::new(["t2".to_string(), "t1".to_string()]).unwrap();
        let selected = select(&rows, &requested);
        let states: Vec<(&str, &str)> = selected
            .iter()
            .map(|row| (row.name.as_str(), row.state.as_str()))
            .collect();
        assert!(states.contains(&("t1", "active")));
        assert!(states.contains(&("t2", "missing")));
        assert_eq!(select(&rows, &TransformerSet::default()), rows);
    }
}
