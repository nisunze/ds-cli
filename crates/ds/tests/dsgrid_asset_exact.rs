//! Exact extraction uses the native package and origin owners; the CLI must
//! preserve leaf ambiguity and never substitute a different role or digest.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use ds_grid_exchange::package::{AssetBytes, PackOptions, pack, unpack};
use ds_grid_exchange::{OriginAuthorityRecord, OriginAuthorityScope, origin_authority_asset};
use serde_json::Value;
use sha2::{Digest, Sha256};

mod common;

const LEAF: &str = "a-w-S190.012";
const CURRENT: &[u8] = b"current native definition\r\n";
const ORIGIN: &[u8] = b"historical native definition\r\n";
const UNCLAIMED: &[u8] = b"unclaimed same-leaf payload\r\n";

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct Fixture {
    root: tempfile::TempDir,
    path: PathBuf,
    bytes: Vec<u8>,
    resource_id: String,
}

fn fixture() -> Fixture {
    // Borrow the owner's typed identities and CRS from its real fixture,
    // then pack a small valid model through that same owner.
    let mut package = unpack(&std::fs::read(common::fixture()).unwrap()).unwrap();
    let mut resource = package.snapshot.resources[0].clone();
    let other_id = package.snapshot.resources[1].id.entity().clone();
    resource.invariant_leaf = LEAF.into();
    resource.content_digest = format!("sha256:{}", sha256(CURRENT));
    resource.byte_len = CURRENT.len() as u64;
    let resource_id = resource.id.as_str().to_owned();
    package.snapshot = Default::default();
    package.snapshot.resources.push(resource.clone());
    let mut assets = [CURRENT, ORIGIN, UNCLAIMED]
        .into_iter()
        .map(|bytes| AssetBytes {
            invariant_leaf: LEAF.into(),
            bytes: bytes.to_vec(),
        })
        .collect::<Vec<_>>();
    let origin = OriginAuthorityRecord {
        source_system: "pls_cadd".into(),
        scope: OriginAuthorityScope::ResourceGraph,
        source_leaf: LEAF.into(),
        asset_leaf: LEAF.into(),
        content_digest: format!("sha256:{}", sha256(ORIGIN)),
        byte_len: ORIGIN.len() as u64,
        interpretation_fingerprint: package.snapshot.snapshot_fingerprint(),
        interpreted_entity_ids: vec![resource.id.entity().clone()],
    };
    // A same-leaf/digest origin for another entity must not satisfy this id.
    let unrelated = OriginAuthorityRecord {
        content_digest: format!("sha256:{}", sha256(UNCLAIMED)),
        byte_len: UNCLAIMED.len() as u64,
        interpreted_entity_ids: vec![other_id],
        ..origin.clone()
    };
    assets.push(origin_authority_asset(vec![origin, unrelated]).unwrap());
    let bytes = pack(
        &package.snapshot,
        &PackOptions {
            model_id: package.manifest.model.model_id,
            model_revision: 2,
            presentation: Default::default(),
            coordinate_system: package.manifest.model.coordinate_system,
            library_pins: Vec::new(),
            library_needs: Vec::new(),
            assets,
            exchange_bindings: Default::default(),
        },
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("unchanged.dsgrid");
    std::fs::write(&path, &bytes).unwrap();
    Fixture {
        root,
        path,
        bytes,
        resource_id,
    }
}

fn extract(path: &Path, out: &Path, selector: &[&str]) -> (Value, i32) {
    let mut args = vec![
        "dsgrid",
        "asset",
        "extract",
        "--path",
        path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--output",
        "json",
    ];
    args.extend_from_slice(selector);
    common::json(&args)
}

fn assert_refused(fixture: &Fixture, selector: &[&str], code: &str) {
    let out = fixture.root.path().join("refused.012");
    let (answer, status) = extract(&fixture.path, &out, selector);
    assert_ne!(status, 0, "{answer}");
    assert_eq!(answer["error"]["code"], code, "{answer}");
    assert!(!out.exists());
    assert_eq!(std::fs::read(&fixture.path).unwrap(), fixture.bytes);
}

#[test]
fn exact_current_and_origin_bytes_share_one_unchanged_package() {
    let fixture = fixture();
    for (role, payload) in [("resource", CURRENT), ("origin_resource", ORIGIN)] {
        let out = fixture.root.path().join(format!("{role}.012"));
        let digest = sha256(payload);
        let canonical_digest = format!("sha256:{digest}");
        let (answer, status) = extract(
            &fixture.path,
            &out,
            &[
                "--resource-id",
                &fixture.resource_id,
                "--expected-digest",
                &canonical_digest,
                "--role",
                role,
            ],
        );
        assert_eq!(status, 0, "{answer}");
        assert_eq!(std::fs::read(&out).unwrap(), payload);
        let data = &answer["data"];
        assert_eq!(data["resource_id"], fixture.resource_id);
        assert_eq!(data["leaf"], LEAF);
        assert_eq!(data["role"], role);
        assert_eq!(data["sha256"], digest);
        assert_eq!(data["byte_len"], payload.len());
        assert_eq!(data["package_sha256"], sha256(&fixture.bytes));
        assert_eq!(data["verified"], true);
        assert_eq!(std::fs::read(&fixture.path).unwrap(), fixture.bytes);
        let (refused, status) = extract(
            &fixture.path,
            &out,
            &[
                "--resource-id",
                &fixture.resource_id,
                "--expected-digest",
                &digest,
                "--role",
                role,
            ],
        );
        assert_ne!(status, 0);
        assert_eq!(refused["error"]["code"], "output_exists");
        assert_eq!(std::fs::read(out).unwrap(), payload);
    }
    assert_refused(&fixture, &["--leaf", LEAF], "asset_leaf_ambiguous");
}

#[test]
fn exact_selection_refuses_wrong_id_digest_role_and_unrelated_origin() {
    let fixture = fixture();
    for (id, digest, role, code) in [
        (
            "missing-resource",
            sha256(CURRENT),
            "resource",
            "asset_resource_not_found",
        ),
        (
            fixture.resource_id.as_str(),
            sha256(ORIGIN),
            "resource",
            "asset_resource_digest_mismatch",
        ),
        (
            fixture.resource_id.as_str(),
            sha256(CURRENT),
            "origin_resource",
            "asset_resource_digest_mismatch",
        ),
        (
            fixture.resource_id.as_str(),
            sha256(UNCLAIMED),
            "origin_resource",
            "asset_resource_digest_mismatch",
        ),
        (
            fixture.resource_id.as_str(),
            "0".repeat(64),
            "resource",
            "asset_resource_digest_mismatch",
        ),
    ] {
        assert_refused(
            &fixture,
            &[
                "--resource-id",
                id,
                "--expected-digest",
                &digest,
                "--role",
                role,
            ],
            code,
        );
    }
}

#[test]
fn exact_selector_requires_one_complete_pin_and_canonical_digest() {
    let fixture = fixture();
    let digest = sha256(CURRENT);
    for selector in [
        vec![],
        vec!["--resource-id", &fixture.resource_id],
        vec![
            "--resource-id",
            &fixture.resource_id,
            "--expected-digest",
            &digest,
        ],
        vec!["--leaf", LEAF, "--expected-digest", &digest],
        vec!["--leaf", LEAF, "--role", "resource"],
        vec![
            "--leaf",
            LEAF,
            "--resource-id",
            &fixture.resource_id,
            "--expected-digest",
            &digest,
            "--role",
            "resource",
        ],
    ] {
        assert_refused(&fixture, &selector, "asset_selector_invalid");
    }
    for invalid in [
        digest.to_uppercase(),
        format!("SHA256:{digest}"),
        "0".repeat(63),
        "g".repeat(64),
    ] {
        assert_refused(
            &fixture,
            &[
                "--resource-id",
                &fixture.resource_id,
                "--expected-digest",
                &invalid,
                "--role",
                "resource",
            ],
            "asset_digest_invalid",
        );
    }
}

#[test]
fn exact_extraction_refuses_corrupted_attested_payload() {
    let mut fixture = fixture();
    let mut archive = zip::ZipArchive::new(Cursor::new(&fixture.bytes)).unwrap();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        if member.name() == format!("assets/{}/{LEAF}", sha256(CURRENT)) {
            bytes[0] ^= 1;
        }
        writer
            .start_file(member.name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    let corrupted = writer.finish().unwrap().into_inner();
    drop(archive);
    fixture.bytes = corrupted;
    std::fs::write(&fixture.path, &fixture.bytes).unwrap();
    assert_refused(
        &fixture,
        &[
            "--resource-id",
            &fixture.resource_id,
            "--expected-digest",
            &sha256(CURRENT),
            "--role",
            "resource",
        ],
        "asset_package_invalid",
    );
}

#[test]
fn leaf_only_extraction_still_reads_an_unambiguous_asset() {
    let fixture = fixture();
    let out = fixture.root.path().join("origin-registry.json");
    let (answer, status) = extract(
        &fixture.path,
        &out,
        &["--leaf", "origin-authorities.v1.json"],
    );
    assert_eq!(status, 0, "{answer}");
    assert_eq!(answer["data"]["verified"], true);
    assert_eq!(std::fs::read(&fixture.path).unwrap(), fixture.bytes);
}

#[test]
#[ignore = "requires DS_GRID_EXACT_TEST_PACKAGE pointing to the unchanged Gisagara Model 1 v2 package"]
fn gisagara_model_1_v2_exact_bytes() {
    let path = PathBuf::from(
        std::env::var("DS_GRID_EXACT_TEST_PACKAGE").expect("explicit real package path"),
    );
    let before = std::fs::read(&path).unwrap();
    let root = tempfile::tempdir().unwrap();
    for (role, digest, size) in [
        (
            "resource",
            "de2f010dbdf1070937268fd55aa254169b0e8ae94c067ea9d1254153fd246d2f",
            8006,
        ),
        (
            "origin_resource",
            "eabd6f8c416b7ffbff6752aafee5881920ac34b647d90b3361bcf7630e5c7cdf",
            2005,
        ),
    ] {
        let out = root.path().join(format!("{role}.012"));
        let (answer, status) = extract(
            &path,
            &out,
            &[
                "--resource-id",
                "res-3ea76f53683c025e",
                "--expected-digest",
                digest,
                "--role",
                role,
            ],
        );
        assert_eq!(status, 0, "{answer}");
        let written = std::fs::read(out).unwrap();
        assert_eq!(written.len(), size);
        assert_eq!(sha256(&written), digest);
        assert_eq!(answer["data"]["package_sha256"], sha256(&before));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let out = root.path().join("ambiguous.012");
    let (answer, status) = extract(&path, &out, &["--leaf", LEAF]);
    assert_ne!(status, 0);
    assert_eq!(answer["error"]["code"], "asset_leaf_ambiguous");
    assert!(!out.exists());
}
