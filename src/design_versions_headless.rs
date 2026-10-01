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
        create: &Create,
    ) -> Result<Value, Self::Error>;
    /// One `create_versions` call: every item is its own object and fence.
    fn create_many(
        &mut self,
        project: &str,
        items: &[BatchItem],
        shared: &Shared,
    ) -> Result<Value, Self::Error>;
    /// One `list_version_summaries` call over a bounded object selection.
    fn summaries(&mut self, project: &str, objects: &[ObjectRef]) -> Result<Value, Self::Error>;
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
/// The mandatory reason, optional milestone, optional source fence and
/// replay key of one created version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Create {
    pub reason: String,
    pub milestone: Option<String>,
    /// MV: the head revision id the caller reviewed, or `-` for "no content
    /// head yet". LV: the RFC3339 transformer update time. A moved source is
    /// refused by the Server instead of being pinned.
    pub expected_source: Option<String>,
    pub idempotency_key: String,
}
/// One object of a governed selection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectRef {
    pub kind: String,
    pub id: String,
}
/// One object of a batched create, with its own fence and replay key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct BatchItem {
    pub object: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_source_revision: Option<String>,
    pub idempotency_key: String,
}
/// Defaults an item without its own reason or milestone takes.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Shared {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
}
/// The Server's bounds: one selection, milestone bytes, fence bytes.
pub const MAX_SELECTION: usize = 200;
pub const MILESTONE_MAX: usize = 120;
pub const FENCE_MAX: usize = 128;
/// The fence that asserts an MV model has no content head yet.
pub const NO_SOURCE_FENCE: &str = "-";

pub enum Command {
    Object {
        kind: String,
        command: Box<Command>,
    },
    /// One assigned version's record, without playback content.
    Show {
        transformer: String,
        version: String,
    },
    /// Versions for many objects in one request; each item reports its own
    /// outcome and a failed item never undoes another.
    BeginBatch {
        items: Vec<BatchItem>,
        shared: Shared,
    },
    /// Latest version and count per object for a bounded selection.
    Summaries {
        objects: Vec<ObjectRef>,
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
        create: Create,
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

fn reason_text(reason: &str) -> bool {
    !reason.is_empty()
        && reason.trim() == reason
        && reason.chars().count() <= crate::design_version::VERSION_REASON_MAX
        && !reason.chars().any(char::is_control)
}
fn replay_key(key: &str) -> bool {
    !key.is_empty() && key.chars().count() <= 128 && !key.chars().any(char::is_control)
}
fn optional_text(value: Option<&str>, bound: usize) -> bool {
    value.is_none_or(|text| {
        !text.is_empty()
            && text.len() <= bound
            && text.trim() == text
            && !text.chars().any(char::is_control)
    })
}
/// The per-kind source fence the Server interprets: an MV revision id in the
/// catalog grammar or `-`; an LV RFC3339 update time. `-` is MV-only.
fn fence(kind: &str, expected: Option<&str>) -> Result<(), String> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let valid = match kind {
        "mv_model" => {
            expected == NO_SOURCE_FENCE || crate::grid_publication::catalog_identifier(expected)
        }
        "lv_transformer" => {
            expected != NO_SOURCE_FENCE
                && expected.len() <= FENCE_MAX
                && expected.contains('T')
                && !expected
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace())
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(
            "expected source must be the reviewed MV head revision id or - for no head; LV takes the transformer's RFC3339 update time",
        ))
    }
}
fn selection<'a>(objects: impl Iterator<Item = &'a ObjectRef>) -> Result<usize, String> {
    let mut seen = std::collections::BTreeSet::new();
    for object in objects {
        if !matches!(object.kind.as_str(), "lv_transformer" | "mv_model") || !identifier(&object.id)
        {
            return Err(invalid(
                "every selected object needs kind lv_transformer or mv_model and an exact id",
            ));
        }
        // The Server skips a repeated object silently; a selection that
        // names one twice is refused here so no item is quietly dropped.
        if !seen.insert((&object.kind, &object.id)) {
            return Err(invalid("an object appears twice in one selection"));
        }
    }
    if seen.is_empty() || seen.len() > MAX_SELECTION {
        return Err(invalid("a selection holds 1 to 200 objects"));
    }
    Ok(seen.len())
}
fn selection_batch(items: &[BatchItem], shared: &Shared) -> Result<(), String> {
    selection(items.iter().map(|item| &item.object))?;
    if shared.reason.as_deref().is_some_and(|r| !reason_text(r))
        || !optional_text(shared.milestone.as_deref(), MILESTONE_MAX)
    {
        return Err(invalid("shared reason or milestone is invalid"));
    }
    for item in items {
        let reason = item.reason.as_deref().or(shared.reason.as_deref());
        if !reason.is_some_and(reason_text)
            || !optional_text(item.milestone.as_deref(), MILESTONE_MAX)
            || !replay_key(&item.idempotency_key)
        {
            return Err(format!(
                "item {} needs a reason (its own or the shared one), a bounded milestone and its own idempotency key",
                item.object.id
            ));
        }
        fence(&item.object.kind, item.expected_source_revision.as_deref())?;
    }
    Ok(())
}

impl Command {
    pub fn transformer(&self) -> &str {
        match self {
            Self::Object { command, .. } => command.transformer(),
            Self::Status { transformer }
            | Self::List { transformer }
            | Self::Show { transformer, .. }
            | Self::Compare { transformer, .. }
            | Self::Begin { transformer, .. }
            | Self::Restore { transformer, .. } => transformer,
            // A selection has no single object; its items carry their own.
            Self::BeginBatch { .. } | Self::Summaries { .. } => "",
        }
    }
    pub fn kind(&self) -> &str {
        match self {
            Self::Object { kind, .. } => kind,
            Self::BeginBatch { .. } | Self::Summaries { .. } => "",
            _ => "lv_transformer",
        }
    }
    /// True for a command over a selection of objects rather than one.
    pub fn is_selection(&self) -> bool {
        matches!(self, Self::BeginBatch { .. } | Self::Summaries { .. })
    }
    pub fn validate(&self, project: &str) -> Result<(), String> {
        if let Self::Object { kind, command } = self {
            if !matches!(kind.as_str(), "lv_transformer" | "mv_model")
                || matches!(command.as_ref(), Self::Object { .. })
                || command.is_selection()
                || (kind == "mv_model" && matches!(command.as_ref(), Self::Restore { .. }))
            {
                return Err("MV restore is unavailable; download its pinned model revision through dsgrid model download".into());
            }
            if let Self::Begin { create, .. } = command.as_ref() {
                fence(kind, create.expected_source.as_deref())?;
            }
            return command.validate(project);
        }
        if !identifier(project) {
            return Err(invalid("invalid project or transformer identity"));
        }
        match self {
            Self::BeginBatch { items, shared } => return selection_batch(items, shared),
            Self::Summaries { objects } => {
                return selection(objects.iter()).map(|_| ());
            }
            _ => {}
        }
        if !identifier(self.transformer()) {
            return Err(invalid("invalid project or transformer identity"));
        }
        if let Self::Show { version: id, .. } = self {
            if !version(id) {
                return Err(invalid("show requires one canonical assigned vN version"));
            }
        }
        if let Self::Compare { from, to, .. } = self {
            if !version(from) || (to != "head" && !version(to)) || from == to {
                return Err(invalid(
                    "choose distinct canonical vN versions; --to may be head. Device-local drafts are not governance versions.",
                ));
            }
        }
        let replay = match self {
            Self::Begin { create, .. } => {
                if !optional_text(create.milestone.as_deref(), MILESTONE_MAX) {
                    return Err(invalid("version milestone is invalid (at most 120 bytes)"));
                }
                Some((create.reason.as_str(), create.idempotency_key.as_str()))
            }
            Self::Restore {
                reason,
                idempotency_key,
                ..
            } => Some((reason.as_str(), idempotency_key.as_str())),
            _ => None,
        };
        if let Some((reason, idempotency_key)) = replay {
            if !reason_text(reason) || !replay_key(idempotency_key) {
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
                let milestone = request
                    .get("milestone")
                    .filter(|v| !v.is_null())
                    .map(|v| v.as_str().map(str::to_owned).ok_or("invalid milestone"))
                    .transpose()?;
                Command::Begin {
                    transformer: id.into(),
                    create: Create {
                        reason: reason.into(),
                        idempotency_key: key.into(),
                        milestone: milestone.clone(),
                        ..Create::default()
                    },
                }
                .validate(project)?;
                payload["version"] = json!({"reason":reason,"idempotency_key":key});
                if let Some(milestone) = milestone {
                    payload["version"]["milestone"] = json!(milestone);
                }
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
            if operation == "create_version" {
                if request
                    .get("expected_source_update_time")
                    .is_some_and(|expected| data["version"]["source_revision"] != *expected)
                {
                    return Err("created version differs from the captured source revision".into());
                }
                if request
                    .get("milestone")
                    .filter(|m| !m.is_null())
                    .is_some_and(|m| data["version"]["milestone"] != *m)
                {
                    return Err("created version did not record the requested milestone".into());
                }
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
    match command {
        Command::BeginBatch { items, shared } => {
            let data = source
                .create_many(project, items, shared)
                .map_err(Error::Source)?;
            return Ok(batch_receipt(project, items, &data)?);
        }
        Command::Summaries { objects } => {
            let data = source.summaries(project, objects).map_err(Error::Source)?;
            return Ok(summaries_receipt(project, objects, &data)?);
        }
        _ => {}
    }
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
        Command::Object { .. } | Command::BeginBatch { .. } | Command::Summaries { .. } => {
            unreachable!("validated unwrapped single object")
        }
        Command::Show { version, .. } => show(source, scope, version),
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
        Command::Begin { create, .. } => {
            let data = source
                .create(project, transformer, create)
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
        Command::Begin { create, .. } => {
            let data = source.create(project, object, create).map_err(Error::Source)?;
            record(&data["version"], project, object, None, kind)?;
            // The Server echoes the milestone it stored; one it dropped would
            // make a submission marker indistinguishable from any other vN.
            if create.milestone.as_deref().is_some_and(|m| data["version"]["milestone"] != m) {
                return Err(bad("created version did not record the requested milestone").into());
            }
            Ok(json!({"project":project,"object":{"kind":kind,"id":object},"version":data["version"],"content_revision":data["version"]["source_revision"],"mutated":true}))
        }
        Command::Show { version, .. } => show(source, scope, version),
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

/// One assigned version's record. LV playback content is never carried: its
/// availability is, and `compare` reads the snapshot when a caller needs it.
/// An MV record names the exact content revision attachments bind to.
fn show<S: Source>(source: &mut S, scope: &Scope, id: &str) -> Result<Value, Error<S::Error>> {
    let (project, object, kind) = (
        scope.project.as_str(),
        scope.object_id.as_str(),
        scope.object_kind.as_str(),
    );
    let data = source
        .versions(project, object, Some(id))
        .map_err(Error::Source)?;
    record(&data["version"], project, object, Some(id), kind)?;
    let mut answer = json!({"project":project,"object":{"kind":kind,"id":object},
        "version":data["version"],"snapshot_included":false,"mutated":false});
    if kind == "mv_model" {
        answer["content_revision"] = data["version"]["source_revision"].clone();
        answer["content_pinned"] = data["version"]["content_pinned"].clone();
    }
    Ok(answer)
}

fn same_object(value: &Value, object: &ObjectRef) -> bool {
    value["kind"] == object.kind && value["id"] == object.id
}

/// Every item answers once, in order, for exactly the object it named; an ok
/// item carries a version record of that object in this project.
fn batch_receipt(project: &str, items: &[BatchItem], data: &Value) -> Result<Value, String> {
    let results = data["results"]
        .as_array()
        .ok_or_else(|| bad("missing batch results"))?;
    if results.len() != items.len() {
        return Err(bad("batch results do not answer every requested object"));
    }
    let mut ok = 0;
    for (result, item) in results.iter().zip(items) {
        if !same_object(&result["object"], &item.object) {
            return Err(bad("batch result belongs to another object"));
        }
        match result["ok"].as_bool() {
            Some(true) => {
                record(
                    &result["version"],
                    project,
                    &item.object.id,
                    None,
                    &item.object.kind,
                )?;
                ok += 1;
            }
            Some(false) => {}
            None => return Err(bad("batch result has no outcome")),
        }
    }
    Ok(json!({"project":project,"total":items.len(),"ok_count":ok,
        "failed_count":items.len()-ok,"results":results,"mutated":ok>0}))
}

/// One row per requested object, in order; a row may carry its own error.
fn summaries_receipt(project: &str, objects: &[ObjectRef], data: &Value) -> Result<Value, String> {
    let rows = data["summaries"]
        .as_array()
        .ok_or_else(|| bad("missing version summaries"))?;
    if rows.len() != objects.len()
        || rows
            .iter()
            .zip(objects)
            .any(|(row, object)| !same_object(&row["object"], object))
    {
        return Err(bad("version summaries do not answer the requested objects"));
    }
    for row in rows {
        if row["error"].is_null() && !row["count"].is_u64() {
            return Err(bad("version summary has no count"));
        }
        if let Some(id) = row["latest_version_id"].as_str() {
            if !version(id) {
                return Err(bad("version summary names no assigned version"));
            }
        }
    }
    Ok(json!({"project":project,"summaries":rows,"mutated":false}))
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

    /// Answers every call from a queue and records what was asked.
    #[derive(Default)]
    struct Recorded {
        answers: std::collections::VecDeque<Value>,
        asked: Vec<Value>,
    }
    impl Source for Recorded {
        type Error = ();
        fn versions(&mut self, _: &str, _: &str, v: Option<&str>) -> Result<Value, ()> {
            self.asked.push(json!({"versions":v}));
            self.answers.pop_front().ok_or(())
        }
        fn head(&mut self, _: &str, _: &str) -> Result<Value, ()> {
            Err(())
        }
        fn create(&mut self, _: &str, _: &str, create: &Create) -> Result<Value, ()> {
            self.asked.push(
                json!({"create":{"milestone":create.milestone,"expected":create.expected_source}}),
            );
            self.answers.pop_front().ok_or(())
        }
        fn create_many(&mut self, _: &str, items: &[BatchItem], _: &Shared) -> Result<Value, ()> {
            self.asked.push(json!({"create_many":items.len()}));
            self.answers.pop_front().ok_or(())
        }
        fn summaries(&mut self, _: &str, objects: &[ObjectRef]) -> Result<Value, ()> {
            self.asked.push(json!({"summaries":objects.len()}));
            self.answers.pop_front().ok_or(())
        }
        fn current(&mut self, _: &str, _: &str) -> Result<Value, ()> {
            Err(())
        }
        fn restore(
            &mut self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<Value, ()> {
            Err(())
        }
    }
    fn scope(kind: &str, id: &str) -> Scope {
        Scope {
            owner: "u".into(),
            audience: "a".into(),
            lane: "stable".into(),
            project: "p".into(),
            object_kind: kind.into(),
            object_id: id.into(),
        }
    }
    fn mv(command: Command) -> Command {
        Command::Object {
            kind: "mv_model".into(),
            command: Box::new(command),
        }
    }
    fn mv_record(id: &str, n: u64) -> Value {
        json!({"project_id":"p","object":{"kind":"mv_model","id":"model-a"},"version_id":id,"version":n,
            "source_revision":"rev-abc","content_pinned":true,"milestone":"Submission 2"})
    }
    #[test]
    fn a_submission_marker_carries_its_milestone_and_its_reviewed_head() {
        let begin = |milestone: Option<&str>, expected: Option<&str>| {
            mv(Command::Begin {
                transformer: "model-a".into(),
                create: Create {
                    reason: "Submitted to REG".into(),
                    milestone: milestone.map(str::to_owned),
                    expected_source: expected.map(str::to_owned),
                    idempotency_key: "submission-2".into(),
                },
            })
        };
        let mut source = Recorded::default();
        source
            .answers
            .push_back(json!({"version":mv_record("v2",2)}));
        let answer = run(
            &scope("mv_model", "model-a"),
            &mut source,
            &begin(Some("Submission 2"), Some("rev-abc")),
        )
        .unwrap();
        assert_eq!(answer["content_revision"], "rev-abc");
        assert_eq!(source.asked[0]["create"]["expected"], "rev-abc");
        // A Server that dropped the milestone is caught, not reported as done.
        let mut source = Recorded::default();
        source
            .answers
            .push_back(json!({"version":mv_record("v2",2)}));
        assert!(run(
            &scope("mv_model", "model-a"),
            &mut source,
            &begin(Some("Submission 3"), None)
        )
        .is_err());
        // Fences are per kind: `-` and revision ids are MV, update times LV.
        assert!(begin(None, Some("-")).validate("p").is_ok());
        assert!(begin(None, Some("Rev ABC")).validate("p").is_err());
        let lv = Command::Object {
            kind: "lv_transformer".into(),
            command: Box::new(Command::Begin {
                transformer: "TR-1".into(),
                create: Create {
                    reason: "r".into(),
                    expected_source: Some("-".into()),
                    idempotency_key: "k".into(),
                    ..Create::default()
                },
            }),
        };
        assert!(lv.validate("p").is_err());
        assert!(begin(Some(&"m".repeat(MILESTONE_MAX + 1)), None)
            .validate("p")
            .is_err());
    }
    #[test]
    fn show_reads_one_version_and_names_its_content_revision() {
        let mut source = Recorded::default();
        source
            .answers
            .push_back(json!({"version":mv_record("v2",2)}));
        let command = mv(Command::Show {
            transformer: "model-a".into(),
            version: "v2".into(),
        });
        let answer = run(&scope("mv_model", "model-a"), &mut source, &command).unwrap();
        assert_eq!(answer["content_revision"], "rev-abc");
        assert_eq!(answer["snapshot_included"], false);
        assert_eq!(source.asked[0]["versions"], "v2");
        // The Server answering with another version is refused.
        let mut source = Recorded::default();
        source
            .answers
            .push_back(json!({"version":mv_record("v3",3)}));
        assert!(run(&scope("mv_model", "model-a"), &mut source, &command).is_err());
        assert!(mv(Command::Show {
            transformer: "model-a".into(),
            version: "head".into()
        })
        .validate("p")
        .is_err());
    }
    #[test]
    fn a_selection_answers_every_object_once_and_in_order() {
        let object = |id: &str| ObjectRef {
            kind: "mv_model".into(),
            id: id.into(),
        };
        let item = |id: &str| BatchItem {
            object: object(id),
            reason: None,
            milestone: None,
            expected_source_revision: Some("-".into()),
            idempotency_key: format!("key-{id}"),
        };
        let shared = Shared {
            reason: Some("Submission 2".into()),
            milestone: None,
        };
        let batch = Command::BeginBatch {
            items: vec![item("model-a"), item("model-b")],
            shared: shared.clone(),
        };
        assert!(batch.validate("p").is_ok());
        // No reason anywhere, a repeated object, an empty selection: refused.
        assert!(Command::BeginBatch {
            items: vec![item("model-a")],
            shared: Shared::default()
        }
        .validate("p")
        .is_err());
        assert!(Command::BeginBatch {
            items: vec![item("model-a"), item("model-a")],
            shared: shared.clone()
        }
        .validate("p")
        .is_err());
        assert!(Command::Summaries { objects: vec![] }
            .validate("p")
            .is_err());
        let mut source = Recorded::default();
        source.answers.push_back(json!({"results":[
            {"object":{"kind":"mv_model","id":"model-a"},"ok":true,"version":mv_record("v2",2)},
            {"object":{"kind":"mv_model","id":"model-b"},"ok":false,"error":"moved","error_code":"conflict"}
        ]}));
        let answer = run(&scope("", ""), &mut source, &batch).unwrap();
        assert_eq!(answer["ok_count"], 1);
        assert_eq!(answer["failed_count"], 1);
        assert_eq!(answer["mutated"], true);
        let mut source = Recorded::default();
        source.answers.push_back(json!({"results":[
            {"object":{"kind":"mv_model","id":"model-b"},"ok":false},
            {"object":{"kind":"mv_model","id":"model-a"},"ok":false}
        ]}));
        assert!(run(&scope("", ""), &mut source, &batch).is_err());
        let summaries = Command::Summaries {
            objects: vec![object("model-a"), object("model-b")],
        };
        let mut source = Recorded::default();
        source.answers.push_back(json!({"summaries":[
            {"object":{"kind":"mv_model","id":"model-a"},"count":2,"truncated":false,"latest_version_id":"v2"},
            {"object":{"kind":"mv_model","id":"model-b"},"count":0,"truncated":false}
        ]}));
        let answer = run(&scope("", ""), &mut source, &summaries).unwrap();
        assert_eq!(answer["summaries"][0]["latest_version_id"], "v2");
        let mut source = Recorded::default();
        source.answers.push_back(json!({"summaries":[
            {"object":{"kind":"mv_model","id":"model-a"},"count":2}
        ]}));
        assert!(run(&scope("", ""), &mut source, &summaries).is_err());
    }
    #[test]
    fn browser_mv_named_version_preserves_milestone_and_source_fence() {
        let input = json!({"action":"plan","operation":"create_version","project":"p",
            "object":{"kind":"mv_model","id":"m"},"reason":"Reviewed",
            "idempotency_key":"named-marker-1","expected_source_update_time":"r1","milestone":"Submission"});
        let plan = authority_command(&serde_json::to_vec(&input).unwrap()).unwrap();
        assert_eq!(plan["payload"]["version"]["milestone"], "Submission");
        assert_eq!(plan["payload"]["version"]["expected_source_revision"], "r1");
        assert!(plan["payload"].get("expected_head_revision_id").is_none());
        let mut answer = input.clone();
        answer["action"] = json!("accept");
        answer["response"] = json!({"success":true,"data":{"version":{"project_id":"p",
            "object":{"kind":"mv_model","id":"m"},"version_id":"v1","version":1,
            "source_revision":"r1","milestone":"Submission","content_pinned":true}}});
        assert!(authority_command(&serde_json::to_vec(&answer).unwrap()).is_ok());
        answer["response"]["data"]["version"]["source_revision"] = json!("r2");
        assert!(authority_command(&serde_json::to_vec(&answer).unwrap()).is_err());
        answer["response"]["data"]["version"]["source_revision"] = json!("r1");
        answer["response"]["data"]["version"]["milestone"] = json!("Dropped");
        assert!(authority_command(&serde_json::to_vec(&answer).unwrap()).is_err());
    }
}
