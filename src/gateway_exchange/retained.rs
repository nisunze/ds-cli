//! Flows that keep a Brain read: one Rust flow reads what this device holds,
//! fetches through the gateway exchange, admits the typed answer and retains
//! it in [`crate::retained_reads`]. TypeScript calls the flow's typed export
//! and renders what it answers; it holds no request, cache key or store.
//!
//! A flow runs on an [`Edge`]: the page's gateway and store ports in the
//! browser and the Desktop webview ([`browser::BrowserEdge`]), an in-memory
//! SQLite and a scripted gateway in tests. It reads first and writes with the
//! `owner` its read observed, so an answer fetched while one account was
//! signed in is never retained under another (`durable_store_owner_changed`).

use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use super::{Exchange, Refusal};
use crate::durable_store::{Call, Request};
use crate::retained_reads::{Kind, Operation, Row};

/// What a flow runs on. Every decision stays in the flow; an edge performs
/// one effect per call.
#[allow(async_fn_in_trait)]
pub trait Edge {
    /// One gateway exchange to its typed answer or refusal.
    async fn fetch<C: Exchange>(&self, call: C) -> Result<C::Output, Refusal>;
    /// One durable-store call to its answer (`generation`, `owner` beside the
    /// store's fields), or the store's refusal.
    async fn store(&self, call: Call) -> Result<Value, Refusal>;
    /// The host's clock.
    fn now_ms(&self) -> u64;
    /// Whether a request could leave now: a link and no offline mode.
    fn online(&self) -> bool;
}

/// A store refusal (`code: message`) as the flow's typed refusal.
pub fn store_refusal(text: String) -> Refusal {
    match text.split_once(": ") {
        Some((code, message))
            if !code.is_empty() && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') =>
        {
            Refusal::new(code, message)
        }
        _ => Refusal::new(crate::durable_store::INVALID, text),
    }
}

/// One held value with the facts its flow decides on.
#[derive(Clone, Debug, PartialEq)]
pub struct Held<T> {
    pub value: T,
    pub saved_at_ms: u64,
    pub invalidated: bool,
}

/// What a read observed: the held value, if it still admits, and the
/// identity and generation a later write of the same flow is fenced by.
#[derive(Clone, Debug, PartialEq)]
pub struct Observed<T> {
    pub held: Option<Held<T>>,
    pub owner: Option<String>,
    pub generation: u64,
}

fn held<T: DeserializeOwned>(row: &Value) -> Option<Held<T>> {
    Some(Held {
        value: T::deserialize(row.get("value")?).ok()?,
        saved_at_ms: row.get("saved_at_ms")?.as_u64()?,
        invalidated: row.get("invalidated")?.as_bool()?,
    })
}
fn fenced(answer: &Value) -> (Option<String>, u64) {
    (
        answer
            .get("owner")
            .and_then(Value::as_str)
            .map(str::to_owned),
        answer
            .get("generation")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    )
}
async fn call(
    edge: &impl Edge,
    operation: Operation,
    owner: Option<&str>,
) -> Result<Value, Refusal> {
    edge.store(Call {
        request: Request::RetainedReads { operation },
        expect_owner: owner.map(str::to_owned),
    })
    .await
}
fn project(project: Option<&str>) -> Option<String> {
    project.map(str::to_owned)
}

/// The held value of one key. A value its flow no longer admits reads as
/// absent.
pub async fn read<T: DeserializeOwned>(
    edge: &impl Edge,
    kind: Kind,
    in_project: Option<&str>,
    key: &str,
) -> Result<Observed<T>, Refusal> {
    let operation = Operation::Read {
        kind,
        project: project(in_project),
        key: key.into(),
        now: edge.now_ms(),
    };
    let answer = call(edge, operation, None).await?;
    let (owner, generation) = fenced(&answer);
    Ok(Observed {
        held: answer.get("held").and_then(held),
        owner,
        generation,
    })
}

/// Every held row of one fence, most recently used first.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed<T> {
    pub rows: Vec<(String, Held<T>)>,
    pub owner: Option<String>,
    pub generation: u64,
}

/// Every held row of a kind in its fence; rows that no longer admit are left
/// out.
pub async fn list<T: DeserializeOwned>(
    edge: &impl Edge,
    kind: Kind,
    in_project: Option<&str>,
) -> Result<Listed<T>, Refusal> {
    let operation = Operation::List {
        kind,
        project: project(in_project),
        now: edge.now_ms(),
    };
    let answer = call(edge, operation, None).await?;
    let (owner, generation) = fenced(&answer);
    let rows = answer
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| Some((row.get("key")?.as_str()?.to_owned(), held(row)?)))
                .collect()
        })
        .unwrap_or_default();
    Ok(Listed {
        rows,
        owner,
        generation,
    })
}

fn json<T: Serialize>(value: &T) -> Result<Value, Refusal> {
    serde_json::to_value(value)
        .map_err(|error| Refusal::new(crate::retained_reads::INVALID, error.to_string()))
}

/// Retain one answer under the identity its flow observed. `expect_generation`
/// refuses the write (`retained_reads_stale`) when the kind changed since.
pub async fn write<T: Serialize>(
    edge: &impl Edge,
    owner: Option<&str>,
    kind: Kind,
    in_project: Option<&str>,
    key: &str,
    value: &T,
    expect_generation: Option<u64>,
) -> Result<(), Refusal> {
    let operation = Operation::Write {
        kind,
        project: project(in_project),
        key: key.into(),
        value: json(value)?,
        now: edge.now_ms(),
        expect_generation,
    };
    call(edge, operation, owner).await.map(drop)
}

/// Make a fence's rows exactly this set.
pub async fn replace_set<T: Serialize>(
    edge: &impl Edge,
    owner: Option<&str>,
    kind: Kind,
    in_project: Option<&str>,
    rows: &[(String, T)],
) -> Result<(), Refusal> {
    let rows = rows
        .iter()
        .map(|(key, value)| {
            Ok(Row {
                key: key.clone(),
                value: json(value)?,
            })
        })
        .collect::<Result<Vec<_>, Refusal>>()?;
    let operation = Operation::ReplaceSet {
        kind,
        project: project(in_project),
        rows,
        now: edge.now_ms(),
    };
    call(edge, operation, owner).await.map(drop)
}

/// Mark held rows stale: one key, one project, or the whole account.
pub async fn invalidate(
    edge: &impl Edge,
    owner: Option<&str>,
    kind: Kind,
    in_project: Option<&str>,
    key: Option<&str>,
) -> Result<(), Refusal> {
    let operation = Operation::Invalidate {
        kind,
        project: project(in_project),
        key: key.map(str::to_owned),
    };
    call(edge, operation, owner).await.map(drop)
}

/// Remove held rows, with the reach of [`invalidate`].
pub async fn forget(
    edge: &impl Edge,
    owner: Option<&str>,
    kind: Kind,
    in_project: Option<&str>,
    key: Option<&str>,
) -> Result<(), Refusal> {
    let operation = Operation::Forget {
        kind,
        project: project(in_project),
        key: key.map(str::to_owned),
    };
    call(edge, operation, owner).await.map(drop)
}

#[cfg(test)]
pub(crate) mod testing;
