//! Survey media on its way to the bucket: the gateway's start and
//! completion of one object's upload, and the drain that carries every
//! pending upload of the media outbox ([`crate::media_outbox`]) through them
//! and the resumable storage transfer.
//!
//! The outbox decides what may start (`claim`), holds the bytes (`read`) and
//! settles what each attempt observed (`settle`); this flow performs the
//! exchanges in order and decides nothing else. An upload is an ordered
//! plan: the original first, then its thumbnail, whose start names the
//! original's verified generation, so a thumbnail never appears for an
//! original that failed. The drain runs on the page's gateway ([`Edge`]),
//! its store ([`StorePort`]) and the storage transfer ([`Transfer`]); one
//! page drains an account at a time.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::retained::{store_refusal, Edge};
use super::{data, Admitted, Exchange, Plan, Refusal, Request, NETWORK_OFFLINE, REQUEST_TIMEOUT};
use crate::byte_parts::PART_BYTES;
use crate::durable_store::port::{Reply, StorePort};
use crate::media_outbox::{Operation, Target as Role};
use crate::survey_media_address;
use crate::survey_photo_outbox::TransportOutcome;
use crate::survey_photo_transport;
use crate::transfer::Cause;

const PATH: &str = "/api/v1/media";
pub const INVALID: &str = "survey_media_answer_invalid";
/// The claims one drain makes at most. Every claim settles what it
/// dispatched and an upload is dispatched at most ten times, so a drain
/// ends long before this.
const MAX_ROUNDS: usize = 1024;

/// One object's upload, as both of its gateway actions carry it.
#[derive(Clone, Debug, PartialEq)]
pub struct Upload {
    pub path: String,
    pub size: u64,
    pub content_type: String,
    pub sha256: String,
    pub expected_generation: Option<String>,
    pub revision: Option<String>,
    pub supersedes: Vec<String>,
    /// A thumbnail's original: its stored file name and verified generation.
    pub source: Option<(String, String)>,
}

impl Upload {
    fn body(&self, action: &str) -> Result<Value, String> {
        let address = survey_media_address::parse(&self.path)?;
        let mut body = survey_media_address::wire(&address);
        body["action"] = json!(action);
        body["size"] = json!(self.size);
        body["content_type"] = json!(self.content_type);
        body["sha256"] = json!(self.sha256);
        if let Some(generation) = &self.expected_generation {
            body["expected_generation"] = json!(generation);
        }
        if let Some(revision) = &self.revision {
            body["revision"] = json!(revision);
            body["supersedes"] = json!(self.supersedes);
        }
        if let Some((file, generation)) = &self.source {
            body["source_file_name"] = json!(file);
            body["source_generation"] = json!(generation);
        }
        Ok(body)
    }
}

fn invalid(message: String) -> Refusal {
    Refusal::new(INVALID, message)
}

/// What the gateway answered a start: the object is already stored at a
/// verified generation, or a resumable session awaits its bytes.
#[derive(Clone, Debug, PartialEq)]
pub enum Started {
    Present { generation: String },
    Session { uri: String },
}

/// Ask the gateway to accept one object's bytes.
pub struct Start(pub Upload);

impl Exchange for Start {
    type Output = Started;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request::post(PATH, self.0.body("survey_upload_start").map_err(invalid)?).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let issued = data(&body);
        let bucket = survey_photo_transport::target(issued, &self.0.path).map_err(invalid)?;
        if issued["already_present"] == true {
            let generation = survey_photo_transport::generation(&issued["generation"]).map_err(invalid)?;
            return Ok(Admitted::Done(Started::Present { generation }));
        }
        let uri = survey_photo_transport::session(&issued["session_uri"], &bucket).map_err(invalid)?;
        Ok(Admitted::Done(Started::Session { uri }))
    }
}

/// Ask the gateway to verify the stored object; answers its generation.
pub struct Complete(pub Upload);

impl Exchange for Complete {
    type Output = String;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request::post(PATH, self.0.body("survey_upload_complete").map_err(invalid)?).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let verified = data(&body);
        survey_photo_transport::target(verified, &self.0.path).map_err(invalid)?;
        if verified["already_present"] != true {
            return Err(invalid("Media completion was not verified".into()));
        }
        let generation = survey_photo_transport::generation(&verified["generation"]).map_err(invalid)?;
        Ok(Admitted::Done(generation))
    }
}

/// One object's bytes, read part by part.
#[allow(async_fn_in_trait)]
pub trait Parts {
    async fn read(&self, offset: u64, length: u64) -> Result<Vec<u8>, Cause>;
}

/// How one storage transfer ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Sent {
    Done,
    /// The session is gone: start again for a new one.
    Reopen,
    Failed(Cause),
}

/// The storage transfer: one object's bytes to the session the gateway
/// minted, resumed after interruptions by the shared transfer machine.
#[allow(async_fn_in_trait)]
pub trait Transfer {
    async fn send(&self, session_uri: &str, size: u64, content_type: &str, parts: &impl Parts) -> Sent;
}

/// What one drain did, and when the next one is due.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct Drained {
    /// Uploads verified and kept as cached photos.
    pub uploaded: u64,
    /// Attempts that failed; the outbox retries or surfaces each.
    pub failed: u64,
    /// When a waiting retry falls due, in epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub wake_at: Option<u64>,
}

/// One upload the outbox dispatched, as `claim` answers it.
#[derive(Debug, Deserialize)]
struct Dispatch {
    path: String,
    generation: String,
    lease: u64,
    targets: Vec<Target>,
}
#[derive(Debug, Deserialize)]
struct Target {
    path: String,
    content_type: String,
    size: u64,
    #[serde(default)]
    expected_generation: Option<String>,
    #[serde(default)]
    revision: Option<String>,
    #[serde(default)]
    supersedes: Vec<String>,
    target: Role,
    sha256: String,
}

async fn call(store: &impl StorePort, owner: Option<&str>, operation: Operation) -> Result<Reply, Refusal> {
    let request = crate::durable_store::Request::MediaOutbox {
        operation: Box::new(operation),
    };
    store
        .call(request, owner.map(str::to_owned), None)
        .await
        .map_err(store_refusal)
}

/// A dispatched target's bytes, read from the outbox under the upload's
/// own generation: a replaced or removed upload reads as cancelled.
struct Bytes<'a, S> {
    store: &'a S,
    owner: &'a str,
    path: &'a str,
    generation: &'a str,
    role: Role,
}

impl<S: StorePort> Parts for Bytes<'_, S> {
    async fn read(&self, offset: u64, length: u64) -> Result<Vec<u8>, Cause> {
        let mut bytes = Vec::with_capacity(length as usize);
        while (bytes.len() as u64) < length {
            let take = (length - bytes.len() as u64).min(PART_BYTES);
            let read = Operation::Read {
                path: self.path.into(),
                generation: self.generation.into(),
                target: self.role,
                offset: offset + bytes.len() as u64,
                length: take,
            };
            let reply = call(self.store, Some(self.owner), read)
                .await
                .map_err(|_| Cause::Cancelled)?;
            let part = reply
                .bytes
                .filter(|part| part.len() as u64 == take)
                .ok_or(Cause::LengthMismatch)?;
            bytes.extend_from_slice(&part);
        }
        Ok(bytes)
    }
}

fn refused(refusal: Refusal) -> TransportOutcome {
    TransportOutcome::TransportFailure {
        cause: None,
        status: refusal.status,
        timed_out: refusal.code == REQUEST_TIMEOUT,
        offline_mode: refusal.code == NETWORK_OFFLINE,
        has_session: !refusal.signed_out,
        error_code: Some(refusal.code),
        retry_after: None,
        message: Some(refusal.message),
    }
}

fn failed(cause: Cause) -> TransportOutcome {
    TransportOutcome::TransportFailure {
        message: Some(cause.describe()),
        cause: Some(cause),
        status: None,
        error_code: None,
        timed_out: false,
        retry_after: None,
        has_session: true,
        offline_mode: false,
    }
}

/// Carry one dispatched upload through its ordered plan and answer what it
/// observed, for the outbox to settle.
async fn carry(edge: &impl Edge, store: &impl StorePort, transfer: &impl Transfer, owner: &str, upload: &Dispatch) -> TransportOutcome {
    let mut original: Option<(String, String)> = None;
    for target in &upload.targets {
        let call = Upload {
            path: target.path.clone(),
            size: target.size,
            content_type: target.content_type.clone(),
            sha256: target.sha256.clone(),
            expected_generation: target.expected_generation.clone(),
            revision: target.revision.clone(),
            supersedes: target.supersedes.clone(),
            source: match target.target {
                Role::Original => None,
                Role::Companion => original.clone(),
            },
        };
        let generation = match edge.fetch(Start(call.clone())).await {
            Err(refusal) => return refused(refusal),
            Ok(Started::Present { generation }) => generation,
            Ok(Started::Session { uri }) => {
                let bytes = Bytes {
                    store,
                    owner,
                    path: &upload.path,
                    generation: &upload.generation,
                    role: target.target,
                };
                match transfer.send(&uri, target.size, &target.content_type, &bytes).await {
                    Sent::Done => {}
                    Sent::Reopen | Sent::Failed(Cause::Cancelled) => return TransportOutcome::Cancelled,
                    Sent::Failed(cause) => return failed(cause),
                }
                match edge.fetch(Complete(call)).await {
                    Ok(generation) => generation,
                    Err(refusal) => return refused(refusal),
                }
            }
        };
        if target.target == Role::Original {
            let Ok(address) = survey_media_address::parse(&target.path) else {
                return failed(Cause::BlockedByHostPolicy);
            };
            original = Some((address.file, generation));
        }
    }
    TransportOutcome::Success
}

/// Claim what may start, carry each upload, settle what it observed, and
/// claim again until nothing is due. Every store call after the first is
/// fenced by the account the first answered for, so a sign-out mid-drain
/// stops it with its uploads left for that account's next drain.
pub async fn drain(edge: &impl Edge, store: &impl StorePort, transfer: &impl Transfer) -> Result<Drained, Refusal> {
    let mut drained = Drained::default();
    let mut owner: Option<String> = None;
    for _ in 0..MAX_ROUNDS {
        let claim = Operation::Claim {
            online: edge.online(),
            in_flight: Vec::new(),
            now: edge.now_ms(),
        };
        let reply = call(store, owner.as_deref(), claim).await?;
        let owner: &str = owner.get_or_insert_with(|| reply.owner().unwrap_or_default().to_owned());
        drained.wake_at = reply.answer.get("wake_at").and_then(Value::as_u64);
        let dispatch: Vec<Dispatch> = serde_json::from_value(reply.answer.get("dispatch").cloned().unwrap_or_default())
            .map_err(|error| invalid(error.to_string()))?;
        if dispatch.is_empty() {
            break;
        }
        for upload in dispatch {
            let outcome = carry(edge, store, transfer, owner, &upload).await;
            let settle = Operation::Settle {
                path: upload.path,
                generation: upload.generation,
                lease: upload.lease,
                outcome,
                now: edge.now_ms(),
            };
            let settled = call(store, Some(owner), settle).await?;
            if settled.answer.get("synced") == Some(&Value::Bool(true)) {
                drained.uploaded += 1;
            } else {
                drained.failed += 1;
            }
        }
    }
    Ok(drained)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::{drain, Parts, Sent, Transfer};
    use crate::durable_store::port::{PageStore, StoreHost};
    use crate::gateway_exchange::browser::{facts, rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use crate::transfer::browser::{upload, Link, PartSource, Upload, Uploaded};
    use crate::transfer::Cause;
    use std::future::Future;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    struct Page<'a>(&'a GatewayHost);

    impl Link for Page<'_> {
        fn offline_mode(&self) -> bool {
            facts(self.0).is_ok_and(|facts| facts.offline_mode)
        }
        fn now_ms(&self) -> u64 {
            js_sys::Date::now().max(0.0) as u64
        }
        fn progress(&self, _committed: u64, _total: u64) {}
        fn principal(&self) -> String {
            facts(self.0).map(|facts| facts.principal).unwrap_or_default()
        }
    }

    struct Source<'a, P>(&'a P);

    impl<P: Parts> PartSource for Source<'_, P> {
        fn read(&self, offset: u64, length: u64) -> impl Future<Output = Result<Vec<u8>, Cause>> {
            self.0.read(offset, length)
        }
    }

    struct Storage(GatewayHost);

    impl Transfer for Storage {
        async fn send(&self, session_uri: &str, size: u64, content_type: &str, parts: &impl Parts) -> Sent {
            let request = Upload {
                session_uri,
                total_bytes: size,
                content_type: Some(content_type),
                chunk_bytes: None,
                yield_on_retry: true,
            };
            match upload(request, &Source(parts), &Page(&self.0)).await {
                Uploaded::Done { .. } => Sent::Done,
                Uploaded::Reopen { .. } => Sent::Reopen,
                Uploaded::Failed { cause, .. } => Sent::Failed(cause),
            }
        }
    }

    /// Carry every pending upload this account may start now, and answer
    /// when the next drain is due. Hold the page's per-account lock around
    /// it: one page drains an account at a time.
    #[wasm_bindgen(js_name = drainMediaOutbox, unchecked_return_type = "Drained")]
    pub async fn drain_media_outbox(host: GatewayHost, store: StoreHost) -> Result<JsValue, JsValue> {
        // Imported hosts clone as their JsValue; each port holds its own handle.
        let edge = BrowserEdge::new(host.clone().unchecked_into(), store.clone().unchecked_into());
        let port = PageStore::new(store);
        let drained = drain(&edge, &port, &Storage(host))
            .await
            .map_err(|refusal| rejection(&refusal))?;
        typed(&drained)
    }
}

#[cfg(test)]
#[path = "survey_media_tests.rs"]
mod tests;
