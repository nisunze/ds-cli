//! Public release reads: the latest desktop shell or component release a lane
//! publishes (`GET /api/v1/app_releases`). The read is anonymous and
//! best-effort — a public download page never shows an error because a
//! release lookup failed — so every failure reads as no release. The page
//! names the lane's release URL from its route manifest; nothing here sends a
//! credential.

use serde_json::Value;

/// A release answer's `release` member: ds-brain's flat wire, or the
/// standard envelope a proxy may keep. `None` for a refusal or an answer
/// without the member.
pub fn release_member(body: &Value) -> Option<Value> {
    if body.get("success") == Some(&Value::Bool(false)) {
        return None;
    }
    let answer = body
        .get("data")
        .filter(|data| data.is_object())
        .unwrap_or(body);
    answer.as_object()?.get("release").cloned()
}

/// The most bytes a release answer may hold.
pub const ANSWER_CAP: usize = 1024 * 1024;

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod exports {
    use super::{release_member, ANSWER_CAP};
    use crate::gateway_exchange::browser::typed;
    use futures_util::StreamExt;
    use wasm_bindgen::prelude::*;

    pub(crate) async fn read(url: &str) -> Option<serde_json::Value> {
        let parsed = url::Url::parse(url).ok()?;
        if !matches!(parsed.scheme(), "https" | "http") {
            return None;
        }
        let response = reqwest::Client::new()
            .get(parsed)
            .header("Accept", "application/json")
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.ok()?;
            if bytes.len() + chunk.len() > ANSWER_CAP {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).ok()
    }

    /// The latest release a lane publishes at `url`, as `{ release }` (`release`
    /// may be null), or `undefined` when there is none or the read failed.
    #[wasm_bindgen(js_name = releaseRead, unchecked_return_type = "{ release: unknown } | undefined")]
    pub async fn release_read(url: String) -> Result<JsValue, JsValue> {
        match read(&url).await.as_ref().map(release_member) {
            Some(Some(release)) => typed(&serde_json::json!({ "release": release })),
            _ => Ok(JsValue::UNDEFINED),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_release_reads_from_the_flat_wire_or_the_envelope() {
        assert_eq!(release_member(&json!({"release": {"version": "1.2.3"}})), Some(json!({"version": "1.2.3"})));
        assert_eq!(release_member(&json!({"success": true, "data": {"release": null}})), Some(json!(null)));
        assert_eq!(release_member(&json!({"success": false, "release": {"version": "1"}})), None);
        assert_eq!(release_member(&json!({"data": {"other": 1}})), None);
        assert_eq!(release_member(&json!([1, 2])), None);
    }
}
