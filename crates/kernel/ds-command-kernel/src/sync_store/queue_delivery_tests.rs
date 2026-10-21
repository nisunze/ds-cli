//! A queued row the shared record already carries byte for byte settles as
//! delivered, and the queue's delivery counts are decided here, once.
use super::*;
use serde_json::json;

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn fence() -> Fence {
    Fence {
        account: "uid".into(),
        deployment: "canary".into(),
        install_id: "install-1".into(),
    }
}

fn row(room: &str, state: ArtifactState) -> ArtifactRow {
    serde_json::from_value(json!({
        "scope": {"kind": "project", "project": "arjgpydw_nyam"},
        "engine": "network_reporter", "operation": format!("export-{room}"), "variant": "default",
        "sha256": SHA, "size_bytes": 8_000_000, "produced_at_ms": 1_000,
        "engine_release": "ds-network-reporter@0.1.0+d8639bb50215d3a6ee6f7a9d478f24f6bcfcaf83",
        "client_publish_id": format!("publish-{room}"), "bytes_locator": format!("network_reporter:batch:{room}"),
        "state": state, "replay_key": format!("publish-{room}"), "updated_at_ms": 1_000,
    }))
    .unwrap()
}

fn head(room: &str, sha256: &str) -> HeadRow {
    serde_json::from_value(json!({
        "scope": {"kind": "project", "project": "arjgpydw_nyam"},
        "engine": "network_reporter", "operation": format!("export-{room}"), "variant": "default",
        "revision": "head-1", "head_revision": 1, "sha256": sha256,
        "published_at_ms": 1_500, "read_at_ms": 9_000,
    }))
    .unwrap()
}

fn in_sync(room: &str) -> ReceiptRow {
    serde_json::from_value(json!({
        "receipt_id": format!("in_sync:{room}"),
        "scope": {"kind": "project", "project": "arjgpydw_nyam"},
        "action": IN_SYNC,
        "identity": {"engine": "network_reporter", "operation": format!("export-{room}"), "variant": "default"},
        "outcome": "published",
        "client_publish_id": format!("publish-{room}"),
        "at_ms": 10_000,
    }))
    .unwrap()
}

fn settle(artifacts: Vec<ArtifactRow>, heads: Vec<HeadRow>, receipt: ReceiptRow) -> Transition {
    apply(&Request {
        schema: SCHEMA.into(),
        fence: fence(),
        now_ms: 10_000,
        snapshot: Snapshot {
            artifacts,
            heads,
            ..Snapshot::default()
        },
        event: Event::ActionReceipt { receipt },
    })
    .unwrap()
}

fn restated(transition: &Transition) -> Vec<&ArtifactRow> {
    transition
        .changes
        .iter()
        .filter_map(|change| match change {
            RowChange::UpsertArtifact(row) => Some(row),
            _ => None,
        })
        .collect()
}

#[test]
fn a_held_row_the_head_already_carries_settles_as_published() {
    // arjgpydw_nyam, 10-15: 94 exports published on 09-28 were re-adopted as
    // held; each pass planned `in_sync` and the rows stayed queued forever.
    let transition = settle(
        vec![row("mugano", ArtifactState::Held)],
        vec![head("mugano", SHA)],
        in_sync("mugano"),
    );
    assert!(transition.refusals.is_empty(), "{:?}", transition.refusals);
    let rows = restated(&transition);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, ArtifactState::Published);
    assert!(rows[0].state_reason.is_none());
    assert!(rows[0].acknowledged_work_id.is_none());
    assert!(transition
        .changes
        .iter()
        .any(|change| matches!(change, RowChange::AppendReceipt(receipt) if receipt.action == IN_SYNC)));
}

#[test]
fn a_row_settles_only_on_the_exact_digest_of_a_held_head() {
    // Another digest at the head is not this publication.
    let other = settle(
        vec![row("mugano", ArtifactState::Held)],
        vec![head("mugano", OTHER)],
        in_sync("mugano"),
    );
    assert_eq!(other.refusals[0].code, "publication_not_in_sync");
    assert!(restated(&other).is_empty());

    // No head at all: nothing was delivered.
    let unread = settle(vec![row("mugano", ArtifactState::Held)], vec![], in_sync("mugano"));
    assert_eq!(unread.refusals[0].code, "publication_not_in_sync");
    assert!(restated(&unread).is_empty());

    // A settled verdict is never rewritten into a delivery.
    let refused = settle(
        vec![row("mugano", ArtifactState::Refused)],
        vec![head("mugano", SHA)],
        in_sync("mugano"),
    );
    assert_eq!(refused.refusals[0].code, "publication_not_in_sync");
    assert!(restated(&refused).is_empty());

    // A receipt for an older publication does not move the current one.
    let mut stale = in_sync("mugano");
    stale.client_publish_id = Some("publish-older".into());
    let replaced = settle(
        vec![row("mugano", ArtifactState::Held)],
        vec![head("mugano", SHA)],
        stale,
    );
    assert_eq!(replaced.refusals[0].code, "publication_replaced");
    assert!(restated(&replaced).is_empty());
}

#[test]
fn the_queue_counts_every_delivery_state_once_per_project_and_in_total() {
    let mut other_project = row("agahehe", ArtifactState::Published);
    other_project.scope = Scope::Project {
        project: "czgmdwth_gisagara".into(),
    };
    let mut archived = row("old", ArtifactState::Published);
    archived.archived = true;
    let rows = vec![
        row("a", ArtifactState::Held),
        row("b", ArtifactState::Held),
        row("c", ArtifactState::Uploading),
        row("d", ArtifactState::Published),
        row("e", ArtifactState::Refused),
        row("f", ArtifactState::Conflict),
        archived,
        other_project,
    ];
    let status = queue_status(&rows, &[], &[], None, 10_000);
    assert_eq!(
        status.counts,
        DeliveryCounts {
            queued: 2,
            uploading: 1,
            published: 2,
            failed: 2,
        }
    );
    let nyam = status
        .projects
        .iter()
        .find(|project| project.project_id == "arjgpydw_nyam")
        .unwrap();
    assert_eq!(
        nyam.counts,
        DeliveryCounts {
            queued: 2,
            uploading: 1,
            published: 1,
            failed: 2,
        }
    );
    let one = queue_status(&rows, &[], &[], Some("czgmdwth_gisagara"), 10_000);
    assert_eq!(one.counts.published, 1);
    assert_eq!(one.counts.queued, 0);

    let combined = combine_queue_statuses(&[status.clone(), one]);
    assert_eq!(combined.counts.published, 3);
    assert_eq!(combined.counts.queued, 2);

    let wire = serde_json::to_value(&status).unwrap();
    assert_eq!(
        wire["counts"],
        json!({"queued": 2, "uploading": 1, "published": 2, "failed": 2})
    );
    assert_eq!(wire["projects"][0]["counts"]["failed"].as_u64(), Some(2));
}
