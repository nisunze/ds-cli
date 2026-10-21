use super::*;
use ds_client_core::grid_models::{
    linked_publication::{LinkedIntent, Participant},
    Command,
};
use ds_command_kernel::grid_publication::Intent;

fn input() -> (LinkedIntent, Vec<Participant>, Vec<u8>) {
    let intent = LinkedIntent {
        project: "p-1".into(),
        graph_id: "linked-main".into(),
        expected_generation: Some(0),
        generation: 1,
        composite: "combined".into(),
        parts: vec!["north".into(), "south".into()],
        canonical_digest: format!("sha256:{}", "a".repeat(64)),
        retire: None,
    };
    let participants = ["combined", "north", "south"]
        .map(|id| Participant {
            bytes: format!("exact {id} package").into_bytes(),
            intent: Intent {
                project: "p-1".into(),
                model_id: id.into(),
                model_kind: "mv_line".into(),
                expected_head_revision_id: format!("old-{id}"),
                source_revision_id: format!("rev:{id}"),
                manifest_model_id: format!("native-{id}"),
                manifest_model_revision: 2,
                model_schema_version: "1".into(),
                engine_version: "engine".into(),
                ..Default::default()
            },
        })
        .into_iter()
        .collect();
    (intent, participants, b"exact linked checkpoint".to_vec())
}

fn response(
    intent: &LinkedIntent,
    plan: &ds_client_core::grid_models::linked_publication::LinkedPlan,
) -> serde_json::Value {
    json!({"linked":{"schema":1,"project_id":intent.project,"graph_id":intent.graph_id,"generation":intent.generation,
        "composite_model_id":intent.composite,"part_model_ids":intent.parts,"canonical_digest":intent.canonical_digest,
        "checkpoint":plan.checkpoint,"request_digest":"b".repeat(64),"versions":plan.versions.iter().map(|p| json!({"model_id":p.intent.model_id,"revision_id":p.revision_id,"model":p.artifact})).collect::<Vec<_>>()}})
}

#[test]
fn linked_grid_stages_all_bytes_then_commits_one_vector_and_reads_it_back() {
    let (intent, participants, checkpoint) = input();
    let plan = intent.plan("p-1", &participants, &checkpoint).unwrap();
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    {
        let mut state = transport.0.lock().unwrap();
        for artifact in plan
            .versions
            .iter()
            .map(|p| &p.artifact)
            .chain(std::iter::once(&plan.checkpoint))
        {
            state.grid_models.push_back(project_response(
                json!({"artifact":artifact,"already_exists":true}),
            ));
        }
        state
            .grid_models
            .push_back(project_response(response(&intent, &plan)));
        state
            .grid_models
            .push_back(project_response(response(&intent, &plan)));
    }
    let receipt = client
        .grid_models(
            "p-1",
            &Command::PublishLinked {
                intent: Box::new(intent),
                participants,
                checkpoint,
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(receipt.data["verified"], true);
    let state = transport.0.lock().unwrap();
    assert_eq!(
        state
            .grid_bodies
            .iter()
            .filter(|b| b["action"] == "publish_linked")
            .count(),
        1
    );
    assert!(!state
        .grid_bodies
        .iter()
        .any(|b| b["action"] == "create_version"));
    let batch = state
        .grid_bodies
        .iter()
        .find(|b| b["action"] == "publish_linked")
        .unwrap();
    for version in batch["linked"]["versions"].as_array().unwrap() {
        assert_eq!(version["project_id"], "p-1");
        assert!(version["expected_head_revision_id"]
            .as_str()
            .unwrap()
            .starts_with("old-"));
    }
    assert_eq!(
        state.grid_bodies.last().unwrap()["action"],
        "get_linked_generation"
    );
    assert!(state.grid_models.is_empty());
}

#[test]
fn linked_grid_failed_staging_never_commits_or_replaces_a_head() {
    let (intent, participants, checkpoint) = input();
    let plan = intent.plan("p-1", &participants, &checkpoint).unwrap();
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    {
        let mut state = transport.0.lock().unwrap();
        state.grid_models.push_back(project_response(
            json!({"artifact":plan.versions[0].artifact,"already_exists":true}),
        ));
        // The second grant attests different bytes; even a stored object may
        // never substitute its digest for the captured participant package.
        state.grid_models.push_back(project_response(
            json!({"artifact":plan.versions[0].artifact,"already_exists":true}),
        ));
    }
    assert!(client
        .grid_models(
            "p-1",
            &Command::PublishLinked {
                intent: Box::new(intent),
                participants,
                checkpoint
            },
            NOW + 1
        )
        .is_err());
    assert!(transport
        .0
        .lock()
        .unwrap()
        .grid_bodies
        .iter()
        .all(|b| b["action"] == "start_upload"));
}

#[test]
fn linked_grid_readback_cannot_replace_one_part_pin() {
    let (intent, participants, checkpoint) = input();
    let plan = intent.plan("p-1", &participants, &checkpoint).unwrap();
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    {
        let mut state = transport.0.lock().unwrap();
        for artifact in plan
            .versions
            .iter()
            .map(|p| &p.artifact)
            .chain(std::iter::once(&plan.checkpoint))
        {
            state.grid_models.push_back(project_response(
                json!({"artifact":artifact,"already_exists":true}),
            ));
        }
        state
            .grid_models
            .push_back(project_response(response(&intent, &plan)));
        let mut wrong = response(&intent, &plan);
        wrong["linked"]["versions"][1]["revision_id"] = json!("changed-part");
        state.grid_models.push_back(project_response(wrong));
    }
    assert!(client
        .grid_models(
            "p-1",
            &Command::PublishLinked {
                intent: Box::new(intent),
                participants,
                checkpoint
            },
            NOW + 1
        )
        .is_err());
}

#[test]
fn linked_grid_interleaved_projects_are_rejected_before_upload() {
    let (intent, participants, checkpoint) = input();
    assert!(intent
        .plan("other-project", &participants, &checkpoint)
        .is_err());
    let mut wrong = participants;
    wrong[1].intent.project = "other-project".into();
    assert!(intent.plan("p-1", &wrong, &checkpoint).is_err());
}

#[test]
fn linked_grid_status_validates_every_pin_and_exact_artifact_path() {
    let (intent, participants, checkpoint) = input();
    let plan = intent.plan("p-1", &participants, &checkpoint).unwrap();
    for malformed in [false, true] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        let mut receipt = response(&intent, &plan);
        if malformed {
            receipt["linked"]["versions"][1]["model"]["object"] = json!("wrong/path");
        }
        transport
            .0
            .lock()
            .unwrap()
            .grid_models
            .push_back(project_response(receipt));
        let result = client.grid_models(
            "p-1",
            &Command::LinkedGeneration {
                graph: intent.graph_id.clone(),
                generation: intent.generation,
            },
            NOW + 1,
        );
        assert_eq!(result.is_ok(), !malformed);
        assert_eq!(transport.0.lock().unwrap().grid_bodies.len(), 1);
    }
}
