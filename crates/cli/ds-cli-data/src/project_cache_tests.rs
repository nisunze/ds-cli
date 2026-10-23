#[test]
#[ignore = "requires a built compatible Network engine and immutable package fixture"]
fn acceptance_native_mv_projection_local_route_context() {
    let path = std::env::var("DS_MV_PROJECTION_MODEL").unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let digest = ds_command_kernel::report_export::sha256_hex(&bytes);
    let (routes, source) = super::local_mv_route_extents(&path, "stable").unwrap();
    assert!(!routes.is_empty());
    assert!(routes.iter().all(|route| {
        route
            .name
            .starts_with(&format!("mv:local-{}:", &digest[..16]))
    }));
    assert_eq!(source["sha256"], format!("sha256:{digest}"));
    assert_eq!(source["revision_id"], format!("local:sha256:{digest}"));
    assert_eq!(source["route_segments"], routes.len());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
#[test]
#[ignore = "requires an explicitly unavailable captured Network engine"]
fn acceptance_native_mv_missing_engine_is_unavailable() {
    let path = std::env::var("DS_MV_PROJECTION_MODEL").unwrap();
    let error = super::local_mv_route_extents(&path, "stable").unwrap_err();
    assert_eq!(
        error.class(),
        ds_cli_contract::outcome::ExitClass::Unavailable
    );
    assert_eq!(error.code(), "network_engine_missing");
}
use super::*;
use ds_command_kernel::project_dataset_cache::CONTOURS_DATASET_ID;

#[test]
fn retained_retired_digests_refuse_without_blocking_other_historical_rooms() {
    let rooms = BTreeMap::from([
        (
            "old-school-digest".into(),
            json!({"parameters":{"layer":"elementary_school"}}),
        ),
        (
            "old-power-digest".into(),
            json!({"parameters":{"layer":"powerlines"}}),
        ),
        (
            "operator-digest".into(),
            json!({"parameters":{"layer":"operator_layer"}}),
        ),
    ]);
    for (requested, alternatives) in [
        ("old-school-digest", "primary_schools, secondary_schools"),
        ("elementary_school", "primary_schools, secondary_schools"),
        ("old-power-digest", "hv_line, mv_line, lv_line"),
        ("powerlines", "hv_line, mv_line, lv_line"),
    ] {
        let error = resolve_explicit(requested, &[], &rooms).unwrap_err();
        assert_eq!(error.code(), "dataset_retired");
        assert!(error.message().contains(alternatives));
    }
    assert_eq!(
        resolve_explicit("operator-digest", &[], &rooms).unwrap(),
        ("operator-digest".into(), None)
    );
    let active: Vec<_> = rooms
        .iter()
        .filter(|(id, room)| policy::retired_dataset_layer(&room_entry(id, room)).is_none())
        .map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(active, ["operator-digest"]);
    assert_eq!(rooms.len(), 3);
    let other_country = ds_project_data::ReferenceResource {
        id: "other-country-power-digest".into(),
        layer: "powerlines".into(),
        country: "Burkina Faso".into(),
        ..Default::default()
    };
    assert_eq!(
        resolve_explicit("powerlines", &[other_country], &rooms).unwrap(),
        (
            "other-country-power-digest".into(),
            Some(json!({
                "requested":"powerlines","authority":{"id":"other-country-power-digest","layer":"powerlines"}
            }))
        )
    );
}

#[test]
fn compact_status_keeps_cloud_discovery_without_coverage_or_query_history() {
    let summary = compact_status(json!({
        "project": "p",
        "seeded": 1,
        "available": 2,
        "catalog": {"read": true},
        "datasets": [{
            "dataset_id": "parcel-digest",
            "label": "Land parcels (UPI)",
            "provider": "bigquery",
            "residency": "cloud",
            "parameters": {"layer": "rwanda_upi_parcels"},
            "seeded": false,
            "ready": false,
            "ready_reason": "cloud_resident",
            "local_holding": false,
            "feature_count": 0,
            "requested": {"cells": []},
            "completed": {"cells": []},
            "stale": {"cells": []},
            "queries": [{"area": {"large": "history"}}]
        }, {
            "dataset_id": "customers-digest",
            "label": "EDCL customers",
            "provider": "bigquery",
            "residency": "cloud",
            "parameters": {"layer": "edcl_customers"},
            "seeded": true,
            "ready": true,
            "local_holding": true,
            "feature_count": 12,
            "requested": {"cells": [[0,0,1,1],[1,1,2,2]]},
            "completed": {"cells": [[0,0,1,1]]},
            "stale": {"cells": [[0,0,1,1]]},
            "queries": [{"area": {"large": "history"}}]
        }]
    }));
    assert_eq!(summary["view"], "summary");
    assert_eq!(summary["seeded"], 1);
    assert_eq!(summary["datasets"][0]["layer"], "rwanda_upi_parcels");
    assert_eq!(summary["datasets"][0]["source"], "bigquery");
    assert_eq!(summary["datasets"][0]["read_command"], "data.parcels.query");
    assert_eq!(summary["datasets"][0]["seed_available"], true);
    assert_eq!(
        summary["datasets"][0]["seed_command"],
        "data.project-cache.seed"
    );
    assert_eq!(summary["datasets"][0]["row_cap"], 5000);
    assert_eq!(
        summary["datasets"][1]["read_command"],
        "data.customers.query"
    );
    assert_eq!(summary["datasets"][1]["requested_cells"], 2);
    assert_eq!(summary["datasets"][1]["completed_cells"], 1);
    assert_eq!(summary["datasets"][1]["stale_cells"], 1);
    assert!(summary["datasets"][0].get("queries").is_none());
    assert!(summary["datasets"][1].get("requested").is_none());
}

#[test]
fn reading_is_free_and_acquiring_is_confirmed() {
    // Status must never be able to spend provider cost, and seeding must
    // never be able to run without an explicit human decision.
    assert_eq!(STATUS_COMMAND.effect, Effect::ReadOnly);
    assert!(!STATUS_COMMAND.effect.needs_confirmation());
    assert_eq!(SEED_COMMAND.effect, Effect::ArtifactWrite);
    assert!(SEED_COMMAND.effect.needs_confirmation());
    assert!(
        SEED_COMMAND
            .refusals
            .iter()
            .any(|r| r.code == "confirmation_required")
    );
}

/// Both commands are background project work under an explicit address.
/// The gateway still decides membership; the saved selection is unused.
#[test]
fn project_address_is_required_without_a_map_app() {
    for command in [&STATUS_COMMAND, &SEED_COMMAND] {
        assert_eq!(command.authority, Authority::HeadlessProject);
        let names: Vec<&str> = command.args.iter().map(|arg| arg.name).collect();
        assert!(names.contains(&"lane"), "{} lost its lane", command.id);
        assert!(
            command
                .args
                .iter()
                .any(|arg| arg.name == "project" && arg.required)
        );
        for forbidden in ["desktop-descriptor", "url", "action", "body"] {
            assert!(
                !names.contains(&forbidden),
                "{} accepts a {forbidden} override",
                command.id
            );
        }
    }
}

/// Only the explicit flag reaches `ds_project_data::refresh`: it is a
/// switch with no default, an ordinary seed parses with it off, and the
/// receipt reports which door ran.
#[test]
fn only_an_explicit_refresh_flag_selects_the_refresh_door() {
    use ds_cli_contract::spec::ArgKind;
    let arg = SEED_COMMAND
        .args
        .iter()
        .find(|arg| arg.name == "refresh")
        .expect("seed declares --refresh");
    assert_eq!(arg.kind, ArgKind::Switch);
    assert!(!arg.required);
    assert_eq!(arg.default, None);
    let parse = |tokens: &[&str]| {
        let tokens: Vec<String> = tokens.iter().map(|token| token.to_string()).collect();
        ds_cli_contract::args::parse(&SEED_COMMAND, &tokens).expect("seed inputs parse")
    };
    assert!(!parse(&["--project", "p"]).switch("refresh"));
    assert!(!parse(&["--project", "p", "--dataset", "google_open_buildings"]).switch("refresh"));
    assert!(parse(&["--project", "p", "--refresh"]).switch("refresh"));
    // A refresh re-acquires, so it is the same confirmed effect as seed.
    assert!(SEED_COMMAND.effect.needs_confirmation());
    assert!(SEED_COMMAND.output.contains("`refresh`"));
    let rendered = render_seed(&json!({"project": "p", "refresh": true, "datasets": []}));
    assert!(
        rendered.starts_with("refreshed 0 dataset(s) for p"),
        "{rendered}"
    );
    let rendered = render_seed(&json!({"project": "p", "refresh": false, "datasets": []}));
    assert!(
        rendered.starts_with("seeded 0 dataset(s) for p"),
        "{rendered}"
    );
}

/// The descriptor and the executor say the same thing about an omitted
/// `--dataset`: first use on any computer seeds what the project declares.
#[test]
fn an_omitted_dataset_declares_the_first_use_behaviour() {
    let dataset = SEED_COMMAND
        .args
        .iter()
        .find(|arg| arg.name == "dataset")
        .expect("seed declares its dataset argument");
    assert!(!dataset.required, "the dataset is optional by contract");
    let summary = dataset.summary.to_lowercase();
    assert!(summary.contains("declares"), "{}", dataset.summary);
    assert!(
        summary.contains("holding none") || summary.contains("holds none"),
        "the first-use case is the one an operator hits first: {}",
        dataset.summary,
    );
    assert!(SEED_COMMAND.purpose.contains("plus what it holds"));
    assert!(!SEED_COMMAND.purpose.contains("everything published"));
}

/// Every failure the holdings crate can answer with maps onto a refusal
/// the seed command declares, under the crate's own code.
#[test]
fn every_holdings_failure_is_a_declared_seed_refusal() {
    use ds_project_data::Failure as Cause;
    for cause in [
        Cause::NotHeld("x".into()),
        Cause::AcquisitionFailed("x".into()),
        Cause::ProviderUnavailable("x".into()),
        Cause::LocalResourceRequired("x".into()),
        Cause::BundleUnavailable("x".into()),
        Cause::Unsupported("x".into()),
        Cause::TooLarge("x".into()),
        Cause::CatalogInvalid("x".into()),
        Cause::Store("x".into()),
    ] {
        let failure = refused(cause.clone());
        let declared = SEED_REFUSALS.iter().any(|r| r.code == failure.code());
        let mapped_to_scope = failure.code() == cause.code()
            || failure.code() == INVALID_SCOPE.code
            || failure.code() == CATALOG_UNAVAILABLE.code;
        assert!(
            declared || mapped_to_scope,
            "{} → {}",
            cause.code(),
            failure.code()
        );
    }
}

/// Contour intervals travel from the dataset document the room was opened
/// with — never from a default of this host's own.
#[test]
fn derived_context_requires_local_input_and_never_calls_brain_compute() {
    let dataset = ds_project_data::contours_dataset(ds_project_data::ContourSettings {
        minor_interval_m: 10,
        index_interval_m: 50,
        sample_spacing_m: 10.0,
    });
    let provider = CliProvider {
        lane: "unconfigured-native-lane",
        project: "fixture-project",
    };
    let mut calls = 0;
    for selected in [dataset.clone(), ds_project_data::buildings_dataset()] {
        let error = provider
            .acquire_with(&selected, &json!({}), |_| {
                calls += 1;
                Err(Failure::failed(
                    "auth_input_invalid",
                    "no remote door is admitted",
                ))
            })
            .unwrap_err();
        assert_eq!(error.code(), "project_dataset_local_source_required");
        if selected.id == CONTOURS_DATASET_ID {
            assert!(error.message().contains("minor_interval_m\":10"));
            assert!(error.message().contains("index_interval_m\":50"));
            assert!(error.message().contains("sample_spacing_m\":10.0"));
        } else {
            assert!(error.message().contains("Google"));
        }
    }
    assert_eq!(
        calls, 0,
        "the actual provider dispatch never constructs a Brain compute request"
    );
    assert_eq!(
        dataset,
        ds_project_data::contours_dataset(ds_project_data::ContourSettings {
            minor_interval_m: 10,
            index_interval_m: 50,
            sample_spacing_m: 10.0,
        }),
        "the legacy dataset address and authored intervals remain intact"
    );
}

#[test]
fn incomplete_seed_envelope_keeps_the_partial_result_and_refuses_success() {
    let receipt = json!({"project":"p","failed":1,"complete":false,"datasets":[
        {"dataset_id":"held","covered":true,"feature_count":12},
        {"dataset_id":"google_open_buildings","covered":false,"error":"project_dataset_local_source_required: missing input"}]});
    let error = finish_seed(receipt.clone()).unwrap_err();
    assert_eq!(error.code(), "project_dataset_seed_incomplete");
    assert_eq!(error.detail_value().unwrap()["result"], receipt);
    let complete = json!({"failed":0,"complete":true,"datasets":[]});
    assert_eq!(finish_seed(complete.clone()).unwrap(), complete);
}

/// A seed that could not finish every dataset renders as partial.
#[test]
fn a_partial_seed_never_renders_as_a_complete_one() {
    let data = serde_json::json!({
        "project": "p",
        "failed": 1,
        "complete": false,
        "datasets": [
            {"dataset_id":"google_open_buildings","clusters":1,"acquired":1,"feature_count":12,
             "local_holding":true,"warnings":[]},
            {"dataset_id":"id-roads","clusters":0,"acquired":0,"feature_count":0,
             "local_holding":false,"warnings":[],
             "error":"This resource is not installed on this computer."},
        ],
    });
    let rendered = render_seed(&data);
    assert!(rendered.contains("held on this computer"), "{rendered}");
    assert!(
        rendered.contains("not acquired: This resource is not installed on this computer."),
        "{rendered}",
    );
    assert!(
        rendered.contains("1 dataset(s) did not complete"),
        "{rendered}"
    );
    assert!(rendered.contains("google_open_buildings"), "{rendered}");
}

#[test]
fn status_render_separates_requested_from_completed() {
    let rendered = render_status(&json!({
        "project": "p",
        "seeded": 1,
        "catalog": {"read": true},
        "datasets": [{
            "dataset_id": "google_open_buildings",
            "seeded": true,
            "feature_count": 12,
            "index_state": "ready",
            "requested": {"cells": [[0,0,1,1],[2,2,3,3]]},
            "completed": {"cells": [[0,0,1,1]]},
            "source_version": "v1",
            "buffer_policy": {"design_buffer_m": 500.0, "isolated_buffer_m": 1000.0,
                "isolated_rule": "point_only", "provisional": true},
            "last_error": "the provider was unreachable"
        }]
    }));
    assert!(rendered.contains("1 covered area(s) of 2 requested"));
    assert!(rendered.contains("(provisional)"));
    assert!(rendered.contains("last error: the provider was unreachable"));
}

#[test]
fn status_render_names_obsolete_coverage_and_unread_catalogues() {
    let rendered = render_status(&json!({
        "project": "p",
        "seeded": 1,
        "catalog": {"read": false, "reason": "data_distribution_unavailable"},
        "datasets": [{
            "dataset_id": "google_open_buildings",
            "seeded": true,
            "feature_count": 12,
            "index_state": "ready",
            "requested": {"cells": [[0,0,1,1]]},
            "completed": {"cells": [[0,0,1,1]]},
            "source_version": "v1",
            "available_version": "v2",
            "stale": {"cells": [[0,0,1,1]]},
            "expired_queries": 2,
        }, {
            "dataset_id": "roads", "seeded": false, "feature_count": 0, "index_state": "absent",
            "requested": {"cells": []}, "completed": {"cells": []},
        }]
    }));
    assert!(rendered.contains("1 held area(s) completed under a superseded version"));
    assert!(rendered.contains("provider now publishes v2"));
    assert!(rendered.contains("2 abandoned acquisition(s) expired"));
    assert!(rendered.contains("catalogue not read (data_distribution_unavailable)"));
    assert!(rendered.contains("not seeded on this computer"));
}
