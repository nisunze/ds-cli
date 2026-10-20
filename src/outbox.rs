//! The outbox: work a person did on this device that still has to be
//! delivered, held by the same rusqlite code in Core's protected Store and the
//! browser's storage worker. A row is work, never a cache: nothing evicts it,
//! and a queue full to its bound refuses the next row. No importer: rows the
//! browser's IndexedDB kept are not carried over.
//!
//! Each [`Queue`] declares its fence (the account, or one project of it), how
//! many rows one account keeps and the byte bound of one payload, which only
//! that queue's drain reads. A row is one step of a `record` (a message, an
//! entry, a task): the steps of one record leave in the order they were
//! queued, and nothing else is ordered. A row's `size` (the bytes its
//! delivery moves) puts it in the small or the large [`Lane`]; each lane has
//! its own bound on deliveries in flight, so one large upload never holds the
//! near-instant items behind it. A large delivery renews its lease and keeps
//! its resume point with [`Operation::Revise`], so an abandoned one resumes
//! where it stopped.
//!
//! A drain claims a startable record head under a lease, delivers it and
//! settles what it observed: a sent row leaves, a failure a later attempt may
//! fix waits for its time, and a failure no retry fixes is parked with the
//! remedy the person is offered, holding only its own record. A queue with a
//! state machine moves its rows instead through [`Operation::Transition`]:
//! the machine reads the partition's rows and its persist and remove actions
//! apply under each row's version inside the one store transaction. Drains
//! run when the person touches the work or the link returns, never on a
//! timer. The host binds the identity ([`crate::durable_store`]); a
//! signed-out identity holds none.
use crate::durable_store::{Identity, Input, Outcome, Principal};
use crate::execution_context::valid_project;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const STORE: &str = "outbox";
/// A malformed call: an unknown queue, a project, id or token its grammar
/// refuses, a time past its bound.
pub const INVALID: &str = "outbox_invalid";
/// A payload past its queue's bound, or a queue holding as many rows as the
/// account keeps.
pub const FULL: &str = "outbox_full";
/// No signed-in account: queued work belongs to one.
pub const SIGNED_OUT: &str = "outbox_signed_out";
/// An id the queue already holds.
pub const EXISTS: &str = "outbox_exists";
/// A settle or revision without the row's live lease: another drain took it
/// over, or the person discarded it.
pub const LEASE_LOST: &str = "outbox_lease_lost";
/// A discard of a row a drain is delivering now.
pub const LEASED: &str = "outbox_leased";
/// A transition whose rows changed since its machine read them, or that
/// reached past the partition it read: nothing of it applies.
pub const FENCE_CHANGED: &str = "outbox_fence_changed";
const TABLE: &str = "CREATE TABLE IF NOT EXISTS outbox (owner TEXT NOT NULL, queue TEXT NOT NULL, project TEXT NOT NULL, id TEXT NOT NULL, record TEXT NOT NULL, seq INTEGER NOT NULL, lane TEXT NOT NULL CHECK(lane IN ('small','large')), size INTEGER NOT NULL CHECK(size>=0), meta TEXT, payload TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('pending','syncing','failed')), attempts INTEGER NOT NULL CHECK(attempts>=0), next_attempt_at INTEGER NOT NULL CHECK(next_attempt_at>=0 AND next_attempt_at<=9007199254740991), lease TEXT, lease_until INTEGER NOT NULL DEFAULT 0, error TEXT, remedy TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, version INTEGER NOT NULL CHECK(version>0), PRIMARY KEY(owner,queue,id)); CREATE INDEX IF NOT EXISTS outbox_record ON outbox(owner,queue,project,record,seq)";
const MAX_SAFE: u64 = 9_007_199_254_740_991;
const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;
/// The longest id, record or lease token, in bytes.
const MAX_TOKEN_BYTES: usize = 128;
/// The longest lease one claim or renewal takes.
pub const MAX_LEASE_MS: u64 = 10 * 60 * 1000;
/// The largest delivery the small lane carries: anything larger is a large
/// transfer.
pub const SMALL_MAX_BYTES: u64 = MIB as u64;
/// The most rows one enqueue admits, all or none.
pub const MAX_BATCH: usize = 100;
/// The longest error a row keeps; a longer one is cut, never refused.
const MAX_ERROR_BYTES: usize = 2 * KIB;
/// The longest remedy token.
const MAX_REMEDY_BYTES: usize = 64;
/// The largest `meta`, in bytes of its JSON text.
const MAX_META_BYTES: usize = 4 * KIB;

/// Whom a queue's rows belong to besides the bound account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fence {
    Account,
    Project,
}

/// Everything one queue declares.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub fence: Fence,
    /// How many rows of the queue one account keeps, across its projects.
    pub max_rows: usize,
    pub max_payload_bytes: usize,
}

macro_rules! queues {
    ($($(#[$doc:meta])* $variant:ident = $name:literal: $fence:ident, $rows:expr, $bytes:expr;)*) => {
        /// The closed vocabulary of outbox queues.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum Queue {
            $($(#[$doc])* #[serde(rename = $name)] $variant,)*
        }
        impl Queue {
            pub const ALL: &'static [Queue] = &[$(Queue::$variant),*];
            pub fn name(self) -> &'static str {
                match self { $(Queue::$variant => $name,)* }
            }
            pub fn rule(self) -> Rule {
                match self {
                    $(Queue::$variant => Rule { fence: Fence::$fence, max_rows: $rows, max_payload_bytes: $bytes },)*
                }
            }
        }
    };
}

queues! {
    /// A message the person sent while it could not leave.
    MessagingSend = "messaging_send": Account, 100, 64 * KIB;
    /// A Project Work command authored offline, per project.
    ProjectWork = "project_work": Project, 1000, 256 * KIB;
    /// A survey entry step captured on this device, per project.
    Survey = "survey": Project, 10_000, MIB;
}

/// Where a row waits by the size of its delivery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// Near-instant deliveries: at most [`SMALL_MAX_BYTES`].
    Small,
    /// Transfers that take a while and resume where they stopped.
    Large,
}
impl Lane {
    fn of(size: u64) -> Self {
        if size <= SMALL_MAX_BYTES {
            Lane::Small
        } else {
            Lane::Large
        }
    }
    fn name(self) -> &'static str {
        match self {
            Lane::Small => "small",
            Lane::Large => "large",
        }
    }
    /// How many of one account's deliveries of a queue run at once in the lane.
    pub fn in_flight(self) -> i64 {
        match self {
            Lane::Small => 4,
            Lane::Large => 1,
        }
    }
}

/// A row's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Syncing,
    Failed,
}
/// One row to queue.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub id: String,
    /// The record this row is one step of; the row's own id when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    /// The bytes its delivery moves; the payload's length when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// A small object the queue's drain or machine reads without the payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    pub payload: Value,
}

/// What one claimed delivery observed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum Settlement {
    /// Delivered: the row leaves.
    Sent,
    /// Nothing was decided (no link, an unreachable service): pending again
    /// without spending an attempt, due at `next_attempt_at` or at once.
    Deferred {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_attempt_at: Option<u64>,
    },
    /// A failure a later attempt may fix: pending again with one attempt
    /// spent, due at `next_attempt_at`.
    Retry { error: String, next_attempt_at: u64 },
    /// A failure no retry fixes: parked until the person acts on `remedy`
    /// (a token the queue's surface names, such as `edit` or `sign_in`).
    Failed { error: String, remedy: String },
}

/// One transaction's work; the host binds the identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Queue rows, pending and due now: all of them, or none.
    Enqueue {
        queue: Queue,
        rows: Vec<Entry>,
        now: u64,
    },
    /// Lease the oldest startable record head of the lane: a head that is
    /// due, or whose drain abandoned it (a lease past its time). Nothing
    /// while the lane's deliveries in flight are at their bound.
    Claim {
        queue: Queue,
        lane: Lane,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        lease: String,
        lease_ms: u64,
        now: u64,
    },
    /// Under a claimed row's live lease, keep what the drain is about to send
    /// or where its transfer stands (`payload`, `meta`), and renew the lease
    /// for `lease_ms` more.
    Revise {
        queue: Queue,
        id: String,
        lease: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        meta: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lease_ms: Option<u64>,
        now: u64,
    },
    /// Settle a claimed row under its live lease.
    Settle {
        queue: Queue,
        id: String,
        lease: String,
        settlement: Settlement,
        now: u64,
    },
    /// Run the queue's state machine over the partition's rows with `event`
    /// and apply what it decides in this transaction; the answer carries the
    /// effects the caller runs after the commit.
    Transition {
        queue: Queue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        event: Value,
        now: u64,
    },
    /// The queue's rows in the fence (every project when none is named), in
    /// the order they were queued, with their payloads.
    List {
        queue: Queue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
    },
    /// How many rows of each status the account holds, per queue.
    Counts,
    /// The person's remedy: one row, or every parked row of the queue, due
    /// again now. A row in delivery is left to its drain.
    Retry {
        queue: Queue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        now: u64,
    },
    /// The person's remedy: drop one row no drain is delivering. An absent
    /// row is success.
    Discard { queue: Queue, id: String, now: u64 },
}

/// What a queue's state machine reads: the partition's rows without their
/// payloads (as [`Operation::List`] answers them), the event and the time.
pub struct Step<'a> {
    pub now: u64,
    pub rows: &'a [Value],
    pub event: &'a Value,
}

/// A change to one row; what is absent stays. Leaving `syncing` drops the
/// lease; entering it takes `lease` until `lease_until`; entering `failed`
/// names the error and the remedy; leaving it clears both.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempts: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_attempt_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_until: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remedy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

/// What a state machine decides. A row is named with the `version` the
/// machine read; a row that moved since refuses the whole transition.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Persist {
        id: String,
        expected_version: u64,
        set: Box<Patch>,
    },
    Remove {
        id: String,
        expected_version: u64,
    },
    /// What the caller does after the commit (dispatch, wake at a time,
    /// surface a refusal): carried to it unread.
    Effect {
        effect: Value,
    },
}

/// A queue's state machine.
pub type Machine = fn(Step) -> Result<Vec<Action>, String>;

/// The state machine a queue's rows move by, if it has one; a queue without
/// one is drained by claim and settle.
fn machine(queue: Queue) -> Option<Machine> {
    match queue {
        Queue::MessagingSend | Queue::ProjectWork | Queue::Survey => None,
    }
}

fn invalid(message: impl std::fmt::Display) -> String {
    format!("{INVALID}: {message}")
}
fn sql(error: rusqlite::Error) -> String {
    invalid(error)
}
fn account(identity: &Identity) -> Result<(), String> {
    match &identity.principal {
        Principal::Account(_) => Ok(()),
        Principal::Anonymous => Err(format!("{SIGNED_OUT}: sign in to queue work")),
    }
}
fn time(now: u64) -> Result<i64, String> {
    if now > MAX_SAFE {
        return Err(invalid("a time or size exceeds its bound"));
    }
    Ok(now as i64)
}
fn lease_ms(ms: u64) -> Result<i64, String> {
    if ms == 0 || ms > MAX_LEASE_MS {
        return Err(invalid("a lease lasts at most ten minutes"));
    }
    Ok(ms as i64)
}

/// The project column of a row: the project of a project queue, empty for
/// an account queue.
fn column(queue: Queue, project: Option<&str>) -> Result<String, String> {
    match (queue.rule().fence, project) {
        (Fence::Account, None) => Ok(String::new()),
        (Fence::Project, Some(id)) if valid_project(id) => Ok(id.into()),
        (Fence::Account, Some(_)) => Err(invalid(format!("{} names no project", queue.name()))),
        _ => Err(invalid(format!("{} names one valid project", queue.name()))),
    }
}
/// A read filter: one project, every project of a project queue, or the
/// account queue.
fn partition(queue: Queue, project: Option<&str>) -> Result<Option<String>, String> {
    match (queue.rule().fence, project) {
        (Fence::Project, None) => Ok(None),
        _ => column(queue, project).map(Some),
    }
}
fn token(name: &str, value: &str) -> Result<(), String> {
    if !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value.trim() == value
        && !value.chars().any(char::is_control)
    {
        Ok(())
    } else {
        Err(invalid(format!("an outbox {name} is a printable token")))
    }
}
fn payload(queue: Queue, value: &Value) -> Result<String, String> {
    if !value.is_object() {
        return Err(invalid("an outbox payload is an object"));
    }
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    if text.len() > queue.rule().max_payload_bytes {
        return Err(format!(
            "{FULL}: a {} row holds at most {} bytes",
            queue.name(),
            queue.rule().max_payload_bytes
        ));
    }
    Ok(text)
}
fn meta(value: &Value) -> Result<String, String> {
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    if !value.is_object() || text.len() > MAX_META_BYTES {
        return Err(invalid("a row's meta is an object of at most 4 KiB"));
    }
    Ok(text)
}
/// An error as kept: cut at a character boundary, never refused, so a
/// settle never strands a row.
fn error_text(error: &str) -> String {
    let mut end = error.len().min(MAX_ERROR_BYTES);
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    error[..end].to_owned()
}
fn remedy(value: &str) -> Result<(), String> {
    if !value.is_empty()
        && value.len() <= MAX_REMEDY_BYTES
        && value.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        Ok(())
    } else {
        Err(invalid("a remedy is a lowercase token"))
    }
}

const HEADER: &str = "id,project,record,lane,size,meta,status,attempts,next_attempt_at,lease_until,error,remedy,created_at,updated_at,version";
fn header(row: &Row) -> rusqlite::Result<Map<String, Value>> {
    let project: String = row.get(1)?;
    let meta: Option<String> = row.get(5)?;
    let value = json!({
        "id": row.get::<_, String>(0)?,
        "project": (!project.is_empty()).then_some(project),
        "record": row.get::<_, String>(2)?,
        "lane": row.get::<_, String>(3)?,
        "size": row.get::<_, i64>(4)?,
        "meta": meta.and_then(|text| serde_json::from_str::<Value>(&text).ok()).unwrap_or(Value::Null),
        "status": row.get::<_, String>(6)?,
        "attempts": row.get::<_, i64>(7)?,
        "next_attempt_at": row.get::<_, i64>(8)?,
        "lease_until": row.get::<_, i64>(9)?,
        "error": row.get::<_, Option<String>>(10)?,
        "remedy": row.get::<_, Option<String>>(11)?,
        "created_at": row.get::<_, i64>(12)?,
        "updated_at": row.get::<_, i64>(13)?,
        "version": row.get::<_, i64>(14)?,
    });
    Ok(match value {
        Value::Object(map) => map,
        _ => Map::new(),
    })
}
fn full_row(row: &Row) -> rusqlite::Result<Value> {
    let mut map = header(row)?;
    let payload: String = row.get(15)?;
    map.insert(
        "payload".into(),
        serde_json::from_str(&payload).unwrap_or(Value::Null),
    );
    Ok(Value::Object(map))
}
fn rows(
    connection: &Connection,
    owner: &str,
    queue: Queue,
    project: Option<&str>,
    with_payload: bool,
) -> Result<Vec<Value>, String> {
    let columns = if with_payload {
        format!("{HEADER},payload")
    } else {
        HEADER.to_owned()
    };
    let mut statement = connection
        .prepare(&format!("SELECT {columns} FROM outbox WHERE owner=?1 AND queue=?2 AND (?3 IS NULL OR project=?3) ORDER BY seq"))
        .map_err(sql)?;
    let found = statement
        .query_map(params![owner, queue.name(), project], |row| {
            if with_payload {
                full_row(row)
            } else {
                header(row).map(Value::Object)
            }
        })
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    Ok(found)
}
fn one(connection: &Connection, owner: &str, queue: Queue, id: &str) -> Result<Value, String> {
    connection
        .query_row(
            &format!("SELECT {HEADER},payload FROM outbox WHERE owner=?1 AND queue=?2 AND id=?3"),
            params![owner, queue.name(), id],
            full_row,
        )
        .map_err(sql)
}

fn enqueue(
    connection: &Connection,
    owner: &str,
    queue: Queue,
    entries: Vec<Entry>,
    now: i64,
) -> Result<usize, String> {
    if entries.is_empty() || entries.len() > MAX_BATCH {
        return Err(invalid("an enqueue carries 1 to 100 rows"));
    }
    let count = entries.len();
    for entry in entries {
        let project = column(queue, entry.project.as_deref())?;
        token("id", &entry.id)?;
        let record = entry.record.unwrap_or_else(|| entry.id.clone());
        token("record", &record)?;
        let text = payload(queue, &entry.payload)?;
        let size = entry.size.unwrap_or(text.len() as u64);
        time(size)?;
        let meta = entry.meta.as_ref().map(meta).transpose()?;
        let inserted = connection
            .execute(
                "INSERT OR IGNORE INTO outbox(owner,queue,project,id,record,seq,lane,size,meta,payload,status,attempts,next_attempt_at,created_at,updated_at,version) VALUES(?1,?2,?3,?4,?5,(SELECT coalesce(max(seq),0)+1 FROM outbox),?6,?7,?8,?9,'pending',0,?10,?10,?10,1)",
                params![owner, queue.name(), project, entry.id, record, Lane::of(size).name(), size as i64, meta, text, now],
            )
            .map_err(sql)?;
        if inserted == 0 {
            return Err(format!(
                "{EXISTS}: {} already holds {}",
                queue.name(),
                entry.id
            ));
        }
    }
    // Counted once the rows are in: a refusal leaves the transaction unapplied.
    let held: i64 = connection
        .query_row(
            "SELECT count(*) FROM outbox WHERE owner=?1 AND queue=?2",
            params![owner, queue.name()],
            |row| row.get(0),
        )
        .map_err(sql)?;
    if held as usize > queue.rule().max_rows {
        return Err(format!(
            "{FULL}: this account keeps at most {} {} rows",
            queue.rule().max_rows,
            queue.name()
        ));
    }
    Ok(count)
}

/// What a claim found: the row it leased, or why none (the lane at its
/// bound, or the time the next head comes due).
struct Claimed {
    row: Option<Value>,
    busy: bool,
    due_at: Option<i64>,
}

fn claim(
    connection: &Connection,
    owner: &str,
    queue: Queue,
    lane: Lane,
    project: Option<&str>,
    (lease, until): (&str, i64),
    now: i64,
) -> Result<Claimed, String> {
    let running: i64 = connection
        .query_row(
            "SELECT count(*) FROM outbox WHERE owner=?1 AND queue=?2 AND lane=?3 AND status='syncing' AND lease_until>?4",
            params![owner, queue.name(), lane.name(), now],
            |row| row.get(0),
        )
        .map_err(sql)?;
    if running >= lane.in_flight() {
        return Ok(Claimed {
            row: None,
            busy: true,
            due_at: None,
        });
    }
    // Every record's head, whatever its lane or state: a later step of a
    // record waits for the earlier one wherever that one is.
    let mut statement = connection
        .prepare(
            "SELECT id,status,next_attempt_at,lease_until FROM outbox o WHERE owner=?1 AND queue=?2 AND (?3 IS NULL OR project=?3) AND lane=?4 AND seq=(SELECT min(seq) FROM outbox h WHERE h.owner=o.owner AND h.queue=o.queue AND h.project=o.project AND h.record=o.record) ORDER BY seq",
        )
        .map_err(sql)?;
    let heads = statement
        .query_map(params![owner, queue.name(), project, lane.name()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    let mut due_at: Option<i64> = None;
    for (id, status, next, leased_until) in heads {
        let startable = match status.as_str() {
            "pending" if next <= now => true,
            "pending" => {
                due_at = Some(due_at.map_or(next, |due| due.min(next)));
                false
            }
            // A live lease is a delivery in flight; a lapsed one was abandoned.
            "syncing" => leased_until <= now,
            // A parked head holds its record until the person acts.
            _ => false,
        };
        if !startable {
            continue;
        }
        connection
            .execute(
                "UPDATE outbox SET status='syncing',lease=?4,lease_until=?5,updated_at=?6,version=version+1 WHERE owner=?1 AND queue=?2 AND id=?3",
                params![owner, queue.name(), id, lease, until, now],
            )
            .map_err(sql)?;
        return Ok(Claimed {
            row: Some(one(connection, owner, queue, &id)?),
            busy: false,
            due_at: None,
        });
    }
    Ok(Claimed {
        row: None,
        busy: false,
        due_at,
    })
}

/// The row under a live lease, or [`LEASE_LOST`].
fn leased(
    connection: &Connection,
    owner: &str,
    queue: Queue,
    id: &str,
    lease: &str,
    now: i64,
) -> Result<(), String> {
    connection
        .query_row(
            "SELECT 1 FROM outbox WHERE owner=?1 AND queue=?2 AND id=?3 AND status='syncing' AND lease=?4 AND lease_until>?5",
            params![owner, queue.name(), id, lease, now],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(sql)?
        .map(|_| ())
        .ok_or_else(|| format!("{LEASE_LOST}: {id} is no longer this drain's to settle"))
}

fn settle(
    connection: &Connection,
    owner: &str,
    queue: Queue,
    id: &str,
    settlement: Settlement,
    now: i64,
) -> Result<(), String> {
    let name = queue.name();
    match settlement {
        Settlement::Sent => connection.execute(
            "DELETE FROM outbox WHERE owner=?1 AND queue=?2 AND id=?3",
            params![owner, name, id],
        ),
        Settlement::Deferred {
            error,
            next_attempt_at,
        } => {
            let next = next_attempt_at.map(time).transpose()?.unwrap_or(now);
            connection.execute(
                "UPDATE outbox SET status='pending',lease=NULL,lease_until=0,next_attempt_at=?4,error=?5,remedy=NULL,updated_at=?6,version=version+1 WHERE owner=?1 AND queue=?2 AND id=?3",
                params![owner, name, id, next, error.as_deref().map(error_text), now],
            )
        }
        Settlement::Retry {
            error,
            next_attempt_at,
        } => connection.execute(
            "UPDATE outbox SET status='pending',lease=NULL,lease_until=0,attempts=attempts+1,next_attempt_at=?4,error=?5,remedy=NULL,updated_at=?6,version=version+1 WHERE owner=?1 AND queue=?2 AND id=?3",
            params![owner, name, id, time(next_attempt_at)?, error_text(&error), now],
        ),
        Settlement::Failed { error, remedy: token } => {
            remedy(&token)?;
            connection.execute(
                "UPDATE outbox SET status='failed',lease=NULL,lease_until=0,attempts=attempts+1,error=?4,remedy=?5,updated_at=?6,version=version+1 WHERE owner=?1 AND queue=?2 AND id=?3",
                params![owner, name, id, error_text(&error), token, now],
            )
        }
    }
    .map_err(sql)?;
    Ok(())
}

/// Apply one persist under its fence.
fn persist(
    connection: &Connection,
    owner: &str,
    queue: Queue,
    id: &str,
    expected: u64,
    set: Patch,
    now: i64,
) -> Result<(), String> {
    let changed = |sql_text: &str, value: &dyn rusqlite::ToSql| -> Result<(), String> {
        connection
            .execute(
                &format!("UPDATE outbox SET {sql_text} WHERE owner=?1 AND queue=?2 AND id=?3"),
                params![owner, queue.name(), id, value],
            )
            .map(|_| ())
            .map_err(sql)
    };
    if let Some(status) = set.status {
        match status {
            Status::Syncing => {
                let lease = set
                    .lease
                    .as_deref()
                    .ok_or_else(|| invalid("syncing takes a lease"))?;
                token("lease", lease)?;
                let until = time(
                    set.lease_until
                        .ok_or_else(|| invalid("syncing takes a lease_until"))?,
                )?;
                changed("status='syncing',lease=?4,error=NULL,remedy=NULL", &lease)?;
                changed("lease_until=?4", &until)?;
            }
            Status::Pending => changed(
                "status=?4,lease=NULL,lease_until=0,error=NULL,remedy=NULL",
                &"pending",
            )?,
            Status::Failed => {
                let (Some(error), Some(token)) = (&set.error, &set.remedy) else {
                    return Err(invalid("failed names its error and remedy"));
                };
                remedy(token)?;
                changed("status='failed',lease=NULL,lease_until=0,remedy=?4", token)?;
                changed("error=?4", &error_text(error))?;
            }
        }
    } else if set.lease.is_some() || set.lease_until.is_some() || set.remedy.is_some() {
        return Err(invalid("a lease or remedy changes with the status"));
    }
    if let (Some(error), None | Some(Status::Pending)) = (&set.error, set.status) {
        changed("error=?4", &error_text(error))?;
    }
    if let Some(attempts) = set.attempts {
        changed("attempts=?4", &time(attempts)?)?;
    }
    if let Some(next) = set.next_attempt_at {
        changed("next_attempt_at=?4", &time(next)?)?;
    }
    if let Some(value) = &set.meta {
        changed("meta=?4", &meta(value)?)?;
    }
    if let Some(value) = &set.payload {
        changed("payload=?4", &payload(queue, value)?)?;
    }
    let bumped = connection
        .execute(
            "UPDATE outbox SET updated_at=?4,version=?5 WHERE owner=?1 AND queue=?2 AND id=?3 AND version=?6",
            params![owner, queue.name(), id, now, expected as i64 + 1, expected as i64],
        )
        .map_err(sql)?;
    if bumped == 1 {
        Ok(())
    } else {
        Err(format!(
            "{FENCE_CHANGED}: {id} moved since the machine read it"
        ))
    }
}

/// Run `machine` over the partition's rows and apply its decision; the
/// effects it names are answered for the caller to run after the commit.
pub(crate) fn transition(
    connection: &Connection,
    owner: &str,
    queue: Queue,
    project: Option<&str>,
    event: &Value,
    now: u64,
    machine: Machine,
) -> Result<Vec<Value>, String> {
    let read = rows(connection, owner, queue, project, false)?;
    let actions = machine(Step {
        now,
        rows: &read,
        event,
    })?;
    let version = |id: &str| {
        read.iter()
            .find(|row| row["id"] == id)
            .and_then(|row| row["version"].as_u64())
            .ok_or_else(|| format!("{FENCE_CHANGED}: {id} is not a row the machine read"))
    };
    let at = time(now)?;
    let mut effects = Vec::new();
    for action in actions {
        match action {
            Action::Persist {
                id,
                expected_version,
                set,
            } => {
                // A version the machine did not read is a fence it cannot hold.
                if version(&id)? != expected_version {
                    return Err(format!("{FENCE_CHANGED}: {id} is not at the version named"));
                }
                persist(connection, owner, queue, &id, expected_version, *set, at)?;
            }
            Action::Remove {
                id,
                expected_version,
            } => {
                version(&id)?;
                let removed = connection
                    .execute(
                        "DELETE FROM outbox WHERE owner=?1 AND queue=?2 AND id=?3 AND version=?4",
                        params![owner, queue.name(), id, expected_version as i64],
                    )
                    .map_err(sql)?;
                if removed != 1 {
                    return Err(format!(
                        "{FENCE_CHANGED}: {id} moved since the machine read it"
                    ));
                }
            }
            Action::Effect { effect } => effects.push(effect),
        }
    }
    Ok(effects)
}

/// Run one operation inside the caller's transaction.
pub fn execute(
    connection: &Connection,
    input: Input,
    operation: Operation,
) -> Result<Outcome, String> {
    if input.bytes.is_some() {
        return Err(invalid("outbox rows carry their payloads as JSON"));
    }
    connection.execute_batch(TABLE).map_err(sql)?;
    let identity = input.identity;
    account(identity)?;
    let owner = identity.owner();
    let mut answer = Map::new();
    let wrote = match operation {
        Operation::Enqueue { queue, rows, now } => {
            let queued = enqueue(connection, &owner, queue, rows, time(now)?)?;
            answer.insert("queued".into(), queued.into());
            true
        }
        Operation::Claim {
            queue,
            lane,
            project,
            lease,
            lease_ms: ms,
            now,
        } => {
            let project = partition(queue, project.as_deref())?;
            token("lease", &lease)?;
            let now = time(now)?;
            let until = now.saturating_add(lease_ms(ms)?);
            let claimed = claim(
                connection,
                &owner,
                queue,
                lane,
                project.as_deref(),
                (&lease, until),
                now,
            )?;
            let wrote = claimed.row.is_some();
            answer.insert("claimed".into(), claimed.row.unwrap_or(Value::Null));
            answer.insert("busy".into(), claimed.busy.into());
            answer.insert(
                "due_at".into(),
                claimed.due_at.map_or(Value::Null, Value::from),
            );
            wrote
        }
        Operation::Revise {
            queue,
            id,
            lease,
            payload: value,
            meta: small,
            lease_ms: renew,
            now,
        } => {
            token("id", &id)?;
            token("lease", &lease)?;
            let now = time(now)?;
            leased(connection, &owner, queue, &id, &lease, now)?;
            if value.is_none() && small.is_none() && renew.is_none() {
                return Err(invalid(
                    "a revision changes the payload, the meta or the lease",
                ));
            }
            let text = value
                .as_ref()
                .map(|value| payload(queue, value))
                .transpose()?;
            let small = small.as_ref().map(meta).transpose()?;
            let until = renew
                .map(lease_ms)
                .transpose()?
                .map(|ms| now.saturating_add(ms));
            connection
                .execute(
                    "UPDATE outbox SET payload=coalesce(?4,payload),meta=coalesce(?5,meta),lease_until=coalesce(?6,lease_until),updated_at=?7,version=version+1 WHERE owner=?1 AND queue=?2 AND id=?3",
                    params![owner, queue.name(), id, text, small, until, now],
                )
                .map_err(sql)?;
            answer.insert("revised".into(), 1.into());
            true
        }
        Operation::Settle {
            queue,
            id,
            lease,
            settlement,
            now,
        } => {
            token("id", &id)?;
            token("lease", &lease)?;
            let now = time(now)?;
            leased(connection, &owner, queue, &id, &lease, now)?;
            settle(connection, &owner, queue, &id, settlement, now)?;
            answer.insert("settled".into(), 1.into());
            true
        }
        Operation::Transition {
            queue,
            project,
            event,
            now,
        } => {
            let project = partition(queue, project.as_deref())?;
            let machine = machine(queue).ok_or_else(|| {
                invalid(format!("{} is drained by claim and settle", queue.name()))
            })?;
            let effects = transition(
                connection,
                &owner,
                queue,
                project.as_deref(),
                &event,
                now,
                machine,
            )?;
            answer.insert("effects".into(), effects.into());
            true
        }
        Operation::List { queue, project } => {
            let project = partition(queue, project.as_deref())?;
            answer.insert(
                "rows".into(),
                rows(connection, &owner, queue, project.as_deref(), true)?.into(),
            );
            false
        }
        Operation::Counts => {
            let mut counts = Map::new();
            for queue in Queue::ALL {
                counts.insert(
                    queue.name().into(),
                    json!({"pending": 0, "syncing": 0, "failed": 0}),
                );
            }
            let mut statement = connection
                .prepare(
                    "SELECT queue,status,count(*) FROM outbox WHERE owner=?1 GROUP BY queue,status",
                )
                .map_err(sql)?;
            let found = statement
                .query_map(params![owner], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .map_err(sql)?;
            for row in found {
                let (queue, status, count) = row.map_err(sql)?;
                if let Some(Value::Object(queue)) = counts.get_mut(&queue) {
                    queue.insert(status, count.into());
                }
            }
            answer.insert("counts".into(), counts.into());
            false
        }
        Operation::Retry { queue, id, now } => {
            if let Some(id) = &id {
                token("id", id)?;
            }
            let retried = connection
                .execute(
                    "UPDATE outbox SET status='pending',next_attempt_at=?4,error=NULL,remedy=NULL,updated_at=?4,version=version+1 WHERE owner=?1 AND queue=?2 AND (?3 IS NULL OR id=?3) AND (status='failed' OR (?3 IS NOT NULL AND status='pending'))",
                    params![owner, queue.name(), id, time(now)?],
                )
                .map_err(sql)?;
            answer.insert("retried".into(), retried.into());
            retried > 0
        }
        Operation::Discard { queue, id, now } => {
            token("id", &id)?;
            let live: bool = connection
                .query_row(
                    "SELECT count(*) FROM outbox WHERE owner=?1 AND queue=?2 AND id=?3 AND status='syncing' AND lease_until>?4",
                    params![owner, queue.name(), id, time(now)?],
                    |row| Ok(row.get::<_, i64>(0)? > 0),
                )
                .map_err(sql)?;
            if live {
                return Err(format!("{LEASED}: {id} is being delivered now"));
            }
            let removed = connection
                .execute(
                    "DELETE FROM outbox WHERE owner=?1 AND queue=?2 AND id=?3",
                    params![owner, queue.name(), id],
                )
                .map_err(sql)?;
            answer.insert("removed".into(), removed.into());
            removed > 0
        }
    };
    Ok(if wrote {
        Outcome::written(answer)
    } else {
        Outcome::read(answer)
    })
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
