//! Immutable downloaded bytes: identity and bounded retention are Rust-owned.
//! No TTL or online revalidation is needed to reopen a digest-pinned local copy.
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const MAX_FILE: u64 = 128 * 1024 * 1024;
const MAX_TOTAL: u64 = 256 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    key: String,
    size_bytes: u64,
    stored_at: u64,
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Identity {
        owner: String,
        origin: String,
        project: String,
        locator: String,
        sha256: String,
        size_bytes: u64,
    },
    Retain {
        key: String,
        size_bytes: u64,
        entries: Vec<Entry>,
    },
}
fn hex(v: &str) -> bool {
    v.len() == 64
        && v.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 256 * 1024 {
        return Err("Artifact cache request exceeds its bound.".into());
    }
    let request: Request =
        serde_json::from_slice(input).map_err(|_| "Invalid artifact cache request.")?;
    let result: Value = match request {
        Request::Identity {
            owner,
            origin,
            project,
            locator,
            sha256,
            size_bytes,
        } => {
            if owner.is_empty()
                || owner.len() > 256
                || project.is_empty()
                || project.len() > 128
                || origin.is_empty()
                || origin.len() > 512
                || !locator.starts_with("compute-artifact://v1/")
                || locator.len() > 2048
                || !hex(&sha256)
                || size_bytes == 0
                || size_bytes > MAX_FILE
            {
                return Err("Invalid immutable artifact cache identity.".into());
            }
            let value =
                serde_json::to_vec(&(owner, origin, project, locator, sha256, size_bytes)).unwrap();
            json!({ "key": format!("{:x}", Sha256::digest(value)) })
        }
        Request::Retain {
            key,
            size_bytes,
            mut entries,
        } => {
            if !hex(&key) || size_bytes == 0 || size_bytes > MAX_FILE || entries.len() > 1024 {
                return Err("Artifact cache retention exceeds its bound.".into());
            }
            let mut seen = std::collections::HashSet::new();
            if entries
                .iter()
                .any(|e| !hex(&e.key) || !seen.insert(e.key.clone()) || e.size_bytes > MAX_FILE)
            {
                return Err("Artifact cache inventory is invalid.".into());
            }
            entries.retain(|e| e.key != key);
            entries.sort_by(|a, b| (a.stored_at, &a.key).cmp(&(b.stored_at, &b.key)));
            let mut total = size_bytes + entries.iter().map(|e| e.size_bytes).sum::<u64>();
            let mut count = entries.len() + 1;
            let mut remove = vec![];
            for entry in entries {
                if total <= MAX_TOTAL && count <= 32 {
                    break;
                }
                total -= entry.size_bytes;
                count -= 1;
                remove.push(entry.key);
            }
            json!({ "remove": remove, "total_bytes": total })
        }
    };
    serde_json::to_string(&result).map_err(|_| "Artifact cache reply encoding failed.".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_names_never_share_bytes_across_accounts_origins_or_revisions() {
        let a = json!({"action":"identity","owner":"u","origin":"https://stable","project":"p","locator":"compute-artifact://v1/p/e/o/v/id/file.pdf","sha256":"a".repeat(64),"size_bytes":3});
        let key = evaluate(&serde_json::to_vec(&a).unwrap()).unwrap();
        for (field, value) in [
            ("owner", json!("other")),
            ("origin", json!("https://dev")),
            ("project", json!("q")),
            ("sha256", json!("b".repeat(64))),
            ("size_bytes", json!(4)),
        ] {
            let mut b = a.clone();
            b[field] = value;
            assert_ne!(key, evaluate(&serde_json::to_vec(&b).unwrap()).unwrap());
        }
    }
    #[test]
    fn bounded_retention_replaces_existing_key_without_double_counting() {
        let entries = vec![
            json!({"key":"a".repeat(64),"size_bytes":MAX_FILE,"stored_at":1}),
            json!({"key":"b".repeat(64),"size_bytes":MAX_FILE,"stored_at":2}),
        ];
        let plan = |key: &str| -> Value {
            serde_json::from_str(&evaluate(&serde_json::to_vec(&json!({"action":"retain","key":key.repeat(64),"size_bytes":MAX_FILE,"entries":entries})).unwrap()).unwrap()).unwrap()
        };
        assert_eq!(plan("c")["remove"], json!(["a".repeat(64)]));
        assert_eq!(plan("a")["remove"], json!([]));
    }
}
