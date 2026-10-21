//! Profile session request, style and projection admission; moved verbatim from profile_sessions.rs.
use super::*;
#[test]
fn closed_schema_refuses_arbitrary_actions_and_extra_fields() {
    for bytes in [br#"{"schema":"ds.core.profile-session/v1","operation":{"action":"run","argv":[]}}"#.as_slice(),
            br#"{"schema":"wrong","operation":{"action":"open","package_base64":"AA==","package_sha256":"a"}}"#,
            br#"{"schema":"ds.core.profile-session/v1","operation":{"action":"open","package_base64":"AA==","package_sha256":"a","path":"/etc/passwd"}}"#] {
            assert!(Request::parse(bytes).is_err());
        }
}
#[test]
fn bulk_update_refusal_preserves_plain_message_and_structured_requirement() {
    let message = "Update Network to edit this terrain batch.";
    let requirement =
        json!({"component":"network", "required_capability":"profile_bulk_controls_v1"});
    let error = native(NetworkFailure {
        code: "network_engine_incompatible",
        message: message.into(),
        owner_error: None,
        owner_code: None,
        owner_detail: Some(Box::new(requirement.clone())),
        owner_remedy: Some(message.into()),
        owner_invalid_input: None,
    });
    assert_eq!(error.code(), "core_profile_native_refused");
    assert_eq!(error.message(), message);
    let detail = error.detail_value().unwrap();
    assert_eq!(detail["native_code"], "network_engine_incompatible");
    assert_eq!(detail["native_detail"], requirement);
    assert_eq!(detail["native_remedy"], message);
}
#[test]
fn only_typed_controls_admit_bulk_requests_up_to_eight_mib() {
    let mut value = json!({"schema":REQUEST_SCHEMA,"operation":{
    "action":"control","session_id":"captured","model_id":"model",
    "expected_history":{},"operation":{"action":"preview_transaction","commands":[{
        "command_id":"one","command":{"owner_payload":"x".repeat(70*1024)}
    }]}}});
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() > 64 * 1024);
    assert!(Request::parse(&bytes).is_ok());
    value["operation"]["operation"]["commands"][0]["command"]["owner_payload"] =
        json!("x".repeat(ds_network_contracts::profile::MAX_PROFILE_CONTROL_INPUT_BYTES));
    assert!(Request::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    let state = json!({"schema":REQUEST_SCHEMA,"operation":{
            "action":"state","session_id":"captured","model_id":"model",
            "expected_history":{"large":"x".repeat(70*1024)}}});
    assert!(Request::parse(&serde_json::to_vec(&state).unwrap()).is_err());
    value["operation"]["operation"]["action"] = json!("unknown");
    assert!(Request::parse(&serde_json::to_vec(&value).unwrap()).is_err());
}
#[test]
fn gis_labels_are_closed_and_bounded() {
    assert!(validate_label_fields(&["station".into(), "number".into(), "number".into()]).is_ok());
    assert!(validate_label_fields(&["arbitrary".into()]).is_err());
    assert!(validate_label_fields(&vec!["number".into(); 65]).is_err());
}
fn style_scope() -> Scope {
    Scope {
        principal: "fixture".into(),
        lane: "canary".into(),
        deployment: "fixture-deployment".into(),
        install: "fixture-install".into(),
        project: "fixture-project".into(),
    }
}
fn style_owner() -> ds_cli_auth::ProviderIdentity {
    ds_cli_auth::ProviderIdentity::new("canary", &"a".repeat(64), "fixture").unwrap()
}
fn authored_style() -> Value {
    serde_json::from_str(include_str!(
        "../../../../ds-command-kernel/tests/fixtures/profile-display-style.json"
    ))
    .unwrap()
}
fn style_catalogue(document: Value) -> StyleCatalogue {
    StyleCatalogue {
        identity: style_owner(),
        lane: "canary".into(),
        project: "fixture-project".into(),
        document: json!({"project_id":"fixture-project","styles":{"ds_grid/profile":document}}),
    }
}
#[test]
fn complete_view_requires_its_capability_and_exact_host_adoption_fences() {
    let history = json!({"model_revision":"revision"});
    let operation = Operation::View {
        session_id: "held".into(),
        model_id: "model".into(),
        expected_history: history.clone(),
        options: json!({}),
        patch: json!({}),
        style_document: Value::Null,
        cached: true,
        observe: false,
    };
    assert_eq!(
        read_capability(&operation),
        Some(NetworkCapability::ProfileViewV1)
    );
    assert!(
        require_read_capability(
            &[NetworkCapability::ProfileRuntimeV1],
            NetworkCapability::ProfileViewV1
        )
        .is_err()
    );
    let display: ds_command_kernel::profile_display::DisplayState = Default::default();
    let good = json!({"model_id":"model","model_revision":"revision","history":history,
            "retained_asset_count":2,"display_state":display});
    assert_eq!(
        validate_view_payload(
            &serde_json::to_vec(&good).unwrap(),
            "model",
            &history,
            2,
            &display
        )
        .unwrap(),
        display
    );
    for key in [
        "model_id",
        "model_revision",
        "history",
        "retained_asset_count",
        "display_state",
    ] {
        let mut bad = good.clone();
        bad[key] = Value::Null;
        assert!(
            validate_view_payload(
                &serde_json::to_vec(&bad).unwrap(),
                "model",
                &history,
                2,
                &display
            )
            .is_err(),
            "{key}"
        );
    }
    let mut bad = good;
    bad["display_state"]["vertical_exaggeration"] = json!(6.);
    assert!(
        validate_view_payload(
            &serde_json::to_vec(&bad).unwrap(),
            "model",
            &history,
            2,
            &display
        )
        .is_err()
    );
}
#[test]
fn view_preparation_refusal_preserves_public_invalid_request_code() {
    let error = |owner: &str| NetworkFailure {
        code: "network_engine_refused",
        message: "invalid authored view".into(),
        owner_error: None,
        owner_code: Some(owner.into()),
        owner_detail: None,
        owner_remedy: None,
        owner_invalid_input: None,
    };
    assert_eq!(
        view_native(error("profile_view_invalid")).code(),
        "core_profile_request_invalid"
    );
    assert_eq!(
        view_native(error("profile_scene_unavailable")).code(),
        "core_profile_native_refused"
    );
    assert_eq!(
        native(error("profile_view_invalid")).code(),
        "core_profile_native_refused"
    );
}
fn physical_style(document: Value) -> Result<(), String> {
    ds_command_kernel::profile_encoding::resolve(&Default::default(), &document)?;
    ds_command_kernel::profile_encoding::review_status_colors(&document)?;
    ds_command_kernel::profile_encoding::review_box_ink(&document)?;
    Ok(())
}
#[test]
fn present_style_never_fetches_or_replaces_malformed_authored_input() {
    for document in [
        json!({}),
        json!({"metadata":{"profile_pens":{"grid":{"width_mm":-1}}}}),
    ] {
        let (actual, capture) = resolve_style(
            &style_scope(),
            document.clone(),
            None,
            &|| panic!("Present style does not acquire authority again"),
            &|| panic!("No native identity read for caller document"),
            &|| panic!("Malformed supplied style is never replaced"),
        )
        .unwrap();
        assert_eq!(actual, document);
        assert!(capture.is_none());
        assert!(physical_style(actual).is_err());
    }
}
#[test]
fn null_style_captures_exact_authored_document_and_repeats_without_fetch() {
    let calls = std::cell::Cell::new(0);
    let mut document = authored_style();
    document["metadata"]["owner_extension"] = json!({"untouched":[1,"exact",false]});
    let fetch = || {
        calls.set(calls.get() + 1);
        Ok(style_catalogue(document.clone()))
    };
    let (actual, candidate) = resolve_style(
        &style_scope(),
        Value::Null,
        None,
        &|| Ok(()),
        &|| Ok(style_owner()),
        &fetch,
    )
    .unwrap();
    assert_eq!(actual, document);
    physical_style(actual.clone()).unwrap(); // Unchanged complete 17-pen/physical owner.
    let mut captured = None;
    adopt_style_capture(&mut captured, candidate, &|| Ok(()), &|| Ok(style_owner())).unwrap();
    let (repeated, _) = resolve_style(
        &style_scope(),
        Value::Null,
        captured.as_ref(),
        &|| Ok(()),
        &|| Ok(style_owner()),
        &fetch,
    )
    .unwrap();
    assert_eq!(repeated, document);
    assert_eq!(calls.get(), 1);
}
#[test]
fn missing_excessive_and_foreign_catalogues_are_not_adopted() {
    let mut candidates = vec![
        style_catalogue(Value::Null),
        style_catalogue(json!({"padding":"x".repeat(MAX_STYLE_BYTES)})),
    ];
    let mut lane = style_catalogue(authored_style());
    lane.lane = "stable".into();
    candidates.push(lane);
    let mut project = style_catalogue(authored_style());
    project.project = "foreign".into();
    candidates.push(project);
    let mut body = style_catalogue(authored_style());
    body.document["project_id"] = json!("foreign");
    candidates.push(body);
    for catalogue in candidates {
        let fetch = || {
            Ok(StyleCatalogue {
                identity: catalogue.identity.clone(),
                lane: catalogue.lane.clone(),
                project: catalogue.project.clone(),
                document: catalogue.document.clone(),
            })
        };
        assert!(
            resolve_style(
                &style_scope(),
                Value::Null,
                None,
                &|| Ok(()),
                &|| Ok(style_owner()),
                &fetch
            )
            .is_err()
        );
    }
}
#[test]
fn delayed_style_read_refuses_changed_uid_lane_or_audience() {
    for changed in [
        ds_cli_auth::ProviderIdentity::new("canary", &"a".repeat(64), "foreign").unwrap(),
        ds_cli_auth::ProviderIdentity::new("stable", &"a".repeat(64), "fixture").unwrap(),
        ds_cli_auth::ProviderIdentity::new("canary", &"b".repeat(64), "fixture").unwrap(),
    ] {
        let current = std::cell::RefCell::new(style_owner());
        let fetch = || {
            *current.borrow_mut() = changed.clone();
            Ok(style_catalogue(authored_style()))
        };
        let error = resolve_style(
            &style_scope(),
            Value::Null,
            None,
            &|| Ok(()),
            &|| Ok(current.borrow().clone()),
            &fetch,
        )
        .err()
        .unwrap();
        assert_eq!(error.code(), "core_profile_styles_unavailable");
    }
}
#[test]
fn missing_or_late_native_authority_never_adopts_a_style() {
    let refused = || {
        Err(Failure::unauthorized(
            "server_owner_changed",
            "fixture authority changed",
        ))
    };
    assert!(
        resolve_style(
            &style_scope(),
            Value::Null,
            None,
            &refused,
            &|| Ok(style_owner()),
            &|| panic!("Signed-out owner must not fetch")
        )
        .is_err()
    );
    let calls = std::cell::Cell::new(0);
    let authorized = || {
        calls.set(calls.get() + 1);
        if calls.get() > 1 { refused() } else { Ok(()) }
    };
    assert!(
        resolve_style(
            &style_scope(),
            Value::Null,
            None,
            &authorized,
            &|| Ok(style_owner()),
            &|| Ok(style_catalogue(authored_style()))
        )
        .is_err()
    );
    let (_, capture) = resolve_style(
        &style_scope(),
        Value::Null,
        None,
        &|| Ok(()),
        &|| Ok(style_owner()),
        &|| Ok(style_catalogue(authored_style())),
    )
    .unwrap();
    let mut adopted = None;
    assert!(
        adopt_style_capture(&mut adopted, capture.clone(), &refused, &|| Ok(
            style_owner()
        ))
        .is_err()
    );
    assert!(adopted.is_none());
    assert!(
        adopt_style_capture(&mut adopted, capture.clone(), &|| Ok(()), &|| {
            ds_cli_auth::ProviderIdentity::new("canary", &"b".repeat(64), "fixture")
        })
        .is_err()
    );
    assert!(
        resolve_style(
            &style_scope(),
            Value::Null,
            capture.as_ref(),
            &|| Ok(()),
            &|| ds_cli_auth::ProviderIdentity::new("canary", &"b".repeat(64), "fixture"),
            &|| panic!("Changed cached audience must not silently refetch")
        )
        .is_err()
    );
}
#[test]
fn simultaneous_intents_coalesce_only_after_physical_and_late_authority_admission() {
    let cache = std::sync::Mutex::new(None::<StyleCapture>);
    let calls = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|threads| {
        for _ in 0..2 {
            threads.spawn(|| {
                // Production Holds this same session-owner mutex through View.
                let mut cached = cache.lock().unwrap();
                let (document, candidate) = resolve_style(
                    &style_scope(),
                    Value::Null,
                    cached.as_ref(),
                    &|| Ok(()),
                    &|| Ok(style_owner()),
                    &|| {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(5));
                        Ok(style_catalogue(authored_style()))
                    },
                )
                .unwrap();
                physical_style(document).unwrap();
                adopt_style_capture(&mut cached, candidate, &|| Ok(()), &|| Ok(style_owner()))
                    .unwrap();
            });
        }
    });
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
#[test]
fn inner_catalogue_owner_aba_is_refused_even_when_ambient_returns_to_a() {
    let (_, capture) = resolve_style(
        &style_scope(),
        Value::Null,
        None,
        &|| Ok(()),
        &|| Ok(style_owner()),
        &|| {
            let mut catalogue = style_catalogue(authored_style());
            catalogue.identity =
                ds_cli_auth::ProviderIdentity::new("canary", &"b".repeat(64), "fixture").unwrap();
            Ok(catalogue)
        },
    )
    .map(|result| (true, result.1))
    .unwrap_or((false, None));
    assert!(capture.is_none());
    for foreign in [
        ds_cli_auth::ProviderIdentity::new("canary", &"b".repeat(64), "fixture").unwrap(),
        ds_cli_auth::ProviderIdentity::new("canary", &"a".repeat(64), "foreign").unwrap(),
    ] {
        let error = resolve_style(
            &style_scope(),
            Value::Null,
            None,
            &|| Ok(()),
            &|| Ok(style_owner()),
            &|| {
                let mut catalogue = style_catalogue(authored_style());
                catalogue.identity = foreign.clone();
                Ok(catalogue)
            },
        )
        .err()
        .unwrap();
        assert_eq!(error.code(), "core_profile_styles_unavailable");
    }
}
#[test]
fn successful_supplied_successor_clears_cache_failed_physical_input_preserves_it() {
    let (_, mut cache) = resolve_style(
        &style_scope(),
        Value::Null,
        None,
        &|| Ok(()),
        &|| Ok(style_owner()),
        &|| Ok(style_catalogue(authored_style())),
    )
    .unwrap();
    let prior = cache.as_ref().unwrap().document.clone();
    let (bad, _) = resolve_style(
        &style_scope(),
        json!({}),
        cache.as_ref(),
        &|| panic!(),
        &|| panic!(),
        &|| panic!(),
    )
    .unwrap();
    assert!(physical_style(bad).is_err());
    assert_eq!(cache.as_ref().unwrap().document, prior);
    let (valid, successor) = resolve_style(
        &style_scope(),
        authored_style(),
        cache.as_ref(),
        &|| panic!(),
        &|| panic!(),
        &|| panic!(),
    )
    .unwrap();
    physical_style(valid).unwrap();
    adopt_style_capture(&mut cache, successor, &|| Ok(()), &|| {
        panic!("Supplied style has no acquired identity")
    })
    .unwrap();
    assert!(cache.is_none());
    let reads = std::cell::Cell::new(0);
    resolve_style(
        &style_scope(),
        Value::Null,
        cache.as_ref(),
        &|| Ok(()),
        &|| Ok(style_owner()),
        &|| {
            reads.set(reads.get() + 1);
            Ok(style_catalogue(authored_style()))
        },
    )
    .unwrap();
    assert_eq!(reads.get(), 1);
}
#[test]
fn native_scope_mismatch_or_fetch_refusal_does_not_use_another_style() {
    let mut scope = style_scope();
    scope.principal = "foreign".into();
    assert!(
        resolve_style(
            &scope,
            Value::Null,
            None,
            &|| Ok(()),
            &|| Ok(style_owner()),
            &|| panic!("Foreign native principal must not fetch")
        )
        .is_err()
    );
    let mut scope = style_scope();
    scope.lane = "stable".into();
    assert!(
        resolve_style(
            &scope,
            Value::Null,
            None,
            &|| Ok(()),
            &|| Ok(style_owner()),
            &|| panic!("Foreign native lane must not fetch")
        )
        .is_err()
    );
    let error = resolve_style(
        &style_scope(),
        Value::Null,
        None,
        &|| Ok(()),
        &|| Ok(style_owner()),
        &|| Err(style_error("Authored catalogue unavailable")),
    )
    .err()
    .unwrap();
    assert_eq!(error.code(), "core_profile_styles_unavailable");
}
#[test]
fn package_digest_is_exact_before_decode_or_process() {
    let bytes = b"synthetic package bytes";
    assert_eq!(
        decode_package(&STANDARD.encode(bytes), &sha(bytes)).unwrap(),
        bytes
    );
    assert!(decode_package(&STANDARD.encode(bytes), &"0".repeat(64)).is_err());
    assert!(decode_package("!", &"0".repeat(64)).is_err());
}
#[test]
fn read_projection_actions_are_closed_typed_and_preserve_ordered_label_fields() {
    let base = |action: &str| json!({"schema":REQUEST_SCHEMA,"operation":{"action":action,"session_id":"captured","model_id":"fixture-model","expected_history":{"model_revision":"head"}}});
    let feature = Request::parse(&serde_json::to_vec(&base("feature_codes")).unwrap()).unwrap();
    assert!(matches!(
        read_projection_operation(&feature.operation, "EPSG:32735").unwrap(),
        Some(NativeProfileOperationV1::FeatureCodes { .. })
    ));
    for table in ["structures", "sections"] {
        let mut request = base("engineering_attributes");
        request["operation"]["table"] = json!(table);
        let parsed = Request::parse(&serde_json::to_vec(&request).unwrap()).unwrap();
        let forwarded = serde_json::to_value(
            read_projection_operation(&parsed.operation, "EPSG:32735")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            forwarded,
            json!({"action":"engineering_attributes","table":table})
        );
    }
    for table in ["unknown", "structure", "Structures", "", "terrain"] {
        let mut request = base("engineering_attributes");
        request["operation"]["table"] = json!(table);
        assert!(Request::parse(&serde_json::to_vec(&request).unwrap()).is_err());
    }
    let mut feature_extra = base("feature_codes");
    feature_extra["operation"]["table"] = json!("structures");
    assert!(Request::parse(&serde_json::to_vec(&feature_extra).unwrap()).is_err());
    let mut gis = base("gis_projection");
    gis["operation"]["label_fields"] = json!(["station", "number", "number"]);
    let parsed = Request::parse(&serde_json::to_vec(&gis).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(
            read_projection_operation(&parsed.operation, "EPSG:32735")
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        json!({"action":"gis_projection","coordinate_system":"EPSG:32735","label_fields":["station","number","number"]})
    );
    let mut caller_crs = gis.clone();
    caller_crs["operation"]["coordinate_system"] = json!("EPSG:4326");
    assert!(Request::parse(&serde_json::to_vec(&caller_crs).unwrap()).is_err());
    gis["operation"]["label_fields"] = json!(["arbitrary"]);
    let parsed = Request::parse(&serde_json::to_vec(&gis).unwrap()).unwrap();
    assert!(read_projection_operation(&parsed.operation, "EPSG:32735").is_err());
}
#[test]
fn old_or_unrelated_engine_capabilities_refuse_the_projection_boundary() {
    for capabilities in [
        vec![],
        vec![
            NetworkCapability::ProfileSessionV1,
            NetworkCapability::ProfileRuntimeV1,
        ],
        vec![NetworkCapability::Unknown("unrelated".into())],
    ] {
        let error =
            require_read_capability(&capabilities, NetworkCapability::ProfileReadProjectionsV1)
                .unwrap_err();
        assert_eq!(error.code(), "core_profile_native_refused");
        assert_eq!(
            error.detail_value().unwrap()["native_code"],
            "network_engine_incompatible"
        );
    }
    require_read_capability(
        &[NetworkCapability::ProfileReadProjectionsV1],
        NetworkCapability::ProfileReadProjectionsV1,
    )
    .unwrap();
}
#[test]
fn properties_is_closed_canonical_and_requires_its_distinct_capability() {
    let base = |entity: Value| json!({"schema":REQUEST_SCHEMA,"operation":{"action":"properties","session_id":"captured","model_id":"fixture-model","expected_history":{"model_revision":"head"},"entity_id":entity}});
    let request = Request::parse(&serde_json::to_vec(&base(json!("str-1"))).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(
            read_projection_operation(&request.operation, "ignored")
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        json!({"action":"properties","entity_id":"str-1"})
    );
    assert_eq!(
        read_capability(&request.operation),
        Some(NetworkCapability::ProfilePropertiesV1)
    );
    for id in [
        "".to_owned(),
        " str-1".into(),
        "str-1 ".into(),
        "str\n1".into(),
        "a".repeat(129),
        "é".repeat(65),
    ] {
        let parsed = Request::parse(&serde_json::to_vec(&base(json!(id))).unwrap()).unwrap();
        assert!(read_projection_operation(&parsed.operation, "").is_err());
    }
    let mut extra = base(json!("str-1"));
    extra["operation"]["mutate"] = json!(true);
    assert!(Request::parse(&serde_json::to_vec(&extra).unwrap()).is_err());
    assert!(Request::parse(&serde_json::to_vec(&base(json!(1))).unwrap()).is_err());
    for caps in [vec![], vec![NetworkCapability::ProfileReadProjectionsV1]] {
        let error =
            require_read_capability(&caps, NetworkCapability::ProfilePropertiesV1).unwrap_err();
        assert_eq!(
            error.detail_value().unwrap()["required_capability"],
            "profile_properties_v1"
        );
    }
    require_read_capability(
        &[NetworkCapability::ProfilePropertiesV1],
        NetworkCapability::ProfilePropertiesV1,
    )
    .unwrap();
}
#[test]
fn invalid_properties_entity_refuses_before_store_or_worker_lookup() {
    let sessions = Sessions::default();
    let error = sessions
        .execute(
            style_scope(),
            Path::new("/absent-properties-fixture"),
            Path::new("/absent-properties-fixture/store.sqlite"),
            Request {
                schema: REQUEST_SCHEMA.into(),
                operation: Operation::Properties {
                    session_id: "not-a-session".into(),
                    model_id: "fixture-model".into(),
                    expected_history: json!({}),
                    entity_id: " invalid".into(),
                },
            },
            &|| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.code(), "core_profile_request_invalid");
    assert_eq!(sessions.held_count_for_test(), 0);
}
#[test]
fn properties_reject_crossed_native_entity_revision_and_schema_before_adoption() {
    let good = json!({"schema_version":1,"entity_id":"str-1","revision":"head","groups":[]});
    validate_properties_payload(&serde_json::to_vec(&good).unwrap(), "str-1", &json!("head"))
        .unwrap();
    for (field, value) in [
        ("schema_version", json!(2)),
        ("entity_id", json!("other")),
        ("revision", json!("old")),
    ] {
        let mut bad = good.clone();
        bad[field] = value;
        assert!(
            validate_properties_payload(
                &serde_json::to_vec(&bad).unwrap(),
                "str-1",
                &json!("head")
            )
            .is_err()
        );
    }
}
#[test]
fn unknown_or_foreign_handles_never_launch_a_process() {
    let sessions = Sessions::default();
    let scope = Scope {
        principal: "fixture".into(),
        lane: "canary".into(),
        deployment: "https://fixture.invalid".into(),
        install: "fixture-install".into(),
        project: "fixture-project".into(),
    };
    let request = Request {
        schema: REQUEST_SCHEMA.into(),
        operation: Operation::Snapshot {
            session_id: "not-a-session".into(),
            model_id: "fixture-model".into(),
            expected_history: Value::Null,
        },
    };
    let refused = sessions
        .execute(
            scope,
            Path::new("/absent"),
            Path::new("/absent/store.sqlite"),
            request,
            &|| Ok(()),
        )
        .unwrap_err();
    assert_eq!(refused.code(), "core_profile_session_unavailable");
}
#[test]
fn selection_is_closed_distinct_cap_and_preserves_exact_requests() {
    let pick = json!({"model_revision":"head","axis_pin_digest":"axis","scene_x":1.,"scene_y":2.,"tolerance_scene_units":3.,"scene_units_per_px":1.,"visibility":{"showGround":true,"showTerrainPoints":true,"showStructures":true,"showWire":false,"showAttachments":false}});
    let range = json!({"model_revision":"head","axis_pin_digest":"axis","from_entity_id":"a","to_entity_id":"b"});
    for (action, query) in [("pick", pick.clone()), ("selection_range", range)] {
        let value = json!({"schema":REQUEST_SCHEMA,"operation":{"action":action,"session_id":"captured","model_id":"model","expected_history":{"model_revision":"head"},"request":query}});
        let parsed = Request::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            read_capability(&parsed.operation),
            Some(NetworkCapability::ProfileSelectionV1)
        );
        assert_eq!(
            serde_json::to_value(
                read_projection_operation(&parsed.operation, "")
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            json!({"action":action,"request":query})
        );
        let mut invalid = value;
        invalid["operation"]["request"]["mutate"] = json!(true);
        assert!(Request::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
    for caps in [
        vec![],
        vec![
            NetworkCapability::ProfileReadProjectionsV1,
            NetworkCapability::ProfilePropertiesV1,
        ],
    ] {
        let error =
            require_read_capability(&caps, NetworkCapability::ProfileSelectionV1).unwrap_err();
        assert_eq!(
            error.detail_value().unwrap()["required_capability"],
            "profile_selection_v1"
        );
    }
    for field in [
        "scene_x",
        "scene_y",
        "tolerance_scene_units",
        "scene_units_per_px",
    ] {
        let mut invalid = pick.clone();
        invalid[field] = Value::Null;
        assert!(Request::parse(&serde_json::to_vec(&json!({"schema":REQUEST_SCHEMA,"operation":{"action":"pick","session_id":"captured","model_id":"model","expected_history":{"model_revision":"head"},"request":invalid}})).unwrap()).is_err());
    }
}
#[test]
fn selection_rejects_crossed_worker_axis_revision_and_result_before_adoption() {
    let operation = Operation::SelectionRange {
        session_id: "captured".into(),
        model_id: "model".into(),
        expected_history: json!({"model_revision":"head"}),
        request: NativeProfileSelectionRangeRequestV1 {
            model_revision: "head".into(),
            axis_pin_digest: "axis".into(),
            from_entity_id: "a".into(),
            to_entity_id: "b".into(),
        },
    };
    let expected = json!({"model_revision":"head","axis_pin_digest":"axis","entity_ids":["a","b"]});
    validate_selection_payload(
        &operation,
        &serde_json::to_vec(&expected).unwrap(),
        &json!("head"),
    )
    .unwrap();
    for (field, value) in [
        ("model_revision", json!("other")),
        ("axis_pin_digest", json!("other")),
        ("entity_ids", json!([1])),
    ] {
        let mut invalid = expected.clone();
        invalid[field] = value;
        assert!(
            validate_selection_payload(
                &operation,
                &serde_json::to_vec(&invalid).unwrap(),
                &json!("head")
            )
            .is_err()
        );
    }
}
