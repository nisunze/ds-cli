//! Lazy MCP resources backed by the exact receipt-verified skill bundle.

use ds_cli_skills::{IndexedBundle, RECEIPT_CONTRACT, RECEIPT_SOURCE};
use serde_json::{Value, json};

const URI_PREFIX: &str = "ds-skill://bundle/";
const URI_SUFFIX: &str = "/SKILL.md";

#[derive(Debug)]
pub struct SkillResources {
    bundle: Option<IndexedBundle>,
    expected_source_sha: String,
    reason: Option<String>,
}

impl SkillResources {
    pub fn load(expected_source_sha: &str) -> Self {
        match ds_cli_skills::indexed_bundle(expected_source_sha) {
            Ok(bundle) => Self {
                bundle: Some(bundle),
                expected_source_sha: expected_source_sha.to_string(),
                reason: None,
            },
            Err(reason) => Self {
                bundle: None,
                expected_source_sha: expected_source_sha.to_string(),
                reason: Some(reason),
            },
        }
    }

    pub fn identity(&self) -> Value {
        json!({
            "status": if self.bundle.is_some() { "ready" } else { "unavailable" },
            "verification": "receipt_indexed_content_verified_on_read",
            "transport": "mcp_resources",
            "contract": RECEIPT_CONTRACT,
            "source": RECEIPT_SOURCE,
            "source_sha": self.bundle.as_ref().map(IndexedBundle::source_sha).unwrap_or(&self.expected_source_sha),
            "dirty": false,
            "count": self.bundle.as_ref().map(|bundle| bundle.skills().len()).unwrap_or(0),
            "reason": self.reason,
            "requires_skills_home": false,
            "uri_template": "ds-skill://bundle/<receipt-skill-id>/SKILL.md",
        })
    }

    pub fn list(&self) -> Value {
        let resources = self
            .bundle
            .as_ref()
            .map(|bundle| {
                bundle
                    .skills()
                    .iter()
                    .map(|name| {
                        json!({
                            "uri": skill_uri(name),
                            "name": name,
                            "title": format!("DS skill: {name}"),
                            "description": "Receipt-verified DS operating guidance. Read lazily before using this workflow.",
                            "mimeType": "text/markdown",
                            "_meta": resource_meta(bundle.source_sha()),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        json!({ "resources": resources, "_meta": { "dsSkills": self.identity() } })
    }

    pub fn read(&self, params: &Value) -> Result<Value, (i64, String)> {
        let object = params.as_object().ok_or_else(|| {
            (
                -32602,
                "resources/read params must be an object".to_string(),
            )
        })?;
        if let Some(key) = object.keys().find(|key| key.as_str() != "uri") {
            return Err((-32602, format!("unknown resources/read property `{key}`")));
        }
        let uri = object
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| (-32602, "`uri` is required and must be a string".to_string()))?;
        let name =
            parse_uri(uri).ok_or_else(|| (-32602, format!("unknown DS skill resource `{uri}`")))?;
        let bundle = self.bundle.as_ref().ok_or_else(|| {
            (
                -32002,
                format!(
                    "the shipped DS skill bundle is unavailable: {}",
                    self.reason
                        .as_deref()
                        .unwrap_or("unknown verification failure")
                ),
            )
        })?;
        let text = bundle
            .read_skill(name)
            .map_err(|reason| (-32002, format!("DS skill resource refused: {reason}")))?;
        Ok(json!({
            "contents": [{
                "uri": uri,
                "mimeType": "text/markdown",
                "text": text,
                "_meta": resource_meta(bundle.source_sha()),
            }]
        }))
    }
}

fn skill_uri(name: &str) -> String {
    format!("{URI_PREFIX}{name}{URI_SUFFIX}")
}

fn parse_uri(uri: &str) -> Option<&str> {
    let name = uri.strip_prefix(URI_PREFIX)?.strip_suffix(URI_SUFFIX)?;
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains('%')
        || !name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
    {
        return None;
    }
    Some(name)
}

fn resource_meta(source_sha: &str) -> Value {
    json!({
        "contract": RECEIPT_CONTRACT,
        "source": RECEIPT_SOURCE,
        "sourceSha": source_sha,
        "dirty": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_identifiers_are_closed_names_not_paths() {
        assert_eq!(parse_uri("ds-skill://bundle/ds/SKILL.md"), Some("ds"));
        for uri in [
            "file:///etc/passwd",
            "ds-skill://bundle/../SKILL.md",
            "ds-skill://bundle/ds/agents/openai.yaml/SKILL.md",
            "ds-skill://bundle/ds%2f..%2f/SKILL.md",
            "ds-skill://bundle/ds\\..\\/SKILL.md",
        ] {
            assert_eq!(parse_uri(uri), None, "{uri}");
        }
    }
}
