//! The Kernel's durable stores on SQLite: one call vocabulary, one
//! transaction and the same rusqlite code on every edge.
//!
//! Core runs it natively in its protected Store for the Desktop and headless
//! hosts (`core.store`); the hosted Web runs it in the browser's storage
//! worker on OPFS (rusqlite over sqlite-wasm-rs and the sahpool VFS). Which
//! edge executes a call is the platform adapter's choice, never a second
//! implementation. Contract: `docs/contracts/durable-store.md`.
//!
//! Identity is never the caller's to state for a store homed in Core's
//! protected Store: the host binds its signed-in principal, lane and audience,
//! and a caller names at most the project. Every answer of such a store
//! carries the bound identity's opaque `owner`; a call may carry
//! `expect_owner`, refused before anything runs when the identity changed.
//! The two older statement-protocol stores (transformer status, project model
//! catalogue) keep their own native commands and caller-built scopes; their
//! statements run through the one shared executor
//! ([`crate::sql_effect::execute`]).
//!
//! Every write to a Core-homed store advances that store's generation in the
//! same transaction, and every answer carries the current generation, so a
//! reactive layer observes change without reading rows it does not need.
use crate::sql_effect;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const INVALID: &str = "durable_store_invalid";
pub const NOT_CORE: &str = "durable_store_not_core";
pub const OWNER_CHANGED: &str = "durable_store_owner_changed";
/// The statements one statement-protocol operation may take. A store that
/// needs more is a Kernel defect, refused rather than looped.
pub const MAX_STEPS: usize = 16;
/// The largest byte payload one call carries in either direction.
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
const GENERATIONS: &str = "CREATE TABLE IF NOT EXISTS durable_store_generations (store TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL CHECK(generation>0 AND generation<=9007199254740991))";
const IMPORTS: &str = "CREATE TABLE IF NOT EXISTS durable_store_imports (owner TEXT NOT NULL, store TEXT NOT NULL, source TEXT NOT NULL, count INTEGER NOT NULL CHECK(count>=0), skipped INTEGER NOT NULL CHECK(skipped>=0), sha256 TEXT NOT NULL, at_ms INTEGER NOT NULL CHECK(at_ms>=0 AND at_ms<=9007199254740991), PRIMARY KEY(owner,store,source))";

fn invalid(message: impl std::fmt::Display) -> String {
    format!("{INVALID}: {message}")
}
fn sql(error: rusqlite::Error) -> String {
    invalid(error)
}
pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Who a Core-homed row belongs to: a signed-in account, or the one reserved
/// anonymous principal of a signed-out browser profile (the map, sketches and
/// local models work signed out). The anonymous principal is a type, never a
/// string a uid could equal.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Principal {
    Anonymous,
    Account(String),
}

/// The identity a host binds into a Core-homed store: Core binds its
/// protected account, the lane and the canonical (account, lane, audience)
/// owner fence; the browser worker the signed-in Firebase uid (or
/// [`Principal::Anonymous`]), the deployment lane and the authority audience.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub principal: Principal,
    pub lane: String,
    pub audience: String,
}
impl Identity {
    pub fn validate(&self) -> Result<(), String> {
        let principal = match &self.principal {
            Principal::Anonymous => None,
            Principal::Account(uid) => Some(uid),
        };
        for (name, value) in principal
            .map(|uid| ("principal", uid))
            .into_iter()
            .chain([("lane", &self.lane), ("audience", &self.audience)])
        {
            if value.is_empty()
                || value.len() > 512
                || value.trim() != value
                || value.chars().any(char::is_control)
            {
                return Err(invalid(format!("the host identity {name} is invalid")));
            }
        }
        Ok(())
    }
    /// The opaque digest every Core-homed answer carries and `expect_owner`
    /// compares; rows and receipts are fenced by it too.
    pub fn owner(&self) -> String {
        sha256(
            serde_json::to_string(&(&self.principal, &self.lane, &self.audience))
                .unwrap_or_default()
                .as_bytes(),
        )
    }
}

/// One store operation, exactly as every host receives it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "store", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Transformer status observations (statement protocol; Core serves them
    /// through `design.status`).
    Status {
        operation: crate::transformer_status_store::Operation,
    },
    /// The retained project model catalogue (statement protocol; Core serves
    /// it through `design.model.catalog`).
    GridProjectCatalog {
        operation: crate::grid_project_catalog::Operation,
    },
    UiPreferences {
        operation: crate::ui_preferences::Operation,
    },
    /// Every plain cache of a Brain read, by closed kind (`retained_reads`).
    RetainedReads {
        operation: crate::retained_reads::Operation,
    },
    /// Messaging push on one browser profile and its fail-closed projection.
    PushState {
        operation: crate::push_state::Operation,
    },
    /// Folder connections; their directory handles stay in the web adapter.
    FolderConnections {
        operation: crate::folder_connections::Operation,
    },
    /// Prepared local layers: the catalogue and each layer's payload.
    LocalLayerStore {
        operation: crate::local_layer_store::Operation,
    },
    /// The local DS Grid workspace: results, temporary groups, sessions.
    GridModelStore {
        operation: crate::grid_model_store::Operation,
    },
    /// LV and MV design rooms with their save state.
    DesignRooms {
        operation: Box<crate::design_rooms::Operation>,
    },
    /// Every reconstructible byte cache, content-addressed per owner.
    ByteCache {
        operation: crate::byte_cache::Operation,
    },
    /// Pending media uploads and the bytes they own.
    MediaOutbox {
        operation: Box<crate::media_outbox::Operation>,
    },
    /// Upload workspace chunks and chunk-backed sketch features.
    UploadChunks {
        operation: crate::upload_chunks::Operation,
    },
}
impl Request {
    pub fn store(&self) -> &'static str {
        match self {
            Request::Status { .. } => "status",
            Request::GridProjectCatalog { .. } => "grid_project_catalog",
            Request::UiPreferences { .. } => "ui_preferences",
            Request::RetainedReads { .. } => crate::retained_reads::STORE,
            Request::PushState { .. } => crate::push_state::STORE,
            Request::FolderConnections { .. } => crate::folder_connections::STORE,
            Request::LocalLayerStore { .. } => crate::local_layer_store::STORE,
            Request::GridModelStore { .. } => crate::grid_model_store::STORE,
            Request::DesignRooms { .. } => crate::design_rooms::STORE,
            Request::ByteCache { .. } => crate::byte_cache::STORE,
            Request::MediaOutbox { .. } => crate::media_outbox::STORE,
            Request::UploadChunks { .. } => crate::upload_chunks::STORE,
        }
    }
    /// Refuse a store whose native home is its own Core command, so one store
    /// never has two native homes.
    pub fn core_homed(&self) -> Result<(), String> {
        match self {
            Request::Status { .. } => Err(format!(
                "{NOT_CORE}: status is served by design.status, not core.store"
            )),
            Request::GridProjectCatalog { .. } => Err(format!(
                "{NOT_CORE}: grid_project_catalog is served by design.model.catalog, not core.store"
            )),
            Request::UiPreferences { .. }
            | Request::RetainedReads { .. }
            | Request::PushState { .. }
            | Request::FolderConnections { .. }
            | Request::LocalLayerStore { .. }
            | Request::GridModelStore { .. } => Ok(()),
            Request::DesignRooms { .. } => Ok(()),
            Request::ByteCache { .. } | Request::MediaOutbox { .. } | Request::UploadChunks { .. } => Ok(()),
        }
    }
}

/// One call on the wire: the StoreHost port, the browser worker and
/// `core.store` all carry exactly this JSON. Payload bytes travel beside it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub request: Request,
    /// The `owner` an earlier answer carried; a different bound identity
    /// refuses `durable_store_owner_changed` before anything runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_owner: Option<String>,
}

/// One Kernel-named file a host writes or removes outside SQLite.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EffectFile {
    /// The fail-closed push projection the service worker reads.
    PushProjection,
}
impl EffectFile {
    /// The OPFS path below the origin root.
    pub fn path(self) -> &'static str {
        match self {
            EffectFile::PushProjection => "ds-kernel-effects/push-projection.json",
        }
    }
}
/// The closed effect vocabulary around a transaction. The browser storage
/// worker's host runs effects on OPFS under the same Web Lock: before-effects
/// ([`effects_before`]) before the transaction, after-effects
/// ([`Answer::effects`]) after its commit. Core treats them as no-ops: the
/// Desktop and headless hosts have no service worker or OPFS reader.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "effect", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    /// Replace the file with these UTF-8 contents.
    WriteFile { file: EffectFile, contents: String },
    /// Remove the file; an absent file is success.
    DeleteFile { file: EffectFile },
}

/// Effects the host must complete before the call's transaction, so a crash
/// between them fails closed.
pub fn effects_before(call: &Call) -> Vec<Effect> {
    match &call.request {
        Request::PushState { operation } => crate::push_state::effects_before(operation),
        _ => Vec::new(),
    }
}

/// What a store module receives besides its operation.
pub struct Input<'a> {
    pub identity: &'a Identity,
    /// Payload bytes of a byte-carrying operation.
    pub bytes: Option<Vec<u8>>,
}
/// What a store module answers inside the transaction.
#[derive(Debug, Default)]
pub struct Outcome {
    pub value: Map<String, Value>,
    /// The operation changed rows: the store's generation advances.
    pub wrote: bool,
    /// Bytes for the caller (byte stores only).
    pub bytes: Option<Vec<u8>>,
    /// Effects the host runs after the commit.
    pub after: Vec<Effect>,
    /// The generation row this call reads or advances; the store name when
    /// `None`. A store with independent kinds names one row per kind, so a
    /// write to one kind never moves another's generation.
    pub generation_key: Option<String>,
}
impl Outcome {
    pub fn read(value: Map<String, Value>) -> Self {
        Self {
            value,
            ..Self::default()
        }
    }
    pub fn written(value: Map<String, Value>) -> Self {
        Self {
            value,
            wrote: true,
            ..Self::default()
        }
    }
}

/// One answer: the store's fields, then the store's `generation` and the
/// bound identity's `owner` (Core-homed stores) and the after-effects. Bytes
/// travel beside the JSON.
#[derive(Debug, Serialize)]
pub struct Answer {
    #[serde(flatten)]
    pub value: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    #[serde(skip)]
    pub bytes: Option<Vec<u8>>,
}

/// The current generation of one store in this database (0 when never written).
pub(crate) fn generation(connection: &Connection, store: &str) -> Result<u64, String> {
    connection.execute_batch(GENERATIONS).map_err(sql)?;
    let held: Option<i64> = connection
        .query_row(
            "SELECT generation FROM durable_store_generations WHERE store=?1",
            params![store],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    Ok(held.unwrap_or(0) as u64)
}
pub(crate) fn advance(connection: &Connection, store: &str) -> Result<u64, String> {
    connection.execute_batch(GENERATIONS).map_err(sql)?;
    connection
        .execute(
            "INSERT INTO durable_store_generations(store,generation) VALUES(?1,1) ON CONFLICT(store) DO UPDATE SET generation=generation+1",
            params![store],
        )
        .map_err(sql)?;
    generation(connection, store)
}

/// The receipt of one completed legacy import, one per owner, store and source.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ImportReceipt {
    pub store: String,
    pub source: String,
    pub count: u64,
    pub skipped: u64,
    pub sha256: String,
    pub at_ms: u64,
}
/// The receipt this identity holds for one store and source, if its import
/// ran: what a store answers as `import_needed` without reading the source.
pub fn held_receipt(
    connection: &Connection,
    identity: &Identity,
    store: &str,
    source: &str,
) -> Result<Option<ImportReceipt>, String> {
    connection.execute_batch(IMPORTS).map_err(sql)?;
    connection
        .query_row(
            "SELECT count,skipped,sha256,at_ms FROM durable_store_imports WHERE owner=?1 AND store=?2 AND source=?3",
            params![identity.owner(), store, source],
            |row| {
                Ok(ImportReceipt {
                    store: store.into(),
                    source: source.into(),
                    count: row.get::<_, i64>(0)? as u64,
                    skipped: row.get::<_, i64>(1)? as u64,
                    sha256: row.get(2)?,
                    at_ms: row.get::<_, i64>(3)? as u64,
                })
            },
        )
        .optional()
        .map_err(sql)
}

/// Run a legacy import at most once per owner, store and source. A held
/// receipt answers without running `import`; otherwise `import` writes its
/// rows (never replacing newer ones) and answers `(count, skipped)`, and the
/// receipt is recorded in the same transaction. `sha256` is the digest of the
/// entries as the source handed them over.
pub fn import_once(
    connection: &Connection,
    identity: &Identity,
    store: &str,
    source: &str,
    sha256: &str,
    now: u64,
    import: impl FnOnce() -> Result<(u64, u64), String>,
) -> Result<(ImportReceipt, bool), String> {
    connection.execute_batch(IMPORTS).map_err(sql)?;
    let owner = identity.owner();
    let held = connection
        .query_row(
            "SELECT count,skipped,sha256,at_ms FROM durable_store_imports WHERE owner=?1 AND store=?2 AND source=?3",
            params![owner, store, source],
            |row| {
                Ok(ImportReceipt {
                    store: store.into(),
                    source: source.into(),
                    count: row.get::<_, i64>(0)? as u64,
                    skipped: row.get::<_, i64>(1)? as u64,
                    sha256: row.get(2)?,
                    at_ms: row.get::<_, i64>(3)? as u64,
                })
            },
        )
        .optional()
        .map_err(sql)?;
    if let Some(receipt) = held {
        return Ok((receipt, false));
    }
    if now > 9_007_199_254_740_991 || sha256.len() != 64 {
        return Err(invalid("an import names its time and the SHA-256 of its entries"));
    }
    let (count, skipped) = import()?;
    connection
        .execute(
            "INSERT INTO durable_store_imports(owner,store,source,count,skipped,sha256,at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![owner, store, source, count as i64, skipped as i64, sha256, now as i64],
        )
        .map_err(sql)?;
    Ok((
        ImportReceipt {
            store: store.into(),
            source: source.into(),
            count,
            skipped,
            sha256: sha256.into(),
            at_ms: now,
        },
        true,
    ))
}

/// Drive one statement-protocol call to `done` through the shared executor
/// and answer its done fields.
macro_rules! statements {
    ($connection:expr, $module:path, $operation:expr) => {{
        use $module as owner;
        let mut step = owner::start($operation)?;
        let mut answer = None;
        for _ in 0..MAX_STEPS {
            step = match step {
                owner::Step::Sql {
                    statement,
                    continuation,
                } => owner::resume(continuation, sql_effect::execute($connection, &statement)?)?,
                done => {
                    let Value::Object(mut fields) =
                        serde_json::to_value(&done).map_err(|e| e.to_string())?
                    else {
                        return Err(invalid("a done step is an object"));
                    };
                    fields.remove("kind");
                    answer = Some(fields);
                    break;
                }
            };
        }
        answer.ok_or_else(|| invalid("the store exceeded its bounded continuation"))
    }};
}

/// Run one call in one IMMEDIATE transaction and commit only on success.
/// A Core-homed store requires the host's identity; bytes reach only a store
/// whose operation takes them.
pub fn execute(
    connection: &mut Connection,
    identity: Option<&Identity>,
    call: Call,
    bytes: Option<Vec<u8>>,
) -> Result<Answer, String> {
    if bytes.as_ref().is_some_and(|bytes| bytes.len() > MAX_BYTES) {
        return Err(invalid("a call carries at most 64 MiB"));
    }
    let Call {
        request,
        expect_owner,
    } = call;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql)?;
    let answer = match request {
        Request::Status { .. } | Request::GridProjectCatalog { .. }
            if bytes.is_some() || expect_owner.is_some() =>
        {
            return Err(invalid(
                "the statement-protocol stores take no bytes and no owner fence",
            ))
        }
        Request::Status { operation } => {
            statements!(
                &transaction,
                crate::transformer_status_store,
                crate::transformer_status_store::Operation::Schema
            )?;
            Answer {
                value: statements!(&transaction, crate::transformer_status_store, operation)?,
                generation: None,
                owner: None,
                effects: Vec::new(),
                bytes: None,
            }
        }
        Request::GridProjectCatalog { operation } => {
            statements!(
                &transaction,
                crate::grid_project_catalog,
                crate::grid_project_catalog::Operation::Schema
            )?;
            Answer {
                value: statements!(&transaction, crate::grid_project_catalog, operation)?,
                generation: None,
                owner: None,
                effects: Vec::new(),
                bytes: None,
            }
        }
        request => {
            let identity = identity.ok_or_else(|| {
                invalid(format!(
                    "the host binds its identity into {}",
                    request.store()
                ))
            })?;
            identity.validate()?;
            let owner = identity.owner();
            if expect_owner.is_some_and(|expected| expected != owner) {
                return Err(format!(
                    "{OWNER_CHANGED}: the signed-in identity changed since this flow began"
                ));
            }
            let store = request.store();
            let input = Input { identity, bytes };
            let outcome = match request {
                Request::UiPreferences { operation } => {
                    crate::ui_preferences::execute(&transaction, input, operation)?
                }
                Request::RetainedReads { operation } => {
                    crate::retained_reads::execute(&transaction, input, operation)?
                }
                Request::PushState { operation } => {
                    crate::push_state::execute(&transaction, input, operation)?
                }
                Request::FolderConnections { operation } => {
                    crate::folder_connections::execute(&transaction, input, operation)?
                }
                Request::LocalLayerStore { operation } => {
                    crate::local_layer_store::execute(&transaction, input, operation)?
                }
                Request::GridModelStore { operation } => {
                    crate::grid_model_store::execute(&transaction, input, operation)?
                }
                Request::DesignRooms { operation } => {
                    crate::design_rooms::execute(&transaction, input, *operation)?
                }
                Request::ByteCache { operation } => {
                    crate::byte_cache::execute(&transaction, input, operation)?
                }
                Request::MediaOutbox { operation } => {
                    crate::media_outbox::execute(&transaction, input, *operation)?
                }
                Request::UploadChunks { operation } => {
                    crate::upload_chunks::execute(&transaction, input, operation)?
                }
                Request::Status { .. } | Request::GridProjectCatalog { .. } => {
                    unreachable!("statement-protocol stores are answered above")
                }
            };
            let key = outcome.generation_key.as_deref().unwrap_or(store);
            if key != store && !key.starts_with(&format!("{store}.")) {
                return Err(invalid("a generation key is the store name or one of its kinds"));
            }
            let generation = if outcome.wrote {
                advance(&transaction, key)?
            } else {
                generation(&transaction, key)?
            };
            if outcome.bytes.as_ref().is_some_and(|bytes| bytes.len() > MAX_BYTES) {
                return Err(invalid("an answer carries at most 64 MiB"));
            }
            Answer {
                value: outcome.value,
                generation: Some(generation),
                owner: Some(owner),
                effects: outcome.after,
                bytes: outcome.bytes,
            }
        }
    };
    transaction.commit().map_err(sql)?;
    Ok(answer)
}

/// How a Kernel flow reaches storage on every host.
pub mod port;
/// The hosted Web's storage worker host (rusqlite over sqlite-wasm-rs on OPFS).
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
pub mod opfs;
/// A process-local in-memory host with the same answers (the Node test host).
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
pub mod memory;

#[cfg(test)]
#[path = "durable_store_tests.rs"]
mod tests;
