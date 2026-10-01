//! CLI-level evidence for the source-fenced native package adapter.

use ds_grid_engine::{CommandEnvelope, GridCommand, GridSession, RevisionId};
use ds_grid_exchange::package::{AssetBytes, GridPackage, PackOptions, pack, unpack};
use ds_grid_exchange::{BlankModelRequest, create_blank_model};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{Run, ds, ok};

const SOURCE: &[u8] = include_bytes!(
    "../../../../../ds-network/crates/ds-io/assets/pls_cadd/witness/cable-units/acsr-70-12-si.wir"
);

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn encode(package: &GridPackage) -> Vec<u8> {
    pack(
        &package.snapshot,
        &PackOptions {
            model_id: package.manifest.model.model_id.clone(),
            model_revision: package.manifest.model.model_revision,
            presentation: package.manifest.model.presentation.clone(),
            coordinate_system: package.manifest.model.coordinate_system.clone(),
            library_pins: package.manifest.model.library_pins.clone(),
            library_needs: package.manifest.model.library_needs.clone(),
            assets: package.assets.clone(),
            exchange_bindings: package.exchange_bindings.clone(),
        },
    )
    .unwrap()
}

struct Fixture {
    root: tempfile::TempDir,
    bytes: Vec<u8>,
    revision: RevisionId,
    source_digest: String,
}

impl Fixture {
    fn new() -> Self {
        let mut package = unpack(
            &create_blank_model(&BlankModelRequest::default())
                .unwrap()
                .bytes,
        )
        .unwrap();
        // Authored test values deliberately differ from the native witness.
        // No importer or mechanical mapping is implemented by this fixture.
        package.snapshot.cables.push(
            serde_json::from_value(json!({
                "id": "cb-70", "engineering_name": "retained.wir",
                "description": "Authored presentation", "diameter_m": 0.012,
                "cross_section_m2": 0.00008, "mass_per_length_kg_per_m": 0.3,
                "rated_strength_n": 30000.0, "nominal_elastic_modulus_pa": null,
                "reference_temperature_c": 20.0, "resource_id": "res-cable",
            }))
            .unwrap(),
        );
        package.snapshot.cable_curves.push(
            serde_json::from_value(json!({
                "cable_id": "cb-70", "family": "outer",
                "initial_stress_strain_pa": [0.0, 1000000000.0],
                "creep_stress_strain_pa": [0.0, 1000000000.0],
                "final_modulus_pa": 1000000000.0,
                "thermal_expansion_per_k": 0.00002,
            }))
            .unwrap(),
        );
        let source_digest = digest(SOURCE);
        package.snapshot.resources.push(
            serde_json::from_value(json!({
                "id": "res-cable", "invariant_leaf": "retained.wir",
                "media": "cable_definition", "content_digest": source_digest,
                "byte_len": SOURCE.len(), "provider": { "kind": "embedded" },
            }))
            .unwrap(),
        );
        package.assets.push(AssetBytes {
            invariant_leaf: "retained.wir".into(),
            bytes: SOURCE.to_vec(),
        });
        // A same-named historical asset is deliberately not authoritative.
        package.assets.push(AssetBytes {
            invariant_leaf: "retained.wir".into(),
            bytes: b"older same-named resource".to_vec(),
        });
        let bytes = encode(&package);
        let revision = GridSession::open(unpack(&bytes).unwrap().snapshot)
            .current_revision()
            .revision_id
            .clone();
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
            bytes,
            revision,
            source_digest,
        };
        std::fs::write(fixture.model(), &fixture.bytes).unwrap();
        fixture
    }

    fn model(&self) -> std::path::PathBuf {
        self.root.path().join("source.dsgrid")
    }

    fn out(&self) -> std::path::PathBuf {
        self.root.path().join("result.dsgrid")
    }

    fn invoke(&self, revision: &str, source_digest: &str, extra: &[&str]) -> Run {
        let model = self.model();
        let mut args = vec![
            "dsgrid",
            "reconcile-cable-source",
            "--model",
            model.to_str().unwrap(),
            "--cable-id",
            "cb-70",
            "--revision",
            revision,
            "--expect-source-digest",
            source_digest,
            "--output",
            "json",
        ];
        args.extend_from_slice(extra);
        ds(&args)
    }

    fn unchanged(&self) {
        assert_eq!(std::fs::read(self.model()).unwrap(), self.bytes);
    }
}

#[test]
fn discovers_and_reconciles_exact_retained_mechanics_to_a_new_package() {
    let descriptor = ok(&[
        "capabilities",
        "dsgrid.reconcile-cable-source",
        "--output",
        "json",
    ]);
    assert_eq!(descriptor["command"]["availability"], "available");
    assert_eq!(descriptor["command"]["effect"], "local_file_write");
    assert_eq!(descriptor["command"]["authority"], "none");
    let help = ds(&["dsgrid", "reconcile-cable-source", "--help"]);
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("--expect-source-digest"));
    assert!(help.stdout.contains("source_digest_mismatch"));

    let fixture = Fixture::new();
    let out = fixture.out();
    let run = fixture.invoke(
        fixture.revision.as_str(),
        &fixture.source_digest,
        &["--out", out.to_str().unwrap()],
    );
    assert_eq!(run.code, 0, "{}", run.envelope);
    let receipt = &run.envelope["data"];
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(receipt["persisted"], true);
    assert_eq!(receipt["dry_run"], false);
    assert_eq!(receipt["source_package_digest"], digest(&fixture.bytes));
    assert_eq!(receipt["resulting_package_digest"], digest(&bytes));
    assert_eq!(receipt["artifact"]["sha256"], digest(&bytes));
    assert_eq!(receipt["artifact"]["byte_len"], bytes.len());
    assert_eq!(receipt["source_digest"], fixture.source_digest);
    assert_eq!(receipt["source_revision"], fixture.revision.as_str());
    assert_eq!(receipt["cable_id"], "cb-70");
    assert_eq!(receipt["resource_id"], "res-cable");
    assert!(receipt.get("bytes").is_none());
    assert!(!receipt["differences"].as_array().unwrap().is_empty());
    let before = unpack(&fixture.bytes).unwrap();
    let after = unpack(&bytes).unwrap();
    assert_eq!(after.assets, before.assets);
    assert_eq!(after.exchange_bindings, before.exchange_bindings);
    assert_eq!(after.snapshot.resources, before.snapshot.resources);
    assert_eq!(
        after.snapshot.cables[0].resource_id,
        before.snapshot.cables[0].resource_id
    );
    assert_eq!(
        after.snapshot.cables[0].description,
        before.snapshot.cables[0].description
    );
    assert_eq!(after.snapshot.cables[0].engineering_name, "retained.wir");
    assert_eq!(after.snapshot.cables[0].rated_strength_n, 26_270.0);
    assert_eq!(
        after.snapshot.cables[0].nominal_elastic_modulus_pa,
        Some(77e9)
    );
    assert_eq!(
        after.manifest.model.model_revision,
        before.manifest.model.model_revision + 1
    );
    assert_eq!(
        receipt["package_revision"],
        after.manifest.model.model_revision
    );
    assert_eq!(
        receipt["resulting_revision"],
        GridSession::open(after.snapshot)
            .current_revision()
            .revision_id
            .as_str()
    );
    assert_ne!(receipt["source_revision"], receipt["resulting_revision"]);
    fixture.unchanged();
}

#[test]
fn stale_authored_revision_refuses_without_writing() {
    let fixture = Fixture::new();
    let stale = RevisionId::from_content_root("stale");
    let out = fixture.out();
    let run = fixture.invoke(
        stale.as_str(),
        &fixture.source_digest,
        &["--out", out.to_str().unwrap()],
    );
    assert_ne!(run.code, 0);
    assert_eq!(run.envelope["error"]["code"], "revision_conflict");
    assert_eq!(
        run.envelope["error"]["detail"]["expected_revision"],
        stale.as_str()
    );
    assert_eq!(
        run.envelope["error"]["detail"]["actual_revision"],
        fixture.revision.as_str()
    );
    assert!(!out.exists());
    let preview = fixture.invoke(stale.as_str(), &fixture.source_digest, &["--dry-run"]);
    assert_eq!(preview.envelope["error"]["code"], "revision_conflict");
    fixture.unchanged();
}

#[test]
fn source_digest_mismatch_refuses_without_writing() {
    let fixture = Fixture::new();
    let wrong = digest(b"same name is not authority");
    let out = fixture.out();
    let run = fixture.invoke(
        fixture.revision.as_str(),
        &wrong,
        &["--out", out.to_str().unwrap()],
    );
    assert_ne!(run.code, 0);
    assert_eq!(run.envelope["error"]["code"], "source_digest_mismatch");
    assert_eq!(
        run.envelope["error"]["detail"]["expected_source_digest"],
        wrong
    );
    assert_eq!(
        run.envelope["error"]["detail"]["actual_source_digest"],
        fixture.source_digest
    );
    assert!(!out.exists());
    let preview = fixture.invoke(fixture.revision.as_str(), &wrong, &["--dry-run"]);
    assert_eq!(preview.envelope["error"]["code"], "source_digest_mismatch");
    fixture.unchanged();
}

#[test]
fn output_exists_including_the_source_refuses_without_overwriting() {
    let fixture = Fixture::new();
    let out = fixture.out();
    std::fs::write(&out, b"existing artifact").unwrap();
    for path in [&out, &fixture.model()] {
        let run = fixture.invoke(
            fixture.revision.as_str(),
            &fixture.source_digest,
            &["--out", path.to_str().unwrap()],
        );
        assert_ne!(run.code, 0);
        assert_eq!(run.envelope["error"]["code"], "output_exists");
    }
    assert_eq!(std::fs::read(out).unwrap(), b"existing artifact");
    fixture.unchanged();
}

#[test]
fn dry_run_validates_a_deterministic_candidate_but_changes_no_file() {
    let fixture = Fixture::new();
    let out = fixture.out();
    let preview = fixture.invoke(
        fixture.revision.as_str(),
        &fixture.source_digest,
        &["--dry-run", "--out", out.to_str().unwrap(), "--limit", "1"],
    );
    assert_eq!(preview.code, 0, "{}", preview.envelope);
    let receipt = &preview.envelope["data"];
    assert_eq!(receipt["dry_run"], true);
    assert_eq!(receipt["persisted"], false);
    assert!(receipt.get("artifact").is_none());
    assert_eq!(receipt["differences"].as_array().unwrap().len(), 1);
    let truncation = &receipt["more"]["truncated"][0];
    assert_eq!(truncation["field"], "differences");
    assert_eq!(truncation["shown"], 1);
    assert_eq!(
        truncation["total"].as_u64().unwrap(),
        truncation["withheld"].as_u64().unwrap() + 1
    );
    assert!(!out.exists());
    fixture.unchanged();
    // Even an existing --out is ignored by dry-run, as the descriptor promises.
    std::fs::write(&out, b"keep").unwrap();
    let existing = fixture.invoke(
        fixture.revision.as_str(),
        &fixture.source_digest,
        &["--dry-run", "--out", out.to_str().unwrap()],
    );
    assert_eq!(existing.code, 0, "{}", existing.envelope);
    assert_eq!(std::fs::read(&out).unwrap(), b"keep");
    std::fs::remove_file(&out).unwrap();
    let written = fixture.invoke(
        fixture.revision.as_str(),
        &fixture.source_digest,
        &["--out", out.to_str().unwrap()],
    );
    assert_eq!(written.code, 0, "{}", written.envelope);
    assert_eq!(
        receipt["resulting_package_digest"],
        written.envelope["data"]["resulting_package_digest"]
    );
    assert_eq!(
        receipt["resulting_revision"],
        written.envelope["data"]["resulting_revision"]
    );
    fixture.unchanged();
}

#[test]
fn generic_cable_edit_still_severs_authority_and_reconciliation_cannot_reattach_it() {
    let fixture = Fixture::new();
    let package = unpack(&fixture.bytes).unwrap();
    let mut row = package.snapshot.cables[0].clone();
    row.rated_strength_n += 1.0;
    let envelope = CommandEnvelope::new(
        "generic-cable-edit",
        fixture.revision.clone(),
        GridCommand::UpdateCableDefinition {
            id: row.id.clone(),
            row,
            curves: package.snapshot.cable_curves.clone(),
        },
    );
    let envelope_path = fixture.root.path().join("command.json");
    let detached_path = fixture.root.path().join("detached.dsgrid");
    std::fs::write(&envelope_path, serde_json::to_vec(&envelope).unwrap()).unwrap();
    ok(&[
        "dsgrid",
        "apply",
        "--model",
        fixture.model().to_str().unwrap(),
        "--envelope",
        envelope_path.to_str().unwrap(),
        "--out",
        detached_path.to_str().unwrap(),
        "--output",
        "json",
    ]);
    let detached = unpack(&std::fs::read(&detached_path).unwrap()).unwrap();
    assert!(detached.snapshot.cables[0].resource_id.is_none());
    assert_eq!(detached.assets, package.assets);
    let revision = GridSession::open(detached.snapshot)
        .current_revision()
        .revision_id
        .clone();
    let refused = ds(&[
        "dsgrid",
        "reconcile-cable-source",
        "--model",
        detached_path.to_str().unwrap(),
        "--cable-id",
        "cb-70",
        "--revision",
        revision.as_str(),
        "--expect-source-digest",
        &fixture.source_digest,
        "--dry-run",
        "--output",
        "json",
    ]);
    assert_eq!(refused.envelope["error"]["code"], "source_unavailable");
    assert_eq!(
        refused.envelope["error"]["detail"]["blocker"]["code"],
        "missing_resource"
    );
    fixture.unchanged();
}

#[test]
fn accepts_only_declared_identity_and_fence_inputs() {
    let fixture = Fixture::new();
    for extra in [
        vec!["--source", "replacement.wir"],
        vec!["--project", "cloud"],
        vec!["--resource-id", "replacement"],
        vec!["--rated-strength-n", "99999"],
    ] {
        let mut flags = vec!["--dry-run"];
        flags.extend(extra);
        let refused = fixture.invoke(fixture.revision.as_str(), &fixture.source_digest, &flags);
        assert_eq!(refused.envelope["error"]["code"], "unknown_flag");
    }
    let missing = fixture.invoke(fixture.revision.as_str(), &fixture.source_digest, &[]);
    assert_eq!(missing.envelope["error"]["code"], "output_required");
    assert!(!fixture.out().exists());
    fixture.unchanged();
}
