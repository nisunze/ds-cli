//! Project Work links to one exact DS Grid model catalog revision.
//!
//! A task, milestone or project note names a whole project model at an exact
//! catalog version (ds-brain `docs/contracts/work-links-and-spatial-anchors.md`
//! §DS Grid model versions). A catalog version stays open until a bump
//! supersedes it, so the immutable pin is the catalog **revision**; the version
//! number and the `.dsgrid` digest ride beside it:
//!
//! ```json
//! {"kind":"ds_object","project_id":"p","object_type":"mv_model",
//!  "entity_id":"<model_id>","object_revision":"<revision_id>",
//!  "model_version":3,"model_digest":"<sha256>","label":"Feeder 7"}
//! ```
//!
//! The server checks a newly introduced pin against the catalog in the same
//! transaction as the Project Work write. This module is the pure half: the
//! pin built from the catalog's own revision record (never from a display
//! name or a guess), the whole-array link change that keeps every other link
//! and its attribution, and the read projections. No IO, no clock.
//!
//! It is not the design-version `vN` governance marker and freezes nothing.
use serde_json::{json, Map, Value};

/// The `ds_object` type of a whole DS Grid project model.
pub const OBJECT_TYPE: &str = "mv_model";
/// The bound ds-brain applies to one task's or note's `links[]`.
pub const MAX_LINKS: usize = 128;
/// Every authored field a Project Work link carries on the wire. A client that
/// re-sends a link set keeps exactly these; the command decoder refuses an
/// unknown field, and the server re-owns attribution and pin observations.
pub const WIRE_FIELDS: &[&str] = &[
    "kind",
    "project_id",
    "entity_id",
    "form_slug",
    "object_type",
    "object_revision",
    "model_version",
    "model_digest",
    "label",
    "attached_by",
    "attached_at",
];

/// One exact catalog revision of one project model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPin {
    pub project_id: String,
    pub model_id: String,
    pub revision_id: String,
    /// The catalog version the revision belongs to.
    pub version: u64,
    /// The sha256 of the revision's `.dsgrid` bytes.
    pub digest: String,
}

/// The catalog identifier rule (`^[a-z0-9][a-z0-9_-]{1,127}$`): opaque model
/// and revision ids, never display names.
pub fn valid_catalog_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    (2..=128).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

fn sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ModelPin {
    /// The pin the catalog's own revision record states: `get_version`'s
    /// `project_id`, `model_id`, `revision_id`, `version` and `model.digest`.
    /// A record that names another project, model or revision is refused.
    pub fn from_revision(
        project: &str,
        model: &str,
        revision: &str,
        record: &Value,
    ) -> Result<Self, &'static str> {
        if !valid_catalog_id(model) || !valid_catalog_id(revision) || project.trim().is_empty() {
            return Err("an exact project, catalog model id and revision id are required");
        }
        if record["project_id"] != project
            || record["model_id"] != model
            || record["revision_id"] != revision
        {
            return Err("the catalog answered a different project, model or revision");
        }
        let version = record["version"]
            .as_u64()
            .filter(|v| *v > 0)
            .ok_or("the catalog revision carries no version")?;
        let digest = record["model"]["digest"]
            .as_str()
            .filter(|d| sha256_hex(d))
            .ok_or("the catalog revision carries no model digest")?;
        Ok(Self {
            project_id: project.into(),
            model_id: model.into(),
            revision_id: revision.into(),
            version,
            digest: digest.into(),
        })
    }

    /// The wire link that names this pin.
    pub fn link(&self, label: Option<&str>) -> Value {
        let mut link = json!({
            "kind": "ds_object", "project_id": self.project_id, "object_type": OBJECT_TYPE,
            "entity_id": self.model_id, "object_revision": self.revision_id,
            "model_version": self.version, "model_digest": self.digest,
        });
        if let Some(label) = label.map(str::trim).filter(|l| !l.is_empty()) {
            link["label"] = json!(label);
        }
        link
    }
}

/// Where `version` stands on one page of `ds dsgrid project versions` — the
/// catalog grouped newest version first, each group's revisions oldest first.
/// Pages run newest first, so the first page that shows the version shows its
/// newest revision: the version as it stands now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionLookup {
    /// The newest revision of the version.
    Found(String),
    /// The page lists an older version, so the requested one does not exist.
    Absent,
    /// Not on this page; follow the cursor (or, with no more pages, absent).
    NotYet,
}

pub fn latest_revision_of_version(groups: &[Value], version: u64) -> VersionLookup {
    for group in groups {
        match group["version"].as_u64() {
            Some(v) if v == version => {
                return group["revisions"]
                    .as_array()
                    .and_then(|r| r.last())
                    .and_then(|r| r["revision_id"].as_str())
                    .map_or(VersionLookup::Absent, |r| VersionLookup::Found(r.into()))
            }
            Some(v) if v < version => return VersionLookup::Absent,
            _ => {}
        }
    }
    VersionLookup::NotYet
}

/// A link naming a DS Grid model (pinned or model-wide).
pub fn is_model_link(link: &Value) -> bool {
    link["kind"] == "ds_object" && link["object_type"] == OBJECT_TYPE
}

/// Whether `link` names `model` in `project`, at exactly `revision` when one is
/// given. A display label is never part of the identity.
pub fn matches(link: &Value, project: &str, model: &str, revision: Option<&str>) -> bool {
    is_model_link(link)
        && link["project_id"] == project
        && link["entity_id"] == model
        && revision.is_none_or(|r| link["object_revision"] == r)
}

/// A link's authored wire fields and nothing else.
pub fn wire_link(link: &Value) -> Value {
    let mut out = Map::new();
    for key in WIRE_FIELDS {
        if let Some(value) = link.get(*key).filter(|value| !value.is_null()) {
            out.insert((*key).into(), value.clone());
        }
    }
    Value::Object(out)
}

/// The whole link array to send, and whether anything changed.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkChange {
    pub links: Vec<Value>,
    pub changed: bool,
}

fn label_valid(label: Option<&str>) -> bool {
    label.is_none_or(|l| l.chars().count() <= 200 && !l.chars().any(char::is_control))
}

/// Add one pin. Every other link keeps its place and fields; an existing link
/// to the same exact revision is a no-op (its first attribution stays).
pub fn add(
    existing: &[Value],
    pin: &ModelPin,
    label: Option<&str>,
) -> Result<LinkChange, &'static str> {
    if existing.len() > MAX_LINKS {
        return Err("the item already exceeds the 128-link bound");
    }
    if !label_valid(label) {
        return Err("a link label is at most 200 characters without control characters");
    }
    let mut links: Vec<Value> = existing.iter().map(wire_link).collect();
    if existing
        .iter()
        .any(|l| matches(l, &pin.project_id, &pin.model_id, Some(&pin.revision_id)))
    {
        return Ok(LinkChange {
            links,
            changed: false,
        });
    }
    if links.len() == MAX_LINKS {
        return Err("the item already holds 128 links");
    }
    links.push(pin.link(label));
    Ok(LinkChange {
        links,
        changed: true,
    })
}

/// Remove the link to one exact revision of one model. Other revisions of the
/// same model, and every other link, stay.
pub fn remove(existing: &[Value], project: &str, model: &str, revision: &str) -> LinkChange {
    let links: Vec<Value> = existing
        .iter()
        .filter(|l| !matches(l, project, model, Some(revision)))
        .map(wire_link)
        .collect();
    let changed = links.len() != existing.len();
    LinkChange { links, changed }
}

/// The model links on one item, as rows a reader branches on. A model-wide
/// link (no revision) is reported with `pinned: false`.
pub fn project_links(links: &[Value], project: &str) -> Vec<Value> {
    links
        .iter()
        .filter(|l| is_model_link(l) && l["project_id"] == project)
        .map(|l| {
            let revision = l["object_revision"].as_str().filter(|r| !r.is_empty());
            json!({
                "model_id": l["entity_id"], "revision_id": revision,
                "model_version": l["model_version"], "model_digest": l["model_digest"],
                "pinned": revision.is_some(), "label": l["label"],
                "attached_by": l["attached_by"], "attached_at": l["attached_at"],
            })
        })
        .collect()
}

/// Every live task or milestone in a raw graph that links `model` (at
/// `revision` when given).
pub fn task_references(
    graph: &Value,
    project: &str,
    model: &str,
    revision: Option<&str>,
) -> Vec<Value> {
    graph["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|task| task["is_deleted"] != true)
        .flat_map(|task| {
            task["links"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(move |l| matches(l, project, model, revision))
                .map(move |l| {
                    json!({
                        "source_kind": task["data"]["type"].as_str().unwrap_or("task"),
                        "source_id": task["id"], "title": task["data"]["title"],
                        "revision_id": l["object_revision"], "model_version": l["model_version"],
                        "attached_by": l["attached_by"], "attached_at": l["attached_at"],
                    })
                })
        })
        .collect()
}

/// Every live project note among `records` that links `model` (at `revision`
/// when given). Other record categories never carry model links.
pub fn note_references(
    records: &[Value],
    project: &str,
    model: &str,
    revision: Option<&str>,
) -> Vec<Value> {
    records
        .iter()
        .filter(|r| r["is_deleted"] != true && note_data(r)["category"] == "project_note")
        .flat_map(|record| {
            let data = note_data(record);
            note_links(record)
                .as_array()
                .into_iter()
                .flatten()
                .filter(move |l| matches(l, project, model, revision))
                .map(move |l| {
                    json!({
                        "source_kind": "project_note", "source_id": record["id"],
                        "title": data["subject"],
                        "revision_id": l["object_revision"], "model_version": l["model_version"],
                        "attached_by": l["attached_by"], "attached_at": l["attached_at"],
                    })
                })
        })
        .collect()
}

/// Closed selection for the existing project-wide records page. No label matching.
pub fn validate_reference_selection(
    model: &str,
    revision: Option<&str>,
    cursor: Option<&str>,
) -> Result<(), String> {
    if !valid_catalog_id(model) || revision.is_some_and(|r| !valid_catalog_id(r)) {
        return Err("model references need exact catalog model and revision IDs".into());
    }
    super::overview::Options {
        limit: super::overview::MAX_ROWS,
        records_cursor: cursor.map(str::to_owned),
    }
    .validate()
}

/// Match only the admitted page. Graph and record versions are independent facts.
pub fn references(
    project: &str,
    model: &str,
    revision: Option<&str>,
    cursor: Option<&str>,
    graph: &Value,
    context: &Value,
) -> Result<Value, String> {
    validate_reference_selection(model, revision, cursor)?;
    let invalid = "model references require one exact project-wide pm_records page";
    if context["loaded_collections"] != json!(["pm_records"])
        || context["truncated_by_collection"]
            .as_object()
            .is_none_or(|m| m.len() != 1)
        || context["next_cursors"]
            .as_object()
            .is_none_or(|m| m.keys().any(|k| k != "pm_records"))
    {
        return Err(invalid.into());
    }
    let next = context["next_cursors"].get("pm_records");
    if next.is_some_and(|v| {
        v.as_str().is_none_or(|s| {
            super::overview::Options {
                limit: super::overview::MAX_ROWS,
                records_cursor: Some(s.into()),
            }
            .validate()
            .is_err()
        })
    }) || (context["truncated_by_collection"]["pm_records"] == false && next.is_some())
    {
        return Err(invalid.into());
    }
    let observed = super::overview::fold(super::overview::Input {
        schema: super::SCHEMA.into(),
        action: "overview".into(),
        ds_project: project.into(),
        graph: graph.clone(),
        context: context.clone(),
        graph_format: "wire".into(),
        options: super::overview::Options {
            limit: super::overview::MAX_ROWS,
            records_cursor: cursor.map(str::to_owned),
        },
    })?;
    let records = context["records"].as_array().ok_or(invalid)?;
    if records.iter().any(|r| {
        r["is_deleted"] != true && r["metadata"]["updated_at"].as_i64().is_none_or(|t| t < 0)
    }) {
        return Err("model reference records have no independently observed update time".into());
    }
    let mut notes = note_references(records, project, model, revision);
    for note in &mut notes {
        let record = records
            .iter()
            .find(|r| r["id"] == note["source_id"])
            .ok_or(invalid)?;
        note["note_version"] = record["version"].clone();
        note["note_updated_at"] = record["metadata"]["updated_at"].clone();
    }
    let partial = observed.project_notes.count_kind == "lower_bound";
    Ok(
        json!({"project":project,"model_id":model,"revision_id":revision,
        "tasks":task_references(graph,project,model,revision),"notes_count":notes.len(),"notes":notes,
        "notes_count_kind":if partial {"lower_bound"} else {"exact"}, "notes_truncated":partial,
        "records_cursor":cursor,"notes_next_cursor":next,"context_returned_records":records.len(),
        "truncated_by_collection":observed.truncated_by_collection,"next_cursors":observed.next_cursors,
        "revision":observed.graph_revision,"schedule_revision":observed.schedule_revision,
        "context_graph_revision":observed.context_graph_revision,"read_consistency":observed.read_consistency}),
    )
}

/// A project note's `links[]`. ds-brain's record envelope keeps `links` (with
/// `geometry` and `version`) beside `data`, not inside it.
pub fn note_links(record: &Value) -> &Value {
    if record["links"].is_array() {
        &record["links"]
    } else {
        &note_data(record)["links"]
    }
}

/// A record's authored fields: the wire nests them under `data`; a projected
/// record row may already be flat.
fn note_data(record: &Value) -> &Value {
    if record["data"].is_object() {
        &record["data"]
    } else {
        record
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D1: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn record(revision: &str, version: u64) -> Value {
        json!({"project_id":"p","model_id":"model-a","revision_id":revision,
            "version":version,"model":{"digest":D1}})
    }

    fn pin(revision: &str, version: u64) -> ModelPin {
        ModelPin::from_revision("p", "model-a", revision, &record(revision, version)).unwrap()
    }

    #[test]
    fn the_pin_is_what_the_catalog_record_states() {
        let p = pin("rev-2", 3);
        assert_eq!((p.version, p.digest.as_str()), (3, D1));
        assert_eq!(
            p.link(Some(" Feeder 7 ")),
            json!({"kind":"ds_object","project_id":"p","object_type":"mv_model",
                "entity_id":"model-a","object_revision":"rev-2","model_version":3,
                "model_digest":D1,"label":"Feeder 7"})
        );
        for (model, revision, rec) in [
            ("model-a", "rev-2", record("rev-3", 3)),
            ("Feeder 7", "rev-2", record("rev-2", 3)),
            (
                "model-a",
                "rev-2",
                json!({"project_id":"p","model_id":"model-a","revision_id":"rev-2","version":0,"model":{"digest":D1}}),
            ),
            (
                "model-a",
                "rev-2",
                json!({"project_id":"p","model_id":"model-a","revision_id":"rev-2","version":1,"model":{"digest":"ABC"}}),
            ),
            (
                "model-a",
                "rev-2",
                json!({"project_id":"q","model_id":"model-a","revision_id":"rev-2","version":1,"model":{"digest":D1}}),
            ),
        ] {
            assert!(
                ModelPin::from_revision("p", model, revision, &rec).is_err(),
                "{rec}"
            );
        }
    }

    #[test]
    fn a_version_resolves_to_its_newest_revision_on_grouped_pages() {
        let page = vec![
            json!({"version":3,"revisions":[{"revision_id":"rev-4"}]}),
            json!({"version":2,"revisions":[{"revision_id":"rev-2"},{"revision_id":"rev-3"}]}),
        ];
        assert_eq!(
            latest_revision_of_version(&page, 2),
            VersionLookup::Found("rev-3".into())
        );
        assert_eq!(latest_revision_of_version(&page, 1), VersionLookup::NotYet);
        assert_eq!(latest_revision_of_version(&page, 9), VersionLookup::Absent);
        let older = vec![json!({"version":1,"revisions":[{"revision_id":"rev-1"}]})];
        assert_eq!(latest_revision_of_version(&older, 2), VersionLookup::Absent);
    }

    #[test]
    fn add_and_remove_touch_only_the_exact_revision() {
        let geometry = json!({"kind":"ds_object","project_id":"p","object_type":"dsgrid_structure",
            "entity_id":"model-a:s-74","object_revision":"dsgrid:r3:abc","projected":"dropped"});
        let first = json!({"kind":"ds_object","project_id":"p","object_type":"mv_model",
            "entity_id":"model-a","object_revision":"rev-1","model_version":1,"model_digest":D1,
            "attached_by":"first@example.com","attached_at":"2026-10-01T00:00:00Z"});
        let added = add(&[geometry.clone(), first.clone()], &pin("rev-2", 2), None).unwrap();
        assert!(added.changed);
        assert_eq!(added.links.len(), 3);
        assert!(
            added.links[0].get("projected").is_none(),
            "projection fields never go back"
        );
        assert_eq!(added.links[1], first);
        let again = add(&added.links, &pin("rev-2", 2), Some("other")).unwrap();
        assert!(!again.changed);
        let removed = remove(&added.links, "p", "model-a", "rev-1");
        assert!(removed.changed);
        assert_eq!(removed.links.len(), 2);
        assert_eq!(removed.links[1]["object_revision"], "rev-2");
        assert!(!remove(&removed.links, "p", "model-a", "rev-1").changed);
        assert!(add(&[], &pin("rev-2", 2), Some(&"x".repeat(201))).is_err());
    }

    #[test]
    fn projections_read_tasks_and_notes_by_exact_identity() {
        let link = pin("rev-2", 2).link(Some("Shared name"));
        let other = ModelPin {
            model_id: "model-b".into(),
            ..pin("rev-2", 2)
        }
        .link(Some("Shared name"));
        let graph = json!({"tasks":[
            {"id":"T1","data":{"title":"Build","type":"task"},"links":[link.clone()]},
            {"id":"M1","data":{"title":"Issue","type":"milestone"},"links":[link.clone()]},
            {"id":"T2","data":{"title":"Other","type":"task"},"links":[other]},
            {"id":"T3","is_deleted":true,"data":{"title":"Gone"},"links":[link.clone()]},
        ]});
        let tasks = task_references(&graph, "p", "model-a", Some("rev-2"));
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[1]["source_kind"], "milestone");
        assert!(task_references(&graph, "p", "model-a", Some("rev-1")).is_empty());
        let records = vec![
            json!({"id":"N1","data":{"category":"project_note","subject":"Review"},"links":[link.clone()]}),
            json!({"id":"R1","data":{"category":"instruction","subject":"x"},"links":[link.clone()]}),
        ];
        let notes = note_references(&records, "p", "model-a", None);
        assert_eq!(
            (notes.len(), notes[0]["source_id"].as_str()),
            (1, Some("N1"))
        );
        let rows = project_links(&[link], "p");
        assert_eq!(rows[0]["revision_id"], "rev-2");
        assert_eq!(rows[0]["pinned"], true);
    }

    #[test]
    fn catalog_ids_follow_the_server_rule() {
        for ok in ["ab", "model-7", "a_b-c", "0x"] {
            assert!(valid_catalog_id(ok), "{ok}");
        }
        for bad in ["a", "-ab", "Model", "a b", "a/b", "v/3", &"a".repeat(129)] {
            assert!(!valid_catalog_id(bad), "{bad}");
        }
    }
    #[test]
    fn bounded_references_keep_canonical_exact_links_and_independent_record_versions() {
        let link = pin("rev-2", 3).link(Some("display"));
        let graph = json!({"project_id":"p","revision":9,"schedule_revision":4,"tasks":[],"dependencies":[],"residual_items":[]});
        let mut context = json!({"project_id":"p","records":[{"id":"note","version":7,"metadata":{"updated_at":123},"data":{"category":"project_note","subject":"Canonical note","memory":{"schema":"ds.project-note-memory/v1"}},"links":[link.clone()]}],"loaded_collections":["pm_records"],"truncated_by_collection":{"pm_records":false},"next_cursors":{}});
        let exact = references("p", "model-a", Some("rev-2"), None, &graph, &context).unwrap();
        assert_eq!(exact["notes_count"], 1);
        assert_eq!(exact["notes_count_kind"], "exact");
        assert_eq!(exact["notes"][0]["note_version"], 7);
        for (field, bad) in [
            ("project_id", "foreign"),
            ("entity_id", "other-model"),
            ("object_revision", "rev-3"),
            ("object_type", "other"),
        ] {
            context["records"][0]["links"][0] = link.clone();
            context["records"][0]["links"][0][field] = json!(bad);
            assert_eq!(
                references("p", "model-a", Some("rev-2"), None, &graph, &context).unwrap()
                    ["notes_count"],
                0,
                "{field}"
            );
        }
        context["records"][0]["links"][0] = link;
        context["records"][0]["links"][0]
            .as_object_mut()
            .unwrap()
            .remove("object_revision");
        assert_eq!(
            references("p", "model-a", None, None, &graph, &context).unwrap()["notes_count"],
            1
        );
        assert_eq!(
            references("p", "model-a", Some("rev-2"), None, &graph, &context).unwrap()
                ["notes_count"],
            0
        );
        context["records"][0]["is_deleted"] = json!(true);
        assert_eq!(
            references("p", "model-a", None, None, &graph, &context).unwrap()["notes_count"],
            0
        );
        context["records"][0]["is_deleted"] = json!(false);
        context["truncated_by_collection"]["pm_records"] = json!(true);
        context["next_cursors"] = json!({"pm_records":"boundary"});
        assert_eq!(
            references("p", "model-a", Some("rev-2"), None, &graph, &context).unwrap()
                ["notes_count_kind"],
            "lower_bound"
        );
        assert!(references("p", "Display label", None, None, &graph, &context).is_err());
    }
}
