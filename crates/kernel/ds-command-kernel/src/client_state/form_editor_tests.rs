use super::*;
use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
use crate::gateway_exchange::Exchange;
use reactive_graph::traits::Get;
use std::cell::RefCell;
use std::task::{Context, Poll, Waker};

fn opened() -> (FormEditor, Rc<TestEdge>) {
    let scope = ArcRwSignal::new(Scope::new(Some("owner"), None));
    (FormEditor::new(&scope), Rc::new(TestEdge::new()))
}
fn schema(version: u64) -> Value {
    json!({"status":"success","form":{"slug":"pole","display_name":"Pole","version":version,"fields":[],"capture_detailed_location":false}})
}
fn registry() -> Value {
    json!({"status":"success","field_types":[{"key":"dropdown","display_name":"Choose"},{"key":"text","display_name":"Text"}]})
}
fn load(editor: &FormEditor, edge: &Rc<TestEdge>) {
    edge.json(schema(7)).json(registry());
    block_on(editor.load(Rc::clone(edge), "pole".into(), false)).unwrap();
}
fn view(editor: &FormEditor) -> Value {
    editor.view.value.get()
}
fn change(editor: &FormEditor, key: &str, value: Value) {
    editor
        .edit(
            Intent::Metadata {
                key: key.into(),
                value,
            },
            "unused",
        )
        .unwrap();
}

#[test]
fn defaults_edits_and_draft_reopen_are_shared_rust_state_offline() {
    let (editor, edge) = opened();
    load(&editor, &edge);
    assert_eq!(view(&editor)["dirty"], false);
    let id = editor
        .edit(
            Intent::Add {
                kind: "dropdown".into(),
            },
            "one",
        )
        .unwrap();
    assert_eq!(id, "f_one");
    assert_eq!(
        view(&editor)["document"]["fields"][0]["options"],
        json!(["Option 1", "Option 2"])
    );
    assert_eq!(view(&editor)["document"]["fields"][0]["label"], "Choose");
    editor
        .edit(
            Intent::Add {
                kind: "text".into(),
            },
            "two",
        )
        .unwrap();
    editor.edit(Intent::Move { from: 1, to: 0 }, "").unwrap();
    assert_eq!(view(&editor)["document"]["fields"][0]["id"], "f_two");
    editor
        .edit(
            Intent::Update {
                id: "f_two".into(),
                patch: Map::from_iter([("label".into(), json!("New label"))]),
            },
            "",
        )
        .unwrap();
    editor
        .edit(Intent::Remove { id: "f_one".into() }, "")
        .unwrap();
    let refused = editor
        .edit(Intent::Move { from: 9, to: 0 }, "")
        .unwrap_err();
    assert_eq!(refused.code, INVALID);
    assert_eq!(
        view(&editor)["document"]["fields"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    edge.online.set(false);
    change(&editor, "capture_detailed_location", json!(true));
    block_on(editor.save_draft(Rc::clone(&edge))).unwrap();
    editor.close();
    block_on(editor.load(Rc::clone(&edge), "pole".into(), false)).unwrap();
    assert_eq!(view(&editor)["version"], 7);
    assert_eq!(view(&editor)["dirty"], true);
    assert_eq!(view(&editor)["draft_saved"], true);
    assert_eq!(view(&editor)["document"]["capture_detailed_location"], true);
    assert_eq!(edge.requests().len(), 2);
    block_on(editor.reset(Rc::clone(&edge))).unwrap();
    assert_eq!(view(&editor)["dirty"], false);
    assert_eq!(view(&editor)["document"]["fields"], json!([]));
    assert_eq!(view(&editor)["draft_saved"], false);
}

#[test]
fn failed_publish_keeps_optimistic_version_and_draft_until_explicit_reset() {
    let (editor, edge) = opened();
    load(&editor, &edge);
    change(&editor, "description", json!("Offline revision"));
    edge.answer(Answer::Json(
        409,
        json!({"error_code":"form_version_conflict","error":"This form changed."}),
    ));
    assert_eq!(
        block_on(editor.save(Rc::clone(&edge))).unwrap_err().code,
        "form_version_conflict"
    );
    assert_eq!(view(&editor)["version"], 7);
    assert_eq!(view(&editor)["dirty"], true);
    assert_eq!(view(&editor)["draft_saved"], true);
    editor.close();
    edge.json(schema(8)).json(registry());
    block_on(editor.load(Rc::clone(&edge), "pole".into(), true)).unwrap();
    assert_eq!(view(&editor)["document"]["description"], "Offline revision");
    assert_eq!(view(&editor)["version"], 7);
    assert_eq!(view(&editor)["error"]["code"], "form_editor_draft_stale");
    block_on(editor.reset(Rc::clone(&edge))).unwrap();
    assert_eq!(view(&editor)["version"], 8);
    assert_eq!(view(&editor)["dirty"], false);
}

struct HookEdge {
    edge: Rc<TestEdge>,
    hook: RefCell<Option<Box<dyn FnOnce()>>>,
}
impl Edge for HookEdge {
    async fn fetch<C: Exchange>(&self, call: C) -> Result<C::Output, Refusal> {
        if let Some(hook) = self.hook.borrow_mut().take() {
            hook();
        }
        self.edge.fetch(call).await
    }
    async fn store(&self, call: Call) -> Result<Value, Refusal> {
        self.edge.store(call).await
    }
    fn now_ms(&self) -> u64 {
        self.edge.now_ms()
    }
    fn online(&self) -> bool {
        self.edge.online()
    }
}
#[test]
fn publish_adopts_confirmed_revision_and_retains_edits_made_in_flight() {
    let (editor, edge) = opened();
    load(&editor, &edge);
    change(&editor, "description", json!("Submitted"));
    edge.json(json!({"status":"success","form":{"version":8}}));
    let during = editor.clone();
    let hooked = Rc::new(HookEdge {
        edge: Rc::clone(&edge),
        hook: RefCell::new(Some(Box::new(move || {
            change(&during, "description", json!("Next edit"))
        }))),
    });
    block_on(editor.save(hooked)).unwrap();
    assert_eq!(view(&editor)["version"], 8);
    assert_eq!(view(&editor)["dirty"], true);
    assert_eq!(view(&editor)["document"]["description"], "Next edit");
    assert_eq!(view(&editor)["draft_saved"], true);
    let sent = edge.requests().last().unwrap().body.clone().unwrap();
    assert_eq!(sent["description"], "Submitted");
    assert_eq!(sent["version"], 7);
    editor.close();
    edge.online.set(false);
    block_on(editor.load(Rc::clone(&edge), "pole".into(), false)).unwrap();
    assert_eq!(view(&editor)["document"]["description"], "Next edit");
    assert_eq!(view(&editor)["version"], 8);
}

struct Waiting {
    edge: Rc<TestEdge>,
    wait: Cell<bool>,
}
impl Edge for Waiting {
    async fn fetch<C: Exchange>(&self, call: C) -> Result<C::Output, Refusal> {
        std::future::poll_fn(|_| {
            if self.wait.get() {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
        self.edge.fetch(call).await
    }
    async fn store(&self, call: Call) -> Result<Value, Refusal> {
        self.edge.store(call).await
    }
    fn now_ms(&self) -> u64 {
        self.edge.now_ms()
    }
    fn online(&self) -> bool {
        self.edge.online()
    }
}
#[test]
fn refresh_paints_held_while_slow_and_preserves_edits_when_freshness_refuses() {
    let (editor, edge) = opened();
    load(&editor, &edge);
    change(&editor, "display_name", json!("Held edit"));
    let waiting = Rc::new(Waiting {
        edge: Rc::clone(&edge),
        wait: Cell::new(true),
    });
    let mut future = Box::pin(editor.load(Rc::clone(&waiting), "pole".into(), true));
    assert!(future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert_eq!(view(&editor)["document"]["display_name"], "Held edit");
    assert_eq!(view(&editor)["loading"], false);
    waiting.wait.set(false);
    edge.online.set(false);
    assert_eq!(
        block_on(future).unwrap_err().code,
        crate::gateway_exchange::NETWORK_OFFLINE
    );
    assert_eq!(view(&editor)["document"]["display_name"], "Held edit");
    assert_eq!(view(&editor)["dirty"], true);
    edge.online.set(true);
    edge.json(schema(8)).json(registry());
    block_on(editor.load(Rc::clone(&edge), "pole".into(), true)).unwrap();
    assert_eq!(view(&editor)["document"]["display_name"], "Held edit");
    assert_eq!(view(&editor)["version"], 7);
}

#[test]
fn old_account_flights_and_drafts_cannot_adopt_after_signout_or_reentry() {
    let scope = ArcRwSignal::new(Scope::new(Some("owner"), None));
    let editor = FormEditor::new(&scope);
    let edge = Rc::new(TestEdge::new());
    edge.json(schema(7));
    let waiting = Rc::new(Waiting {
        edge: Rc::clone(&edge),
        wait: Cell::new(true),
    });
    let mut flight = Box::pin(editor.load(Rc::clone(&waiting), "pole".into(), false));
    assert!(flight
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    editor.scope_changed();
    scope.set(Scope::default());
    assert_eq!(view(&editor), json!({"open":false}));
    editor.scope_changed();
    scope.set(Scope::new(Some("owner"), None));
    waiting.wait.set(false);
    block_on(flight).unwrap();
    assert_eq!(view(&editor)["open"], false);
    edge.json(schema(7)).json(registry());
    block_on(editor.load(Rc::clone(&edge), "pole".into(), true)).unwrap();
    change(&editor, "description", json!("Account A"));
    block_on(editor.save_draft(Rc::clone(&edge))).unwrap();
    editor.scope_changed();
    scope.set(Scope::new(Some("other"), None));
    edge.sign_in("other");
    edge.online.set(false);
    assert_eq!(
        block_on(editor.load(Rc::clone(&edge), "pole".into(), false))
            .unwrap_err()
            .code,
        crate::gateway_exchange::NETWORK_OFFLINE
    );
    assert!(view(&editor)["document"].is_null());
}

struct Refusing {
    edge: Rc<TestEdge>,
    refuse: Cell<bool>,
}
impl Edge for Refusing {
    async fn fetch<C: Exchange>(&self, call: C) -> Result<C::Output, Refusal> {
        self.edge.fetch(call).await
    }
    async fn store(&self, call: Call) -> Result<Value, Refusal> {
        if self.refuse.get()
            && matches!(
                call.request,
                Request::WorkDrafts {
                    operation: Operation::Delete { .. }
                }
            )
        {
            return Err(Refusal::new("disk_full", "The draft could not be removed."));
        }
        self.edge.store(call).await
    }
    fn now_ms(&self) -> u64 {
        self.edge.now_ms()
    }
    fn online(&self) -> bool {
        self.edge.online()
    }
}
#[test]
fn reset_refusal_keeps_work_and_retry_recovers_without_defaults() {
    let (editor, edge) = opened();
    load(&editor, &edge);
    change(&editor, "description", json!("Work"));
    block_on(editor.save_draft(Rc::clone(&edge))).unwrap();
    let refusing = Rc::new(Refusing {
        edge,
        refuse: Cell::new(true),
    });
    assert_eq!(
        block_on(editor.reset(Rc::clone(&refusing)))
            .unwrap_err()
            .code,
        "disk_full"
    );
    assert_eq!(view(&editor)["document"]["description"], "Work");
    assert_eq!(view(&editor)["draft_saved"], true);
    refusing.refuse.set(false);
    block_on(editor.reset(refusing)).unwrap();
    assert_eq!(view(&editor)["dirty"], false);
}

#[test]
fn draft_writes_keep_the_store_owner_observed_when_the_form_opened() {
    let (editor, edge) = opened();
    load(&editor, &edge);
    change(&editor, "description", json!("Account A work"));
    // The host changed first; central identity publication has not reached
    // the page yet. The store's opaque owner still protects the write.
    edge.sign_in("other");
    assert_eq!(
        block_on(editor.save_draft(Rc::clone(&edge)))
            .unwrap_err()
            .code,
        crate::durable_store::OWNER_CHANGED
    );
    let other = block_on(edge.store(Call {
        request: Request::WorkDrafts {
            operation: Operation::Read {
                kind: Kind::FormEditor,
                project: None,
                key: "pole".into(),
            },
        },
        expect_owner: None,
    }))
    .unwrap();
    assert!(other["held"].is_null());
    assert_eq!(view(&editor)["document"]["description"], "Account A work");
    edge.sign_in("owner");
    block_on(editor.save_draft(Rc::clone(&edge))).unwrap();
    assert_eq!(view(&editor)["draft_saved"], true);
}

#[test]
fn signed_out_open_refuses_before_storage_or_gateway_work() {
    let scope = ArcRwSignal::new(Scope::default());
    let editor = FormEditor::new(&scope);
    let edge = Rc::new(TestEdge::new());
    assert_eq!(
        block_on(editor.load(Rc::clone(&edge), "pole".into(), false))
            .unwrap_err()
            .code,
        UNAVAILABLE
    );
    assert!(edge.requests().is_empty());
    assert_eq!(view(&editor), json!({"open":false}));
    let tables: i64 = edge
        .db
        .borrow()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
}

#[test]
fn refresh_and_another_draft_cannot_interrupt_an_in_flight_publish() {
    let (editor, edge) = opened();
    load(&editor, &edge);
    change(&editor, "description", json!("Submitted"));
    edge.json(json!({"status":"success","form":{"version":8}}));
    let during = editor.clone();
    let effects = Rc::clone(&edge);
    let hooked = Rc::new(HookEdge {
        edge: Rc::clone(&edge),
        hook: RefCell::new(Some(Box::new(move || {
            assert_eq!(
                block_on(during.load(Rc::clone(&effects), "pole".into(), true))
                    .unwrap_err()
                    .code,
                UNAVAILABLE
            );
            assert_eq!(
                block_on(during.save_draft(effects)).unwrap_err().code,
                UNAVAILABLE
            );
            assert_eq!(view(&during)["saving"], true);
        }))),
    });
    block_on(editor.save(hooked)).unwrap();
    assert_eq!(view(&editor)["saving"], false);
    assert_eq!(view(&editor)["version"], 8);
    assert_eq!(view(&editor)["dirty"], false);
    assert_eq!(edge.requests().len(), 3);
}
