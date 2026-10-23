//! Survey media on its way to the bucket: the gateway's start and
//! completion of one object's upload, and the carry of one upload the media
//! outbox ([`crate::media_outbox`]) dispatched through them and the resumable
//! storage transfer. The survey queue's drain
//! ([`super::survey_queue`]) leases each upload as a step of its entry and
//! settles what the carry observed.
//!
//! The outbox holds the bytes (`read`) and settles each attempt (`settle`);
//! the carry performs the exchanges in order and decides nothing else. An
//! upload is an ordered plan: the original first, then its thumbnail, whose
//! start names the original's verified generation, so a thumbnail never
//! appears for an original that failed.

use serde::Deserialize;
use serde_json::{Value, json};

use super::retained::{Edge, store_refusal};
use super::{Admitted, Exchange, NETWORK_OFFLINE, Plan, REQUEST_TIMEOUT, Refusal, Request, data};
use crate::byte_parts::PART_BYTES;
use crate::durable_store::port::{Reply, StorePort};
use crate::media_outbox::{Operation, Target as Role};
use crate::survey_media_address;
use crate::survey_photo_outbox::TransportOutcome;
use crate::survey_photo_transport;
use crate::transfer::Cause;

const PATH: &str = "/api/v1/media";
pub const INVALID: &str = "survey_media_answer_invalid";

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
    /// A thumbnail's original: its stored file name, verified generation and
    /// the clockwise quarter-turns the thumbnail shows.
    pub source: Option<(String, String, u8)>,
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
        if let Some((file, generation, quarter_turns)) = &self.source {
            body["source_file_name"] = json!(file);
            body["source_generation"] = json!(generation);
            body["source_quarter_turns"] = json!(quarter_turns);
        }
        Ok(body)
    }
}

fn invalid(message: String) -> Refusal {
    Refusal::new(INVALID, message)
}

/// Inspect an exact existing storage object before a replacement. Its signed
/// source must identify both the original path and observed generation.
#[derive(Clone, Debug)]
pub struct Inspect(pub String);

impl Exchange for Inspect {
    type Output = survey_photo_transport::Inspection;
    fn plan(&self) -> Result<Plan, Refusal> {
        let address = survey_media_address::parse(&self.0).map_err(invalid)?;
        let mut body = survey_media_address::wire(&address);
        body["action"] = json!("survey_upload_inspect");
        Ok(Plan {
            request: Request::post(PATH, body),
            retries: 0,
        })
    }
    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        survey_photo_transport::inspect(data(&body), &self.0)
            .map(Admitted::Done)
            .map_err(invalid)
    }
}

fn inspected(
    answer: Result<survey_photo_transport::Inspection, Refusal>,
    allow_missing: bool,
) -> Result<Option<survey_photo_transport::Inspection>, Refusal> {
    match answer {
        Err(refusal) if allow_missing && refusal.status == Some(404) => Ok(None),
        answer => answer.map(Some),
    }
}

/// Optional absence is exactly a 404, never an authorization or link refusal.
pub async fn inspect(
    edge: &impl Edge,
    path: String,
    allow_missing: bool,
) -> Result<Option<survey_photo_transport::Inspection>, Refusal> {
    inspected(edge.fetch(Inspect(path)).await, allow_missing)
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
            let generation =
                survey_photo_transport::generation(&issued["generation"]).map_err(invalid)?;
            return Ok(Admitted::Done(Started::Present { generation }));
        }
        let uri =
            survey_photo_transport::session(&issued["session_uri"], &bucket).map_err(invalid)?;
        Ok(Admitted::Done(Started::Session { uri }))
    }
}

/// Ask the gateway to verify the stored object; answers its generation.
pub struct Complete(pub Upload);

impl Exchange for Complete {
    type Output = String;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request::post(
            PATH,
            self.0.body("survey_upload_complete").map_err(invalid)?,
        )
        .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let verified = data(&body);
        survey_photo_transport::target(verified, &self.0.path).map_err(invalid)?;
        if verified["already_present"] != true {
            return Err(invalid("Media completion was not verified".into()));
        }
        let generation =
            survey_photo_transport::generation(&verified["generation"]).map_err(invalid)?;
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
    async fn send(
        &self,
        session_uri: &str,
        size: u64,
        content_type: &str,
        parts: &impl Parts,
    ) -> Sent;
}

/// One upload the outbox dispatched, as `claim` answers it.
#[derive(Debug, Deserialize)]
pub(super) struct Dispatch {
    pub path: String,
    pub generation: String,
    pub lease: u64,
    targets: Vec<Target>,
}

/// Before each part of a transfer: keep the delivery's lease, or answer
/// false when it is lost and the transfer stops.
#[allow(async_fn_in_trait)]
pub(super) trait Renew {
    async fn renew(&self) -> bool;
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
    #[serde(default)]
    source: Option<crate::survey_photo_outbox::Source>,
    #[serde(default)]
    quarter_turns: u8,
    target: Role,
    sha256: String,
}

async fn call(
    store: &impl StorePort,
    owner: Option<&str>,
    operation: Operation,
) -> Result<Reply, Refusal> {
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
struct Bytes<'a, S, R> {
    store: &'a S,
    renew: &'a R,
    owner: &'a str,
    path: &'a str,
    generation: &'a str,
    role: Role,
}

impl<S: StorePort, R: Renew> Parts for Bytes<'_, S, R> {
    async fn read(&self, offset: u64, length: u64) -> Result<Vec<u8>, Cause> {
        if !self.renew.renew().await {
            return Err(Cause::Cancelled);
        }
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
pub(super) async fn carry(
    edge: &impl Edge,
    store: &impl StorePort,
    transfer: &impl Transfer,
    owner: &str,
    upload: &Dispatch,
    renew: &impl Renew,
) -> TransportOutcome {
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
                // A thumbnail refresh names its stored original itself.
                Role::Original => target.source.as_ref().and_then(|source| {
                    let file = survey_media_address::parse(&source.object_path).ok()?.file;
                    Some((file, source.generation.clone(), source.quarter_turns))
                }),
                Role::Companion => original
                    .clone()
                    .map(|(file, generation)| (file, generation, target.quarter_turns)),
            },
        };
        let generation = match edge.fetch(Start(call.clone())).await {
            Err(refusal) => return refused(refusal),
            Ok(Started::Present { generation }) => generation,
            Ok(Started::Session { uri }) => {
                let bytes = Bytes {
                    store,
                    renew,
                    owner,
                    path: &upload.path,
                    generation: &upload.generation,
                    role: target.target,
                };
                match transfer
                    .send(&uri, target.size, &target.content_type, &bytes)
                    .await
                {
                    Sent::Done => {}
                    Sent::Reopen | Sent::Failed(Cause::Cancelled) => {
                        return TransportOutcome::Cancelled;
                    }
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

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(super) mod browser {
    use super::{Parts, Sent, Transfer};
    use crate::gateway_exchange::browser::{GatewayHost, attempt, facts, rejection, typed};
    use crate::transfer::Cause;
    use crate::transfer::browser::{Link, PartSource, Upload, Uploaded, upload};
    use js_sys::{Function, Promise};
    use std::future::Future;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::AbortSignal;

    struct InspectAbort {
        signal: AbortSignal,
        listener: Function,
        fired: JsFuture,
    }
    impl InspectAbort {
        fn new(signal: AbortSignal) -> Self {
            let mut listener = None;
            let fired = Promise::new(&mut |resolve, _| listener = Some(resolve));
            let listener = listener.expect("a promise invokes its executor immediately");
            if signal.aborted() {
                let _ = listener.call0(&JsValue::NULL);
            } else {
                let _ = signal.add_event_listener_with_callback("abort", &listener);
            }
            Self {
                signal,
                listener,
                fired: JsFuture::from(fired),
            }
        }
    }
    impl Drop for InspectAbort {
        fn drop(&mut self) {
            let _ = self
                .signal
                .remove_event_listener_with_callback("abort", &self.listener);
        }
    }

    /// Cancelling drops the exchange and aborts its unfinished request.
    #[wasm_bindgen(js_name = surveyMediaInspect)]
    pub async fn survey_media_inspect(
        host: GatewayHost,
        path: String,
        allow_missing: bool,
        signal: Option<AbortSignal>,
    ) -> Result<JsValue, JsValue> {
        if signal.as_ref().is_some_and(AbortSignal::aborted) {
            return Err(rejection(&super::Refusal::new(
                "survey_media_cancelled",
                "Media inspection was cancelled.",
            )));
        }
        let running = Box::pin(async {
            let answer = attempt(&host, super::Inspect(path)).await?;
            typed(&super::inspected(answer, allow_missing).map_err(|refusal| rejection(&refusal))?)
        });
        match signal {
            None => running.await,
            Some(signal) => {
                let mut abort = InspectAbort::new(signal);
                match futures_util::future::select(running, &mut abort.fired).await {
                    futures_util::future::Either::Left((answer, _)) => answer,
                    futures_util::future::Either::Right(_) => Err(rejection(&super::Refusal::new(
                        "survey_media_cancelled",
                        "Media inspection was cancelled.",
                    ))),
                }
            }
        }
    }

    /// The original a rotation turns, read exactly: inspected at its current
    /// generation, then read from the generation-pinned signature that
    /// inspection answered — without any DS credential, within the media
    /// admission limit, and exactly the size the inspection declared.
    #[wasm_bindgen(js_name = surveyMediaSourceBytes, unchecked_return_type = "SurveyMediaSourceBytes")]
    pub async fn survey_media_source_bytes(
        host: GatewayHost,
        path: String,
        signal: Option<AbortSignal>,
    ) -> Result<JsValue, JsValue> {
        use crate::gateway_exchange::storage::{self, browser::signed_get};
        let work = async {
            let principal = facts(&host)?.principal;
            let inspection =
                crate::gateway_exchange::browser::run(&host, super::Inspect(path)).await?;
            let bound = crate::survey_photo_media::MAX_INPUT_BYTES as u64;
            if inspection.size.is_some_and(|size| size > bound) {
                return Err(rejection(&storage::too_large(bound)));
            }
            let now = facts(&host)?;
            if now.offline_mode {
                return Err(rejection(&crate::gateway_exchange::offline_refusal()));
            }
            let read = signed_get(
                &inspection.source_url,
                inspection.size.unwrap_or(bound),
                now.link_up,
            )
            .await;
            let bytes = storage::admit_declared_size(inspection.size, read.map(|read| read.bytes))
                .map_err(|refusal| rejection(&refusal))?;
            if facts(&host)?.principal != principal {
                return Err(rejection(
                    &crate::gateway_exchange::session_changed_refusal(),
                ));
            }
            // The bytes are the answer; the spent signature stays here.
            let described = typed(&inspection)?;
            js_sys::Reflect::delete_property(&described.clone().into(), &"source_url".into())?;
            let answer = js_sys::Object::new();
            js_sys::Reflect::set(&answer, &"inspection".into(), &described)?;
            js_sys::Reflect::set(
                &answer,
                &"bytes".into(),
                &js_sys::Uint8Array::from(&bytes[..]),
            )?;
            Ok(JsValue::from(answer))
        };
        storage::browser::until_aborted(work, signal).await
    }

    #[wasm_bindgen(typescript_custom_section)]
    const SOURCE_TYPES: &'static str = r#"
/** One survey original, inspected and read at the generation it was inspected at. */
export interface SurveyMediaSourceBytes {
    inspection: { blob_path: string; bucket: string; generation: string; revision_protocol?: number; size?: number };
    bytes: Uint8Array;
}
"#;

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
            facts(self.0)
                .map(|facts| facts.principal)
                .unwrap_or_default()
        }
    }

    struct Source<'a, P>(&'a P);

    impl<P: Parts> PartSource for Source<'_, P> {
        fn read(&self, offset: u64, length: u64) -> impl Future<Output = Result<Vec<u8>, Cause>> {
            self.0.read(offset, length)
        }
    }

    /// The resumable storage transfer on the page's own link.
    pub(in crate::gateway_exchange) struct Storage(pub GatewayHost);

    impl Transfer for Storage {
        async fn send(
            &self,
            session_uri: &str,
            size: u64,
            content_type: &str,
            parts: &impl Parts,
        ) -> Sent {
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
}

#[cfg(test)]
#[path = "survey_media_inspect_tests.rs"]
mod inspect_tests;
