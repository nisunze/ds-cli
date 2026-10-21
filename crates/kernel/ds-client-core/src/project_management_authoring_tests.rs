use super::*;
fn model() -> ds_command_kernel::project_management::authoring_model::AuthoringModel {
    serde_json::from_value(json!({"model":"GPT-6"})).unwrap()
}
#[test]
fn authoring_none_preserves_reads_and_each_write_isolated() {
    let read = Command::Graph;
    assert_eq!(
        read.clone().with_authoring(None).unwrap().payload("p"),
        read.payload("p")
    );
    assert!(read.with_authoring(Some(model())).is_err());
    let write = Command::TaskCommentCreate {
        task_id: "t".into(),
        id: "c".into(),
        body: "Observed".into(),
    };
    let original = write.payload("p");
    assert_eq!(
        write.clone().with_authoring(None).unwrap().payload("p"),
        original
    );
    let declared = write.clone().with_authoring(Some(model())).unwrap();
    declared.validate().unwrap();
    let mut body = declared.payload("p");
    assert_eq!(
        body.as_object_mut().unwrap().remove("authoring_model"),
        Some(json!({"model":"GPT-6"}))
    );
    assert_eq!(body, original);
    assert_eq!(write.payload("p"), original);
    assert!(declared.with_authoring(Some(model())).is_err());
}
#[test]
fn authored_milestone_uses_the_existing_commit_payload_without_new_authority() {
    let write = Command::Commit {
        command_id: "command-1".into(),
        base_revision: 7,
        command: json!({"kind":"create_task","create_task":{"id":"m","kind":"milestone"}}),
    };
    let original = write.payload("p");
    let declared = write.with_authoring(Some(model())).unwrap();
    declared.validate().unwrap();
    let mut payload = declared.payload("p");
    payload.as_object_mut().unwrap().remove("authoring_model");
    assert_eq!(payload, original);
}
