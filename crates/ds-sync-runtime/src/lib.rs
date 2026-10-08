//! The shared native sync runtime — the IO half of `ds_command_kernel::sync`,
//! shared by native hosts. Desktop and the Linux server supply their own
//! authenticated gateway, install registration and producer adapters.
//!
//! The decision lives once, in the kernel: given what this machine holds, what
//! the shared record holds, the credential in hand and the clock, `sync::plan`
//! answers upload / download / conflict / nothing and says when to look again.
//! This crate observes the two inventories, hands them to that function, and
//! performs the returned actions **in the order they came back**
//! ([`run::run`]). Between two triggers everything the loop carries — heads,
//! grants, artifact states, receipts, the lease — lives in the durable sync
//! store ([`host::StoreHost`]), so closing a window, a crash or a restart
//! loses nothing.
//!
//! What differs between hosts is supplied through three narrow seams, each a
//! typed door rather than a tunnel:
//!
//! * [`gateway::Gateway`] — one authenticated JSON call on one of the three
//!   declared routes ([`routes::SyncRoute`]); never a URL, never a bearer the
//!   loop can see;
//! * [`host::Producer`] — where the bytes come from and how they leave through
//!   the host's one native transport host; a host that runs several engines
//!   supplies one [`host::Producers`] set keyed by engine, so every row is
//!   moved and freed by the producer that owns its bytes;
//! * [`host::Reads`] — where downloaded heads land.
//!
//! No Tauri, no webview, no credential of its own. Contracts:
//! `ds-command-kernel/docs/contracts/ds-sync-engine.md` (the plan),
//! `ds-sync-store.md` (the rows), `ds-headless-application-runtime.md` (the
//! mandate), `ds-brain/docs/contracts/edge-admission-and-publish.md`
//! §1.2/§1.3 (the grant and the publish).
//!
//! Project data is fenced by account, deployment and installation in the store.
//! A project inventory is not authority: its grant must name the same install
//! and project before publication.

pub mod gateway;
mod guard;
pub mod host;
mod lease;
pub mod native_transfer;
pub mod reads;
pub mod retained_preview;
pub mod routes;
pub mod rows;
pub mod run;
pub mod seal;
pub mod solar;
pub mod solar_producer;

#[cfg(feature = "fakes")]
pub mod fakes;

pub use ds_command_kernel::sync::{self as kernel_sync, Trigger};
pub use ds_command_kernel::sync_store as store;
pub use gateway::{BearerGateway, Gateway, GatewayError, UnavailableGateway};
pub use guard::run_contained;
pub use host::{
    open_store, projects_of_fence, ActivityRow, Producer, Producers, Reads, SharedStore, StoreHost,
    StoreOnlyProducer, LEASE_TTL_MS,
};
#[cfg(feature = "fakes")]
pub use native_transfer::drive_loopback_output;
pub use native_transfer::{
    drive_verified_output, transfer_verified_output, validated_storage_session,
    NativeTransferOutcome, NativeTransferProgress, NativeTransferResult, SessionRefusal,
};
pub use reads::VerifiedReads;
pub use routes::{Route, SyncRoute, COMPUTE_ARTIFACTS};
pub use rows::{
    inventory_digest, now_ms, server_time_ms, LocalOutput, LocalRow, Receipt, Reclaimed,
    RecordAnswer, RemoteOutput, RemoteRow, SyncRun, TransferReceipt, MAX_ROWS, SCHEMA,
};
pub use run::{plan, plan_once, run, SyncHost, DEFAULT_TRIGGER};

pub mod reports;
