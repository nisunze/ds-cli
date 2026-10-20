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

use super::maps::{Maps3dLoadState, Maps3dLoadStatus};
use super::{until, ClientState, Effects, Scope, Want};
use crate::gateway_exchange::{resume, start, Exchange, Facts, Observed, Refusal, Request, Step};

const MAPS2D: &str = "/api/v1/maps2d";
const MAPS3D: &str = "/api/v1/maps3d";

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

/// Wants both map reads, as a page showing them does.
fn watch(state: &ClientState) -> (Want, Want) {
    (
        state.maps.dark_basemap.demand.want(),
        state.maps.config_3d.demand.want(),
    )
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

fn config(available: bool, root_url: &str) -> Value {
    json!({"success": true, "data": {
        "default_provider": "auto",
        "google": {"available": available, "root_url": root_url, "attribution": "Google"},
        "aws_terrain": {"available": true, "tiles_url": "https://terrain.test/{z}/{x}/{y}.png",
            "encoding": "terrarium", "max_zoom": 15, "default_exaggeration": 1.5,
            "attribution": "AWS"}
    }})
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

fn load(state: &ClientState) -> (Maps3dLoadStatus, String) {
    let Maps3dLoadState { status, detail } = state.maps.load_3d.value.get_untracked();
    (status, detail)
}

fn root(state: &ClientState) -> Option<String> {
    state
        .maps
        .config_3d
        .value
        .get_untracked()
        .map(|config| config.google.root_url)
}

/// Answers the held dark-session and 3D-config reads of one scope.
fn answer_both(gateway: &Gateway, dark: Value, config: Value) {
    gateway.take(MAPS2D).answer(200, dark);
    gateway.take(MAPS3D).answer(200, config);
}

/// Refuses both reads of one scope.
fn refuse_both(gateway: &Gateway) {
    gateway.take(MAPS2D).answer(400, refused());
    gateway.take(MAPS3D).answer(400, refused());
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
            assert_eq!(root(&state), None);
            assert_eq!(load(&state), (Maps3dLoadStatus::Idle, String::new()));
            assert!(!state.maps.google_3d.value.get_untracked());
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
        gateway.take(MAPS3D).answer(200, config(true, "https://3d.test/root?key=k"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/north"));

        state.set_scope(scope("uid_1", "south"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/north"));
        let no_tiles = json!({"success": true, "data": {"basemap": {"tiles": []}}});
        answer_both(&gateway, no_tiles, config(true, "https://3d.test/root?key=k"));
        settle().await;
        assert_eq!(dark_tile(&state), None);

        state.set_scope(scope("uid_1", "north"));
        settle().await;
        answer_both(&gateway, dark_session("https://tiles.test/north-2"), config(true, "r"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/north-2"));
        state.set_scope(Scope::new(None, Some("north")));
        assert_eq!(dark_tile(&state), None);
        settle().await;
        assert_eq!(gateway.held(), 0);
    }
}

scenario! {
    /// A refused session read leaves the dark basemap out.
    async fn a_refused_dark_session_leaves_the_basemap_out() {
        let (state, gateway) = state();
        let _watch = watch(&state);
        state.set_scope(scope("uid_1", "north"));
        settle().await;
        refuse_both(&gateway);
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
        assert_eq!(gateway.held(), 2, "one dark and one 3D read, both for a");
        answer_both(&gateway, dark_session("https://tiles.test/a"), config(true, "https://3d.test/a"));
        settle().await;
        assert_eq!(load(&state).0, Maps3dLoadStatus::Loading);
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/a"));
        assert_eq!(gateway.held(), 2, "then one read each for c, none for b");
        let held = gateway.take(MAPS2D);
        let project = held.body()["ds_project"].clone();
        assert_eq!(project, json!("c"));
        held.answer(200, dark_session("https://tiles.test/c"));
        gateway.take(MAPS3D).answer(200, config(true, "https://3d.test/c"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/c"));
        assert_eq!(root(&state).as_deref(), Some("https://3d.test/c"));
        assert_eq!(load(&state).0, Maps3dLoadStatus::Ready);
    }
}

scenario! {
    /// The 3D configuration's load state names what the answer offers, and
    /// a retry reads it again.
    async fn the_3d_load_state_names_the_answer_and_a_retry_reads_again() {
        let (state, gateway) = state();
        let _watch = watch(&state);
        state.set_scope(scope("uid_1", "north"));
        settle().await;
        assert_eq!(load(&state), (Maps3dLoadStatus::Loading, "Loading Maps3D API config.".into()));
        gateway.take(MAPS2D).answer(200, dark_session("https://tiles.test/north"));
        let held = gateway.take(MAPS3D);
        assert_eq!(held.body(), json!({"action": "get_config"}));
        held.answer(200, config(true, "https://3d.test/root?key=k"));
        settle().await;
        assert_eq!(load(&state), (Maps3dLoadStatus::Ready, "Google 3D Tiles API root URL loaded.".into()));
        assert!(state.maps.google_3d.value.get_untracked());

        let cases = [
            (config(true, " "), Maps3dLoadStatus::Unavailable, "Maps3D API did not return a Google 3D root URL."),
            (config(false, ""), Maps3dLoadStatus::Unavailable, "Maps3D API reports Google 3D unavailable."),
        ];
        for (answer, status, detail) in cases {
            state.maps.retry_3d();
            settle().await;
            assert_eq!(load(&state).0, Maps3dLoadStatus::Loading);
            gateway.take(MAPS3D).answer(200, answer);
            settle().await;
            assert_eq!(load(&state), (status, detail.into()));
            assert!(!state.maps.google_3d.value.get_untracked());
        }

        state.maps.retry_3d();
        settle().await;
        gateway.take(MAPS3D).answer(400, refused());
        settle().await;
        assert_eq!(load(&state), (Maps3dLoadStatus::Error, "Maps are not enabled for this project.".into()));
        assert_eq!(root(&state), None);

        state.maps.retry_3d();
        settle().await;
        assert_eq!(root(&state), None, "a retry after a refusal shows no configuration");
        gateway.take(MAPS3D).answer(200, config(true, "https://3d.test/root?key=k"));
        settle().await;
        assert_eq!(load(&state).0, Maps3dLoadStatus::Ready);
        assert_eq!(gateway.held(), 0);
    }
}

scenario! {
    /// A keyed provider root never shows for another principal, nor for
    /// another project once that project's read is refused.
    async fn a_keyed_root_never_crosses_principals_or_refused_projects() {
        let (state, gateway) = state();
        let _watch = watch(&state);
        state.set_scope(scope("uid_1", "north"));
        settle().await;
        answer_both(&gateway, dark_session("https://tiles.test/north"), config(true, "https://3d.test/root?key=north"));
        settle().await;
        assert_eq!(root(&state).as_deref(), Some("https://3d.test/root?key=north"));

        state.set_scope(scope("uid_1", "south"));
        settle().await;
        assert_eq!(root(&state).as_deref(), Some("https://3d.test/root?key=north"));
        refuse_both(&gateway);
        settle().await;
        assert_eq!(root(&state), None);
        assert_eq!(load(&state).0, Maps3dLoadStatus::Error);

        state.set_scope(scope("uid_1", "north"));
        settle().await;
        answer_both(&gateway, dark_session("https://tiles.test/north"), config(true, "https://3d.test/root?key=north"));
        settle().await;
        state.set_scope(scope("uid_2", "north"));
        assert_eq!(root(&state), None);
        assert_eq!(dark_tile(&state), None);
        assert!(!state.maps.google_3d.value.get_untracked());
        assert_eq!(load(&state).0, Maps3dLoadStatus::Loading);
    }
}

scenario! {
    /// Settling waits for this scope's answer; an idle scope is settled.
    async fn settled_waits_for_this_scopes_answer() {
        let (state, gateway) = state();
        let _watch = watch(&state);
        assert_eq!(state.maps.settled_3d().await.status, Maps3dLoadStatus::Idle);
        state.set_scope(scope("uid_1", "north"));
        let settled = Rc::new(RefCell::new(None));
        let pending = state.maps.settled_3d();
        spawn({
            let settled = Rc::clone(&settled);
            async move { *settled.borrow_mut() = Some(pending.await) }
        });
        settle().await;
        assert!(settled.borrow().is_none());
        answer_both(&gateway, dark_session("https://tiles.test/north"), config(true, "https://3d.test/r"));
        settle().await;
        assert_eq!(settled.borrow().as_ref().map(|load| load.status), Some(Maps3dLoadStatus::Ready));
    }
}

scenario! {
    /// The scope is trimmed, and an unchanged scope reads nothing again.
    async fn an_unchanged_scope_reads_nothing_again() {
        let (state, gateway) = state();
        let _watch = watch(&state);
        state.set_scope(scope(" uid_1 ", "north "));
        assert_eq!(state.scope(), scope("uid_1", "north"));
        settle().await;
        answer_both(&gateway, dark_session("https://tiles.test/north"), config(true, "https://3d.test/r"));
        settle().await;
        state.set_scope(scope("uid_1", " north"));
        settle().await;
        assert_eq!(gateway.held(), 0);
        let ready = until(&state.maps.google_3d.value, |offered| *offered).await;
        assert!(ready);
    }
}

scenario! {
    /// A read runs only while something wants it, and an answered scope is
    /// not read again when it is wanted again.
    async fn a_read_runs_only_while_wanted() {
        let (state, gateway) = state();
        state.set_scope(scope("uid_1", "north"));
        settle().await;
        assert_eq!(gateway.held(), 0, "nothing shows the maps, so nothing is read");

        let dark = state.maps.dark_basemap.demand.want();
        settle().await;
        assert_eq!(gateway.held(), 1, "only the wanted dark basemap is read");
        gateway.take(MAPS2D).answer(200, dark_session("https://tiles.test/north"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/north"));

        drop(dark);
        state.set_scope(scope("uid_1", "south"));
        settle().await;
        assert_eq!(gateway.held(), 0, "an unwanted read waits");
        let dark = state.maps.dark_basemap.demand.want();
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/north"));
        let held = gateway.take(MAPS2D);
        assert_eq!(held.body()["ds_project"], json!("south"));
        held.answer(200, dark_session("https://tiles.test/south"));
        settle().await;
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/south"));

        drop(dark);
        let _dark = state.maps.dark_basemap.demand.want();
        settle().await;
        assert_eq!(gateway.held(), 0, "an answered scope is not read again");
        assert_eq!(dark_tile(&state).as_deref(), Some("https://tiles.test/south"));

        let settled = state.maps.settled_3d();
        settle().await;
        gateway.take(MAPS3D).answer(200, config(true, "https://3d.test/r"));
        assert_eq!(settled.await.status, Maps3dLoadStatus::Ready, "settling wants its read");
    }
}

fn spawn(task: impl Future<Output = ()> + 'static) {
    any_spawner::Executor::spawn_local(task);
}
