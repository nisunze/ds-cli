//! Signed reads of stored objects. Storage keeps canonical object addresses
//! (`gs://bucket/object`, or a storage URL an older surface kept); a fresh
//! signed URL is minted at the moment of a read by ds-brain (`POST /report`
//! `sign_url`), which signs only the objects its caller may read, and is used
//! once, never kept. [`storage_path`] reads an address; the browser reads a
//! signed URL's bytes against the size and digest a caller holds.
//!
//! Which object a record's declared reference names is decided here too
//! ([`StoredObject::resolve`]): the page never builds a storage URL, guesses
//! a bucket or walks a fallback chain. A read of one stored object
//! ([`StoredObjectRead`]) is one exchange: sign exactly that object
//! ([`SignObject`]), read its bytes in the page without any DS credential,
//! and answer them or one typed refusal — missing, forbidden, expired, too
//! large, not the declared bytes, or not readable now.

use ds_gateway_client::Operation;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

/// Brain signs a batch in one answer of at most a few kilobytes per path.
const RESPONSE_CAP: u64 = 4 * 1024 * 1024;
pub const PATHS_MAX: usize = 500;
pub const INTEGRITY: &str = "storage_bytes_integrity";
pub const READ_FAILED: &str = "storage_read_failed";
/// The read request is not one stored object and a bound.
pub const READ_REQUEST_INVALID: &str = "storage_read_request_invalid";
/// The signer's answer does not sign exactly the object asked for.
pub const READ_RESPONSE_INVALID: &str = "storage_read_response_invalid";
/// Storage holds no object under that name.
pub const OBJECT_MISSING: &str = "storage_object_missing";
/// The signer refused to sign the object for this account, or storage
/// refused the signature.
pub const READ_FORBIDDEN: &str = "storage_read_forbidden";
/// Storage answered that the signature had expired.
pub const READ_EXPIRED: &str = "storage_read_expired";
/// The object is larger than the read's bound.
pub const READ_TOO_LARGE: &str = "storage_read_too_large";
/// The caller stopped waiting.
pub const READ_CANCELLED: &str = "storage_read_cancelled";
/// A stored object's name is at most this many bytes (Cloud Storage's own
/// limit); a bucket name at most 222 characters.
const OBJECT_NAME_MAX: usize = 1024;
const BUCKET_NAME_MAX: usize = 222;
/// How much of a refused read's body is read to learn why it was refused.
pub const ERROR_BODY_CAP: usize = 4 * 1024;
/// How much of the signer's refusal reason an operator is shown.
const REASON_MAX_CHARS: usize = 200;
/// A signed URL longer than this is not one Brain mints.
const SIGNED_URL_MAX: usize = 8 * 1024;
/// The one host Brain's signer answers for.
const SIGNED_HOST: &str = "storage.googleapis.com";
/// The bucket domains a bare reference may name as its first segment.
const BUCKET_DOMAINS: [&str; 2] = [".firebasestorage.app", ".appspot.com"];

/// The `bucket/object` an object address names, or `None` when it is not a
/// storage address: `gs://bucket/object`, a Firebase download URL, or a
/// storage URL (signed or not).
pub fn storage_path(address: &str) -> Option<String> {
    let address = address.trim();
    if let Some(rest) = address.strip_prefix("gs://") {
        let (bucket, object) = rest.split_once('/')?;
        return (!bucket.is_empty() && !object.is_empty()).then(|| rest.to_owned());
    }
    let parsed = url::Url::parse(address).ok()?;
    if !matches!(parsed.scheme(), "https" | "http") {
        return None;
    }
    let decoded = |text: &str| {
        percent_encoding::percent_decode_str(text)
            .decode_utf8()
            .ok()
            .map(|text| text.into_owned())
    };
    let path = parsed.path().trim_start_matches('/');
    let found = match parsed.host_str()? {
        "firebasestorage.googleapis.com" => {
            let rest = path.strip_prefix("v0/b/")?;
            let (bucket, object) = rest.split_once("/o/")?;
            format!("{bucket}/{}", decoded(object)?)
        }
        "storage.googleapis.com" | "storage.cloud.google.com" => decoded(path)?,
        _ => return None,
    };
    let (bucket, object) = found.split_once('/')?;
    (!bucket.is_empty() && !object.is_empty()).then_some(found)
}

/// The bucket this build's deployment keeps its media in: Brain's
/// `sign_url` signs in no other (`FIREBASE_BUCKET`, the Firebase project's
/// default bucket). `None` when the build names no Firebase project.
pub fn media_bucket() -> Option<String> {
    crate::gateway_target::FIREBASE_PROJECT.map(|project| format!("{project}.firebasestorage.app"))
}

/// One stored object: the bucket it lies in and its name there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct StoredObject {
    pub bucket: String,
    pub object: String,
}

impl StoredObject {
    /// The object a record's declared storage reference names.
    ///
    /// A storage address (`gs://`, a Firebase download URL, a storage URL,
    /// signed or not) names its own bucket. A bare name may carry its bucket
    /// as its first segment (`<bucket>.firebasestorage.app/…`); otherwise it
    /// lies in `media_bucket`, the one bucket the deployment signs in. Only
    /// the decoration a host put around the name goes — surrounding
    /// whitespace, a query or fragment, leading slashes. Nothing inside the
    /// name is repaired: a malformed name is handed on as written, and the
    /// signer refuses it. Any other scheme (`data:`, `blob:`, a web page, a
    /// local file) names no stored object.
    pub fn resolve(reference: &str, media_bucket: Option<&str>) -> Option<Self> {
        let reference = reference.trim();
        if reference.is_empty() {
            return None;
        }
        if let Some(path) = storage_path(reference) {
            let (bucket, object) = path.split_once('/')?;
            return Some(Self {
                bucket: bucket.to_owned(),
                object: object.to_owned(),
            });
        }
        if has_scheme(reference) {
            return None;
        }
        let name = reference
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .trim()
            .trim_start_matches('/');
        if name.is_empty() {
            return None;
        }
        if let Some((first, rest)) = name.split_once('/') {
            let lower = first.to_ascii_lowercase();
            if BUCKET_DOMAINS
                .iter()
                .any(|domain| lower.len() > domain.len() && lower.ends_with(domain))
            {
                return (!rest.is_empty()).then(|| Self {
                    bucket: first.to_owned(),
                    object: rest.to_owned(),
                });
            }
        }
        Some(Self {
            bucket: media_bucket.filter(|bucket| !bucket.is_empty())?.to_owned(),
            object: name.to_owned(),
        })
    }

    /// The `bucket/object` the signer is asked for.
    pub fn path(&self) -> String {
        format!("{}/{}", self.bucket, self.object)
    }

    /// Whether this names one object a signer could be asked for.
    fn check(&self) -> Result<(), Refusal> {
        let control = |text: &str| text.chars().any(char::is_control);
        let bucket = &self.bucket;
        if bucket.is_empty()
            || bucket.len() > BUCKET_NAME_MAX
            || bucket.contains('/')
            || bucket.chars().any(char::is_whitespace)
            || control(bucket.as_str())
            || self.object.is_empty()
            || self.object.len() > OBJECT_NAME_MAX
            || control(&self.object)
        {
            return Err(Refusal::new(
                READ_REQUEST_INVALID,
                "A stored object read names one bucket and one object name.",
            ));
        }
        Ok(())
    }

    /// The signed URL Brain answered for this object, when it reads exactly
    /// this object on the one host Brain signs for.
    fn signed(&self, url: &str) -> Result<String, Refusal> {
        let invalid = || {
            Refusal::new(
                READ_RESPONSE_INVALID,
                "The signer answered a URL that does not read the object asked for.",
            )
        };
        let parsed = url::Url::parse(url).map_err(|_| invalid())?;
        let exact = url.len() <= SIGNED_URL_MAX
            && parsed.scheme() == "https"
            && parsed.host_str() == Some(SIGNED_HOST)
            && parsed.port().is_none()
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.fragment().is_none()
            && storage_path(url).as_deref() == Some(self.path().as_str());
        exact.then(|| url.to_owned()).ok_or_else(invalid)
    }
}

/// Whether a reference begins with a URL scheme (`data:`, `https:`, `C:`).
fn has_scheme(reference: &str) -> bool {
    let Some((scheme, _)) = reference.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Fresh signed URLs for object addresses, keyed by the `bucket/object` each
/// names. An address that is not a storage address, or an object the caller
/// may not read, has no entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignPaths {
    pub paths: Vec<String>,
}

impl SignPaths {
    /// The distinct storage paths among `addresses`, in order.
    pub fn of(addresses: &[String]) -> Self {
        let mut paths: Vec<String> = Vec::new();
        for path in addresses.iter().filter_map(|address| storage_path(address)) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        Self { paths }
    }
}

impl Exchange for SignPaths {
    type Output = BTreeMap<String, String>;

    fn plan(&self) -> Result<Plan, Refusal> {
        if self.paths.is_empty() || self.paths.len() > PATHS_MAX {
            return Err(Refusal::new(
                "storage_sign_input_invalid",
                format!("A signing request names 1 to {PATHS_MAX} stored objects."),
            ));
        }
        let body = json!({"action": "sign_url", "paths": self.paths});
        Ok(Request::generated(Operation::DsSystemReport, Some(body))?
            .response_cap(RESPONSE_CAP)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let urls = data(&body)
            .get("urls")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter(|(path, _)| self.paths.contains(path))
            .filter_map(|(path, url)| {
                let url = url.as_str()?;
                let parsed = url::Url::parse(url).ok()?;
                matches!(parsed.scheme(), "https" | "http").then(|| (path.clone(), url.to_owned()))
            })
            .collect();
        Ok(Admitted::Done(urls))
    }
}

/// A fresh signed GET of exactly one stored object, or why it has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignObject(pub StoredObject);

impl Exchange for SignObject {
    type Output = String;

    fn plan(&self) -> Result<Plan, Refusal> {
        self.0.check()?;
        let body = json!({"action": "sign_url", "paths": [self.0.path()]});
        Ok(Request::generated(Operation::DsSystemReport, Some(body))?
            .response_cap(RESPONSE_CAP)
            .into())
    }

    /// Brain names a path it will not sign in `refused`, with its reason; a
    /// path it could not sign is in neither map. Other paths and fields are
    /// additive and ignored.
    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let malformed = || {
            Refusal::new(
                READ_RESPONSE_INVALID,
                "The signer's answer is not a signing of the object asked for.",
            )
        };
        let urls = answer
            .get("urls")
            .and_then(Value::as_object)
            .ok_or_else(malformed)?;
        let refused = match answer.get("refused") {
            None | Some(Value::Null) => None,
            Some(Value::Object(refused)) => Some(refused),
            Some(_) => return Err(malformed()),
        };
        let path = self.0.path();
        match (
            urls.get(&path),
            refused.and_then(|refused| refused.get(&path)),
        ) {
            (Some(Value::String(url)), None) => self.0.signed(url).map(Admitted::Done),
            (None, Some(Value::String(reason))) => Err(Refusal {
                status: Some(403),
                ..Refusal::new(
                    READ_FORBIDDEN,
                    format!(
                        "This account may not read the stored object ({}).",
                        operator_reason(reason)
                    ),
                )
            }),
            (None, None) => Err(Refusal {
                transient: true,
                retryable: true,
                ..Refusal::new(
                    READ_FAILED,
                    "The stored object could not be signed for reading now.",
                )
            }),
            _ => Err(malformed()),
        }
    }
}

/// A signer's reason, bounded and stripped of control characters.
fn operator_reason(reason: &str) -> String {
    let clean: String = reason
        .chars()
        .filter(|c| !c.is_control())
        .take(REASON_MAX_CHARS)
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "refused".to_owned()
    } else {
        clean.to_owned()
    }
}

/// One stored object to read, and what its bytes must be. The object is
/// named either by a record's declared `reference` — resolved as
/// [`StoredObject::resolve`] resolves it — or by the `bucket` and `object`
/// a resolution already answered; never both.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct StoredObjectRead {
    /// A record's declared reference: a storage address, a bare name, or a
    /// signed storage URL a DS read minted.
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub reference: Option<String>,
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub bucket: Option<String>,
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub object: Option<String>,
    /// The most bytes to read; omitted, the media admission limit
    /// (`survey_photo_media::MAX_INPUT_BYTES`, 50 MiB) every survey and
    /// design photo shares.
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub max_bytes: Option<u64>,
    /// The exact size the caller holds for the object, when it holds one.
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub size_bytes: Option<u64>,
    /// The SHA-256 the caller holds for the object, when it holds one.
    #[serde(default)]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub sha256: Option<String>,
}

/// A read admitted for signing: the object, the authority the reference
/// itself carries, and the bound its bytes keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittedRead {
    pub object: StoredObject,
    /// The reference, when it is itself a signed storage URL: read as given
    /// when the signer grants no fresh signature.
    pub given: Option<String>,
    pub max_bytes: u64,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
}

impl StoredObjectRead {
    /// The read, or the refusal that stops it before anything is asked.
    pub fn admit(self, media_bucket: Option<&str>) -> Result<AdmittedRead, Refusal> {
        let invalid = |message: &str| Refusal::new(READ_REQUEST_INVALID, message.to_owned());
        let (object, given) = match (self.reference, self.bucket, self.object) {
            (Some(reference), None, None) => {
                let object = StoredObject::resolve(&reference, media_bucket)
                    .ok_or_else(|| invalid("The reference names no stored object."))?;
                (object, signed_url(&reference))
            }
            (None, Some(bucket), Some(object)) => (StoredObject { bucket, object }, None),
            _ => {
                return Err(invalid(
                    "A read names one stored object: a reference, or a bucket and an object.",
                ))
            }
        };
        object.check()?;
        let max_bytes = match self.max_bytes {
            None => crate::survey_photo_media::MAX_INPUT_BYTES as u64,
            Some(0) => return Err(invalid("A read's byte bound is at least one byte.")),
            Some(bound) => bound,
        };
        let sha256 = match self.sha256 {
            None => None,
            Some(sha) => {
                let lower = sha.trim().to_ascii_lowercase();
                if !crate::report_export::exact_lower_sha256(&lower) {
                    return Err(invalid("A declared SHA-256 is 64 hexadecimal characters."));
                }
                Some(lower)
            }
        };
        if self.size_bytes.is_some_and(|size| size > max_bytes) {
            return Err(too_large(max_bytes));
        }
        Ok(AdmittedRead {
            object,
            given,
            max_bytes,
            size_bytes: self.size_bytes,
            sha256,
        })
    }
}

/// The reference as a URL readable as given: a storage URL on one of the two
/// hosts storage answers on, carrying its own authority — a V4 or V2
/// signature, or a Firebase download token — and nothing that would send it
/// anywhere else.
pub fn signed_url(reference: &str) -> Option<String> {
    let reference = reference.trim();
    let parsed = url::Url::parse(reference).ok()?;
    let host = parsed.host_str()?;
    let authority = parsed.query_pairs().any(|(key, value)| {
        !value.is_empty()
            && match host {
                SIGNED_HOST => matches!(key.as_ref(), "X-Goog-Signature" | "Signature"),
                "firebasestorage.googleapis.com" => key == "token",
                _ => false,
            }
    });
    (authority
        && reference.len() <= SIGNED_URL_MAX
        && parsed.scheme() == "https"
        && parsed.port().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.fragment().is_none()
        && storage_path(reference).is_some())
    .then(|| reference.to_owned())
}

/// Which URL a read sends its GET to: the fresh signature when the signer
/// granted one; otherwise the authority the reference carries itself, unless
/// the account changed, the device is offline or the read was cancelled.
pub fn read_target(
    signed: Result<String, Refusal>,
    given: Option<&str>,
) -> Result<String, Refusal> {
    match (signed, given) {
        (Ok(url), _) => Ok(url),
        (Err(refusal), Some(given))
            if !matches!(
                refusal.code.as_str(),
                super::SESSION_CHANGED
                    | super::NETWORK_OFFLINE
                    | super::SIGNED_OUT
                    | READ_CANCELLED
            ) =>
        {
            Ok(given.to_owned())
        }
        (Err(refusal), _) => Err(refusal),
    }
}

/// The refusal for an object larger than its read's bound.
pub fn too_large(max_bytes: u64) -> Refusal {
    Refusal::new(
        READ_TOO_LARGE,
        format!("The stored object is larger than this read's {max_bytes}-byte bound."),
    )
}

/// What storage's refusal of a signed read means, from its status and the
/// first [`ERROR_BODY_CAP`] bytes of its XML error body.
pub fn read_refusal(status: u16, body: &[u8]) -> Refusal {
    let body = &body[..body.len().min(ERROR_BODY_CAP)];
    let text = String::from_utf8_lossy(body);
    let code = text
        .split_once("<Code>")
        .and_then(|(_, rest)| rest.split_once("</Code>"))
        .map(|(code, _)| code.trim());
    let expired = code == Some("ExpiredToken")
        || (matches!(status, 400 | 403)
            && text.to_ascii_lowercase().contains("request has expired"));
    let refusal = if status == 404 || matches!(code, Some("NoSuchKey" | "NoSuchBucket")) {
        Refusal::new(OBJECT_MISSING, "Storage holds no object under this name.")
    } else if expired {
        Refusal {
            retryable: true,
            ..Refusal::new(
                READ_EXPIRED,
                "The read's signature expired before storage answered.",
            )
        }
    } else if matches!(status, 401 | 403) {
        Refusal::new(READ_FORBIDDEN, "Storage refused this read's signature.")
    } else if status >= 500 || matches!(status, 408 | 429) {
        Refusal {
            transient: true,
            retryable: true,
            ..Refusal::new(
                READ_FAILED,
                format!("Storage could not answer now (HTTP {status})."),
            )
        }
    } else {
        Refusal::new(
            READ_FAILED,
            format!("Storage refused the read (HTTP {status})."),
        )
    };
    Refusal {
        status: Some(status),
        ..refusal
    }
}

/// Whether read bytes are exactly what the read declared, when it declared
/// anything.
pub fn admit_bytes(bytes: &[u8], read: &AdmittedRead) -> Result<(), Refusal> {
    if bytes.len() as u64 > read.max_bytes {
        return Err(too_large(read.max_bytes));
    }
    match (read.size_bytes, &read.sha256) {
        (Some(size), Some(sha)) => verify(bytes, size, sha),
        (Some(size), None) if bytes.len() as u64 != size => Err(Refusal::new(
            INTEGRITY,
            "The download size does not match its published declaration.",
        )),
        (None, Some(sha)) => verify(bytes, bytes.len() as u64, sha),
        _ => Ok(()),
    }
}

/// A read of an object whose size its answer declared: more bytes than
/// declared, or fewer, are not the declared object.
pub fn admit_declared_size(
    declared: Option<u64>,
    read: Result<Vec<u8>, Refusal>,
) -> Result<Vec<u8>, Refusal> {
    let mismatch = || {
        Refusal::new(
            INTEGRITY,
            "The download size does not match its published declaration.",
        )
    };
    match (declared, read) {
        (Some(_), Err(refusal)) if refusal.code == READ_TOO_LARGE => Err(mismatch()),
        (Some(size), Ok(bytes)) if bytes.len() as u64 != size => Err(mismatch()),
        (_, read) => read,
    }
}

/// Whether bytes read from a signed URL are exactly the declared object.
pub fn verify(bytes: &[u8], size_bytes: u64, sha256: &str) -> Result<(), Refusal> {
    use sha2::{Digest, Sha256};
    if bytes.len() as u64 != size_bytes {
        return Err(Refusal::new(
            INTEGRITY,
            "The download size does not match its published declaration.",
        ));
    }
    if format!("{:x}", Sha256::digest(bytes)) != sha256.to_ascii_lowercase() {
        return Err(Refusal::new(
            INTEGRITY,
            "The download digest does not match its published declaration.",
        ));
    }
    Ok(())
}

/// Read a signed URL's bytes in the page, without any DS credential, and
/// verify them against the declared size and digest.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) async fn fetch_verified(
    url: &str,
    size_bytes: u64,
    sha256: &str,
) -> Result<Vec<u8>, Refusal> {
    let bytes = fetch_bounded(url, size_bytes).await?;
    verify(&bytes, size_bytes, sha256)?;
    Ok(bytes)
}

/// Read exactly the declared number of bytes from a signed URL in the page,
/// without any DS credential. More bytes than declared stop the read.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) async fn fetch_bounded(url: &str, size_bytes: u64) -> Result<Vec<u8>, Refusal> {
    use futures_util::StreamExt;
    let failed = |message: &str| Refusal {
        transient: true,
        retryable: true,
        ..Refusal::new(READ_FAILED, message.to_owned())
    };
    let parsed = url::Url::parse(url).map_err(|_| failed("The signed address is not a URL."))?;
    if !matches!(parsed.scheme(), "https" | "http") {
        return Err(failed("The signed address is not a web URL."));
    }
    let response = reqwest::Client::new()
        .get(parsed)
        .send()
        .await
        .map_err(|_| failed("The stored object could not be read."))?;
    if !response.status().is_success() {
        return Err(failed(&format!(
            "The stored object could not be read (HTTP {}).",
            response.status().as_u16()
        )));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| failed("The stored object's read broke off."))?;
        if (bytes.len() + chunk.len()) as u64 > size_bytes {
            return Err(Refusal::new(
                INTEGRITY,
                "The download size does not match its published declaration.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() as u64 != size_bytes {
        return Err(Refusal::new(
            INTEGRITY,
            "The download size does not match its published declaration.",
        ));
    }
    Ok(bytes)
}

/// The browser half of a stored object read: the signed GET itself and the
/// exports Svelte calls.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod browser {
    use super::{
        admit_bytes, read_refusal, read_target, storage_path, too_large, AdmittedRead, SignObject,
        SignPaths, StoredObject, StoredObjectRead, ERROR_BODY_CAP, READ_CANCELLED, READ_FAILED,
    };
    use crate::gateway_exchange::browser::{attempt, facts, rejection, run, typed, GatewayHost};
    use crate::gateway_exchange::{offline_refusal, session_changed_refusal, Refusal};
    use futures_util::StreamExt;
    use js_sys::{Function, Promise, Reflect, Uint8Array};
    use std::future::Future;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::AbortSignal;

    #[wasm_bindgen(typescript_custom_section)]
    const TYPES: &'static str = r#"
/** One stored object's bytes, read by the Kernel through a fresh signature. */
export interface StoredObjectBytes { bucket: string; object: string; content_type?: string; bytes: Uint8Array }
"#;

    /// What one signed GET answered: its bytes and their declared type.
    pub(crate) struct Read {
        pub bytes: Vec<u8>,
        pub content_type: Option<String>,
    }

    /// One signed GET in the page, without credentials, cookies, a referrer
    /// or the HTTP cache, bounded by `max_bytes`; a refused GET answers why.
    pub(crate) async fn signed_get(
        url: &str,
        max_bytes: u64,
        link_up: bool,
    ) -> Result<Read, Refusal> {
        let failed = |message: &str| Refusal {
            transient: true,
            retryable: true,
            ..Refusal::new(READ_FAILED, message.to_owned())
        };
        let response = match reqwest::Client::new()
            .get(url)
            .fetch_credentials_omit()
            .fetch_cache_no_store()
            .fetch_referrer_policy("no-referrer")
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) if !link_up => return Err(offline_refusal()),
            Err(_) => {
                return Err(failed(
                    "The stored object could not be read from this device.",
                ))
            }
        };
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        if !response.status().is_success() {
            let mut stream = response.bytes_stream();
            let mut body = Vec::new();
            while body.len() < ERROR_BODY_CAP {
                match stream.next().await {
                    Some(Ok(chunk)) => body.extend_from_slice(&chunk),
                    _ => break,
                }
            }
            return Err(read_refusal(status, &body));
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes)
        {
            return Err(too_large(max_bytes));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| failed("The stored object's read broke off."))?;
            if (bytes.len() + chunk.len()) as u64 > max_bytes {
                // Dropping the stream cancels the rest of the body.
                return Err(too_large(max_bytes));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Read {
            bytes,
            content_type,
        })
    }

    /// Run `work` until it ends or `signal` aborts it; an abort drops the
    /// work, which aborts its unfinished request.
    pub(crate) async fn until_aborted<T>(
        work: impl Future<Output = Result<T, JsValue>>,
        signal: Option<AbortSignal>,
    ) -> Result<T, JsValue> {
        let cancelled = || rejection(&Refusal::new(READ_CANCELLED, "The read was cancelled."));
        let Some(signal) = signal else {
            return work.await;
        };
        if signal.aborted() {
            return Err(cancelled());
        }
        let mut listener: Option<Function> = None;
        let fired = Promise::new(&mut |resolve, _| listener = Some(resolve));
        let listener = listener.expect("a promise runs its executor at once");
        let _ = signal.add_event_listener_with_callback("abort", &listener);
        let answer = match futures_util::future::select(Box::pin(work), JsFuture::from(fired)).await
        {
            futures_util::future::Either::Left((answer, _)) => answer,
            futures_util::future::Either::Right(_) => Err(cancelled()),
        };
        let _ = signal.remove_event_listener_with_callback("abort", &listener);
        answer
    }

    /// Sign one admitted read and read its bytes, for the account that asked.
    async fn read(host: &GatewayHost, read: &AdmittedRead) -> Result<Read, JsValue> {
        let principal = facts(host)?.principal;
        let signed = attempt(host, SignObject(read.object.clone())).await?;
        let url =
            read_target(signed, read.given.as_deref()).map_err(|refusal| rejection(&refusal))?;
        let now = facts(host)?;
        if now.offline_mode {
            return Err(rejection(&offline_refusal()));
        }
        if now.principal != principal {
            return Err(rejection(&session_changed_refusal()));
        }
        let answer = signed_get(&url, read.max_bytes, now.link_up)
            .await
            .map_err(|refusal| rejection(&refusal))?;
        if facts(host)?.principal != principal {
            return Err(rejection(&session_changed_refusal()));
        }
        admit_bytes(&answer.bytes, read).map_err(|refusal| rejection(&refusal))?;
        Ok(answer)
    }

    fn input<T: serde::de::DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
        tsify::serde_wasm_bindgen::from_value(value).map_err(|error| {
            rejection(&Refusal::new(
                super::READ_REQUEST_INVALID,
                format!("The stored object read is not readable: {error}"),
            ))
        })
    }

    /// The `bucket/object` an object address names, or `undefined`.
    #[wasm_bindgen(js_name = storagePath)]
    pub fn storage_path_export(address: &str) -> Option<String> {
        storage_path(address)
    }

    /// The stored object a record's declared reference names — its bucket
    /// and its name there — or `undefined` when it names none. No network.
    #[wasm_bindgen(js_name = storedObject, unchecked_return_type = "StoredObject | undefined")]
    pub fn stored_object(reference: &str) -> Result<JsValue, JsValue> {
        match StoredObject::resolve(reference, super::media_bucket().as_deref()) {
            Some(object) => typed(&object),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// One stored object's bytes: signed fresh for this account, read in the
    /// page without any DS credential, bounded and checked against what the
    /// read declares. Rejects with the typed refusal (`storage_object_missing`,
    /// `storage_read_forbidden`, `storage_read_expired`,
    /// `storage_read_too_large`, `storage_bytes_integrity`,
    /// `storage_read_failed`, `storage_read_cancelled`, …).
    #[wasm_bindgen(js_name = storedObjectBytes, unchecked_return_type = "StoredObjectBytes")]
    pub async fn stored_object_bytes(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "StoredObjectRead")] request: JsValue,
        signal: Option<AbortSignal>,
    ) -> Result<JsValue, JsValue> {
        let request: StoredObjectRead = input(request)?;
        let admitted = request
            .admit(super::media_bucket().as_deref())
            .map_err(|refusal| rejection(&refusal))?;
        let answer = until_aborted(read(&host, &admitted), signal).await?;
        let typed_answer = typed(&admitted.object)?;
        if let Some(content_type) = &answer.content_type {
            Reflect::set(
                &typed_answer,
                &JsValue::from_str("content_type"),
                &JsValue::from_str(content_type),
            )?;
        }
        Reflect::set(
            &typed_answer,
            &JsValue::from_str("bytes"),
            &Uint8Array::from(&answer.bytes[..]),
        )?;
        Ok(typed_answer)
    }

    /// A fresh signed URL of exactly one stored object, for a platform effect
    /// the page cannot read through (a media element's streaming, a file
    /// download). Use it at once; it is never kept.
    #[wasm_bindgen(js_name = storedObjectUrl)]
    pub async fn stored_object_url(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "StoredObject")] object: JsValue,
    ) -> Result<String, JsValue> {
        let object: StoredObject = input(object)?;
        run(&host, SignObject(object)).await
    }

    /// Fresh signed URLs for the storage addresses among `addresses`, keyed by
    /// the `bucket/object` each names. Nothing is asked when none is one.
    #[wasm_bindgen(js_name = signStorageUrls, unchecked_return_type = "Record<string, string>")]
    pub async fn sign_storage_urls(
        host: GatewayHost,
        addresses: Vec<String>,
    ) -> Result<JsValue, JsValue> {
        let call = SignPaths::of(&addresses);
        if call.paths.is_empty() {
            return typed(&std::collections::BTreeMap::<String, String>::new());
        }
        typed(&run(&host, call).await?)
    }

    /// One signed URL's bytes, verified against the declared size and digest.
    #[wasm_bindgen(js_name = verifiedBytes)]
    pub async fn verified_bytes(
        url: String,
        size_bytes: f64,
        sha256: String,
    ) -> Result<Vec<u8>, JsValue> {
        if !(size_bytes.is_finite() && size_bytes >= 0.0) {
            return Err(js_sys::Error::new("A declared size is a whole number of bytes.").into());
        }
        super::fetch_verified(&url, size_bytes as u64, &sha256)
            .await
            .map_err(|refusal| rejection(&refusal))
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
