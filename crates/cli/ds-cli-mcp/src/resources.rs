//! Lazy MCP resources backed by the exact receipt-verified skill bundle.

use ds_cli_skills::{BundleVerdict, IndexedBundle, RECEIPT_CONTRACT, RECEIPT_SOURCE};
use serde_json::{Value, json};

const URI_PREFIX: &str = "ds-skill://bundle/";
const URI_SUFFIX: &str = "/SKILL.md";

enum ResourcePath<'a> {
    Skill(&'a str),
    Reference(&'a str, &'a str),
}

#[derive(Debug)]
pub struct SkillResources {
    verdict: BundleVerdict,
    expected_source_sha: String,
}

impl SkillResources {
    /// The one verdict `ds doctor` reads too; see `ds_cli_skills::verdict`.
    pub fn load(expected_source_sha: &str) -> Self {
        Self {
            verdict: ds_cli_skills::verdict(expected_source_sha),
            expected_source_sha: expected_source_sha.to_string(),
        }
    }

    fn bundle(&self) -> Option<&IndexedBundle> {
        self.verdict.bundle.as_ref()
    }

    pub fn identity(&self) -> Value {
        let mut identity = self.verdict.json();
        if identity["source_sha"].is_null() {
            identity["source_sha"] = json!(self.expected_source_sha);
        }
        identity["transport"] = json!("mcp_resources");
        identity["dirty"] = json!(false);
        identity["requires_skills_home"] = json!(false);
        identity["uri_template"] = json!("ds-skill://bundle/<receipt-skill-id>/SKILL.md");
        identity["reference_uri_template"] =
            json!("ds-skill://bundle/<receipt-skill-id>/references/<receipt-reference-id>.md");
        identity
    }

    pub fn list(&self) -> Value {
        let resources = self
            .bundle()
            .map(|bundle| {
                let mut listed = bundle
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
                    .collect::<Vec<_>>();
                listed.extend(bundle.references().into_iter().map(|(skill, stem)| {
                    json!({
                        "uri": reference_uri(&skill, &stem),
                        "name": format!("{skill}/{stem}"),
                        "title": format!("DS skill reference: {skill}/{stem}"),
                        "description": "Receipt-verified workflow detail. Read when its entry skill cites this reference.",
                        "mimeType": "text/markdown",
                        "_meta": resource_meta(bundle.source_sha()),
                    })
                }));
                listed
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
        let resource =
            parse_uri(uri).ok_or_else(|| (-32602, format!("unknown DS skill resource `{uri}`")))?;
        let bundle = self.bundle().ok_or_else(|| {
            (
                -32002,
                format!(
                    "the shipped DS skill bundle is {}: {}",
                    self.verdict.status,
                    self.verdict
                        .reason
                        .as_deref()
                        .unwrap_or("unknown verification failure")
                ),
            )
        })?;
        let text = match resource {
            ResourcePath::Skill(name) => bundle.read_skill(name),
            ResourcePath::Reference(skill, stem) => bundle.read_reference(skill, stem),
        }
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

fn reference_uri(skill: &str, stem: &str) -> String {
    format!("{URI_PREFIX}{skill}/references/{stem}.md")
}

fn closed_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn parse_uri(uri: &str) -> Option<ResourcePath<'_>> {
    let path = uri.strip_prefix(URI_PREFIX)?;
    if path.contains('\\') || path.contains('%') {
        return None;
    }
    if let Some(name) = path.strip_suffix(URI_SUFFIX) {
        return closed_name(name).then_some(ResourcePath::Skill(name));
    }
    let (skill, file) = path.split_once("/references/")?;
    let stem = file.strip_suffix(".md")?;
    (closed_name(skill) && closed_name(stem)).then_some(ResourcePath::Reference(skill, stem))
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
        assert!(matches!(
            parse_uri("ds-skill://bundle/ds/SKILL.md"),
            Some(ResourcePath::Skill("ds"))
        ));
        assert!(matches!(
            parse_uri("ds-skill://bundle/ds-printout/references/mv-plan-profile-booklet.md"),
            Some(ResourcePath::Reference(
                "ds-printout",
                "mv-plan-profile-booklet"
            ))
        ));
        for uri in [
            "file:///etc/passwd",
            "ds-skill://bundle/../SKILL.md",
            "ds-skill://bundle/ds/agents/openai.yaml/SKILL.md",
            "ds-skill://bundle/ds%2f..%2f/SKILL.md",
            "ds-skill://bundle/ds\\..\\/SKILL.md",
            "ds-skill://bundle/ds/references/../SKILL.md",
            "ds-skill://bundle/ds/references/a/b.md",
            "ds-skill://bundle/ds/references/a%2fb.md",
        ] {
            assert!(parse_uri(uri).is_none(), "{uri}");
        }
    }
}
