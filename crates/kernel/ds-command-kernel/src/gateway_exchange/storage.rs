//! Signed reads of stored objects. Storage keeps canonical object addresses
//! (`gs://bucket/object`, or a storage URL an older surface kept); a fresh
//! signed URL is minted at the moment of a read by ds-brain (`POST /report`
//! `sign_url`), which signs only the objects its caller may read, and is used
//! once, never kept. [`storage_path`] reads an address; the browser reads a
//! signed URL's bytes against the size and digest a caller holds.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/report";
/// Brain signs a batch in one answer of at most a few kilobytes per path.
const RESPONSE_CAP: u64 = 4 * 1024 * 1024;
pub const PATHS_MAX: usize = 500;
pub const INTEGRITY: &str = "storage_bytes_integrity";
pub const READ_FAILED: &str = "storage_read_failed";

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
        Ok(Request::post(PATH, body).response_cap(RESPONSE_CAP).into())
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

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{fetch_verified, storage_path, SignPaths};
    use crate::gateway_exchange::browser::{rejection, run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// The `bucket/object` an object address names, or `undefined`.
    #[wasm_bindgen(js_name = storagePath)]
    pub fn storage_path_export(address: &str) -> Option<String> {
        storage_path(address)
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
        fetch_verified(&url, size_bytes as u64, &sha256)
            .await
            .map_err(|refusal| rejection(&refusal))
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
