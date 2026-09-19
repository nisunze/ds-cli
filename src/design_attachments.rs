//! `design_attachments` — what an operator may do to a versioned file, and
//! what each of those things is called.
//!
//! A `.dsgrid` model's attachments are immutable revisions bound to a model
//! version. The browser used to decide, in a chain of `{#if}`s, which verbs a
//! file and each of its revisions offered, and it named them itself. It named
//! them WRONG, and it named them TWICE:
//!
//!   * The file row said "Archive" and the revision row said "Retire" for the
//!     same call. There is one declared verb, `ds design attachment retire`,
//!     whose own example reads "Omit --revision to retire the whole file". The
//!     scope is the argument; the verb never changes. "Archive" was a word the
//!     browser invented, and an operator reading the two rows could not know
//!     they were the same act.
//!   * The file row said "Download latest" and every revision row said
//!     "Download". On a file with ONE revision those two buttons fetch the
//!     same bytes, so the dialog offered the same download twice and made an
//!     operator choose between them.
//!
//! So this module answers, once: which actions exist, on which row, under
//! which authority, and by which label key. Every caller — the browser, the
//! CLI's `ds design attachment list`, an agent through MCP — renders the same
//! answer. No surface may add a verb, drop one, or rename one.
//!
//! The `dsa_` label keys this module can emit, exactly:
//! `dsa_action_download`, `dsa_action_download_latest`, `dsa_action_retire`,
//! `dsa_action_restore`, `dsa_action_make_latest`, `dsa_state_retired`,
//! `dsa_state_latest`, `dsa_reason_no_revision`, `dsa_reason_read_only`,
//! `dsa_reason_not_permitted`.
//!
//! Pure: no clock, no I/O, no storage paths. A download here is the DECISION
//! that a download is offered, never a URL — URLs stay server-minted and
//! generation-pinned, which is the one thing the dialog already got right.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA: &str = "ds.design-attachments/v1";

/// What a caller may do to this project's attachments at all.
#[derive(Deserialize, Debug, Clone, Copy, Default)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    /// `design.attachment.manage` — retire, restore and move `latest`.
    #[serde(default)]
    pub can_manage: bool,
    /// The project is open read-only, which withdraws every write regardless
    /// of capability.
    #[serde(default)]
    pub read_only: bool,
}

/// Which row an action sits on. The verb does not change with the scope; only
/// the argument does, exactly as the CLI declares it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    File,
    Revision,
}

/// One thing an operator may do, named once.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// What the caller dispatches on. Stable; never shown to a person.
    pub kind: String,
    /// What a person reads, in their own language.
    pub label_key: String,
    pub scope: Scope,
    /// Present on a revision-scoped action; absent on a file-scoped one, where
    /// the command omits `--revision` and means the whole file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<String>,
    pub enabled: bool,
    /// Why it is offered but not available. Absent when `enabled`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_key: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct AttachmentIn {
    pub attachment_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub latest_revision_id: Option<String>,
    #[serde(default)]
    pub revision_count: usize,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct RevisionIn {
    pub revision_id: String,
    #[serde(default)]
    pub revision: i64,
    #[serde(default)]
    pub deleted_at: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct ListingIn {
    pub attachment: AttachmentIn,
    #[serde(default)]
    pub revisions: Vec<RevisionIn>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    #[serde(default)]
    authority: Authority,
    #[serde(default)]
    listings: Vec<ListingIn>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct RevisionOut {
    pub revision_id: String,
    pub revision: i64,
    pub retired: bool,
    pub latest: bool,
    /// `dsa_state_latest` or `dsa_state_retired`; a revision can be neither.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub state_keys: Vec<String>,
    pub actions: Vec<Action>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct FileOut {
    pub attachment_id: String,
    pub label: String,
    pub retired: bool,
    pub revision_count: usize,
    pub actions: Vec<Action>,
    pub revisions: Vec<RevisionOut>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub schema: &'static str,
    pub files: Vec<FileOut>,
}

fn action(
    kind: &str,
    label_key: &str,
    scope: Scope,
    revision_id: Option<&str>,
    enabled: bool,
    reason_key: Option<&str>,
) -> Action {
    Action {
        kind: kind.to_string(),
        label_key: label_key.to_string(),
        scope,
        revision_id: revision_id.map(str::to_string),
        enabled,
        reason_key: reason_key.map(str::to_string),
    }
}

/// Why a write is not available, or `None` when it is.
fn write_block(authority: Authority) -> Option<&'static str> {
    if !authority.can_manage {
        Some("dsa_reason_not_permitted")
    } else if authority.read_only {
        Some("dsa_reason_read_only")
    } else {
        None
    }
}

fn project_listing(listing: &ListingIn, authority: Authority) -> FileOut {
    let file = &listing.attachment;
    let retired = file.state == "archived";
    let block = write_block(authority);

    // A file-scoped download is offered only when it says something a revision
    // row does not already say. With one revision it is the SAME bytes, and a
    // dialog that offers one download twice makes an operator wonder which is
    // which. With several, "the latest" is a real question the rows below
    // cannot answer at a glance.
    let mut actions = Vec::new();
    let has_latest = file
        .latest_revision_id
        .as_deref()
        .is_some_and(|id| !id.is_empty());
    if listing.revisions.len() > 1 || (has_latest && listing.revisions.is_empty()) {
        actions.push(action(
            "download",
            "dsa_action_download_latest",
            Scope::File,
            None,
            has_latest,
            (!has_latest).then_some("dsa_reason_no_revision"),
        ));
    }

    // ONE verb for retirement, at both scopes, because there is one command.
    // The file row omits `--revision` and therefore means the whole file; the
    // revision row names one. Calling the first "Archive" said they were two
    // different acts.
    if block.is_none() || !retired {
        let key = if retired {
            "dsa_action_restore"
        } else {
            "dsa_action_retire"
        };
        actions.push(action(
            if retired { "restore" } else { "retire" },
            key,
            Scope::File,
            None,
            block.is_none(),
            block,
        ));
    }

    let revisions = listing
        .revisions
        .iter()
        .map(|revision| {
            let revision_retired = revision
                .deleted_at
                .as_deref()
                .is_some_and(|at| !at.is_empty());
            let latest = file
                .latest_revision_id
                .as_deref()
                .is_some_and(|id| id == revision.revision_id);
            let mut state_keys = Vec::new();
            if latest {
                state_keys.push("dsa_state_latest".to_string());
            }
            if revision_retired {
                state_keys.push("dsa_state_retired".to_string());
            }

            let mut row = vec![action(
                "download",
                "dsa_action_download",
                Scope::Revision,
                Some(&revision.revision_id),
                true,
                None,
            )];
            // Moving `latest` is only a question for a live revision that is
            // not already it.
            if !revision_retired && !latest {
                row.push(action(
                    "make_latest",
                    "dsa_action_make_latest",
                    Scope::Revision,
                    Some(&revision.revision_id),
                    block.is_none(),
                    block,
                ));
            }
            row.push(action(
                if revision_retired {
                    "restore"
                } else {
                    "retire"
                },
                if revision_retired {
                    "dsa_action_restore"
                } else {
                    "dsa_action_retire"
                },
                Scope::Revision,
                Some(&revision.revision_id),
                block.is_none(),
                block,
            ));

            RevisionOut {
                revision_id: revision.revision_id.clone(),
                revision: revision.revision,
                retired: revision_retired,
                latest,
                state_keys,
                actions: row,
            }
        })
        .collect();

    FileOut {
        attachment_id: file.attachment_id.clone(),
        label: file.label.clone(),
        retired,
        revision_count: file.revision_count,
        actions,
        revisions,
        truncated: listing.truncated,
    }
}

pub fn project(request: Value) -> Result<Value, String> {
    let request: Request = serde_json::from_value(request)
        .map_err(|error| format!("Design attachments request is malformed: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!(
            "Unsupported design attachments schema `{}`.",
            request.schema
        ));
    }
    if request.action != "project" {
        return Err(format!(
            "Design attachments does not know the action `{}`.",
            request.action
        ));
    }
    let files = request
        .listings
        .iter()
        .map(|listing| project_listing(listing, request.authority))
        .collect();
    serde_json::to_value(Reply {
        schema: SCHEMA,
        files,
    })
    .map_err(|error| format!("Design attachments reply could not be written: {error}"))
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    let request: Value = serde_json::from_slice(input)
        .map_err(|error| format!("Design attachments input is not JSON: {error}"))?;
    let reply = project(request)?;
    serde_json::to_string(&reply)
        .map_err(|error| format!("Design attachments reply could not be written: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(listings: Value, can_manage: bool, read_only: bool) -> Value {
        project(json!({
            "schema": SCHEMA,
            "action": "project",
            "authority": {"can_manage": can_manage, "read_only": read_only},
            "listings": listings,
        }))
        .expect("projection")
    }

    fn one_revision() -> Value {
        json!([{
            "attachment": {
                "attachment_id": "att-bak",
                "label": "Nyamagabe&Nyaruguru.bak (original PLS-CADD 16.81 backup)",
                "state": "ready",
                "latest_revision_id": "rev-1",
                "revision_count": 1,
            },
            "revisions": [{"revision_id": "rev-1", "revision": 1}],
        }])
    }

    fn keys(actions: &Value) -> Vec<String> {
        actions
            .as_array()
            .expect("actions")
            .iter()
            .map(|a| a["label_key"].as_str().expect("label_key").to_string())
            .collect()
    }

    /// The screenshot's exact case: one file, one revision, and the dialog
    /// offered "Download latest" beside "Download" for the same bytes.
    #[test]
    fn a_file_with_one_revision_offers_its_download_once() {
        let reply = run(one_revision(), true, false);
        let file = &reply["files"][0];
        assert_eq!(
            keys(&file["actions"]),
            ["dsa_action_retire"],
            "a lone revision's bytes are already downloadable from its own row",
        );
        assert_eq!(
            keys(&file["revisions"][0]["actions"]),
            ["dsa_action_download", "dsa_action_retire"],
        );
    }

    /// With several revisions "the latest one" is a question the rows below
    /// cannot answer at a glance, so the file-scoped download earns its place.
    #[test]
    fn a_file_with_several_revisions_keeps_the_latest_download() {
        let reply = run(
            json!([{
                "attachment": {
                    "attachment_id": "att-bak", "label": "m.bak", "state": "ready",
                    "latest_revision_id": "rev-2", "revision_count": 2,
                },
                "revisions": [
                    {"revision_id": "rev-2", "revision": 2},
                    {"revision_id": "rev-1", "revision": 1},
                ],
            }]),
            true,
            false,
        );
        let file = &reply["files"][0];
        assert_eq!(
            keys(&file["actions"]),
            ["dsa_action_download_latest", "dsa_action_retire"],
        );
        // The newest carries the badge and offers no "make latest"; the older
        // one does.
        assert_eq!(file["revisions"][0]["latest"], true);
        assert_eq!(
            keys(&file["revisions"][0]["actions"]),
            ["dsa_action_download", "dsa_action_retire"],
        );
        assert_eq!(
            keys(&file["revisions"][1]["actions"]),
            [
                "dsa_action_download",
                "dsa_action_make_latest",
                "dsa_action_retire"
            ],
        );
    }

    /// The defect this module exists for: one command, one word, both scopes.
    #[test]
    fn retirement_is_the_same_word_on_the_file_and_on_the_revision() {
        let reply = run(one_revision(), true, false);
        let file = &reply["files"][0];
        let file_verb = &file["actions"][0];
        let revision_verb = file["revisions"][0]["actions"]
            .as_array()
            .expect("actions")
            .iter()
            .find(|a| a["kind"] == "retire")
            .expect("a revision retire");
        assert_eq!(file_verb["label_key"], revision_verb["label_key"]);
        assert_eq!(file_verb["kind"], revision_verb["kind"]);
        // Scope is carried by the row and by the argument, never by the word:
        // the file-scoped one names no revision, exactly as the CLI omits
        // `--revision` to mean the whole file.
        assert_eq!(file_verb["scope"], "file");
        assert!(file_verb.get("revision_id").is_none());
        assert_eq!(revision_verb["scope"], "revision");
        assert_eq!(revision_verb["revision_id"], "rev-1");
    }

    #[test]
    fn a_retired_file_and_a_retired_revision_both_offer_restore() {
        let reply = run(
            json!([{
                "attachment": {
                    "attachment_id": "att", "label": "m.bak", "state": "archived",
                    "latest_revision_id": null, "revision_count": 1,
                },
                "revisions": [{"revision_id": "rev-1", "revision": 1, "deleted_at": "2026-09-19T00:00:00Z"}],
            }]),
            true,
            false,
        );
        let file = &reply["files"][0];
        assert_eq!(file["retired"], true);
        assert_eq!(keys(&file["actions"]), ["dsa_action_restore"]);
        let revision = &file["revisions"][0];
        assert_eq!(revision["retired"], true);
        assert_eq!(revision["state_keys"], json!(["dsa_state_retired"]));
        assert_eq!(
            keys(&revision["actions"]),
            ["dsa_action_download", "dsa_action_restore"],
            "a retired revision keeps its bytes readable and offers no `make latest`",
        );
    }

    /// A reader still reads. Every write is offered-but-disabled with the
    /// reason, rather than vanishing: a button that is simply absent reads as
    /// a missing feature, not as a withheld permission.
    #[test]
    fn a_read_only_project_disables_the_writes_and_says_why() {
        for (can_manage, read_only, reason) in [
            (true, true, "dsa_reason_read_only"),
            (false, false, "dsa_reason_not_permitted"),
        ] {
            let reply = run(one_revision(), can_manage, read_only);
            let file = &reply["files"][0];
            assert_eq!(file["actions"][0]["enabled"], false);
            assert_eq!(file["actions"][0]["reason_key"], reason);
            let revision = &file["revisions"][0];
            assert_eq!(
                revision["actions"][0]["enabled"], true,
                "reading the bytes is never a write",
            );
            let retire = revision["actions"]
                .as_array()
                .expect("actions")
                .iter()
                .find(|a| a["kind"] == "retire")
                .expect("retire");
            assert_eq!(retire["enabled"], false);
            assert_eq!(retire["reason_key"], reason);
        }
    }

    /// A file whose revisions were not all returned must still offer the
    /// latest download: the rows on screen cannot stand in for it.
    #[test]
    fn a_truncated_listing_keeps_the_file_scoped_download() {
        let reply = run(
            json!([{
                "attachment": {
                    "attachment_id": "att", "label": "m.bak", "state": "ready",
                    "latest_revision_id": "rev-9", "revision_count": 9,
                },
                "revisions": [],
                "truncated": true,
            }]),
            true,
            false,
        );
        let file = &reply["files"][0];
        assert_eq!(file["truncated"], true);
        assert_eq!(
            keys(&file["actions"]),
            ["dsa_action_download_latest", "dsa_action_retire"],
        );
    }

    #[test]
    fn the_schema_and_the_action_are_each_refused_by_name() {
        let err = project(json!({"schema": "ds.other/v1", "action": "project"})).unwrap_err();
        assert!(
            err.starts_with("Unsupported design attachments schema"),
            "{err}"
        );
        let err = project(json!({"schema": SCHEMA, "action": "sweep"})).unwrap_err();
        assert!(err.contains("does not know the action"), "{err}");
    }
}
