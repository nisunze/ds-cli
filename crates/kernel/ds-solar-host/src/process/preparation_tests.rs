use super::*;
use crate::preparation::{AddonPreparation, PreparationTasks};
use ds_solar_contracts::{
    CityIdentity, ReferencePin, RunOptions, SnapshotRevision, SolarCityInput, SolarErrorCode,
};

fn city() -> SolarCityInput {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/solar/aderm_bere/city.snapshot.json");
    let snapshot = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: "ds-solar-fixtures".into(),
            root: "solar".into(),
            city_id: "aderm_bere".into(),
            display_name: "Bere".into(),
        },
        SnapshotRevision {
            source: "frozen-fixture".into(),
            captured_at: "2026-08-08T00:00:00Z".into(),
            record_updated_at_ms: None,
            revision: None,
        },
        RunOptions::default(),
        &snapshot,
    )
    .unwrap()
}

#[cfg(unix)]
#[test]
fn old_addon_refuses_update_only_when_preparation_is_requested() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let script = root.path().join("solar");
    let marker = root.path().join("unexpected-task");
    let identity = serde_json::json!({"name":"ds-solar-engine","version":"old","schemas":["ds-solar.prepared-calculation-result/v1"]});
    fs::write(&script, format!("#!/bin/sh\nif [ \"$1\" = build-info ]; then\nprintf '%s\\n' '{}'\nelse\ntouch '{}'\nexit 91\nfi\n",identity,marker.display())).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let capture_path = script.clone();
    let tasks = AddonPreparation::from_factory(root.path(), move || {
        ds_cli_exec::solar::Solar::capture_pinned(&capture_path)
            .map_err(|error| SolarError::artifact_failed(error.to_string()))
    });
    // Merely holding a capability leaves the executable untouched.
    fs::remove_file(&script).unwrap();
    assert!(!marker.exists());
    fs::write(&script, format!("#!/bin/sh\nif [ \"$1\" = build-info ]; then\nprintf '%s\\n' '{}'\nelse\ntouch '{}'\nexit 91\nfi\n",identity,marker.display())).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let refused = tasks.plan(&city(), ReferencePin::current()).unwrap_err();
    assert_eq!(refused.code, SolarErrorCode::SolarEngineUpdateRequired);
    assert!(!marker.exists());
}

#[test]
fn cancelled_preparation_never_captures_an_addon() {
    let root = tempfile::tempdir().unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let tasks =
        AddonPreparation::from_factory(root.path(), || panic!("cancelled task captured addon"))
            .with_cancel(cancel);
    assert_eq!(
        tasks
            .plan(&city(), ReferencePin::current())
            .unwrap_err()
            .code,
        SolarErrorCode::Cancelled
    );
}

#[test]
fn completion_is_actual_producer_integrity_and_never_a_cross_component_pin() {
    let mut identity = EngineIdentity::current();
    identity.schema.clear();
    for stamp in [
        "".to_owned(),
        "a".repeat(63),
        "A".repeat(64),
        "z".repeat(64),
    ] {
        assert!(validate_completion_stamp(&identity, &stamp).is_err());
    }
    let actual = "a".repeat(64);
    validate_completion_stamp(&identity, &actual).unwrap();
    identity.schema = "ds.engine-build/v1".into();
    identity.build_manifest_sha256 = actual.clone();
    validate_completion_stamp(&identity, &actual).unwrap();
    assert!(validate_completion_stamp(&identity, &"b".repeat(64)).is_err());
}

#[test]
fn copied_json_preserves_owner_weather_and_every_hourly_float_bit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/fixtures/solar/opened");
    let provenance: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("PROVENANCE.json")).unwrap()).unwrap();
    for (city, source) in provenance["cities"].as_object().unwrap() {
        let bytes = fs::read(root.join(format!("{city}.json"))).unwrap();
        assert_eq!(
            ds_solar_contracts::digest::hex_sha256(&bytes),
            source["sha256"].as_str().unwrap()
        );
        let (weather, unit): (
            ds_solar_contracts::WeatherDataset,
            ds_solar_contracts::SealedReferenceUnit,
        ) = serde_json::from_slice(&bytes).unwrap();
        weather.validate().unwrap();
        unit.validate().unwrap();
        unit.matches_weather(&weather).unwrap();
        assert_eq!(weather.content_digest, source["weather_content_digest"]);
        assert_eq!(unit.receipt.source_key, source["reference_source_key"]);
        assert_eq!(
            unit.manifest.bundle_digest,
            source["reference_bundle_digest"]
        );
        for (name, values) in [
            (
                "effective_irradiance_w_m2",
                &unit.profile.effective_irradiance_w_m2,
            ),
            (
                "gross_unit_ac_energy_mwh",
                &unit.profile.gross_unit_ac_energy_mwh,
            ),
            ("net_unit_energy_mwh", &unit.profile.net_unit_energy_mwh),
            (
                "unit_inverter_loss_mwh",
                &unit.profile.unit_inverter_loss_mwh,
            ),
        ] {
            let expected = source["profile_columns"][name]["f64_le_sha256"]
                .as_str()
                .unwrap();
            assert_eq!(values.len(), 8760);
            let bits: Vec<u8> = values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            assert_eq!(
                ds_solar_contracts::digest::hex_sha256(&bits),
                expected,
                "{city}/{name}"
            );
            let mut changed = bits;
            changed[0] ^= 1;
            assert_ne!(
                ds_solar_contracts::digest::hex_sha256(&changed),
                expected,
                "one changed f64 bit must refuse"
            );
        }
    }
}
