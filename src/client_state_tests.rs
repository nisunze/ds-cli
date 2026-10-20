//! The map provider state on the test host (`testing`): the active project's
//! dark basemap, read on demand per scope.

use reactive_graph::traits::GetUntracked;
use serde_json::{json, Value};

use super::testing::{scenario, settle, state, Host};
use super::{ClientState, Scope, Want};

const MAPS2D: &str = "/api/v1/maps2d";

/// Wants the dark basemap read, as a page showing it does.
fn watch(state: &ClientState<Host>) -> Want {
    state.maps.dark_basemap.demand.want()
}

fn scope(principal: &str, project: &str) -> Scope {
    Scope::new(Some(principal), Some(project))
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

fn dark_tile(state: &ClientState<Host>) -> Option<String> {
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
