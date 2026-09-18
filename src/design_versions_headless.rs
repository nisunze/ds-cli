//! Headless transformer version workflow. Hosts supply authenticated reads;
//! all validation, snapshot preparation and comparison decisions stay here.
use crate::design_versions::Scope;
use serde_json::{json, Value};

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
    Object {
        kind: String,
        command: Box<Command>,
    },
    Status {
        transformer: String,
    },
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
            Self::Object { command, .. } => command.transformer(),
            Self::Status { transformer }
            | Self::List { transformer }
            | Self::Compare { transformer, .. }
            | Self::Begin { transformer, .. }
            | Self::Restore { transformer, .. } => transformer,
        }
    }
    pub fn kind(&self) -> &str {
        match self {
            Self::Object { kind, .. } => kind,
            _ => "lv_transformer",
        }
    }
    pub fn validate(&self, project: &str) -> Result<(), String> {
        if let Self::Object { kind, command } = self {
            if !matches!(kind.as_str(), "lv_transformer" | "mv_model")
                || matches!(command.as_ref(), Self::Object { .. })
                || (kind == "mv_model" && matches!(command.as_ref(), Self::Restore { .. }))
            {
                return Err("MV restore is unavailable; download its pinned model revision through dsgrid model download".into());
            }
            return command.validate(project);
        }
        if !identifier(project) || !identifier(self.transformer()) {
            return Err(invalid("invalid project or transformer identity"));
        }
        if let Self::Compare { from, to, .. } = self {
            if !version(from) || (to != "head" && !version(to)) || from == to {
                return Err(invalid(
                    "choose distinct canonical vN versions; --to may be head. Device-local drafts are not governance versions.",
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
    kind: &str,
) -> Result<(), String> {
    let id = value["version_id"]
        .as_str()
        .ok_or_else(|| bad("missing version identity"))?;
    if value["project_id"] != project
        || value["object"]["kind"] != kind
        || value["object"]["id"] != transformer
        || !version(id)
        || value["version"].as_u64() != Some(ordinal(id))
        || expected.is_some_and(|s| s != id)
    {
        return Err(bad(
            "version response belongs to another project, transformer or revision",
        ));
    }
    Ok(())
}
fn snapshot(data: &Value, project: &str, transformer: &str, id: &str) -> Result<Value, String> {
    record(
        &data["version"],
        project,
        transformer,
        Some(id),
        "lv_transformer",
    )?;
    if data["version"]["playback_available"] != true
        || data["snapshot"]["schema"] != "ds.transformer-version-snapshot/v1"
        || !data["snapshot"]["layers"].is_object()
    {
        return Err("playback_unavailable: retained version has no readable snapshot; choose a version with playback_available=true".into());
    }
    Ok(data["snapshot"].clone())
}

fn listed<'a>(
    data: &'a Value,
    project: &str,
    transformer: &str,
    kind: &str,
) -> Result<&'a [Value], String> {
    let rows = data["versions"]
        .as_array()
        .ok_or_else(|| bad("missing versions list"))?;
    if rows.len() > 200 {
        return Err(bad("version list exceeds 200 records"));
    }
    for row in rows {
        record(row, project, transformer, None, kind)?;
    }
    Ok(rows)
}

fn ordinal(id: &str) -> u64 {
    id.strip_prefix('v')
        .and_then(|value| value.parse().ok())
        .unwrap_or_default()
}

/// Hosts provide authenticated transport only. This facade plans server-owned
/// history requests and validates their returned scope without local capture.
pub fn authority_command(input: &[u8]) -> Result<Value, String> {
    if input.len() > 64 * 1024 * 1024 {
        return Err("version authority request exceeds 64 MiB".into());
    }
    let request: Value = serde_json::from_slice(input).map_err(|error| error.to_string())?;
    let project = request["project"].as_str().ok_or("missing project")?;
    let object = &request["object"];
    let kind = object["kind"].as_str().ok_or("missing object kind")?;
    let id = object["id"].as_str().ok_or("missing object identity")?;
    if !identifier(project) || !identifier(id) || !matches!(kind, "lv_transformer" | "mv_model") {
        return Err("invalid project or version object identity".into());
    }
    let operation = request["operation"].as_str().ok_or("missing operation")?;
    if !matches!(
        operation,
        "create_version" | "list_versions" | "get_head" | "get_version"
    ) {
        return Err("unsupported version authority operation".into());
    }
    match request["action"].as_str() {
        Some("plan") => {
            let mut payload = json!({"action":operation,"project_id":project,"object":object});
            if operation == "create_version" {
                let reason = request["reason"].as_str().ok_or("missing reason")?;
                let key = request["idempotency_key"]
                    .as_str()
                    .ok_or("missing idempotency key")?;
                Command::Begin {
                    transformer: id.into(),
                    reason: reason.into(),
                    idempotency_key: key.into(),
                }
                .validate(project)?;
                payload["version"] = json!({"reason":reason,"idempotency_key":key});
                if let Some(expected) = request.get("expected_source_update_time") {
                    let expected = expected
                        .as_str()
                        .filter(|value| {
                            !value.is_empty()
                                && value.len() <= 128
                                && !value.chars().any(char::is_control)
                        })
                        .ok_or("invalid expected source update time")?;
                    payload["version"]["expected_source_revision"] = json!(expected);
                }
            }
            if operation == "get_version" {
                let version_id = request["version_id"]
                    .as_str()
                    .filter(|value| version(value))
                    .ok_or("choose an assigned vN version")?;
                payload["version_id"] = json!(version_id);
            }
            Ok(json!({"method":"POST","path":"/api/v1/design/versions","payload":payload}))
        }
        Some("accept") => {
            let response = &request["response"];
            if response["success"] != true || !response["data"].is_object() {
                return Err("version server response is not successful".into());
            }
            let data = &response["data"];
            let check = |row: &Value, assigned: bool| -> Result<(), String> {
                if assigned {
                    return record(row, project, id, None, kind);
                }
                if row["project_id"] != project
                    || row["object"]["kind"] != kind
                    || row["object"]["id"] != id
                    || !row["source_revision"].is_string()
                {
                    return Err("saved head response belongs to another project or object".into());
                }
                Ok(())
            };
            match operation {
                "list_versions" => {
                    let rows = data["versions"].as_array().ok_or("missing versions list")?;
                    if rows.len() > 200 {
                        return Err("version list exceeds 200 records".into());
                    }
                    for row in rows {
                        check(row, true)?;
                    }
                }
                "get_head" => check(&data["head"], false)?,
                _ => check(&data["version"], true)?,
            }
            Ok(data.clone())
        }
        _ => Err("choose plan or accept".into()),
    }
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
    let kind = command.kind();
    if scope.object_kind != kind || scope.object_id != transformer {
        return Err(invalid("version scope does not match transformer").into());
    }
    let command = match command {
        Command::Object { command, .. } => command.as_ref(),
        _ => command,
    };
    if kind == "mv_model" {
        return run_model(scope, source, command);
    }
    match command {
        Command::Object { .. } => unreachable!("validated unwrapped object"),
        Command::Status { .. } => {
            let data = source
                .current(project, transformer)
                .map_err(Error::Source)?;
            validate_head(&data["head"], scope)?;
            Ok(
                json!({"project":project,"object":{"kind":kind,"id":transformer},"head":data["head"],"mutated":false}),
            )
        }
        Command::List { .. } => {
            let data = source
                .versions(project, transformer, None)
                .map_err(Error::Source)?;
            let rows = listed(&data, project, transformer, kind)?;
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
            let rows = listed(&inventory, project, transformer, kind)?;
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
            record(&data["version"], project, transformer, None, kind)?;
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
            record(&data["version"], project, transformer, Some(version), kind)?;
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
fn validate_head(head: &Value, scope: &Scope) -> Result<(), String> {
    if head["project_id"] != scope.project
        || head["object"]["kind"] != scope.object_kind
        || head["object"]["id"] != scope.object_id
        || !head["source_revision"].is_string()
    {
        return Err("saved head belongs to another project or object".into());
    }
    Ok(())
}

fn run_model<S: Source>(
    scope: &Scope,
    source: &mut S,
    command: &Command,
) -> Result<Value, Error<S::Error>> {
    let project = scope.project.as_str();
    let object = scope.object_id.as_str();
    let kind = scope.object_kind.as_str();
    match command {
        Command::List { .. } => {
            let data = source.versions(project, object, None).map_err(Error::Source)?;
            let rows = listed(&data, project, object, kind)?;
            Ok(json!({"project":project,"object":{"kind":kind,"id":object},"versions":rows,"truncated":rows.len()==200,"scope":"published_versions","mutated":false}))
        }
        Command::Status { .. } => {
            let data = source.current(project, object).map_err(Error::Source)?;
            validate_head(&data["head"], scope)?;
            Ok(json!({"project":project,"object":{"kind":kind,"id":object},"head":data["head"],"mutated":false}))
        }
        Command::Begin { reason, idempotency_key, .. } => {
            let data = source.create(project, object, reason, idempotency_key).map_err(Error::Source)?;
            record(&data["version"], project, object, None, kind)?;
            Ok(json!({"project":project,"object":{"kind":kind,"id":object},"version":data["version"],"mutated":true}))
        }
        Command::Compare { from, to, .. } => {
            let left = source.versions(project, object, Some(from)).map_err(Error::Source)?;
            record(&left["version"], project, object, Some(from), kind)?;
            let right = if to == "head" {
                let data = source.current(project, object).map_err(Error::Source)?;
                validate_head(&data["head"], scope)?;
                data["head"].clone()
            } else {
                let data = source.versions(project, object, Some(to)).map_err(Error::Source)?;
                record(&data["version"], project, object, Some(to), kind)?;
                data["version"].clone()
            };
            let keys = ["source_revision", "pinned_model_digest", "manifest_model_id", "manifest_model_revision"];
            let changed: Vec<_> = keys.into_iter().filter(|key| left["version"][key] != right[key]).collect();
            Ok(json!({"project":project,"object":{"kind":kind,"id":object},"from":left["version"],"to":right,"comparison":{"scope":"published_model_revision_metadata","changed_fields":changed},"geometry_included":false,"mutated":false}))
        }
        _ => Err("MV restore is unavailable; download its pinned model revision through dsgrid model download".to_string().into()),
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
    fn server_authority_facade_fences_interleaved_projects_and_forwards_source_guard() {
        let invoke = |value: Value| authority_command(&serde_json::to_vec(&value).unwrap());
        let plan = invoke(json!({"action":"plan","project":"a","object":{"kind":"lv_transformer","id":"tr"},"operation":"create_version","reason":"Reviewed baseline","idempotency_key":"retry-a","expected_source_update_time":"2026-09-18T12:00:00Z"})).unwrap();
        assert_eq!(
            plan["payload"]["version"]["expected_source_revision"],
            "2026-09-18T12:00:00Z"
        );
        assert_eq!(plan["payload"]["project_id"], "a");
        let response = json!({"success":true,"data":{"version":{"project_id":"a","object":{"kind":"lv_transformer","id":"tr"},"version_id":"v3","version":3}}});
        let mut accept = json!({"action":"accept","project":"a","object":{"kind":"lv_transformer","id":"tr"},"operation":"create_version","response":response});
        assert!(invoke(accept.clone()).is_ok());
        accept["project"] = json!("b");
        assert!(invoke(accept.clone()).is_err());
        accept["project"] = json!("a");
        accept["object"]["kind"] = json!("mv_model");
        assert!(invoke(accept).is_err());
    }
    #[test]
    fn exact_scope_and_versions_are_required() {
        assert!(Command::Compare {
            transformer: "tr".into(),
            from: "v1".into(),
            to: "head".into()
        }
        .validate("p")
        .is_ok());
        for id in ["v0", "v01", "v-1", "local-abc", "v18446744073709551615"] {
            assert!(!version(id));
        }
        let row = json!({"project_id":"p","object":{"kind":"lv_transformer","id":"tr"},"version_id":"v1","version":1});
        assert!(record(&row, "p", "tr", Some("v1"), "lv_transformer").is_ok());
        assert!(record(&row, "other", "tr", Some("v1"), "lv_transformer").is_err());
        assert!(record(&row, "p", "other", Some("v1"), "lv_transformer").is_err());
        assert!(record(&row, "p", "tr", Some("v2"), "lv_transformer").is_err());
        assert!(snapshot(&json!({"version":row}), "p", "tr", "v1").is_err());
    }
}
