//! The Server's working-area form routes: which survey forms the working area
//! loads for the project a caller names, read and chosen with no browser and
//! no paired map.
//!
//! Every decision is `ds_layer_ops::working_area_forms`' — the same owner
//! `ds survey working-area forms|select|clear` calls natively — so the answer
//! is the same whichever host executed it. This module only binds it to the
//! protected loopback transport exactly as `layers.rs` binds the drawer: the
//! owner-only bearer, the connection's lane, the principal observed at request
//! time, the project NAMED by the caller (`?project=<exact-id>`) and held
//! against the document that comes back. The selection is admitted under the
//! layer operations (`layer_read` / `layer_write`): it is read from the layer
//! document and remembered beside the drawer's visibility, under the same
//! (lane, account, project) key.

use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use ds_layer_ops::working_area_forms::SelectRequest;

use crate::host::App;
use crate::layers::{ScopeQuery, invalid, run};

const LAYER_READ: &str = ds_command_kernel::execution_context::LAYER_READ;
const LAYER_WRITE: &str = ds_command_kernel::execution_context::LAYER_WRITE;

/// `GET /v1/survey/working-area-forms?project=<exact-id>` — the forms and
/// what loads.
pub async fn read(State(app): State<App>, query: Option<Query<ScopeQuery>>) -> Response {
    let Some(Query(query)) = query else {
        return invalid("this route accepts a project query parameter only");
    };
    match run(
        app,
        LAYER_READ,
        query.project,
        b"working-area-forms:read".to_vec(),
        move |documents, preferences| {
            ds_layer_ops::working_area_forms::read(documents, preferences)
        },
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(response) => response,
    }
}

/// `POST /v1/survey/working-area-forms/select?project= {"forms": [slugs]}`,
/// `{"all": true}` or `{"none": true}` — the body is the owner's request type.
pub async fn select(
    State(app): State<App>,
    query: Option<Query<ScopeQuery>>,
    body: axum::body::Bytes,
) -> Response {
    let Some(Query(query)) = query else {
        return invalid("this route accepts a project query parameter only");
    };
    let request: SelectRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => return invalid(format!("invalid working-area forms request: {error}")),
    };
    match run(
        app,
        LAYER_WRITE,
        query.project,
        body.to_vec(),
        move |documents, preferences| {
            ds_layer_ops::working_area_forms::select(documents, preferences, &request)
        },
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(response) => response,
    }
}

/// `POST /v1/survey/working-area-forms/clear?project=` — forget the choice.
pub async fn clear(
    State(app): State<App>,
    query: Option<Query<ScopeQuery>>,
    body: axum::body::Bytes,
) -> Response {
    let Some(Query(query)) = query else {
        return invalid("this route accepts a project query parameter only");
    };
    if !body.is_empty() && body.as_ref() != b"{}" {
        return invalid("clear takes no body");
    }
    match run(
        app,
        LAYER_WRITE,
        query.project,
        b"working-area-forms:clear".to_vec(),
        move |documents, preferences| {
            ds_layer_ops::working_area_forms::clear(documents, preferences)
        },
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(response) => response,
    }
}

#[cfg(test)]
mod tests {
    //! The realistic workflow through a REAL loopback listener with the layer
    //! fixture identity and upstream: never chosen, select, restart, read the
    //! retained choice, the second project on its own terms, unknown slug,
    //! project change during a request, clear.
    use crate::layers::tests::{TOKEN, fixture_upstream, start, wire};
    use serde_json::{Value, json};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test(flavor = "multi_thread")]
    async fn the_choice_is_per_project_survives_restart_and_never_defaults_to_all() {
        let dir = tempfile::tempdir().unwrap();
        let upstream = fixture_upstream("uid-a");
        let allowed = Arc::new(AtomicBool::new(true));
        let server = start(dir.path(), upstream.clone(), allowed.clone()).await;

        // No project named: refused before anything is read.
        let (status, refused) = wire(
            server.address,
            "GET",
            "/v1/survey/working-area-forms",
            None,
            TOKEN,
        )
        .await;
        assert_eq!(status, 400, "{refused}");
        assert_eq!(refused["code"], "project_required");

        // Never chosen: nothing loads, and the answer says how to choose.
        let (status, first) = wire(
            server.address,
            "GET",
            "/v1/survey/working-area-forms?project=proj-kigali",
            None,
            TOKEN,
        )
        .await;
        assert_eq!(status, 200, "{first}");
        assert_eq!(first["project"], "proj-kigali");
        assert_eq!(first["chosen"], false);
        assert_eq!(first["form_count"], 2);
        assert_eq!(first["loads"], json!([]));
        assert!(
            first["remedy"]
                .as_str()
                .unwrap()
                .contains("working-area select")
        );

        // An unknown slug is refused by name and writes nothing.
        let (status, unknown) = wire(
            server.address,
            "POST",
            "/v1/survey/working-area-forms/select?project=proj-kigali",
            Some(json!({"forms": ["poles", "not_here"]})),
            TOKEN,
        )
        .await;
        assert_eq!(status, 400, "{unknown}");
        assert_eq!(unknown["code"], "unknown_form");
        assert!(unknown["error"].as_str().unwrap().contains("not_here"));
        assert_eq!(
            ds_layer_store::working_area_forms::read_at(
                &dir.path().join("layers"),
                "canary",
                "uid-a",
                "proj-kigali"
            )
            .unwrap(),
            None
        );

        // A choice persists under the named project only.
        let (status, chosen) = wire(
            server.address,
            "POST",
            "/v1/survey/working-area-forms/select?project=proj-kigali",
            Some(json!({"forms": ["customers"]})),
            TOKEN,
        )
        .await;
        assert_eq!(status, 200, "{chosen}");
        assert_eq!(chosen["changed"], true);
        assert_eq!(chosen["loads"], json!(["customers"]));
        assert_eq!(chosen["persisted"], "native_local");
        let (_, other) = wire(
            server.address,
            "GET",
            "/v1/survey/working-area-forms?project=proj-lome",
            None,
            TOKEN,
        )
        .await;
        assert_eq!(other["chosen"], false, "{other}");
        assert_eq!(other["form_count"], 1);

        // Restart: the choice is retained on disk, not in the process.
        server.handle.abort();
        let server = start(dir.path(), upstream.clone(), allowed.clone()).await;
        let (_, retained) = wire(
            server.address,
            "GET",
            "/v1/survey/working-area-forms?project=proj-kigali",
            None,
            TOKEN,
        )
        .await;
        assert_eq!(retained["chosen"], true, "{retained}");
        assert_eq!(retained["loads"], json!(["customers"]));

        // A source that answers about another project is never applied.
        upstream
            .switch_project_on_read
            .store(true, Ordering::SeqCst);
        let (status, switched) = wire(
            server.address,
            "POST",
            "/v1/survey/working-area-forms/select?project=proj-kigali",
            Some(json!({"all": true})),
            TOKEN,
        )
        .await;
        assert_eq!(status, 409, "{switched}");
        assert_eq!(switched["code"], "project_context_changed");
        upstream
            .switch_project_on_read
            .store(false, Ordering::SeqCst);

        // All, then clear: back to never chosen.
        let (_, all) = wire(
            server.address,
            "POST",
            "/v1/survey/working-area-forms/select?project=proj-kigali",
            Some(json!({"all": true})),
            TOKEN,
        )
        .await;
        assert_eq!(all["loads"], json!(["poles", "customers"]), "{all}");
        let (status, cleared) = wire(
            server.address,
            "POST",
            "/v1/survey/working-area-forms/clear?project=proj-kigali",
            Some(json!({})),
            TOKEN,
        )
        .await;
        assert_eq!(status, 200, "{cleared}");
        assert_eq!(cleared["chosen"], false);
        assert_eq!(cleared["changed"], true);
        let (_, again) = wire(
            server.address,
            "GET",
            "/v1/survey/working-area-forms?project=proj-kigali",
            None,
            TOKEN,
        )
        .await;
        assert_eq!(again["chosen"], false);
        assert_eq!(again["loads"], Value::Array(vec![]));

        // A revoked owner is refused at the door.
        allowed.store(false, Ordering::SeqCst);
        let (status, revoked) = wire(
            server.address,
            "GET",
            "/v1/survey/working-area-forms?project=proj-kigali",
            None,
            TOKEN,
        )
        .await;
        assert_eq!(status, 401, "{revoked}");
        server.handle.abort();
    }
}
