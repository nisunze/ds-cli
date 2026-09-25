//! Design collaboration annotations, headlessly: tags, tag groups, consumer
//! groupings, administrative enrichment and comment threads.
//!
//! Until 2026-09-20 twenty `ds design` commands — `tag.*`, `group.*`,
//! `consumer-grouping.*`, `comment.*` and the two enrichment commands —
//! relayed through the paired desktop to the page's own adapter
//! (`ds-web/src/lib/desktop/cli-design.ts`), which built the action bodies of
//! `POST /api/v1/design/annotations` (`api/design-collab.ts`). ds-brain is the
//! only authority on every one of them and authenticates from the bearer; the
//! window was habit, never contract. `design_tags` beside this module already
//! walked the same route for four native commands; this is the rest of the
//! vocabulary, on the same transport call.
//!
//! **This module is the DOOR, not the decisions.** Each variant builds the
//! action body exactly as the browser built it, bounded before a round trip,
//! and returns the member of `data` the browser read. What a tag definition
//! may look like is `ds_command_kernel::design_tags`'s verdict (the CLI asks
//! it before sending); what a plan means is ds-brain's.
//!
//! Closed by construction: [`Command`] is the complete set of actions this
//! owner can send, every key is written here, and ds-brain refuses unknown
//! fields.

use serde_json::{json, Map, Value};

use crate::{
    authorized::{classify_tile_error, Authority},
    AuthorizedTransport, ClientError, ClientProfile, DesignTagsCall, ErrorKind,
};

/// The longest id, name or title one field carries (`MAX_ID`, cli-design.ts).
pub const MAX_ID: usize = 200;
/// The longest free text one field carries (`MAX_TEXT`, cli-design.ts).
pub const MAX_TEXT: usize = 4_000;
/// The most values one tag definition or assignment carries.
pub const MAX_VALUES: usize = 100;
/// The most transformers one group batch names — ds-brain's write bound
/// (`MAX_TAG_GROUP_BATCH`).
pub const MAX_GROUP_BATCH: usize = 200;
/// The most transformers one group listing names (`MAX_TAG_GROUP_LISTING`).
pub const MAX_GROUP_LISTING: usize = 200;
/// The most transformers one projection or consumer grouping covers
/// (`MAX_TAG_PROJECTION_TRANSFORMERS`).
pub const MAX_PROJECTION_TRANSFORMERS: usize = 2_000;
/// The most definitions one projection or grouping orders.
pub const MAX_DEFINITION_IDS: usize = 16;
/// The most transformers one enrichment names — one Firestore transaction.
pub const MAX_ENRICHMENT_TRANSFORMERS: usize = 50;
/// The most predicates one tag query evaluates.
pub const MAX_QUERY_FILTERS: usize = 20;
/// The most rows one tag query returns.
pub const MAX_QUERY_ROWS: i64 = 2_000;
/// The longest moderation reason (`designCollabMaxReason`).
pub const MAX_REASON: usize = 2_000;
/// The largest structured input (a definition, a filter list, bindings) in
/// serialised bytes.
const MAX_STRUCTURED_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

fn id(s: &str) -> bool {
    !s.is_empty()
        && s.trim() == s
        && s.chars().count() <= MAX_ID
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
fn text(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.chars().count() <= max
}
fn ids(list: &[String], max: usize) -> bool {
    !list.is_empty() && list.len() <= max && list.iter().all(|s| id(s))
}
fn distinct(list: &[String]) -> bool {
    list.iter().collect::<std::collections::BTreeSet<_>>().len() == list.len()
}
fn digest(d: &str) -> bool {
    d.len() == 64
        && d.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn bounded_object(value: &Value) -> bool {
    value.is_object() && value.to_string().len() <= MAX_STRUCTURED_BYTES
}
fn bounded_array(value: &Value, max: usize) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.len() <= max && value.to_string().len() <= MAX_STRUCTURED_BYTES)
}

/// The anchor every annotation names: a transformer or a DS Grid model, and
/// optionally one exact version of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRef {
    /// `lv_transformer` or `mv_model`.
    pub kind: String,
    pub id: String,
    pub version_id: Option<String>,
}

impl ObjectRef {
    fn valid(&self) -> bool {
        matches!(self.kind.as_str(), "lv_transformer" | "mv_model")
            && id(&self.id)
            && self.version_id.as_deref().is_none_or(id)
    }
    /// `objectPayload`: only the keys that are set.
    fn payload(&self) -> Value {
        let mut object = json!({ "kind": self.kind, "id": self.id });
        if let Some(version) = &self.version_id {
            object["version_id"] = json!(version);
        }
        object
    }
}

/// One governed annotations action.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Every tag definition of the project, archived ones included on request.
    ListTagDefinitions {
        include_archived: bool,
    },
    /// Create or update one project tag definition. `definition` is the
    /// browser's `definition` object (`saveDesignTagDefinition`), already
    /// admitted by the kernel's `design_tags` verdict.
    SaveTagDefinition {
        definition: Value,
    },
    /// The assignments one object carries.
    ListTags {
        object: ObjectRef,
    },
    /// Set one definition's values on one object, pinned to the assignment's
    /// version. Exactly one of `values` / `typed_values`.
    SetTags {
        object: ObjectRef,
        definition_id: String,
        values: Option<Vec<String>>,
        typed_values: Option<Value>,
        expected_version: Option<i64>,
    },
    /// Which transformers carry which tags: bounded predicates, `all`/`any`.
    QueryTags {
        filters: Value,
        match_all: bool,
        limit: i64,
    },
    /// The governed administrative enrichment: a read that writes nothing.
    PreviewLocationEnrichment {
        transformers: Vec<String>,
        reference_revision: Option<String>,
    },
    /// Apply exactly the previewed enrichment.
    ApplyLocationEnrichment {
        transformers: Vec<String>,
        reference_revision: Option<String>,
        plan_digest: String,
    },
    /// The groups, their vocabularies, and what these transformers carry.
    ListTagGroups {
        transformers: Vec<String>,
    },
    /// Plan a group batch without writing; an entry with no value plans the
    /// unassign.
    PreviewTagGroup {
        group: String,
        entries: Vec<(String, Option<String>)>,
    },
    /// Commit the plan the operator approved.
    ApplyTagGroup {
        group: String,
        entries: Vec<(String, Option<String>)>,
        plan_digest: String,
    },
    /// Clear the group's value on every named transformer.
    UnassignTagGroup {
        group: String,
        transformers: Vec<String>,
        plan_digest: String,
    },
    /// The `ds-report.design-tags/v1` projection for a transformer set.
    ExportTagProjection {
        transformers: Vec<String>,
        definition_ids: Vec<String>,
    },
    /// Plan a consumer grouping. `bindings` are the browser's binding rows.
    PreviewConsumerGrouping {
        purpose: String,
        transformers: Vec<String>,
        definition_ids: Vec<String>,
        bindings: Value,
    },
    /// Apply exactly the previewed grouping.
    ApplyConsumerGrouping {
        purpose: String,
        transformers: Vec<String>,
        definition_ids: Vec<String>,
        bindings: Value,
        plan_digest: String,
    },
    ReadConsumerGrouping {
        purpose: String,
    },
    ArchiveConsumerGrouping {
        purpose: String,
    },
    /// The comment threads on one object.
    ListThreads {
        object: ObjectRef,
        include_resolved: bool,
    },
    /// One thread with its comments.
    GetThread {
        thread_id: String,
    },
    /// Open a thread with its first comment. The thread id is minted by the
    /// host (`ds` has entropy; the kernel does not).
    CreateThread {
        thread_id: String,
        object: ObjectRef,
        title: String,
        body: String,
    },
    /// Append one comment, with a host-minted comment id.
    AddComment {
        thread_id: String,
        comment_id: String,
        body: String,
    },
    /// Resolve, or reopen, one thread at its current version.
    ResolveThread {
        thread_id: String,
        expected_version: i64,
        reopen: bool,
    },
    /// Redact one comment's body under the thread version the moderator
    /// read, with the reason the audit keeps. Irreversible by construction:
    /// ds-brain does not retain the text it clears.
    RedactComment {
        thread_id: String,
        comment_id: String,
        expected_version: i64,
        reason: String,
    },
    /// Promote one thread to a Project Management task, at its current
    /// version, against the plan's current revision.
    PromoteThread {
        thread_id: String,
        expected_version: i64,
        title: String,
        command_id: String,
        base_revision: i64,
    },
}

fn purpose_valid(purpose: &str) -> bool {
    matches!(purpose, "solar_report" | "report_archive")
}

fn entries_valid(entries: &[(String, Option<String>)]) -> bool {
    !entries.is_empty()
        && entries.len() <= MAX_GROUP_BATCH
        && entries
            .iter()
            .all(|(name, value)| id(name) && value.as_deref().is_none_or(id))
        && entries
            .iter()
            .map(|(name, _)| name)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == entries.len()
}

impl Command {
    fn action(&self) -> &'static str {
        match self {
            Self::ListTagDefinitions { .. } => "list_tag_definitions",
            Self::SaveTagDefinition { .. } => "save_tag_definition",
            Self::ListTags { .. } => "list_tags",
            Self::SetTags { .. } => "set_tags",
            Self::QueryTags { .. } => "query_tags",
            Self::PreviewLocationEnrichment { .. } => "preview_location_enrichment",
            Self::ApplyLocationEnrichment { .. } => "apply_location_enrichment",
            Self::ListTagGroups { .. } => "list_tag_groups",
            Self::PreviewTagGroup { .. } => "preview_tag_group",
            Self::ApplyTagGroup { .. } => "apply_tag_group",
            Self::UnassignTagGroup { .. } => "unassign_tag_group",
            Self::ExportTagProjection { .. } => "export_tag_projection",
            Self::PreviewConsumerGrouping { .. } => "preview_consumer_grouping",
            Self::ApplyConsumerGrouping { .. } => "apply_consumer_grouping",
            Self::ReadConsumerGrouping { .. } => "read_consumer_grouping",
            Self::ArchiveConsumerGrouping { .. } => "archive_consumer_grouping",
            Self::ListThreads { .. } => "list_threads",
            Self::GetThread { .. } => "get_thread",
            Self::CreateThread { .. } => "create_thread",
            Self::AddComment { .. } => "add_comment",
            Self::ResolveThread { .. } => "resolve_thread",
            Self::RedactComment { .. } => "redact_comment",
            Self::PromoteThread { .. } => "promote_thread",
        }
    }

    /// Refuse locally what ds-brain would refuse by name, before a round
    /// trip is spent on it.
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        let ok = id(project)
            && match self {
                Self::ListTagDefinitions { .. } => true,
                Self::SaveTagDefinition { definition } => {
                    bounded_object(definition)
                        && definition["definition_id"].as_str().is_some_and(id)
                        && definition["name"].as_str().is_some_and(id)
                }
                Self::ListTags { object } => object.valid(),
                Self::SetTags {
                    object,
                    definition_id,
                    values,
                    typed_values,
                    expected_version,
                } => {
                    object.valid()
                        && id(definition_id)
                        && (values.is_some() != typed_values.is_some())
                        && values.as_ref().is_none_or(|list| {
                            list.len() <= MAX_VALUES && list.iter().all(|v| text(v, MAX_ID))
                        })
                        && typed_values
                            .as_ref()
                            .is_none_or(|list| bounded_array(list, MAX_VALUES))
                        && expected_version.is_none_or(|v| v >= 0)
                }
                Self::QueryTags { filters, limit, .. } => {
                    bounded_array(filters, MAX_QUERY_FILTERS)
                        && filters.as_array().is_some_and(|rows| {
                            !rows.is_empty() && rows.iter().all(Value::is_object)
                        })
                        && (1..=MAX_QUERY_ROWS).contains(limit)
                }
                Self::PreviewLocationEnrichment {
                    transformers,
                    reference_revision,
                } => {
                    ids(transformers, MAX_ENRICHMENT_TRANSFORMERS)
                        && distinct(transformers)
                        && reference_revision.as_deref().is_none_or(|r| text(r, 128))
                }
                Self::ApplyLocationEnrichment {
                    transformers,
                    reference_revision,
                    plan_digest,
                } => {
                    ids(transformers, MAX_ENRICHMENT_TRANSFORMERS)
                        && distinct(transformers)
                        && reference_revision.as_deref().is_none_or(|r| text(r, 128))
                        && digest(plan_digest)
                }
                Self::ListTagGroups { transformers } => {
                    transformers.len() <= MAX_GROUP_LISTING
                        && transformers.iter().all(|s| id(s))
                        && distinct(transformers)
                }
                Self::PreviewTagGroup { group, entries } => id(group) && entries_valid(entries),
                Self::ApplyTagGroup {
                    group,
                    entries,
                    plan_digest,
                } => id(group) && entries_valid(entries) && digest(plan_digest),
                Self::UnassignTagGroup {
                    group,
                    transformers,
                    plan_digest,
                } => {
                    id(group)
                        && ids(transformers, MAX_GROUP_BATCH)
                        && distinct(transformers)
                        && digest(plan_digest)
                }
                Self::ExportTagProjection {
                    transformers,
                    definition_ids,
                } => {
                    ids(transformers, MAX_PROJECTION_TRANSFORMERS)
                        && distinct(transformers)
                        && definition_ids.len() <= MAX_DEFINITION_IDS
                        && definition_ids.iter().all(|s| id(s))
                        && distinct(definition_ids)
                }
                Self::PreviewConsumerGrouping {
                    purpose,
                    transformers,
                    definition_ids,
                    bindings,
                } => {
                    purpose_valid(purpose)
                        && ids(transformers, MAX_PROJECTION_TRANSFORMERS)
                        && distinct(transformers)
                        && ids(definition_ids, MAX_DEFINITION_IDS)
                        && distinct(definition_ids)
                        && bounded_array(bindings, MAX_PROJECTION_TRANSFORMERS)
                        && (purpose != "report_archive"
                            || bindings.as_array().is_some_and(Vec::is_empty))
                }
                Self::ApplyConsumerGrouping {
                    purpose,
                    transformers,
                    definition_ids,
                    bindings,
                    plan_digest,
                } => {
                    purpose_valid(purpose)
                        && ids(transformers, MAX_PROJECTION_TRANSFORMERS)
                        && distinct(transformers)
                        && ids(definition_ids, MAX_DEFINITION_IDS)
                        && distinct(definition_ids)
                        && bounded_array(bindings, MAX_PROJECTION_TRANSFORMERS)
                        && (purpose != "report_archive"
                            || bindings.as_array().is_some_and(Vec::is_empty))
                        && digest(plan_digest)
                }
                Self::ReadConsumerGrouping { purpose }
                | Self::ArchiveConsumerGrouping { purpose } => purpose_valid(purpose),
                Self::ListThreads { object, .. } => object.valid(),
                Self::GetThread { thread_id } => id(thread_id),
                Self::CreateThread {
                    thread_id,
                    object,
                    title,
                    body,
                } => id(thread_id) && object.valid() && text(title, MAX_ID) && text(body, MAX_TEXT),
                Self::AddComment {
                    thread_id,
                    comment_id,
                    body,
                } => id(thread_id) && id(comment_id) && text(body, MAX_TEXT),
                Self::ResolveThread {
                    thread_id,
                    expected_version,
                    ..
                } => id(thread_id) && *expected_version >= 0,
                Self::RedactComment {
                    thread_id,
                    comment_id,
                    expected_version,
                    reason,
                } => {
                    id(thread_id)
                        && id(comment_id)
                        && *expected_version >= 1
                        && text(reason, MAX_REASON)
                }
                Self::PromoteThread {
                    thread_id,
                    expected_version,
                    title,
                    command_id,
                    base_revision,
                } => {
                    id(thread_id)
                        && *expected_version >= 0
                        && text(title, MAX_ID)
                        && id(command_id)
                        && *base_revision >= 0
                }
            };
        if ok {
            Ok(())
        } else {
            Err(ClientError::new(
                ErrorKind::InvalidInput,
                "invalid design annotation request: an anchor, an id, a list, a bound or a digest is outside the vocabulary",
            ))
        }
    }

    /// The action body, key for key as `api/design-collab.ts` builds it.
    fn payload(&self, project: &str) -> Value {
        let mut payload = json!({ "action": self.action(), "project_id": project });
        let put = |payload: &mut Value, key: &str, value: Value| {
            payload[key] = value;
        };
        match self {
            Self::ListTagDefinitions { include_archived } => {
                if *include_archived {
                    put(&mut payload, "include_archived", json!(true));
                }
            }
            Self::SaveTagDefinition { definition } => {
                put(&mut payload, "definition", definition.clone())
            }
            Self::ListTags { object } => put(&mut payload, "object", object.payload()),
            Self::SetTags {
                object,
                definition_id,
                values,
                typed_values,
                expected_version,
            } => {
                let mut tags =
                    json!({ "object": object.payload(), "definition_id": definition_id });
                match (typed_values, values) {
                    (Some(typed), _) => tags["typed_values"] = typed.clone(),
                    (None, Some(values)) => tags["values"] = json!(values),
                    (None, None) => {}
                }
                if let Some(version) = expected_version {
                    tags["expected_version"] = json!(version);
                }
                put(&mut payload, "tags", tags);
            }
            Self::QueryTags {
                filters,
                match_all,
                limit,
            } => put(
                &mut payload,
                "tag_query",
                json!({
                    "object_kind": "lv_transformer",
                    "match": if *match_all { "all" } else { "any" },
                    "filters": filters,
                    "limit": limit,
                }),
            ),
            Self::PreviewLocationEnrichment {
                transformers,
                reference_revision,
            } => {
                let mut enrichment = json!({ "transformers": transformers });
                if let Some(revision) = reference_revision {
                    enrichment["reference_revision"] = json!(revision);
                }
                put(&mut payload, "location_enrichment", enrichment);
            }
            Self::ApplyLocationEnrichment {
                transformers,
                reference_revision,
                plan_digest,
            } => {
                let mut enrichment =
                    json!({ "transformers": transformers, "plan_digest": plan_digest });
                if let Some(revision) = reference_revision {
                    enrichment["reference_revision"] = json!(revision);
                }
                put(&mut payload, "location_enrichment", enrichment);
            }
            Self::ListTagGroups { transformers } => {
                if !transformers.is_empty() {
                    put(&mut payload, "transformers", json!(transformers));
                }
            }
            Self::PreviewTagGroup { group, entries } => put(
                &mut payload,
                "group_batch",
                json!({ "group": group, "entries": entries_payload(entries) }),
            ),
            Self::ApplyTagGroup {
                group,
                entries,
                plan_digest,
            } => put(
                &mut payload,
                "group_batch",
                json!({ "group": group, "entries": entries_payload(entries), "expected_plan_digest": plan_digest }),
            ),
            Self::UnassignTagGroup {
                group,
                transformers,
                plan_digest,
            } => put(
                &mut payload,
                "group_batch",
                json!({
                    "group": group,
                    "entries": transformers.iter().map(|t| json!({ "transformer": t })).collect::<Vec<_>>(),
                    "expected_plan_digest": plan_digest,
                }),
            ),
            Self::ExportTagProjection {
                transformers,
                definition_ids,
            } => {
                put(&mut payload, "transformers", json!(transformers));
                put(&mut payload, "definition_ids", json!(definition_ids));
            }
            Self::PreviewConsumerGrouping {
                purpose,
                transformers,
                definition_ids,
                bindings,
            } => put(
                &mut payload,
                "consumer_grouping",
                json!({ "purpose": purpose, "transformers": transformers, "definition_ids": definition_ids, "bindings": bindings }),
            ),
            Self::ApplyConsumerGrouping {
                purpose,
                transformers,
                definition_ids,
                bindings,
                plan_digest,
            } => put(
                &mut payload,
                "consumer_grouping",
                json!({ "purpose": purpose, "transformers": transformers, "definition_ids": definition_ids, "bindings": bindings, "plan_digest": plan_digest }),
            ),
            Self::ReadConsumerGrouping { purpose } | Self::ArchiveConsumerGrouping { purpose } => {
                put(&mut payload, "purpose", json!(purpose))
            }
            Self::ListThreads {
                object,
                include_resolved,
            } => {
                put(&mut payload, "object", object.payload());
                if *include_resolved {
                    put(&mut payload, "include_resolved", json!(true));
                }
            }
            Self::GetThread { thread_id } => put(&mut payload, "thread_id", json!(thread_id)),
            Self::CreateThread {
                thread_id,
                object,
                title,
                body,
            } => put(
                &mut payload,
                "thread",
                json!({ "thread_id": thread_id, "object": object.payload(), "title": title, "body": body }),
            ),
            Self::AddComment {
                thread_id,
                comment_id,
                body,
            } => put(
                &mut payload,
                "comment",
                json!({ "thread_id": thread_id, "comment_id": comment_id, "body": body }),
            ),
            Self::ResolveThread {
                thread_id,
                expected_version,
                reopen,
            } => {
                let mut resolution =
                    json!({ "thread_id": thread_id, "expected_version": expected_version });
                if *reopen {
                    resolution["reopen"] = json!(true);
                }
                put(&mut payload, "resolution", resolution);
            }
            Self::RedactComment {
                thread_id,
                comment_id,
                expected_version,
                reason,
            } => put(
                &mut payload,
                "redaction",
                json!({
                    "thread_id": thread_id, "comment_id": comment_id,
                    "expected_version": expected_version, "reason": reason.trim(),
                }),
            ),
            Self::PromoteThread {
                thread_id,
                expected_version,
                title,
                command_id,
                base_revision,
            } => put(
                &mut payload,
                "promotion_task",
                json!({
                    "thread_id": thread_id, "expected_version": expected_version, "title": title,
                    "review_required": true, "command_id": command_id, "base_revision": base_revision,
                }),
            ),
        }
        payload
    }

    /// The member of `data` the browser read for this action.
    fn answer(&self, data: &Value) -> Option<Value> {
        let member = match self {
            Self::ListTagDefinitions { .. } => "definitions",
            Self::SaveTagDefinition { .. } => "definition",
            Self::ListTags { .. } | Self::SetTags { .. } => "tags",
            Self::QueryTags { .. } => "query",
            Self::PreviewLocationEnrichment { .. } | Self::ApplyLocationEnrichment { .. } => {
                "enrichment"
            }
            Self::ListTagGroups { .. } => "groups",
            Self::PreviewTagGroup { .. }
            | Self::ApplyTagGroup { .. }
            | Self::UnassignTagGroup { .. }
            | Self::PreviewConsumerGrouping { .. }
            | Self::ApplyConsumerGrouping { .. }
            | Self::ReadConsumerGrouping { .. }
            | Self::ArchiveConsumerGrouping { .. } => "plan",
            Self::ExportTagProjection { .. } => "projection",
            Self::ListThreads { .. } => "threads",
            // A thread action answers the resolved thread whole.
            Self::GetThread { .. }
            | Self::CreateThread { .. }
            | Self::AddComment { .. }
            | Self::ResolveThread { .. }
            | Self::RedactComment { .. }
            | Self::PromoteThread { .. } => return Some(data.clone()),
        };
        let value = data.get(member)?;
        // A list the server left null is an empty list, as `list()` read it.
        Some(match (member, value) {
            ("definitions" | "tags" | "groups" | "threads", Value::Null) => json!([]),
            _ => value.clone(),
        })
    }
}

fn entries_payload(entries: &[(String, Option<String>)]) -> Vec<Value> {
    entries
        .iter()
        .map(|(transformer, value)| match value {
            Some(value) => json!({ "transformer": transformer, "value": value }),
            None => json!({ "transformer": transformer }),
        })
        .collect()
}

fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "design annotations did not answer in a shape this build understands",
    )
}

/// Send one governed annotations action for one project and return the
/// member of `data` the action answers.
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate(project)?;
    let response = transport.design_tags(DesignTagsCall {
        profile,
        authority: authority.call(),
        payload: command.payload(project),
    })?;
    if response.status != 200 {
        let body = if response.body.len() <= 64 * 1024 {
            serde_json::from_slice::<Value>(&response.body).ok()
        } else {
            None
        };
        let error = body.as_ref().and_then(|v| v.get("error"));
        let message = error
            .and_then(|v| v.get("message"))
            .and_then(Value::as_str)
            .or_else(|| {
                body.as_ref()
                    .and_then(|v| v.get("message"))
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                body.as_ref()
                    .and_then(|v| v.get("detail"))
                    .and_then(Value::as_str)
            });
        let code = error
            .and_then(|v| v.get("code"))
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase);
        let kind = match response.status {
            404 => ErrorKind::ResourceNotFound,
            409 | 412 => ErrorKind::InvalidInput,
            _ => classify_tile_error(response.status).kind(),
        };
        return Err(ClientError::with_service_refusal(
            kind,
            "design annotations refused the project operation",
            crate::ServiceRefusal::new(response.status, code.as_deref(), message),
        ));
    }
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(bad());
    }
    let reply: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if reply["success"] != true || !reply["data"].is_object() {
        return Err(bad());
    }
    command.answer(&reply["data"]).ok_or_else(bad)
}

/// The keys a caller may pass through as a structured input, kept as one
/// bounded object so a definition authored on the CLI is the same object the
/// tag editor sends.
pub fn structured(value: Value) -> Result<Value, ClientError> {
    if bounded_object(&value) {
        Ok(value)
    } else {
        Err(ClientError::new(
            ErrorKind::InvalidInput,
            "a structured design input is one JSON object of at most 64 KiB",
        ))
    }
}

/// An empty map, for callers building a definition key by key.
pub fn object() -> Map<String, Value> {
    Map::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor() -> ObjectRef {
        ObjectRef {
            kind: "lv_transformer".into(),
            id: "T-1".into(),
            version_id: None,
        }
    }

    #[test]
    fn every_action_is_named_and_its_body_is_the_browsers() {
        let set = Command::SetTags {
            object: anchor(),
            definition_id: "city".into(),
            values: Some(vec!["gagal".into()]),
            typed_values: None,
            expected_version: Some(2),
        };
        assert_eq!(set.action(), "set_tags");
        let body = set.payload("p");
        assert_eq!(
            body["tags"]["object"],
            json!({"kind": "lv_transformer", "id": "T-1"})
        );
        assert_eq!(body["tags"]["values"], json!(["gagal"]));
        assert_eq!(body["tags"]["expected_version"], 2);

        let query = Command::QueryTags {
            filters: json!([{"definition_id": "city", "operator": "exists"}]),
            match_all: false,
            limit: 100,
        };
        assert_eq!(query.payload("p")["tag_query"]["match"], "any");

        let unassign = Command::UnassignTagGroup {
            group: "city".into(),
            transformers: vec!["T-1".into(), "T-2".into()],
            plan_digest: "a".repeat(64),
        };
        assert_eq!(unassign.action(), "unassign_tag_group");
        assert_eq!(
            unassign.payload("p")["group_batch"]["entries"],
            json!([{"transformer": "T-1"}, {"transformer": "T-2"}])
        );

        let promote = Command::PromoteThread {
            thread_id: "thread-1".into(),
            expected_version: 3,
            title: "Fix".into(),
            command_id: "c1".into(),
            base_revision: 7,
        };
        let body = promote.payload("p");
        assert_eq!(body["promotion_task"]["review_required"], true);
        assert_eq!(body["promotion_task"]["base_revision"], 7);

        let resolve = Command::ResolveThread {
            thread_id: "thread-1".into(),
            expected_version: 3,
            reopen: true,
        };
        assert_eq!(resolve.payload("p")["resolution"]["reopen"], true);
        let threads = Command::ListThreads {
            object: anchor(),
            include_resolved: false,
        };
        assert!(threads.payload("p").get("include_resolved").is_none());
    }

    /// The moderation action carries the version the moderator read and the
    /// reason, exactly as `redactDesignComment` builds it; a missing reason
    /// or an unread (zero) version never leaves this owner.
    #[test]
    fn a_redaction_names_the_comment_the_version_and_the_reason() {
        let redact = Command::RedactComment {
            thread_id: "thread-1".into(),
            comment_id: "c-9".into(),
            expected_version: 4,
            reason: "  personal data  ".into(),
        };
        assert_eq!(redact.action(), "redact_comment");
        assert!(redact.validate("p").is_ok());
        assert_eq!(
            redact.payload("p")["redaction"],
            json!({"thread_id": "thread-1", "comment_id": "c-9", "expected_version": 4, "reason": "personal data"})
        );
        for refused in [
            Command::RedactComment {
                thread_id: "thread-1".into(),
                comment_id: "c-9".into(),
                expected_version: 0,
                reason: "why".into(),
            },
            Command::RedactComment {
                thread_id: "thread-1".into(),
                comment_id: "c-9".into(),
                expected_version: 4,
                reason: "   ".into(),
            },
            Command::RedactComment {
                thread_id: "thread-1".into(),
                comment_id: "c/9".into(),
                expected_version: 4,
                reason: "why".into(),
            },
            Command::RedactComment {
                thread_id: "thread-1".into(),
                comment_id: "c-9".into(),
                expected_version: 4,
                reason: "x".repeat(MAX_REASON + 1),
            },
        ] {
            assert!(refused.validate("p").is_err());
        }
    }

    #[test]
    fn a_request_outside_its_bounds_is_refused_before_it_is_sent() {
        let refused = |command: Command| command.validate("p").expect_err("refused").kind();
        assert_eq!(
            refused(Command::SetTags {
                object: anchor(),
                definition_id: "city".into(),
                values: Some(vec![]),
                typed_values: Some(json!([])),
                expected_version: None,
            }),
            ErrorKind::InvalidInput,
            "values and typed_values together"
        );
        assert!(
            Command::SetTags {
                object: anchor(),
                definition_id: "city".into(),
                values: Some(vec![]),
                typed_values: None,
                expected_version: None,
            }
            .validate("p")
            .is_ok(),
            "an empty list clears the values"
        );
        assert_eq!(
            refused(Command::QueryTags {
                filters: json!([]),
                match_all: true,
                limit: 10
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Command::QueryTags {
                filters: json!([{"definition_id": "x", "operator": "exists"}]),
                match_all: true,
                limit: 2_001
            }),
            ErrorKind::InvalidInput
        );
        let many: Vec<String> = (0..51).map(|i| format!("T-{i}")).collect();
        assert_eq!(
            refused(Command::PreviewLocationEnrichment {
                transformers: many,
                reference_revision: None
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Command::ApplyTagGroup {
                group: "city".into(),
                entries: vec![("T-1".into(), Some("gagal".into()))],
                plan_digest: "not-a-digest".into(),
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Command::PreviewConsumerGrouping {
                purpose: "report_archive".into(),
                transformers: vec!["T-1".into()],
                definition_ids: vec!["city".into()],
                bindings: json!([{"group_key": "g", "source_city_id": "c"}]),
            }),
            ErrorKind::InvalidInput,
            "an archive grouping binds no source"
        );
        assert_eq!(
            refused(Command::ReadConsumerGrouping {
                purpose: "other".into()
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Command::ListThreads {
                object: ObjectRef {
                    kind: "survey".into(),
                    id: "x".into(),
                    version_id: None
                },
                include_resolved: false
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Command::CreateThread {
                thread_id: "t".into(),
                object: anchor(),
                title: "".into(),
                body: "b".into()
            }),
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn the_answer_is_the_member_the_browser_read_and_a_null_list_is_empty() {
        let data = json!({"definitions": null});
        assert_eq!(
            Command::ListTagDefinitions {
                include_archived: false
            }
            .answer(&data),
            Some(json!([]))
        );
        let data = json!({"plan": {"plan_digest": "x"}});
        assert_eq!(
            Command::ReadConsumerGrouping {
                purpose: "solar_report".into()
            }
            .answer(&data),
            Some(json!({"plan_digest": "x"}))
        );
        let data = json!({"thread": {"thread_id": "t"}, "comments": []});
        assert_eq!(
            Command::GetThread {
                thread_id: "t".into()
            }
            .answer(&data),
            Some(data.clone())
        );
        assert_eq!(
            Command::ListTagGroups {
                transformers: vec![]
            }
            .answer(&json!({})),
            None
        );
    }
}
