use super::*;
use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
use crate::gateway_exchange::{Exchange, Refusal};

fn invocation(command: &str, args: Value) -> Value {
    json!({"command":command,"args":args,"context":{"uid":"owner","project":"p1"},"held":{
        "document":{"project_id":"p1","survey_layers":[{"key":"tr","label":"Transformers","layer_ids":["tr_pts"]},
            {"key":"lv","label":"Poles","layer_ids":["lv_pts"]}]},"counts":{"tr":12}}})
}
fn call(edge: &impl Edge, command: &str, args: Value) -> Value {
    block_on(invoke(edge, "owner", invocation(command, args)))
}

#[test]
fn never_chosen_is_offline_local_and_choices_survive_reopen_without_unchanged_writes() {
    let edge = TestEdge::new();
    edge.online.set(false);
    let before = call(&edge, "survey.working-area.forms", json!({}));
    assert_eq!(before["status"], "ok");
    assert_eq!(before["data"]["chosen"], false);
    assert_eq!(before["data"]["forms"][0]["count"], 12);
    assert!(before["data"]["loads"].as_array().unwrap().is_empty());
    let chosen = call(
        &edge,
        "survey.working-area.select",
        json!({"form":["lv","tr"]}),
    );
    assert_eq!(chosen["data"]["loads"], json!(["tr", "lv"]));
    let generation = edge.store(Call {
        request: Request::UiPreferences {
            operation: Operation::Get {
                project: Some("p1".into()),
                keys: vec![Key::WorkingAreaForms],
            },
        },
        expect_owner: None,
    });
    let generation = block_on(generation).unwrap()["generation"].clone();
    let same = call(
        &edge,
        "survey.working-area.select",
        json!({"form":["tr","lv"]}),
    );
    assert_eq!(same["data"]["changed"], false);
    let after = block_on(edge.store(Call {
        request: Request::UiPreferences {
            operation: Operation::Get {
                project: Some("p1".into()),
                keys: vec![Key::WorkingAreaForms],
            },
        },
        expect_owner: None,
    }))
    .unwrap();
    assert_eq!(after["generation"], generation);
    assert_eq!(
        call(&edge, "survey.working-area.forms", json!({}))["data"]["loads"],
        json!(["tr", "lv"])
    );
    assert!(edge.requests().is_empty());
    assert_eq!(
        call(&edge, "survey.working-area.select", json!({"none":true}))["data"]["chosen"],
        true
    );
    assert_eq!(
        call(&edge, "survey.working-area.clear", json!({}))["data"]["chosen"],
        false
    );
}

#[test]
fn unknown_ambiguous_wrong_project_and_wrong_account_refuse_before_storage() {
    for (command, args, code) in [
        (
            "survey.working-area.select",
            json!({"form":["unknown"]}),
            "unknown_form",
        ),
        ("survey.working-area.select", json!({}), "no_forms_named"),
        (
            "survey.working-area.select",
            json!({"all":true,"none":true}),
            "ambiguous_selection",
        ),
    ] {
        let edge = TestEdge::new();
        let refused = call(&edge, command, args);
        assert_eq!(refused["error"]["code"], code);
        assert!(refused["error"]["remedy"].as_str().unwrap().len() > 5);
        let tables: i64 = edge
            .db
            .borrow()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='ui_preferences'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
    }
    let edge = TestEdge::new();
    let mut wrong = invocation("survey.working-area.forms", json!({}));
    wrong["held"]["document"]["project_id"] = json!("another");
    assert_eq!(
        block_on(invoke(&edge, "owner", wrong))["error"]["code"],
        "project_context_changed"
    );
    assert_eq!(
        block_on(invoke(
            &edge,
            "another",
            invocation("survey.working-area.forms", json!({}))
        ))["error"]["code"],
        "auth_context_mismatch"
    );
}

#[test]
fn selection_is_isolated_by_account_and_project() {
    let edge = TestEdge::new();
    call(&edge, "survey.working-area.select", json!({"form":["tr"]}));
    edge.sign_in("second");
    let mut second = invocation("survey.working-area.forms", json!({}));
    second["context"]["uid"] = json!("second");
    assert_eq!(
        block_on(invoke(&edge, "second", second))["data"]["chosen"],
        false
    );
    edge.sign_in("owner");
    let mut another = invocation("survey.working-area.forms", json!({}));
    another["context"]["project"] = json!("p2");
    another["held"]["document"]["project_id"] = json!("p2");
    assert_eq!(
        block_on(invoke(&edge, "owner", another))["data"]["chosen"],
        false
    );
    assert_eq!(
        call(&edge, "survey.working-area.forms", json!({}))["data"]["loads"],
        json!(["tr"])
    );
}

struct Changed(TestEdge);
impl Edge for Changed {
    async fn fetch<C: Exchange>(&self, call: C) -> Result<C::Output, Refusal> {
        self.0.fetch(call).await
    }
    async fn store(&self, call: Call) -> Result<Value, Refusal> {
        let read = matches!(
            &call.request,
            Request::UiPreferences {
                operation: Operation::Get { .. }
            }
        );
        let answer = self.0.store(call).await?;
        if read {
            self.0.sign_in("changed");
        }
        Ok(answer)
    }
    fn now_ms(&self) -> u64 {
        self.0.now_ms()
    }
    fn online(&self) -> bool {
        false
    }
}
#[test]
fn account_change_between_read_and_write_preserves_the_owner_fence() {
    let edge = Changed(TestEdge::new());
    let refused = call(&edge, "survey.working-area.select", json!({"form":["tr"]}));
    assert_eq!(refused["error"]["code"], "durable_store_owner_changed");
}
