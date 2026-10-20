//! The client state graph, natively and in the browser's WASM. Each
//! scenario runs the real gateway step machine against a scripted gateway
//! that holds every request until the scenario answers it: on futures'
//! local pool natively, and on wasm-bindgen-futures in WASM.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use reactive_graph::traits::GetUntracked;
use serde_json::{json, Value};

use super::{ClientState, Effects, Scope, Want};
use crate::gateway_exchange::{resume, start, Exchange, Facts, Observed, Refusal, Request, Step};

const MAPS2D: &str = "/api/v1/maps2d";

/// One request the scripted gateway holds until the scenario answers it.
struct Held {
    request: Request,
    answer: futures_channel::oneshot::Sender<(u16, Value)>,
}

impl Held {
    fn body(&self) -> Value {
        self.request.body.clone().unwrap_or(Value::Null)
    }

    fn answer(self, status: u16, body: Value) {
        self.answer.send((status, body)).expect("the read awaits its answer");
    }
}

#[derive(Clone, Default)]
struct Gateway(Rc<RefCell<VecDeque<Held>>>);

impl Gateway {
    /// The first held request to `path`.
    fn take(&self, path: &str) -> Held {
        let mut held = self.0.borrow_mut();
        let index = held
            .iter()
            .position(|held| held.request.path == path)
            .unwrap_or_else(|| panic!("a request to {path} is held"));
        held.remove(index).expect("the position is held")
    }

    fn held(&self) -> usize {
        self.0.borrow().len()
    }
}

impl Effects for Gateway {
    fn exchange<C: Exchange + 'static>(
        &self,
        call: C,
    ) -> impl Future<Output = Result<C::Output, Refusal>> + use<C> {
        let held = Rc::clone(&self.0);
        async move {
            let facts = Facts {
                now_ms: 0,
                has_session: true,
                offline_mode: false,
            };
            let mut step = start(call);
            loop {
                step = match step {
                    Step::Done(value) => return Ok(value),
                    Step::Refused(refusal) => return Err(refusal),
                    Step::Wait { continuation, .. } => {
                        resume(continuation, Observed::Waited, &[], &facts)
                    }
                    Step::Fetch {
                        request,
                        continuation,
                    } => {
                        let (answer, answered) = futures_channel::oneshot::channel();
                        held.borrow_mut().push_back(Held { request, answer });
                        let (status, body) = answered.await.expect("the scenario answers");
                        let observed = Observed::Response {
                            status,
                            content_type: Some("application/json".into()),
                            retry_after: None,
                        };
                        resume(continuation, observed, body.to_string().as_bytes(), &facts)
                    }
                }
            }
        }
    }
}

fn state() -> (ClientState, Gateway) {
    let gateway = Gateway::default();
    (ClientState::new(gateway.clone()), gateway)
}

/// Wants the dark basemap read, as a page showing it does.
fn watch(state: &ClientState) -> Want {
    state.maps.dark_basemap.demand.want()
}

fn scope(principal: &str, project: &str) -> Scope {
    Scope::new(Some(principal), Some(project))
}

/// Lets every task the last change woke run to its next pause.
async fn settle() {
    struct Yield(bool);
    impl Future for Yield {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                return Poll::Ready(());
            }
            self.0 = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }
    for _ in 0..16 {
        Yield(false).await;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run(scenario: impl Future<Output = ()> + 'static) {
    use any_spawner::Executor;
    use std::cell::Cell;
    let _ = Executor::init_futures_executor();
    let done = Rc::new(Cell::new(false));
    Executor::spawn_local({
        let done = Rc::clone(&done);
        async move {
            scenario.await;
            done.set(true);
        }
    });
    Executor::poll_local();
    assert!(done.get(), "the scenario stalled");
}

/// A test natively and under wasm-bindgen-test.
macro_rules! scenario {
    ($(#[$meta:meta])* async fn $name:ident() $body:block) => {
        $(#[$meta])*
        #[cfg(not(target_arch = "wasm32"))]
        #[test]
        fn $name() {
            run(async $body);
        }

        $(#[$meta])*
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            let _ = any_spawner::Executor::init_wasm_bindgen();
            async $body.await;
        }
    };
}

fn dark_session(tile: &str) -> Value {
    json!({"success": true, "data": {
        "basemap": {"id": "google-dark", "label": "Dark (Google)", "type": "raster",
            "tiles": [tile], "tileSize": 256, "attribution": "Map data © Google"},
        "expires_at": "1760003600"
    }})
}

fn no_tiles() -> Value {
    json!({"success": true, "data": {"basemap": {"tiles": []}}})
}

fn refused() -> Value {
    json!({"success": false, "error_code": "MAPS_DENIED", "message": "Maps are not enabled for this project."})
}

fn dark_tile(state: &ClientState) -> Option<String> {
    state
        .maps
        .dark_basemap
        .value
        .get_untracked()
        .map(|basemap| basemap.tiles[0].clone())
}

scenario! {
    /// No signed-in principal or no active project: nothing is read or shown.
    async fn nothing_is_read_without_a_complete_scope() {
        let (state, gateway) = state();
        let _watch = watch(&state);
        for incomplete in [Scope::default(), scope("uid_1", " "), scope("", "north")] {
            state.set_scope(incomplete);
            settle().await;
            assert_eq!(gateway.held(), 0);
            assert_eq!(dark_tile(&state), None);
        }
    }
}

scenario! {
    /// The dark basemap is the active project's; the previous project's
    /// stays while the next one's read is in flight, and signing out clears
    /// it at once.
    async fn the_dark_basemap_follows_the_project() {
        let (state, gateway) = state();
        let _watch = watch(&state);
        state.set_scope(scope("uid_1", "north"));
        settle().await;
        let held = gateway.take(MAPS2D);
        assert_eq!(held.body(), json!({"action": "get_google_dark_session", "ds_project": "north"}));
        held.answer(200, dark_session("https://tiles.test/north"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/north"));

        state.set_scope(scope("uid_1", "south"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/north"));
        gateway.take(MAPS2D).answer(200, no_tiles());
        settle().await;
        assert_eq!(dark_tile(&state), None);

        state.set_scope(Scope::new(None, Some("south")));
        assert_eq!(dark_tile(&state), None);
        settle().await;
        assert_eq!(gateway.held(), 0);
    }
}

scenario! {
    /// A refused session read leaves the dark basemap out, and another
    /// principal never sees the previous one's basemap.
    async fn a_refusal_or_another_principal_leaves_the_basemap_out() {
        let (state, gateway) = state();
        let _watch = watch(&state);
        state.set_scope(scope("uid_1", "north"));
        settle().await;
        gateway.take(MAPS2D).answer(200, dark_session("https://tiles.test/north"));
        settle().await;
        state.set_scope(scope("uid_2", "north"));
        assert_eq!(dark_tile(&state), None, "never another principal's basemap");
        settle().await;
        gateway.take(MAPS2D).answer(400, refused());
        settle().await;
        assert_eq!(dark_tile(&state), None);
    }
}

scenario! {
    /// A change while a read is in flight waits for it, then reads only the
    /// latest scope; the earlier answer never counts as the later scope's.
    async fn an_answer_counts_only_for_the_scope_it_was_read_for() {
        let (state, gateway) = state();
        let _watch = watch(&state);
        state.set_scope(scope("uid_1", "a"));
        settle().await;
        state.set_scope(scope("uid_1", "b"));
        state.set_scope(scope("uid_1", "c"));
        settle().await;
        assert_eq!(gateway.held(), 1, "only the read for a");
        gateway.take(MAPS2D).answer(200, dark_session("https://tiles.test/a"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/a"));
        let held = gateway.take(MAPS2D);
        assert_eq!(held.body()["ds_project"], json!("c"), "then c, never b");
        held.answer(200, dark_session("https://tiles.test/c"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/c"));
    }
}

scenario! {
    /// A read runs only while something wants it, and an answered scope is
    /// not read again when it is wanted again or set again unchanged.
    async fn a_read_runs_only_while_wanted() {
        let (state, gateway) = state();
        state.set_scope(scope(" uid_1 ", "north "));
        assert_eq!(state.scope(), scope("uid_1", "north"));
        settle().await;
        assert_eq!(gateway.held(), 0, "nothing shows the basemap, so nothing is read");

        let dark = watch(&state);
        settle().await;
        gateway.take(MAPS2D).answer(200, dark_session("https://tiles.test/north"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/north"));

        drop(dark);
        state.set_scope(scope("uid_1", "south"));
        settle().await;
        assert_eq!(gateway.held(), 0, "an unwanted read waits");
        let dark = watch(&state);
        settle().await;
        let held = gateway.take(MAPS2D);
        assert_eq!(held.body()["ds_project"], json!("south"));
        held.answer(200, dark_session("https://tiles.test/south"));
        settle().await;

        drop(dark);
        let _dark = watch(&state);
        state.set_scope(scope("uid_1", " south"));
        settle().await;
        assert_eq!(gateway.held(), 0, "an answered scope is not read again");
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/south"));
    }
}
