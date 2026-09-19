//! Closed storage-response validation shared by native and browser photo hosts.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

pub fn generation(value: &Value) -> Result<String, String> {
    value
        .as_str()
        .filter(|s| {
            s.as_bytes()
                .first()
                .is_some_and(|c| matches!(c, b'1'..=b'9'))
                && s.bytes().all(|c| c.is_ascii_digit())
                && s.parse::<i64>().is_ok()
        })
        .map(str::to_owned)
        .ok_or_else(|| "Media response omitted its exact storage generation".into())
}
pub fn target(value: &Value, path: &str) -> Result<String, String> {
    let bucket = value["bucket"].as_str().unwrap_or_default();
    if value["blob_path"] != path
        || bucket.len() < 2
        || bucket.len() > 222
        || !bucket.as_bytes()[0].is_ascii_alphanumeric()
        || !bucket.bytes().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-')
        })
    {
        return Err("Media response did not identify the requested storage target".into());
    }
    Ok(bucket.into())
}
fn storage_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|_| "Invalid media storage URL")?;
    if raw.len() > 8192
        || url.scheme() != "https"
        || url.host_str() != Some("storage.googleapis.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Invalid media storage URL".into());
    }
    Ok(url)
}
pub fn session(value: &Value, bucket: &str) -> Result<String, String> {
    let raw = value.as_str().ok_or("Invalid media upload session")?;
    let url = storage_url(raw).map_err(|_| "Invalid media upload session")?;
    let pairs: Vec<_> = url.query_pairs().collect();
    if raw.len() > 4096
        || url.path() != format!("/upload/storage/v1/b/{bucket}/o")
        || pairs.iter().filter(|(k, _)| k == "upload_id").count() != 1
        || !pairs.iter().any(|(k, v)| k == "upload_id" && !v.is_empty())
        || pairs.iter().filter(|(k, _)| k == "uploadType").count() != 1
        || !pairs
            .iter()
            .any(|(k, v)| k == "uploadType" && v == "resumable")
    {
        return Err("Invalid media upload session".into());
    }
    Ok(raw.into())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inspection {
    pub blob_path: String,
    pub bucket: String,
    pub generation: String,
    pub source_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_protocol: Option<u8>,
}
pub fn inspect(value: &Value, path: &str) -> Result<Inspection, String> {
    let bucket = target(value, path)?;
    let generation = generation(&value["generation"])?;
    let raw = value["source_url"]
        .as_str()
        .ok_or("Media response omitted its signed source")?;
    let url = storage_url(raw)?;
    // Stored survey names are admitted ASCII segments. Comparing the encoded
    // URL path also preserves the exact storage identity; no host repair occurs.
    if url.path() != format!("/{bucket}/{path}")
        || url.query_pairs().filter(|(k, _)| k == "generation").count() != 1
        || !url
            .query_pairs()
            .any(|(k, v)| k == "generation" && v == generation)
    {
        return Err("Media response did not pin the requested source generation".into());
    }
    Ok(Inspection {
        blob_path: path.into(),
        bucket,
        generation,
        source_url: raw.into(),
        revision_protocol: (value["revision_protocol"] == 1).then_some(1),
    })
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Generation { value: Value },
    Target { value: Value, path: String },
    Session { value: Value, bucket: String },
    Inspect { value: Value, path: String },
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 32 * 1024 {
        return Err("Media response exceeds its byte bound".into());
    }
    let request: Request =
        serde_json::from_slice(input).map_err(|_| "Invalid media response request")?;
    let value = match request {
        Request::Generation { value } => Value::String(generation(&value)?),
        Request::Target { value, path } => Value::String(target(&value, &path)?),
        Request::Session { value, bucket } => Value::String(session(&value, &bucket)?),
        Request::Inspect { value, path } => {
            serde_json::to_value(inspect(&value, &path)?).map_err(|e| e.to_string())?
        }
    };
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn pins_source_and_refuses_credential_redirects_and_ambiguous_sessions() {
        let mut source = json!({"blob_path":"project/forms/form/entry/photo.jpg","bucket":"bucket","generation":"7","source_url":"https://storage.googleapis.com/bucket/project/forms/form/entry/photo.jpg?generation=7"});
        let path = "project/forms/form/entry/photo.jpg";
        assert!(inspect(&source, path).is_ok());
        source["generation"] = json!("8");
        assert!(inspect(&source, path).is_err());
        for raw in [
            "https://evil.test/upload/storage/v1/b/bucket/o?uploadType=resumable&upload_id=a",
            "https://storage.googleapis.com/upload/storage/v1/b/bucket/o?uploadType=resumable&upload_id=a&upload_id=b",
            "https://user@storage.googleapis.com/upload/storage/v1/b/bucket/o?uploadType=resumable&upload_id=a",
        ] {
            assert!(session(&json!(raw), "bucket").is_err());
        }
    }
}
