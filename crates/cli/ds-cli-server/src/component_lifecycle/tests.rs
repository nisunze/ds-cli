use super::*;
use std::cell::Cell;
use std::sync::Arc;
use tower::ServiceExt;

fn identity() -> ds_compute_runtime::HostIdentity {
    ds_compute_runtime::HostIdentity {
        owner: "fixture-owner".into(),
        principal: ds_command_kernel::execution_context::Principal {
            uid: "fixture-user".into(),
            lane: "canary".into(),
            deployment: "https://fixture.example".into(),
            install_id: "fixture-install".into(),
        },
    }
}
fn request(activate: bool, generation: u64) -> Request {
    if activate {
        Request::Activate(ActivateRequest {
            component: ModuleComponent::Network,
            channel: "canary".into(),
            expected_generation: generation,
            receipt_id: "a".repeat(32),
        })
    } else {
        Request::Rollback(RollbackRequest {
            component: ModuleComponent::Network,
            channel: "canary".into(),
            expected_generation: generation,
        })
    }
}
#[test]
fn lifecycle_contracts_confirm_closed_inputs_and_bound_generation() {
    for command in [&ACTIVATE, &ROLLBACK] {
        assert_eq!(command.effect, Effect::MachineWrite);
        assert_eq!(command.authority, Authority::None);
        assert!(command.requires_confirmation());
        assert_eq!(command.contract, 2);
        assert!(crate::DOMAIN.commands.iter().any(|c| c.id == command.id));
        let mut arguments = vec![
            "--component",
            "network",
            "--channel",
            "canary",
            "--expected-generation",
            "0",
        ];
        if command.id == ACTIVATE.id {
            arguments.extend(["--receipt-id", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]);
        }
        let inputs = ds_cli_contract::parse(
            command,
            &arguments
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(from_inputs(&inputs, command.id == ACTIVATE.id).is_ok());
        arguments.extend(["--idle", "true"]);
        assert!(
            ds_cli_contract::parse(
                command,
                &arguments
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect::<Vec<_>>()
            )
            .is_err()
        );
    }
    for generation in ["-1", "+1", "1.0", "18446744073709551616"] {
        let arguments = [
            "--component",
            "network",
            "--channel",
            "canary",
            "--expected-generation",
            generation,
        ]
        .map(str::to_owned);
        let inputs = ds_cli_contract::parse(&ROLLBACK, &arguments).unwrap();
        assert_eq!(
            from_inputs(&inputs, false).err().unwrap().code(),
            "component_generation_invalid"
        );
    }
}
#[test]
fn malformed_and_cross_lane_requests_stop_before_database_or_store_effects() {
    for activate in [true, false] {
        for body in [
            b"{}".to_vec(),
            vec![b' '; MAX_REQUEST_BYTES + 1],
            br#"{"component":"network","channel":"canary","expectedGeneration":0,"idle":true}"#
                .to_vec(),
        ] {
            assert_eq!(
                decode_request(&body, activate).err().unwrap().code(),
                "component_request_invalid"
            );
        }
        let base = ds_cli_contract::testing::private_tempdir(".ds-lifecycle-fixture-");
        let database = base.path().join("absent.sqlite");
        let mut crossed = request(activate, 0);
        match &mut crossed {
            Request::Activate(r) => r.channel = "stable".into(),
            Request::Rollback(r) => r.channel = "stable".into(),
        }
        let opened = Cell::new(false);
        let error = execute(&database, &identity(), crossed, |_, _| {
            opened.set(true);
            panic!("crossed lane reached store")
        })
        .unwrap_err();
        assert_eq!(error.code(), "component_runtime_identity_mismatch");
        assert!(!opened.get());
        assert!(!database.exists());
    }
    for receipt in [
        "../selection.json",
        "A0000000000000000000000000000000",
        "",
        "a",
    ] {
        let mut value = serde_json::from_slice::<Value>(&request(true, 0).body().unwrap()).unwrap();
        value["receiptId"] = receipt.into();
        assert_eq!(
            decode_request(&serde_json::to_vec(&value).unwrap(), true)
                .err()
                .unwrap()
                .code(),
            "component_request_invalid"
        );
    }
}
#[test]
fn absent_previous_and_corrupt_receipts_preserve_state() {
    let base = ds_cli_contract::testing::private_tempdir(".ds-lifecycle-fixture-");
    let runtime = ds_cli_contract::testing::private_tempdir(".ds-lifecycle-fixture-");
    let database = runtime.path().join("store.sqlite");
    let open = |component, channel: &str| ModuleStore::new(base.path(), component, channel);
    let store = open(ModuleComponent::Network, "canary").unwrap();
    assert_eq!(
        execute(&database, &identity(), request(false, 0), open)
            .unwrap_err()
            .code(),
        "module_rollback_unavailable"
    );
    assert!(store.selection_record().unwrap().is_none());
    let receipt = base
        .path()
        .join("network/canary/receipts")
        .join(format!("{}.json", "a".repeat(32)));
    std::fs::write(&receipt, b"{corrupt").unwrap();
    assert_eq!(
        execute(&database, &identity(), request(true, 0), open)
            .unwrap_err()
            .code(),
        "module_receipt_invalid"
    );
    assert_eq!(std::fs::read(receipt).unwrap(), b"{corrupt");
    assert!(store.selection_record().unwrap().is_none());
    let removed = store
        .remove(
            0,
            ds_command_kernel::module_install::ActivationGuard::default(),
            &|| false,
        )
        .unwrap();
    assert_eq!(
        execute(&database, &identity(), request(false, 1), open)
            .unwrap_err()
            .code(),
        "module_removed"
    );
    assert_eq!(
        serde_json::to_value(store.selection_record().unwrap()).unwrap(),
        serde_json::to_value(removed).unwrap()
    );
}
#[test]
fn old_or_malformed_runtime_replies_do_not_establish_activation() {
    for (status, body, expected) in [
        (
            404,
            br#"{"error":"unknown route"}"#.as_slice(),
            "component_runtime_unavailable",
        ),
        (
            409,
            br#"{"code":"module_selection_changed","error":"stale"}"#.as_slice(),
            "module_selection_changed",
        ),
        (
            503,
            br#"{"code":"module_rollback_unavailable","error":"no previous"}"#.as_slice(),
            "module_rollback_unavailable",
        ),
        (
            200,
            br#"{"state":"ready"}"#.as_slice(),
            "module_receipt_invalid",
        ),
    ] {
        let error = decode_reply(
            crate::transport::Reply {
                status,
                body: body.to_vec(),
            },
            &request(true, 0),
        )
        .unwrap_err();
        assert_eq!(error.code(), expected);
        assert!(error.remedy_text().is_some());
    }
}
struct LocalOwner;
impl ds_compute_runtime::Authorizer for LocalOwner {
    fn authorize(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}
fn app(path: &Path) -> crate::host::App {
    let identity = identity();
    let connection = crate::host::Connection {
        owner: identity.owner.clone(),
        lane: "canary".into(),
        socket: crate::transport::socket_path(path),
        legacy_address: None,
    };
    let database = path.join("store.sqlite");
    let sessions = crate::server_sync::sessions::ServerSessions::with(
        connection.clone(),
        database.clone(),
        crate::host::tests::limits(),
        identity,
        Arc::new(crate::host::tests::NoGateway),
    );
    crate::host::App {
        database,
        connection,
        os_account: Some(crate::transport::Account::Unix(crate::host::tests::OS_UID)),
        auth: Arc::new(LocalOwner),
        requests: Arc::new(crate::host::Door::new(4)),
        activity: None,
        sessions,
        solar: Arc::new(crate::solar_application::Applications::default()),
        profiles: Arc::new(crate::profile_sessions::Sessions::default()),
        layers: crate::layers::NativeLayerHost::fixture_native("canary"),
    }
}
async fn route(
    app: crate::host::App,
    request: &Request,
    body: Vec<u8>,
    owned: bool,
) -> (u16, Value) {
    let mut native = axum::http::Request::builder()
        .method("POST")
        .uri(request.route())
        .body(axum::body::Body::from(body))
        .unwrap();
    if owned {
        native
            .extensions_mut()
            .insert(crate::host::tests::OWNER_PEER);
    }
    let response = crate::host::router(app).oneshot(native).await.unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
async fn actual_routes_refuse_foreign_peers_excessive_requests_and_uncaptured_jobs() {
    let runtime = ds_cli_contract::testing::private_tempdir(".ds-lifecycle-fixture-");
    let app = app(runtime.path());
    let store = ds_compute_runtime::open(&app.database).unwrap();
    let legacy = serde_json::json!({"id":"a".repeat(64),"owner":identity().owner,"lane":"canary","input_sha256":"b".repeat(64),"engine":"lv_processing","input_tag":"ds.lv.process/v1","phase":"queued","attempts":0,"created_at_ms":1,"updated_at_ms":1,"worker":null,"lease_until_ms":0,"result_sha256":null,"error":null});
    let row = serde_json::to_string(&legacy).unwrap();
    store.connection_for_test().execute("INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES(?1,?2,'canary','queued',0,1,?3,?4)", rusqlite::params!["a".repeat(64), identity().owner, row, b"retained input".as_slice()]).unwrap();
    for activate in [true, false] {
        let request = request(activate, 0);
        let (status, reply) = route(app.clone(), &request, request.body().unwrap(), false).await;
        assert_eq!(status, 401);
        assert_eq!(reply["code"], "server_peer_refused");
        let (status, reply) = route(app.clone(), &request, request.body().unwrap(), true).await;
        assert_eq!(status, 503);
        assert_eq!(reply["code"], "component_uncaptured_jobs");
        let (status, reply) = route(
            app.clone(),
            &request,
            vec![b' '; MAX_REQUEST_BYTES + 1],
            true,
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(reply["code"], "component_request_invalid");
    }
    assert_eq!(
        store
            .connection_for_test()
            .query_row("SELECT row FROM compute_jobs", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        row
    );
}

#[test]
#[ignore = "requires an explicitly selected qualified Network stage tree; copies it into disposable stores"]
fn qualified_staged_network_activates_reopens_and_rolls_back_without_probes() {
    let source = std::path::PathBuf::from(
        std::env::var_os("DS_COMPONENT_LIFECYCLE_STAGE_TREE")
            .expect("select qualified Network canary tree"),
    );
    let base = ds_cli_contract::testing::private_tempdir(".ds-lifecycle-fixture-");
    let runtime = ds_cli_contract::testing::private_tempdir(".ds-lifecycle-fixture-");
    let database = runtime.path().join("store.sqlite");
    let store = ModuleStore::new(base.path(), ModuleComponent::Network, "canary").unwrap();
    let root = base.path().join("network/canary");
    let first_id = "750a5e5202fb4b66b49ba3fa3dfaea4a";
    let mut staged: Value = serde_json::from_slice(
        &std::fs::read(source.join("receipts").join(format!("{first_id}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(staged["expectedGeneration"], 0);
    let digest = staged["installed"]["release"]["assets"][0]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let version = root.join("versions").join(&digest);
    std::fs::create_dir(&version).unwrap();
    std::fs::copy(
        source.join("versions").join(&digest).join("ds-network"),
        version.join("ds-network"),
    )
    .unwrap();
    std::fs::write(
        root.join("receipts").join(format!("{first_id}.json")),
        serde_json::to_vec(&staged).unwrap(),
    )
    .unwrap();
    let open = |component, channel: &str| ModuleStore::new(base.path(), component, channel);
    let activation = |id: &str, generation| {
        Request::Activate(ActivateRequest {
            component: ModuleComponent::Network,
            channel: "canary".into(),
            receipt_id: id.into(),
            expected_generation: generation,
        })
    };
    let error = execute(&database, &identity(), activation(first_id, 1), open).unwrap_err();
    assert_eq!(error.code(), "module_selection_changed");
    assert!(store.selection_record().unwrap().is_none());
    let first = execute(&database, &identity(), activation(first_id, 0), open).unwrap();
    assert_eq!(first["generation"], 1);
    assert_eq!(first["active"]["release"]["assets"][0]["sha256"], digest);
    let selection_path = root.join("selection.json");
    let before = std::fs::read(&selection_path).unwrap();
    assert_eq!(
        execute(&database, &identity(), request(false, 1), open)
            .unwrap_err()
            .code(),
        "module_rollback_unavailable"
    );
    assert_eq!(std::fs::read(&selection_path).unwrap(), before);
    let second_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    staged["receiptId"] = second_id.into();
    staged["expectedGeneration"] = 1.into();
    std::fs::write(
        root.join("receipts").join(format!("{second_id}.json")),
        serde_json::to_vec(&staged).unwrap(),
    )
    .unwrap();
    assert_eq!(
        execute(&database, &identity(), activation(second_id, 1), open).unwrap()["generation"],
        2
    );
    let rolled = execute(&database, &identity(), request(false, 2), open).unwrap();
    assert_eq!(rolled["generation"], 3);
    assert_eq!(rolled["active"], first["active"]);
    assert_eq!(
        serde_json::to_value(store.selection_record().unwrap()).unwrap(),
        rolled
    );
    let reply = crate::transport::Reply {
        status: 200,
        body: serde_json::to_vec(&rolled).unwrap(),
    };
    assert_eq!(decode_reply(reply, &request(false, 2)).unwrap(), rolled);
    assert_eq!(
        execute(&database, &identity(), activation(first_id, 0), open)
            .unwrap_err()
            .code(),
        "module_selection_changed"
    );
    assert!(version.join("ds-network").is_file());
}
