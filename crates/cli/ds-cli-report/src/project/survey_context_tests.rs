use super::*;
use serde_json::json;

#[test]
fn selected_survey_geometry_uses_the_complete_hold_without_photo_authority() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope {
        principal: "user-a".into(),
        project: "project-a".into(),
    };
    let identity = ds_cli_auth::ProviderIdentity::new("stable", &"a".repeat(64), "user-a").unwrap();
    let row = json!({"type":"Feature","geometry":{"type":"Point","coordinates":[29.5,-2.0]},
        "properties":{"id":"pole-1","photo":"unavailable-photo", "metadata":{"firestore_updated_at":"2026-09-30T00:00:00Z","is_deleted":false}}});
    store::refresh(
        dir.path(),
        &scope,
        "lv_poles_as_built",
        &Filter::default(),
        Refresh::Full,
        ds_command_kernel::time::OffsetDateTime::now_utc(),
        |_| {
            Ok::<_, String>(
                format!(
                    "{row}\n{}\n",
                    json!({"__type":"summary","form_count":1,"errors":[],"total_features":1})
                )
                .into_bytes(),
            )
        },
    )
    .unwrap();
    let selected = [PrintContextLayer {
        id: "survey_existing_poles".into(),
        label: "Surveyed poles".into(),
        source: PrintContextSource::Survey {
            form: "lv_poles_as_built".into(),
        },
    }];
    let captured = load_at(
        dir.path(),
        "stable",
        &scope,
        &identity,
        &selected,
        "local",
        &mut super::super::hold::Link::default(),
    )
    .unwrap();
    assert_eq!(captured[0].collection["features"], json!([row]));
    assert_eq!(captured[0].source["source_project"], "project-a");
    let foreign = Scope {
        principal: "user-b".into(),
        ..scope.clone()
    };
    let refused = load_at(
        dir.path(),
        "stable",
        &foreign,
        &identity,
        &selected,
        "local",
        &mut super::super::hold::Link::default(),
    )
    .unwrap_err();
    assert_eq!(refused.code(), "print_context_invalid");
}

#[test]
fn production_capture_uses_survey_layer_root_when_geographic_root_differs() {
    const PROBE: &str = "DS_SURVEY_PRINT_ROOT_PROBE";
    let scope = Scope {
        principal: "user-a".into(),
        project: "project-a".into(),
    };
    let identity = ds_cli_auth::ProviderIdentity::new("stable", &"a".repeat(64), "user-a").unwrap();
    let selected = [PrintContextLayer {
        id: "survey_existing_poles".into(),
        label: "Surveyed poles".into(),
        source: PrintContextSource::Survey {
            form: "lv_poles_as_built".into(),
        },
    }];
    if std::env::var_os(PROBE).is_some() {
        let actual_root = ds_layer_store::default_root().unwrap();
        let geographic_root = ds_report_host::shared_root().unwrap();
        assert_ne!(actual_root, geographic_root);
        assert!(
            store::printing_layers(&geographic_root, &scope, &selected).is_err(),
            "the geographic root must not accidentally contain the survey hold"
        );
        let captured = load(
            "stable",
            &scope,
            &identity,
            &selected,
            "local",
            &mut super::super::hold::Link::default(),
        )
        .unwrap();
        assert_eq!(captured[0].source["declared_count"], 1);
        assert_eq!(
            captured[0].collection["features"][0]["properties"]["id"],
            "pole-a"
        );
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let survey_root = dir.path().join("survey-layers");
    let geographic_data = dir.path().join("geographic-data");
    let row = json!({"type":"Feature","geometry":{"type":"Point","coordinates":[29.5,-2.0]},
        "properties":{"id":"pole-a","metadata":{"firestore_updated_at":"2026-09-30T00:00:00Z","is_deleted":false}}});
    store::refresh(
        &survey_root,
        &scope,
        "lv_poles_as_built",
        &Filter::default(),
        Refresh::Full,
        ds_command_kernel::time::OffsetDateTime::now_utc(),
        |_| {
            Ok::<_, String>(
                format!(
                    "{row}\n{}\n",
                    json!({"__type":"summary","form_count":1,"errors":[],"total_features":1})
                )
                .into_bytes(),
            )
        },
    )
    .unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "project::survey_context::tests::production_capture_uses_survey_layer_root_when_geographic_root_differs", "--nocapture"])
        .env(PROBE, "child")
        .env("DS_LAYER_HOME", &survey_root)
        .env("XDG_DATA_HOME", &geographic_data)
        .env("LOCALAPPDATA", &geographic_data)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
