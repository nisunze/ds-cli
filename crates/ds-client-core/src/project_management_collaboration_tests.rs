use super::*;
#[test]
fn public_index_has_no_project_or_write_fields_and_bounds_pages() {
    let command = Command::PublicProjects {
        status: "active".into(),
        cursor: "next-id".into(),
        limit: 50,
    };
    command.validate().unwrap();
    assert_eq!(
        command.payload(""),
        json!({"action":"list_public_projects","status":"active","cursor":"next-id","limit":50})
    );
    assert!(Command::PublicProjects {
        status: "all".into(),
        cursor: String::new(),
        limit: 50
    }
    .validate()
    .is_err());
    assert!(Command::PublicProjects {
        status: "active".into(),
        cursor: String::new(),
        limit: 101
    }
    .validate()
    .is_err());
}
#[test]
fn task_comment_is_scoped_and_cursors_use_the_existing_collection_name() {
    let command = Command::TaskCommentCreate {
        task_id: "t".into(),
        id: "comment-1".into(),
        body: "Please check this task".into(),
    };
    command.validate().unwrap();
    assert_eq!(
        command.payload("p"),
        json!({"action":"create_comment","ds_project":"p","id":"comment-1","data":{"task_id":"t","body":"Please check this task","kind":"comment","state":"open"}})
    );
    let page = Command::TaskComments {
        task_id: "t".into(),
        cursor: "old-comment".into(),
        limit: 20,
    };
    assert_eq!(
        page.payload("p")["cursors"],
        json!({"pm_comments":"old-comment"})
    );
    assert!(Command::TaskCommentCreate {
        task_id: "t".into(),
        id: "comment-1".into(),
        body: " ".into()
    }
    .validate()
    .is_err());
    assert!(Command::TaskCommentCreate {
        task_id: "t".into(),
        id: "comment-1".into(),
        body: "é".repeat(2001)
    }
    .validate()
    .is_err());
}
