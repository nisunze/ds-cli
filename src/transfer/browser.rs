//! The browser executor of resumable uploads: the shared transfer machine
//! driven with reqwest's WebAssembly client over the page's own `fetch`.
//!
//! One upload PUTs one artifact's bytes to a DS-minted storage session in
//! `Content-Range` parts, probes the session to resume after an interruption,
//! and answers the session's final answer as its outcome. Every decision —
//! what a status or `Range` means, the committed prefix, each retry and its
//! pause, when to reopen or fail — is the machine's. This module performs the
//! one request each action names and reports what it observed.
//!
//! The session URI is the whole credential: it is validated before any
//! request, nothing else is attached, credentials and caches are omitted, and
//! the URI never appears in a cause or an answer. reqwest's WebAssembly client
//! cannot forbid redirects, so an answer from any other URL is refused as the
//! host policy it is; Cloud Storage never redirects a session.

use std::future::Future;
use std::pin::pin;

use futures_util::future::{select, Either};
use futures_util::StreamExt;
use gloo_timers::future::TimeoutFuture;

use super::{
    machine::apply, unanswered, validated_storage_session, Action, Cause, Event, Phase,
    RangeObservation, TransferPlan, TransferState,
};

const PROBE_TIMEOUT_MS: u64 = 30_000;
const MIN_CHUNK_TIMEOUT_MS: u64 = 60_000;
const MIN_THROUGHPUT_BYTES_PER_SECOND: u64 = 8 * 1024;
/// The session's final answer (the stored object's metadata) is small.
const MAX_ANSWER_BYTES: usize = 64 * 1024;
/// Parts sent at one offset that commit nothing before the upload is idle.
const MAX_UNPRODUCTIVE_PARTS: u32 = 6;

/// The bytes of one artifact, read one part at a time.
pub(crate) trait PartSource {
    fn read(&self, offset: u64, length: u64) -> impl Future<Output = Result<Vec<u8>, Cause>>;
}

/// What the page knows about its own link at the moment of a request.
pub(crate) trait Link {
    /// Whether intentional offline mode is on: no part leaves while it is.
    fn offline_mode(&self) -> bool;
    /// Epoch milliseconds, the machine's only clock.
    fn now_ms(&self) -> u64;
    /// Committed bytes after each proven commit.
    fn progress(&self, committed: u64, total: u64);
    /// The signed-in account's stable id: an upload stops, resumable, when it
    /// changes or ends.
    fn principal(&self) -> String;
}

/// One upload's parameters.
pub(crate) struct Upload<'a> {
    pub session_uri: &'a str,
    pub total_bytes: u64,
    pub content_type: Option<&'a str>,
    pub chunk_bytes: Option<u64>,
    /// Answer the first retry's cause as a retryable failure instead of
    /// waiting it out, for a caller whose own durable schedule owns retries.
    pub yield_on_retry: bool,
}

/// How one upload ended.
#[derive(Debug, PartialEq)]
pub(crate) enum Uploaded {
    /// Every byte is committed; `status` and `answer` are the session's
    /// final answer, `answer` bounded and empty when it was not readable.
    Done {
        committed: u64,
        status: u16,
        answer: Vec<u8>,
    },
    /// The session is proven gone; the caller may mint a new one.
    Reopen {
        cause: Cause,
    },
    Failed {
        cause: Cause,
        retryable: bool,
    },
}

enum Answer {
    Response {
        status: u16,
        range: RangeObservation,
        body: Vec<u8>,
    },
    Failure(Cause),
}

fn range(response: &reqwest::Response) -> RangeObservation {
    let headers = response.headers();
    match headers.get("range") {
        // No readable header at all: the platform filtered the answer.
        None if headers.is_empty() => RangeObservation::Unobservable,
        None => RangeObservation::Absent,
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|text| text.trim().strip_prefix("bytes=0-"))
            .and_then(|end| end.parse::<u64>().ok())
            .map_or(RangeObservation::Malformed, |end| RangeObservation::Value {
                end,
            }),
    }
}

async fn within<T>(work: impl Future<Output = T>, deadline_ms: u64) -> Option<T> {
    let deadline = TimeoutFuture::new(u32::try_from(deadline_ms).unwrap_or(u32::MAX));
    match select(pin!(work), deadline).await {
        Either::Left((value, _)) => Some(value),
        Either::Right(_) => None,
    }
}

async fn put(
    client: &reqwest::Client,
    session: &str,
    content_type: Option<&str>,
    content_range: String,
    part: Option<Vec<u8>>,
    timeout_ms: u64,
) -> Answer {
    let mut builder = client
        .put(session)
        .header("Content-Range", content_range)
        .fetch_credentials_omit()
        .fetch_cache_no_store();
    if let Some(part) = part {
        if let Some(content_type) = content_type {
            builder = builder.header("Content-Type", content_type);
        }
        builder = builder.body(part);
    }
    let exchange = async {
        let response = builder.send().await.map_err(|_| unanswered())?;
        if response.url().as_str() != session {
            return Err(Cause::BlockedByHostPolicy);
        }
        let status = response.status().as_u16();
        let range = range(&response);
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let Ok(chunk) = chunk else { break };
            let room = MAX_ANSWER_BYTES.saturating_sub(body.len());
            body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        }
        Ok((status, range, body))
    };
    match within(exchange, timeout_ms).await {
        None => Answer::Failure(Cause::Timeout {
            phase: Phase::Response,
        }),
        Some(Err(cause)) => Answer::Failure(cause),
        Some(Ok((status, range, body))) => Answer::Response {
            status,
            range,
            body,
        },
    }
}

/// Drive one upload to its outcome. Dropping the future cancels it; the
/// session stays resumable from the server's own committed prefix.
pub(crate) async fn upload(
    upload: Upload<'_>,
    source: &impl PartSource,
    link: &impl Link,
) -> Uploaded {
    let Ok(session) = validated_storage_session(upload.session_uri) else {
        return Uploaded::Failed {
            cause: Cause::BlockedByHostPolicy,
            retryable: false,
        };
    };
    let client = reqwest::Client::new();
    let principal = link.principal();
    let plan = TransferPlan {
        total_bytes: upload.total_bytes,
        chunk_bytes: upload.chunk_bytes,
    };
    let mut state: Option<TransferState> = None;
    let mut event = Event::Start;
    let mut last = (0_u16, Vec::new());
    // A server that answers each part without ever advancing is reported as
    // an idle transfer, not looped on.
    let (mut last_offset, mut unproductive) = (None, 0_u32);
    let mut reported = 0_u64;
    loop {
        let reply = match apply(state.take(), Some(plan.clone()), event, link.now_ms()) {
            Ok(reply) => reply,
            Err(_) => {
                return Uploaded::Failed {
                    cause: Cause::Inconclusive,
                    retryable: false,
                }
            }
        };
        let total = reply.state.total_bytes;
        let committed = reply.state.committed;
        if committed > reported {
            reported = committed;
            link.progress(committed, total);
        }
        state = Some(reply.state);
        event = match reply.action {
            Action::Done { committed } => {
                let (status, answer) = std::mem::take(&mut last);
                return Uploaded::Done {
                    committed,
                    status,
                    answer,
                };
            }
            Action::Reopen { cause } => return Uploaded::Reopen { cause },
            Action::Fail { cause, retryable } => return Uploaded::Failed { cause, retryable },
            Action::Wait { cause, .. } if upload.yield_on_retry => {
                return Uploaded::Failed {
                    cause,
                    retryable: true,
                }
            }
            Action::Wait { until, .. } => {
                let pause = until.saturating_sub(link.now_ms());
                if pause > 0 {
                    TimeoutFuture::new(u32::try_from(pause).unwrap_or(u32::MAX)).await;
                }
                Event::Start
            }
            _ if link.principal() != principal => Event::Cancelled,
            _ if link.offline_mode() => Event::TransportFailure {
                cause: Cause::Offline,
                phase: Phase::Connect,
                bytes_written: 0,
            },
            Action::Probe => {
                match put(
                    &client,
                    &session,
                    upload.content_type,
                    format!("bytes */{total}"),
                    None,
                    PROBE_TIMEOUT_MS,
                )
                .await
                {
                    Answer::Response {
                        status,
                        range,
                        body,
                    } => {
                        last = (status, body);
                        Event::ProbeResponse { status, range }
                    }
                    Answer::Failure(cause) => Event::TransportFailure {
                        cause,
                        phase: Phase::Response,
                        bytes_written: 0,
                    },
                }
            }
            Action::SendChunk { offset, .. }
                if last_offset == Some(offset) && unproductive + 1 >= MAX_UNPRODUCTIVE_PARTS =>
            {
                return Uploaded::Failed {
                    cause: Cause::Timeout { phase: Phase::Idle },
                    retryable: true,
                }
            }
            Action::SendChunk { offset, length } => match {
                if last_offset == Some(offset) {
                    unproductive += 1;
                } else {
                    (last_offset, unproductive) = (Some(offset), 0);
                }
                source.read(offset, length).await
            } {
                Err(cause) => Event::TransportFailure {
                    cause,
                    phase: Phase::Connect,
                    bytes_written: 0,
                },
                Ok(part) if part.len() as u64 != length => Event::IntegrityFailure {
                    cause: Cause::LengthMismatch,
                },
                Ok(part) => {
                    let end = offset + length - 1;
                    let timeout_ms = MIN_CHUNK_TIMEOUT_MS
                        .max(length.saturating_mul(1000) / MIN_THROUGHPUT_BYTES_PER_SECOND);
                    match put(
                        &client,
                        &session,
                        upload.content_type,
                        format!("bytes {offset}-{end}/{total}"),
                        Some(part),
                        timeout_ms,
                    )
                    .await
                    {
                        Answer::Response {
                            status,
                            range,
                            body,
                        } => {
                            last = (status, body);
                            Event::ChunkResponse {
                                status,
                                range,
                                bytes_written: length,
                            }
                        }
                        // A browser cannot say how much reached the socket: the
                        // machine probes rather than assume either way.
                        Answer::Failure(cause) => Event::TransportFailure {
                            cause,
                            phase: Phase::Response,
                            bytes_written: 0,
                        },
                    }
                }
            },
        };
    }
}

/// The typed browser export Svelte calls for one resumable upload, and the
/// same upload for Kernel flows that send bytes between two exchanges.
pub(crate) mod exports {
    use js_sys::{Function, Promise, Uint8Array};
    use serde::Serialize;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{AbortSignal, Blob};

    use super::{upload, within, Link, PartSource, Upload, Uploaded};
    use crate::gateway_exchange::browser::{facts, typed, GatewayHost};
    use crate::gateway_exchange::Refusal;
    use crate::transfer::{single_put_end, unanswered, validated_storage_session, Cause};

    /// The most bytes one single-request upload carries.
    const SINGLE_PUT_MAX_BYTES: u64 = 64 * 1024 * 1024;
    const SINGLE_PUT_DEFAULT_TIMEOUT_MS: u64 = 120_000;
    const SINGLE_PUT_MAX_TIMEOUT_MS: u64 = 600_000;

    #[wasm_bindgen(typescript_custom_section)]
    const UPLOAD_SOURCE: &'static str = r#"
/** A bounded byte source: a Blob or File, or any object answering the same slice contract. */
export interface UploadSource {
    readonly size: number;
    slice(start: number, end: number): Blob | Promise<Blob>;
}
"#;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(typescript_type = "UploadSource")]
        pub type UploadSource;

        #[wasm_bindgen(method, getter)]
        pub(crate) fn size(this: &UploadSource) -> f64;

        #[wasm_bindgen(method, catch)]
        fn slice(this: &UploadSource, start: f64, end: f64) -> Result<JsValue, JsValue>;
    }

    /// The refusal of a flow whose upload did not finish.
    pub const UPLOAD_FAILED: &str = "upload_failed";

    /// How one resumable upload ended, as the page reads it.
    #[derive(Serialize, tsify::Tsify)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum UploadOutcome {
        /// Every byte is committed; `answer` is the session's final answer.
        Done {
            committed: u64,
            status: u16,
            answer: String,
        },
        /// The session is proven gone; mint a new one to try again.
        Reopen { cause: Cause, message: String },
        Failed {
            cause: Cause,
            retryable: bool,
            message: String,
        },
    }

    /// What storage has acknowledged: `pct` reaches 100 only on the final
    /// commit, so bytes handed to a socket never read as progress.
    #[derive(Serialize, tsify::Tsify)]
    pub struct UploadProgress {
        pub committed: u64,
        pub total: u64,
        pub pct: u8,
    }

    struct Parts(UploadSource);

    impl PartSource for Parts {
        async fn read(&self, offset: u64, length: u64) -> Result<Vec<u8>, Cause> {
            let part = self
                .0
                .slice(offset as f64, (offset + length) as f64)
                .map_err(|_| Cause::Inconclusive)?;
            let blob: Blob = JsFuture::from(Promise::resolve(&part))
                .await
                .ok()
                .and_then(|part| part.dyn_into().ok())
                .ok_or(Cause::Inconclusive)?;
            let bytes = JsFuture::from(blob.array_buffer())
                .await
                .map_err(|_| Cause::Inconclusive)?;
            Ok(Uint8Array::new(&bytes).to_vec())
        }
    }

    struct Page<'a> {
        host: &'a GatewayHost,
        progress: Option<Function>,
    }

    impl Link for Page<'_> {
        fn offline_mode(&self) -> bool {
            facts(self.host).is_ok_and(|facts| facts.offline_mode)
        }

        fn principal(&self) -> String {
            facts(self.host)
                .map(|facts| facts.principal)
                .unwrap_or_default()
        }

        fn now_ms(&self) -> u64 {
            js_sys::Date::now().max(0.0) as u64
        }

        fn progress(&self, committed: u64, total: u64) {
            let Some(progress) = &self.progress else {
                return;
            };
            let pct = if committed >= total {
                100
            } else {
                (committed.saturating_mul(100) / total.max(1)).min(99) as u8
            };
            if let Ok(report) = typed(&UploadProgress {
                committed,
                total,
                pct,
            }) {
                let _ = progress.call1(&JsValue::NULL, &report);
            }
        }
    }

    /// The caller's abort signal as a future, its listener removed when the
    /// upload ends so a long-lived signal never accumulates listeners.
    struct Abort {
        signal: AbortSignal,
        listener: Function,
        fired: JsFuture,
    }

    impl Abort {
        fn new(signal: AbortSignal) -> Self {
            let mut listener = None;
            let fired = Promise::new(&mut |resolve, _| listener = Some(resolve));
            let listener = listener.expect("a promise runs its executor at once");
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

    impl Drop for Abort {
        fn drop(&mut self) {
            let _ = self
                .signal
                .remove_event_listener_with_callback("abort", &self.listener);
        }
    }

    impl UploadOutcome {
        /// A finished upload, or the refusal a flow that uploaded rejects with.
        pub(crate) fn finished(self) -> Result<(), Refusal> {
            let (cause, message, retryable) = match self {
                UploadOutcome::Done { .. } => return Ok(()),
                UploadOutcome::Reopen { cause, message } => (cause, message, true),
                UploadOutcome::Failed {
                    cause,
                    message,
                    retryable,
                } => (cause, message, retryable),
            };
            let mut refusal = Refusal::new(UPLOAD_FAILED, message);
            refusal.status = match cause {
                Cause::HttpStatus { status } => Some(status),
                _ => None,
            };
            refusal.retryable = retryable;
            Err(refusal)
        }
    }

    /// PUT one byte source to a DS-minted resumable storage session, resuming
    /// after an interruption, until the session's final answer. `progress`
    /// is told each acknowledged commit; aborting `signal` stops the upload
    /// with the session left resumable.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn send(
        host: &GatewayHost,
        session_uri: &str,
        source: UploadSource,
        content_type: Option<&str>,
        chunk_bytes: Option<u64>,
        yield_on_retry: bool,
        progress: Option<Function>,
        signal: Option<AbortSignal>,
    ) -> UploadOutcome {
        let total_bytes = source.size().max(0.0) as u64;
        let page = Page { host, progress };
        let parts = Parts(source);
        let request = Upload {
            session_uri,
            total_bytes,
            content_type,
            chunk_bytes,
            yield_on_retry,
        };
        let running = std::pin::pin!(upload(request, &parts, &page));
        let outcome = match signal {
            None => running.await,
            Some(signal) => {
                let mut abort = Abort::new(signal);
                match futures_util::future::select(running, &mut abort.fired).await {
                    futures_util::future::Either::Left((uploaded, _)) => uploaded,
                    futures_util::future::Either::Right(_) => Uploaded::Failed {
                        cause: Cause::Cancelled,
                        retryable: true,
                    },
                }
            }
        };
        match outcome {
            Uploaded::Done {
                committed,
                status,
                answer,
            } => UploadOutcome::Done {
                committed,
                status,
                answer: String::from_utf8_lossy(&answer).into_owned(),
            },
            Uploaded::Reopen { cause } => UploadOutcome::Reopen {
                message: cause.describe(),
                cause,
            },
            Uploaded::Failed { cause, retryable } => UploadOutcome::Failed {
                message: cause.describe(),
                cause,
                retryable,
            },
        }
    }

    /// One resumable upload, as Svelte calls it.
    #[wasm_bindgen(js_name = resumableUpload, unchecked_return_type = "UploadOutcome")]
    #[allow(clippy::too_many_arguments)]
    pub async fn resumable_upload(
        host: GatewayHost,
        session_uri: String,
        source: UploadSource,
        content_type: Option<String>,
        chunk_bytes: Option<f64>,
        yield_on_retry: bool,
        #[wasm_bindgen(
            unchecked_param_type = "((progress: UploadProgress) => void) | null | undefined"
        )]
        progress: Option<Function>,
        signal: Option<AbortSignal>,
    ) -> Result<JsValue, JsValue> {
        let chunk_bytes = chunk_bytes.map(|bytes| bytes.max(0.0) as u64);
        typed(
            &send(
                &host,
                &session_uri,
                source,
                content_type.as_deref(),
                chunk_bytes,
                yield_on_retry,
                progress,
                signal,
            )
            .await,
        )
    }

    fn failed(cause: Cause, retryable: bool) -> UploadOutcome {
        UploadOutcome::Failed {
            message: cause.describe(),
            cause,
            retryable,
        }
    }

    /// PUT one byte source to a DS-minted storage URL in a single bounded
    /// request: the URL admitted by the shared session rule, only its
    /// content type sent, and no credentials.
    pub(crate) async fn single_put(
        host: &GatewayHost,
        url: &str,
        source: UploadSource,
        content_type: &str,
        timeout_ms: u64,
        signal: Option<AbortSignal>,
    ) -> UploadOutcome {
        let Ok(url) = validated_storage_session(url) else {
            return failed(Cause::BlockedByHostPolicy, false);
        };
        let Ok(before) = facts(host) else {
            return failed(Cause::Inconclusive, false);
        };
        if before.offline_mode {
            return failed(Cause::Offline, true);
        }
        let size = source.size().max(0.0) as u64;
        if size > SINGLE_PUT_MAX_BYTES {
            return failed(Cause::LengthMismatch, false);
        }
        let bytes = match Parts(source).read(0, size).await {
            Ok(bytes) if bytes.len() as u64 == size => bytes,
            Ok(_) => return failed(Cause::LengthMismatch, false),
            Err(cause) => return failed(cause, false),
        };
        let sent = async {
            let response = reqwest::Client::new()
                .put(&url)
                .header("Content-Type", content_type)
                .fetch_credentials_omit()
                .fetch_cache_no_store()
                .body(bytes)
                .send()
                .await
                .map_err(|_| unanswered())?;
            // A redirect away from the admitted URL is the host policy refusing it.
            Ok((response.url().as_str() == url, response.status().as_u16()))
        };
        let bounded = std::pin::pin!(within(sent, timeout_ms));
        let answer = match signal {
            None => bounded.await,
            Some(signal) => {
                let mut abort = Abort::new(signal);
                match futures_util::future::select(bounded, &mut abort.fired).await {
                    futures_util::future::Either::Left((answer, _)) => answer,
                    futures_util::future::Either::Right(_) => {
                        return failed(Cause::Cancelled, true)
                    }
                }
            }
        };
        if facts(host).map_or(true, |after| after.principal != before.principal) {
            return failed(Cause::Cancelled, false);
        }
        let answer = match answer {
            Some(Ok((false, _))) => return failed(Cause::BlockedByHostPolicy, false),
            Some(Ok((true, status))) => Some(Ok(status)),
            Some(Err(cause)) => Some(Err(cause)),
            None => None,
        };
        match single_put_end(answer) {
            Ok(status) => UploadOutcome::Done {
                committed: size,
                status,
                answer: String::new(),
            },
            Err((cause, retryable)) => failed(cause, retryable),
        }
    }

    /// One single-request upload to a storage URL, as Svelte calls it.
    /// `timeout_ms` defaults to two minutes and is bounded to ten.
    #[wasm_bindgen(js_name = singlePutUpload, unchecked_return_type = "UploadOutcome")]
    pub async fn single_put_upload(
        host: GatewayHost,
        url: String,
        source: UploadSource,
        content_type: String,
        timeout_ms: Option<f64>,
        signal: Option<AbortSignal>,
    ) -> Result<JsValue, JsValue> {
        let timeout_ms = timeout_ms
            .map(|ms| (ms.max(1.0) as u64).min(SINGLE_PUT_MAX_TIMEOUT_MS))
            .unwrap_or(SINGLE_PUT_DEFAULT_TIMEOUT_MS);
        typed(&single_put(&host, &url, source, &content_type, timeout_ms, signal).await)
    }
}
