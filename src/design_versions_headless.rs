//! Headless transformer version workflow. Hosts supply authenticated reads;
//! all validation, snapshot preparation and comparison decisions stay here.
use crate::design_versions::Scope;
use serde_json::{Value, json};

pub trait Source {
    type Error;
    fn versions(
        &mut self,
        project: &str,
        transformer: &str,
        version: Option<&str>,
    ) -> Result<Value, Self::Error>;
    fn head(&mut self, project: &str, transformer: &str) -> Result<Value, Self::Error>;
    fn create(
        &mut self,
        project: &str,
        transformer: &str,
        reason: &str,
        idempotency_key: &str,
    ) -> Result<Value, Self::Error>;
    fn current(&mut self, project: &str, transformer: &str) -> Result<Value, Self::Error>;
    fn restore(
        &mut self,
        project: &str,
        transformer: &str,
        version: &str,
        reason: &str,
        expected_source_revision: &str,
        idempotency_key: &str,
    ) -> Result<Value, Self::Error>;
}
#[derive(Debug)]
pub enum Error<E> {
    Source(E),
    Invalid(String),
    VersionNotFound {
        transformer: String,
        requested: String,
        latest: u64,
    },
}
impl<E> From<String> for Error<E> {
    fn from(v: String) -> Self {
        Self::Invalid(v)
    }
}
pub enum Command {
    List {
        transformer: String,
    },
    Compare {
        transformer: String,
        from: String,
        to: String,
    },
    Begin {
        transformer: String,
        reason: String,
        idempotency_key: String,
    },
    Restore {
        transformer: String,
        version: String,
        reason: String,
        idempotency_key: String,
    },
}
fn invalid(message: &'static str) -> String {
    message.into()
}
fn bad(message: &'static str) -> String {
    message.into()
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.trim() == s
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
fn version(s: &str) -> bool {
    s.starts_with('v')
        && crate::design_versions::valid_target(s)
        && s[1..].parse::<u64>().is_ok_and(|n| n <= i64::MAX as u64)
}

impl Command {
    pub fn transformer(&self) -> &str {
        match self {
            Self::List { transformer }
            | Self::Compare { transformer, .. }
            | Self::Begin { transformer, .. }
            | Self::Restore { transformer, .. } => transformer,
        }
    }
    pub fn validate(&self, project: &str) -> Result<(), String> {
        if !identifier(project) || !identifier(self.transformer()) {
            return Err(invalid("invalid project or transformer identity"));
        }
        if let Self::Compare { from, to, .. } = self {
            if !version(from) || (to != "head" && !version(to)) || from == to {
                return Err(invalid(
                    "choose distinct canonical vN versions; --to may be head. Local browser versions must first be published.",
                ));
            }
        }
        if let Self::Begin {
            reason,
            idempotency_key,
            ..
        }
        | Self::Restore {
            reason,
            idempotency_key,
            ..
        } = self
        {
            if reason.is_empty()
                || reason.trim() != reason
                || reason.chars().count() > crate::design_version::VERSION_REASON_MAX
                || reason.chars().any(char::is_control)
                || idempotency_key.is_empty()
                || idempotency_key.chars().count() > 128
                || idempotency_key.chars().any(char::is_control)
            {
                return Err(invalid("version reason or idempotency key is invalid"));
            }
        }
        if let Self::Restore {
            version: target, ..
        } = self
        {
            if !version(target) {
                return Err(invalid(
                    "restore requires one canonical assigned vN version",
                ));
            }
        }
        Ok(())
    }
}
fn record(
    value: &Value,
    project: &str,
    transformer: &str,
    expected: Option<&str>,
) -> Result<(), String> {
    let id = value["version_id"]
        .as_str()
        .ok_or_else(|| bad("missing version identity"))?;
    if value["project_id"] != project
        || value["object"]["kind"] != "lv_transformer"
        || value["object"]["id"] != transformer
        || !version(id)
        || expected.is_some_and(|s| s != id)
    {
        return Err(bad(
            "version response belongs to another project, transformer or revision",
        ));
    }
    Ok(())
}
fn snapshot(data: &Value, project: &str, transformer: &str, id: &str) -> Result<Value, String> {
    record(&data["version"], project, transformer, Some(id))?;
    if data["version"]["playback_available"] != true
        || data["snapshot"]["schema"] != "ds.transformer-version-snapshot/v1"
        || !data["snapshot"]["layers"].is_object()
    {
        return Err("playback_unavailable: retained version has no readable snapshot; choose a version with playback_available=true".into());
    }
    Ok(data["snapshot"].clone())
}

fn listed<'a>(data: &'a Value, project: &str, transformer: &str) -> Result<&'a [Value], String> {
    let rows = data["versions"]
        .as_array()
        .ok_or_else(|| bad("missing versions list"))?;
    if rows.len() > 200 {
        return Err(bad("version list exceeds 200 records"));
    }
    for row in rows {
        record(row, project, transformer, None)?;
    }
    Ok(rows)
}

fn ordinal(id: &str) -> u64 {
    id.strip_prefix('v')
        .and_then(|value| value.parse().ok())
        .unwrap_or_default()
}
fn kernel(request: Value) -> Result<Value, String> {
    let bytes =
        serde_json::to_vec(&request).map_err(|_| bad("version kernel response is unreadable"))?;
    let answer = crate::design_versions::evaluate(&bytes)
        .map_err(|_| bad("version snapshot failed kernel integrity validation"))?;
    serde_json::from_str(&answer).map_err(|_| bad("version kernel response is unreadable"))
}

pub fn run<S: Source>(
    scope: &Scope,
    source: &mut S,
    command: &Command,
) -> Result<Value, Error<S::Error>> {
    let project = &scope.project;
    command.validate(project)?;
    let transformer = command.transformer();
    if scope.object_kind != "lv_transformer" || scope.object_id != transformer {
        return Err(invalid("version scope does not match transformer").into());
    }
    match command {
        Command::List { .. } => {
            let data = source
                .versions(project, transformer, None)
                .map_err(Error::Source)?;
            let rows = listed(&data, project, transformer)?;
            Ok(
                json!({"project":project,"transformer":transformer,"versions":rows,"truncated":rows.len()==200,"scope":"published_versions","mutated":false}),
            )
        }
        Command::Compare { from, to, .. } => {
            // A complete bounded list lets the kernel distinguish a missing
            // version from authentication before asking for snapshot bytes.
            // A 200-row page may be truncated, so older identities still go
            // to the exact read and retain its authoritative answer.
            let inventory = source
                .versions(project, transformer, None)
                .map_err(Error::Source)?;
            let rows = listed(&inventory, project, transformer)?;
            if rows.len() < 200 {
                let present = rows
                    .iter()
                    .filter_map(|row| row["version_id"].as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                let latest = present
                    .iter()
                    .map(|id| ordinal(id))
                    .max()
                    .unwrap_or_default();
                for requested in [Some(from.as_str()), (to != "head").then_some(to.as_str())]
                    .into_iter()
                    .flatten()
                {
                    if !present.contains(requested) {
                        return Err(Error::VersionNotFound {
                            transformer: transformer.to_string(),
                            requested: requested.to_string(),
                            latest,
                        });
                    }
                }
            }
            let left = snapshot(
                &source
                    .versions(project, transformer, Some(from))
                    .map_err(Error::Source)?,
                project,
                transformer,
                from,
            )?;
            let (right, descriptor) = if to == "head" {
                let head = source.head(project, transformer).map_err(Error::Source)?;
                if head["project"] != *project || head["transformer"] != transformer {
                    return Err(bad("saved head scope mismatch").into());
                }
                (
                    head["snapshot"].clone(),
                    json!({"kind":"saved_head","server_version":head["server_version"],"content_digest":head["content_digest"]}),
                )
            } else {
                (
                    snapshot(
                        &source
                            .versions(project, transformer, Some(to))
                            .map_err(Error::Source)?,
                        project,
                        transformer,
                        to,
                    )?,
                    json!({"kind":"version","version_id":to}),
                )
            };
            let prepare = |snapshot| {
                kernel(
                    json!({"action":"prepare","scope":scope,"operation":"comparison","parent":null,"local_sequence":1,"created_at":"comparison-observation","reason":"read-only comparison","snapshot":snapshot,"project_config":null}),
                )
            };
            let l = prepare(left)?;
            let r = prepare(right)?;
            let mut result = kernel(
                json!({"action":"compare","scope":scope,"left":l,"right":r,"tolerance_m":0}),
            )?;
            bound_comparison(&mut result["comparison"]);
            bound_comparison(&mut result["metadata_comparison"]);
            Ok(
                json!({"project":project,"transformer":transformer,"from":{"kind":"version","version_id":from},"to":descriptor,
                "from_snapshot_digest":l["manifest"]["id"],"to_snapshot_digest":r["manifest"]["id"],"comparison":result["comparison"],"consistency":result["consistency"],"metadata_comparison":result["metadata_comparison"],"details_included":false,"mutated":false}),
            )
        }
        Command::Begin {
            reason,
            idempotency_key,
            ..
        } => {
            let data = source
                .create(project, transformer, reason, idempotency_key)
                .map_err(Error::Source)?;
            record(&data["version"], project, transformer, None)?;
            Ok(json!({
                "project": project,
                "transformer": transformer,
                "version": data["version"],
                "mutated": true,
            }))
        }
        Command::Restore {
            version,
            reason,
            idempotency_key,
            ..
        } => {
            let current = source
                .current(project, transformer)
                .map_err(Error::Source)?;
            let head = &current["head"];
            let expected = head["source_revision"]
                .as_str()
                .ok_or_else(|| bad("saved transformer head has no exact source revision"))?;
            if head["project_id"] != *project
                || head["object"]["kind"] != "lv_transformer"
                || head["object"]["id"] != transformer
                || expected.is_empty()
            {
                return Err(bad("saved transformer head scope mismatch").into());
            }
            let data = source
                .restore(
                    project,
                    transformer,
                    version,
                    reason,
                    expected,
                    idempotency_key,
                )
                .map_err(Error::Source)?;
            record(&data["version"], project, transformer, Some(version))?;
            Ok(json!({
                "project": project,
                "transformer": transformer,
                "version": data["version"],
                "prepared_source_revision": expected,
                "mutated": true,
            }))
        }
    }
}
fn bound_comparison(value: &mut Value) {
    fn keys(v: &mut Value) {
        if let Some(map) = v["changedAttributeKeyCounts"].as_object_mut() {
            let total = map.len();
            let kept = map
                .iter()
                .take(100)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            *map = kept;
            v["attribute_keys_truncated"] = json!(total > 100);
        }
    }
    keys(value);
    if let Some(layers) = value["layers"].as_array_mut() {
        let count = layers.len();
        layers.truncate(200);
        for layer in layers {
            if let Some(object) = layer.as_object_mut() {
                object.remove("reviewCollections");
                object.remove("details");
            }
            keys(layer);
        }
        value["layers_truncated"] = json!(count > 200);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_scope_and_versions_are_required() {
        assert!(
            Command::Compare {
                transformer: "tr".into(),
                from: "v1".into(),
                to: "head".into()
            }
            .validate("p")
            .is_ok()
        );
        for id in ["v0", "v01", "v-1", "local-abc", "v18446744073709551615"] {
            assert!(!version(id));
        }
        let row = json!({"project_id":"p","object":{"kind":"lv_transformer","id":"tr"},"version_id":"v1"});
        assert!(record(&row, "p", "tr", Some("v1")).is_ok());
        assert!(record(&row, "other", "tr", Some("v1")).is_err());
        assert!(record(&row, "p", "other", Some("v1")).is_err());
        assert!(record(&row, "p", "tr", Some("v2")).is_err());
        assert!(snapshot(&json!({"version":row}), "p", "tr", "v1").is_err());
    }
}
