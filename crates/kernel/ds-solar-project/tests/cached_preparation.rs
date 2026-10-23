//! The governed cache-only owner reuses preparation and exact handoff claims.
use ds_solar_reference_host::ReferencePreparer;
#[path = "../../ds-solar-reference-host/tests/common/mod.rs"]
mod common;
use ds_solar_contracts::version::GOVERNED_CITY_INTAKE_SCHEMA_VERSION;
use ds_solar_host::cache::{DirectoryReferenceCache, ReferenceCache};
use ds_solar_project::{
    intake::{GovernedCityIntake, GovernedIntakeAuthority},
    preparation::{prepare_cached, prepared_publication_claim, validate_governed_intake},
};
use serde_json::json;

fn intake() -> GovernedCityIntake {
    let mut city = common::city_input("aderm_bere");
    city.identity.project_id = "cache-only-test".into();
    city.identity.root = "eds_project/cache-only-test/eds_solar".into();
    city.revision.source = "ds-brain".into();
    city.revision.revision = Some("a".repeat(64));
    GovernedCityIntake {
        schema_version: GOVERNED_CITY_INTAKE_SCHEMA_VERSION.into(),
        city_input: city.sealed(),
        authority: GovernedIntakeAuthority {
            source_snapshot_sha256: "b".repeat(64),
            input_base_fingerprint: "a".repeat(64),
            snapshot_receipt_id: "11111111-1111-4111-8111-111111111111".into(),
            snapshot_receipt_expires_at: "2099-01-01T00:00:00Z".into(),
            firestore_read_time: common::PREPARED_AT.into(),
            snapshot_bytes: 10,
            source_document_count: 1,
            media_download_count: 0,
        },
    }
}

#[test]
fn cached_preparation_matches_the_original_preparer_and_exact_claim_bytes() {
    let root = tempfile::tempdir().unwrap();
    let cache = DirectoryReferenceCache::new(root.path());
    cache
        .put(
            &common::reference_key("aderm_bere"),
            &common::reference_bytes("aderm_bere"),
        )
        .unwrap();
    let original = intake();
    let city = original.city_input.clone();
    let authority = original.authority.clone();
    let (prepared, claim) = prepare_cached(original, root.path()).unwrap();
    let legacy = ReferencePreparer::new()
        .with_cache(&cache)
        .prepare(city, prepared.prepared_at.clone())
        .unwrap();
    assert_eq!(
        serde_json::to_vec_pretty(&prepared).unwrap(),
        serde_json::to_vec_pretty(&legacy.input).unwrap()
    );
    let expected = format!(
        concat!(
            "{{\n",
            "  \"schema_version\": \"ds-solar.prepared-publication-claim/v1\",\n",
            "  \"claim_kind\": \"server-revalidated-publication-handoff\",\n",
            "  \"project_id\": \"cache-only-test\",\n",
            "  \"city_id\": \"aderm_bere\",\n",
            "  \"city_content_digest\": \"{}\",\n",
            "  \"prepared_input_digest\": \"{}\",\n",
            "  \"source_snapshot_sha256\": \"{}\",\n",
            "  \"input_base_fingerprint\": \"{}\",\n",
            "  \"snapshot_receipt_id\": \"11111111-1111-4111-8111-111111111111\",\n",
            "  \"snapshot_receipt_expires_at\": \"2099-01-01T00:00:00Z\"\n}}"
        ),
        prepared.city.content_digest,
        prepared.input_digest,
        "b".repeat(64),
        "a".repeat(64)
    );
    assert_eq!(
        serde_json::to_vec_pretty(&claim).unwrap(),
        expected.as_bytes()
    );
    assert_eq!(
        serde_json::to_value(&claim).unwrap(),
        serde_json::to_value(prepared_publication_claim(&legacy.input, &authority).unwrap())
            .unwrap()
    );
    let mut changed = authority;
    changed.input_base_fingerprint = "c".repeat(64);
    assert!(prepared_publication_claim(&prepared, &changed).is_err());
}

#[test]
fn missing_and_tampered_cache_refuse_without_repair_or_provider() {
    let root = tempfile::tempdir().unwrap();
    let cache = DirectoryReferenceCache::new(root.path());
    let key = common::reference_key("aderm_bere");
    let miss = prepare_cached(intake(), root.path()).unwrap_err();
    assert!(miss.to_string().contains("no provider"), "{miss:#}");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    cache
        .put(&key, &common::reference_bytes("aderm_bere"))
        .unwrap();
    let weather = cache.entry_dir(&key).join("weather.json");
    let changed = b"tampered reference";
    std::fs::write(&weather, changed).unwrap();
    assert!(prepare_cached(intake(), root.path()).is_err());
    assert_eq!(std::fs::read(&weather).unwrap(), changed);
}

#[test]
fn every_governed_authority_field_and_city_seal_is_admitted_before_cache_use() {
    let valid = serde_json::to_value(intake()).unwrap();
    let cases = [
        ("/schema_version", json!("wrong")),
        ("/city_input/content_digest", json!("sha256:bad")),
        ("/authority/source_snapshot_sha256", json!("not-a-digest")),
        ("/authority/input_base_fingerprint", json!("c".repeat(64))),
        ("/authority/snapshot_receipt_id", json!("not-a-uuid")),
        (
            "/authority/snapshot_receipt_expires_at",
            json!("2000-01-01T00:00:00Z"),
        ),
        ("/authority/firestore_read_time", json!("not-a-time")),
        ("/authority/snapshot_bytes", json!(0)),
        ("/authority/snapshot_bytes", json!(8 * 1024 * 1024 + 1)),
        ("/authority/source_document_count", json!(0)),
        ("/authority/source_document_count", json!(513)),
        ("/authority/media_download_count", json!(513)),
    ];
    for (pointer, replacement) in cases {
        let mut value = valid.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        let input = serde_json::from_value(value).unwrap();
        assert!(validate_governed_intake(&input).is_err(), "{pointer}");
    }
    for (project, source) in [
        ("other-project", "ds-brain"),
        ("cache-only-test", "fixture"),
    ] {
        let mut value = intake();
        value.city_input.identity.project_id = project.into();
        value.city_input.revision.source = source.into();
        value.city_input = value.city_input.sealed();
        assert!(validate_governed_intake(&value).is_err());
    }
}
