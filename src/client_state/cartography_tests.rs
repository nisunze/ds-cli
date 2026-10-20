//! Map defaults and relief on the test host (`testing`).

use reactive_graph::traits::GetUntracked;
use serde_json::{json, Value};

use super::Relief;
use crate::client_state::testing::{scenario, settle, state, Host};
use crate::client_state::{ClientState, Scope, Want};
use crate::gateway_exchange::map_defaults::MapDefaults;

const MAP_DEFAULTS: &str = "/api/v1/map_defaults";

fn watch(state: &ClientState<Host>) -> Want {
    state.cartography.defaults.demand.want()
}

fn sign_in(state: &ClientState<Host>, host: &Host, principal: Option<&str>) {
    host.sign_in(principal);
    state.set_scope(Scope::new(principal, Some("north")));
}

fn defaults(state: &ClientState<Host>) -> MapDefaults {
    state.cartography.defaults.value.get_untracked()
}

fn relief(state: &ClientState<Host>) -> Relief {
    state.cartography.relief.value.get_untracked()
}

fn answer(document: Value) -> Value {
    json!({"success": true, "data": document})
}

scenario! {
    /// What this edge holds shows at once; Brain's document replaces it and
    /// is held in its place.
    async fn the_held_defaults_show_until_brains_answer() {
        let (state, host) = state();
        host.hold(Some("uid_1"), "map_defaults", json!({"scale_bar": {"enabled": true}}));
        sign_in(&state, &host, Some("uid_1"));
        let _watch = watch(&state);
        settle().await;
        assert!(defaults(&state).scale_bar.enabled, "the held document shows at once");

        let read = host.take(MAP_DEFAULTS);
        assert_eq!(read.body(), json!({"action": "get_config"}));
        read.answer(200, answer(json!({"measure": {"halo_color": "#ffffff"}})));
        settle().await;
        let shown = defaults(&state);
        assert!(!shown.scale_bar.enabled);
        assert_eq!(shown.measure.halo_color, "#ffffff");
        let held = host.preference(Some("uid_1"), "map_defaults").expect("held");
        assert_eq!(MapDefaults::admit(&held), shown);
    }
}

scenario! {
    /// Nothing is read while nothing shows the defaults, and signed out only
    /// this edge is asked: Brain is not.
    async fn brain_is_read_only_for_a_signed_in_page_that_shows_the_defaults() {
        let (state, host) = state();
        sign_in(&state, &host, None);
        settle().await;
        assert_eq!((host.store_calls(), host.held()), (0, 0));

        let watching = watch(&state);
        settle().await;
        assert_eq!(host.held(), 0, "signed out, Brain is not asked");
        assert!(host.store_calls() > 0);
        assert_eq!(defaults(&state), MapDefaults::default());

        drop(watching);
        sign_in(&state, &host, Some("uid_1"));
        settle().await;
        assert_eq!(host.held(), 0, "an unwanted read waits");
        let _watch = watch(&state);
        settle().await;
        host.take(MAP_DEFAULTS).answer(503, json!({"success": false,
            "error": {"code": "UNAVAILABLE", "message": "down"}}));
        settle().await;
        assert_eq!(defaults(&state), MapDefaults::default(), "a refused read keeps what shows");
    }
}

scenario! {
    /// A publish shows for its account at once and is held; another account
    /// sees its own read.
    async fn a_publish_shows_at_once_and_is_held() {
        let (state, host) = state();
        sign_in(&state, &host, Some("admin"));
        let _watch = watch(&state);
        settle().await;
        host.take(MAP_DEFAULTS).answer(200, answer(json!({})));
        settle().await;

        let mut next = MapDefaults::default();
        next.scale_bar.enabled = true;
        let publishing = state.publish_map_defaults(next.clone());
        any_spawner::Executor::spawn_local(async move {
            assert!(publishing.await.is_ok());
        });
        settle().await;
        let sent = host.take(MAP_DEFAULTS);
        assert_eq!(sent.body()["action"], "update");
        assert_eq!(sent.body()["data"]["scale_bar"]["enabled"], true);
        sent.answer(200, answer(serde_json::to_value(&next).unwrap()));
        settle().await;
        assert!(defaults(&state).scale_bar.enabled);
        let held = host.preference(Some("admin"), "map_defaults").expect("held");
        assert!(MapDefaults::admit(&held).scale_bar.enabled);

        sign_in(&state, &host, Some("uid_2"));
        assert!(!defaults(&state).scale_bar.enabled, "never another account's publish");
        settle().await;
        host.take(MAP_DEFAULTS).answer(200, answer(json!({})));
        settle().await;
        assert!(!defaults(&state).scale_bar.enabled);
    }
}

scenario! {
    /// Relief starts off; its intensity is the account's choice, else what
    /// the account holds, else the defaults' intensity at the nearest preset.
    async fn relief_starts_off_and_keeps_the_chosen_intensity() {
        let (state, host) = state();
        sign_in(&state, &host, Some("uid_1"));
        let _relief = state.cartography.relief.demand.want();
        settle().await;
        host.take(MAP_DEFAULTS).answer(200, answer(json!({"hillshade": {"enabled": true, "exaggeration": 1.2}})));
        settle().await;
        assert_eq!(
            relief(&state),
            Relief { hillshade: false, exaggeration: 1.0, presets: vec![0.3, 0.6, 1.0, 1.5, 2.5] }
        );

        state.cycle_relief();
        settle().await;
        assert_eq!((relief(&state).hillshade, relief(&state).exaggeration), (true, 1.5));
        assert_eq!(host.preference(Some("uid_1"), "hillshade_exaggeration"), Some(json!(1.5)));
        state.toggle_relief();
        assert_eq!((relief(&state).hillshade, relief(&state).exaggeration), (false, 1.5));
        state.cycle_relief();
        state.cycle_relief();
        assert_eq!(relief(&state).exaggeration, 0.3, "the cycle wraps");

        let next = ClientState::new(host.clone());
        sign_in(&next, &host, Some("uid_1"));
        let _held = next.cartography.relief.demand.want();
        settle().await;
        assert_eq!((relief(&next).hillshade, relief(&next).exaggeration), (false, 0.3));
    }
}
