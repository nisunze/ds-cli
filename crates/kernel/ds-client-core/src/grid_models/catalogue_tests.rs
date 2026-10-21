use super::*;
#[test]
fn paired_read_uses_the_same_native_list_decoder_and_rejects_crossed_projects() {
    let read = Read::new("template", None).unwrap();
    let body: Value = serde_json::from_slice(read.body().as_bytes()).unwrap();
    assert_eq!(
        body,
        json!({"action":"list_models","project_id":"template","page_size":100,"cursor":"","include_deleted":false})
    );
    let answer = |project: &str| {
        TransportResponse::new(200,serde_json::to_vec(&json!({"success":true,"data":{"models":[{"model_id":"model-1","project_id":project}],"has_more":false,"next_cursor":""}})).unwrap())
    };
    assert_eq!(
        read.decode(answer("template")).unwrap().data["models"][0]["model_id"],
        "model-1"
    );
    assert!(read.decode(answer("foreign")).is_err());
    assert!(Read::new("template", Some("bad\ncursor".into())).is_err());
}
#[test]
fn a_consuming_walk_follows_empty_pages_without_downloading_and_refuses_cycles() {
    let mut calls = Vec::new();
    let complete=collect("template",|cursor| { calls.push(cursor.clone()); Ok(Receipt{data:if cursor.is_none() {json!({"project":"template","models":[],"more":true,"next_cursor":"next"})} else {json!({"project":"template","models":[{"model_id":"model-1"}],"more":false,"next_cursor":""})},bytes:None}) }).unwrap();
    assert_eq!(calls, vec![None, Some("next".into())]);
    assert_eq!(complete["models"].as_array().unwrap().len(), 1);
    assert!(collect("template", |_| Ok(Receipt {
        data: json!({"project":"template","models":[],"more":true,"next_cursor":"same"}),
        bytes: None
    }))
    .is_err());
}
