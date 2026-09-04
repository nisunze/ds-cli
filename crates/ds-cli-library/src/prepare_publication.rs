//! Prepare one verified local standards-library seed for governed publication.

use crate::{engine_failure, read, sha256};
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::{
    StandardsLibraryMemberPlane, StandardsLibraryNativeCategory, parse_standards_library_manifest,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub static COMMAND: Command = Command {
    id: "library.prepare-publication",
    path: &["library", "prepare-publication"],
    contract: 1,
    summary: "Prepare one immutable local seed for governed global publication.",
    purpose: "Verifies the exact parallel DS Grid/PLS-CADD seed, copies its declared immutable bytes into a fresh prepared directory, and writes the typed library.json and validation report consumed by library global publish-library. It never publishes, overwrites, approves solver results, or synthesizes native assets.",
    chapter: Chapter::PlsCadd,
    effect: Effect::ArtifactWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "release",
            "<dir>",
            "Exact local library/<id>/<version> directory.",
        )
        .required(),
        Arg::value("out", "<dir>", "Fresh prepared publication directory.").required(),
        Arg::value(
            "display-name",
            "<name>",
            "Operator-facing governed library name.",
        )
        .required(),
        Arg::value(
            "description",
            "<text>",
            "Bounded engineering scope and authority statement.",
        )
        .required(),
        Arg::value("visibility", "<scope>", "organization, private, or public.")
            .default("organization")
            .choices(&["organization", "private", "public"]),
        Arg::value(
            "expected-head-release",
            "<id>",
            "Optional exact current governed head fence.",
        ),
    ],
    output: "Fresh prepared directory, exact library coordinates, asset count and source content-root digest; cloud write remains false.",
    examples: &[Example {
        command: "ds library prepare-publication --release ./library/design-huye-shaded/2026-09-04-v2 --out ./prepared/design-huye-shaded --display-name 'Design — Shaded' --description 'Native shading retained; engineering review pending.' --yes --output json",
        note: "Prepare exact files before the separate paired global publish command.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "publication_source_invalid",
            when: "the seed manifest, bundle, member path, byte length, or digest does not verify",
            remedy: "reseed from the ruled source; never repair native bytes from DS Grid",
        },
        Refusal {
            code: "output_exists",
            when: "the prepared directory already exists",
            remedy: "choose a fresh prepared path",
        },
        Refusal {
            code: "output_unwritable",
            when: "the prepared directory cannot be materialized atomically",
            remedy: "choose a writable fresh path",
        },
    ],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn asset_class(category: StandardsLibraryNativeCategory) -> &'static str {
    match category {
        StandardsLibraryNativeCategory::Structures => "structure",
        StandardsLibraryNativeCategory::Cables => "cable",
        StandardsLibraryNativeCategory::Criteria => "criteria",
        StandardsLibraryNativeCategory::Templates => "template_project_plane",
        StandardsLibraryNativeCategory::Components => "component",
        StandardsLibraryNativeCategory::Other => "other",
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
    }
    fs::write(path, bytes).map_err(|error| Failure::failed("output_unwritable", error.to_string()))
}

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let release = PathBuf::from(inputs.require("release")?);
    let out = PathBuf::from(inputs.require("out")?);
    if out.exists() {
        return Err(Failure::conflict(
            "output_exists",
            format!("`{}` already exists", out.display()),
        ));
    }
    let manifest_bytes = read(release.join("manifest.json").to_string_lossy().as_ref())?;
    let manifest = parse_standards_library_manifest(&manifest_bytes)
        .map_err(|error| engine_failure("publication_source_invalid", error))?;
    let display_name = inputs.require("display-name")?.trim();
    let description = inputs.require("description")?.trim();
    if display_name.is_empty()
        || description.is_empty()
        || display_name.len() > 160
        || description.len() > 1000
    {
        return Err(Failure::invalid(
            "publication_source_invalid",
            "display name or description is empty or unbounded",
        ));
    }
    let parent = out
        .parent()
        .ok_or_else(|| Failure::invalid("output_unwritable", "prepared output has no parent"))?;
    fs::create_dir_all(parent)
        .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
    let leaf = out
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("publication");
    let stage = parent.join(format!(".{leaf}-stage-{}", std::process::id()));
    if stage.exists() {
        return Err(Failure::conflict(
            "output_exists",
            format!("`{}` already exists", stage.display()),
        ));
    }

    let materialize = (|| -> Result<Value, Failure> {
        fs::create_dir(&stage)
            .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
        write_file(&stage.join("manifest.json"), &manifest_bytes)?;
        let bundle_path = &manifest.dsgrid_bundle_path;
        if !safe_relative(bundle_path) {
            return Err(Failure::invalid(
                "publication_source_invalid",
                "manifest DS Grid bundle path is unsafe",
            ));
        }
        let bundle = read(release.join(bundle_path).to_string_lossy().as_ref())?;
        if bundle.len() as u64 != manifest.dsgrid_bundle_byte_length
            || sha256(&bundle) != manifest.dsgrid_bundle_sha256
        {
            return Err(Failure::failed(
                "publication_source_invalid",
                "DS Grid bundle differs from the seed manifest",
            ));
        }
        write_file(&stage.join(bundle_path), &bundle)?;

        let mut assets = vec![json!({
            "relative_path": bundle_path,
            "class": "other",
            "path": bundle_path,
            "provenance": { "kind": "seed", "reference": manifest.source_provenance, "source_digest": manifest.dsgrid_bundle_sha256 },
        })];
        let mut structure_names = BTreeSet::new();
        let mut original_bytes = bundle.len() as u64;
        let mut model_resources = 0u64;
        for member in &manifest.members {
            if !safe_relative(&member.pls_cadd_path) {
                return Err(Failure::invalid(
                    "publication_source_invalid",
                    format!("unsafe member path {:?}", member.pls_cadd_path),
                ));
            }
            let bytes = read(
                release
                    .join(&member.pls_cadd_path)
                    .to_string_lossy()
                    .as_ref(),
            )?;
            if bytes.len() as u64 != member.byte_length || sha256(&bytes) != member.sha256 {
                return Err(Failure::failed(
                    "publication_source_invalid",
                    format!(
                        "member {:?} differs from the seed manifest",
                        member.pls_cadd_path
                    ),
                ));
            }
            write_file(&stage.join(&member.pls_cadd_path), &bytes)?;
            if member.category == StandardsLibraryNativeCategory::Structures {
                structure_names.insert(member.canonical_typed_name.to_ascii_lowercase());
            }
            if member.plane == StandardsLibraryMemberPlane::ExampleModel {
                model_resources += 1;
            }
            original_bytes += member.byte_length;
            assets.push(json!({
                "relative_path": member.pls_cadd_path,
                "class": asset_class(member.category),
                "path": member.pls_cadd_path,
                "provenance": { "kind": "seed", "reference": member.source_provenance, "source_digest": member.sha256 },
            }));
        }
        let validation = json!({
            "schema_version": 1,
            "library_id": manifest.library_id,
            "release_id": manifest.version,
            "content_root_sha256": manifest.content_root_sha256,
            "checks": { "seed_manifest_verified": true, "member_digests_verified": true, "parallel_dsgrid_pls_layout": true, "native_assets_not_synthesized": true },
            "loss_rollup": manifest.loss_rollup,
            "solver_acceptance": "not_claimed",
            "engineering_approval": "not_claimed",
        });
        let validation_bytes = serde_json::to_vec_pretty(&validation)
            .map_err(|error| Failure::internal("publication_source_invalid", error.to_string()))?;
        write_file(&stage.join("validation-report.json"), &validation_bytes)?;
        let mut library = json!({
            "visibility": inputs.value("visibility").unwrap_or("organization"),
            "library": {
                "display_name": display_name,
                "description": description,
                "release": {
                    "library_id": manifest.library_id,
                    "release_id": manifest.version,
                    "state": manifest.status,
                    "artifact_schema_version": manifest.schema_version.to_string(),
                    "manifest": { "path": "manifest.json" },
                    "validation_report": { "path": "validation-report.json" },
                    "assets": assets,
                    "summary": {
                        "resource_count": manifest.members.len() + 1,
                        "model_resource_count": model_resources,
                        "structure_family_count": structure_names.len(),
                        "structure_realization_count": structure_names.len(),
                        "common_object_count": manifest.members.len().saturating_sub(structure_names.len()),
                        "original_byte_length": original_bytes,
                    },
                    "minimum_engine_capabilities": manifest.compatibility,
                }
            }
        });
        if let Some(expected) = inputs
            .value("expected-head-release")
            .filter(|value| !value.trim().is_empty())
        {
            library["library"]["expected_head_release_id"] =
                Value::String(expected.trim().to_string());
        }
        let library_bytes = serde_json::to_vec_pretty(&library)
            .map_err(|error| Failure::internal("publication_source_invalid", error.to_string()))?;
        write_file(&stage.join("library.json"), &library_bytes)?;
        Ok(json!({
            "prepared": out,
            "library_id": manifest.library_id,
            "release_id": manifest.version,
            "content_root_sha256": format!("sha256:{}", manifest.content_root_sha256),
            "asset_count": manifest.members.len() + 1,
            "cloud_write": false,
            "native_assets_synthesized": false,
        }))
    })();
    match materialize {
        Ok(value) => {
            fs::rename(&stage, &out)
                .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
            Ok(value)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&stage);
            Err(error)
        }
    }
}

pub fn render(data: &Value) -> String {
    format!(
        "prepared {}\nassets {}\ncloud write: false",
        data["prepared"].as_str().unwrap_or(""),
        data["asset_count"].as_u64().unwrap_or(0),
    )
}
