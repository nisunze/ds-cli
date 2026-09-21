//! The object index of a `.dsgrid` — what the kernel's task-geometry module
//! reads instead of a package.
//!
//! `ds_command_kernel::task_geometry` resolves a typed reference such as
//! `dsgrid:local-<id>:structure:74,76,77` to geometry, but the kernel never
//! opens a package (`ds-command-kernel/docs/contracts/ds-kernel-network-boundary.md`).
//! This module is the host's half: it reads the package once with the engine
//! crates this domain already links, takes the engine's own projection of
//! structures and alignments, reprojects it to EPSG:4326 with the same
//! `GridModelCrs` the MV print projection uses, and hands the kernel an
//! `ObjectIndex`. It computes no engineering value of its own: every position
//! is the engine's, and a vertex's station is the cumulative length of the
//! engine's route polyline in the model CRS — the engine's own chainage
//! definition, read off its output.

use std::path::PathBuf;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Refusal;
use ds_command_kernel::local_models::Scope;
use ds_command_kernel::task_geometry::{IndexedAlignment, IndexedStructure, ObjectIndex};
use ds_geo::projection::GridModelCrs;
use ds_grid_exchange::gis::{GisGeometry, GisProjectionOptions, ProjectionKind, project_model};
use serde_json::json;

use crate::package;

pub const MODEL_CRS_UNSUPPORTED: Refusal = Refusal {
    code: "model_crs_unsupported",
    when: "the package declares a coordinate system the model-CRS vocabulary cannot reproject",
    remedy: "re-export the model in a WGS84 UTM zone or the Rwanda TM system",
};

/// The refusals reading a reference's model can raise, for the pm commands to
/// splice into their own lists: this domain's shared package refusals, the
/// catalogue's, and the reprojection's.
pub const REFUSALS: &[Refusal] = &[
    package::SHARED_REFUSALS[0],
    package::SHARED_REFUSALS[1],
    package::SHARED_REFUSALS[2],
    package::SHARED_REFUSALS[3],
    Refusal {
        code: "package_decode_failed",
        when: "the package manifest or canonical tables do not verify",
        remedy: "run `ds dsgrid validate --model <path>` and repair the package",
    },
    crate::model::workspace::STORE_UNAVAILABLE,
    crate::model::workspace::SCOPE_MISMATCH,
    MODEL_CRS_UNSUPPORTED,
];

/// Where a working copy's package is on this machine, and what the catalogue
/// calls it. `None` when the catalogue holds no such id — the kernel names
/// that `model_unknown` with the reference, so this answers nothing louder.
pub fn local_package(scope: &Scope, id: &str) -> Result<Option<(PathBuf, String)>, Failure> {
    let root = ds_layer_store::local_models::default_root().map_err(|error| {
        Failure::unavailable(crate::model::workspace::STORE_UNAVAILABLE.code, error)
            .remedy(crate::model::workspace::STORE_UNAVAILABLE.remedy)
    })?;
    let catalogue = ds_layer_store::local_models::read_at(&root, scope)
        .map_err(crate::model::workspace::refuse)?;
    let Some(model) = catalogue.models.iter().find(|model| model.id == id) else {
        return Ok(None);
    };
    let dir = ds_layer_store::local_models::scope_dir(&root, scope)
        .and_then(|dir| ds_layer_store::local_models::package_path(&dir, id))
        .map_err(crate::model::workspace::refuse)?;
    Ok(Some((dir, model.display_name.clone())))
}

/// Read one package and index it under `r#ref`.
pub fn index(
    r#ref: &str,
    raw_path: &str,
    display_name: Option<String>,
) -> Result<ObjectIndex, Failure> {
    let bytes = package::read_bytes(raw_path)?;
    let package = package::decode(raw_path, &bytes)?;
    let declared = package.manifest.model.coordinate_system.to_string();
    let crs = GridModelCrs::parse(&declared).map_err(|error| {
        Failure::invalid(
            MODEL_CRS_UNSUPPORTED.code,
            format!("`{raw_path}` declares `{declared}`, which cannot be reprojected to WGS84"),
        )
        .remedy(MODEL_CRS_UNSUPPORTED.remedy)
        .detail(json!({ "crs": declared, "detail": error.to_string() }))
    })?;
    let to_wgs84 = |x: f64, y: f64| -> Result<[f64; 2], Failure> {
        crs.to_map([x, y]).map_err(|error| {
            Failure::invalid(
                MODEL_CRS_UNSUPPORTED.code,
                format!("a position of `{raw_path}` does not reproject from `{declared}`"),
            )
            .remedy(MODEL_CRS_UNSUPPORTED.remedy)
            .detail(json!({ "crs": declared, "detail": error.to_string() }))
        })
    };

    let projection = project_model(&package.snapshot, &GisProjectionOptions::new(&declared));
    let mut structures = Vec::new();
    let mut alignments = Vec::new();
    for layer in &projection.layers {
        match layer.kind {
            ProjectionKind::Structure => {
                for feature in &layer.features {
                    let GisGeometry::Point { x, y, .. } = feature.geometry else {
                        continue;
                    };
                    structures.push(IndexedStructure {
                        id: feature.entity_id.clone(),
                        number: feature.properties["struct_no"].as_str().map(str::to_owned),
                        alignment: feature.properties["align_id"].as_str().map(str::to_owned),
                        station_m: feature.properties["station_m"].as_f64(),
                        position: to_wgs84(x, y)?,
                    });
                }
            }
            ProjectionKind::Alignment => {
                for feature in &layer.features {
                    let GisGeometry::Line(points) = &feature.geometry else {
                        continue;
                    };
                    let mut vertices = Vec::with_capacity(points.len());
                    let mut station = 0.0;
                    let mut previous: Option<(f64, f64)> = None;
                    for &(x, y, _) in points {
                        if let Some((px, py)) = previous {
                            station += ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                        }
                        previous = Some((x, y));
                        let [lon, lat] = to_wgs84(x, y)?;
                        vertices.push([lon, lat, station]);
                    }
                    alignments.push(IndexedAlignment {
                        id: feature.entity_id.clone(),
                        label: feature.properties["label"]
                            .as_str()
                            .unwrap_or(&feature.entity_id)
                            .to_owned(),
                        vertices,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(ObjectIndex {
        r#ref: r#ref.to_owned(),
        model_id: package.manifest.model.model_id.as_str().to_owned(),
        model_revision: package.manifest.model.model_revision,
        fingerprint: package.manifest.model.snapshot_fingerprint.clone(),
        crs: declared,
        display_name,
        structures,
        alignments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real `.dsgrid` fixture in the repository that owns the format —
    /// referenced, never copied, exactly as `crates/ds/tests/common.rs` does.
    fn fixture() -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
        let path = path.canonicalize().unwrap_or(path);
        assert!(
            path.is_file(),
            "the ds-network fixture is missing at {}",
            path.display()
        );
        path.display().to_string()
    }

    #[test]
    fn the_fixture_indexes_its_structures_and_alignment_in_wgs84_with_the_engine_s_chainage() {
        let index = index("package", &fixture(), Some("humble".into())).expect("indexes");
        assert_eq!(index.r#ref, "package");
        assert_eq!(index.model_id, "pls-import-fnv1a64:bd82b3ba");
        assert_eq!(index.model_revision, 0);
        assert_eq!(index.crs, "EPSG:32735");
        assert_eq!(index.display_name.as_deref(), Some("humble"));
        assert!(index.fingerprint.starts_with("fnv1a64:"));

        // Two routed structures at stations 100 and 300 on the one alignment,
        // every position a longitude/latitude the declared UTM zone holds.
        assert_eq!(index.structures.len(), 2);
        let crs = GridModelCrs::parse("EPSG:32735").unwrap();
        for structure in &index.structures {
            assert_eq!(
                structure.alignment.as_deref(),
                Some("aln-e9bc0619ae359761-1")
            );
            let [lon, lat] = structure.position;
            assert!(
                (26.0..28.0).contains(&lon) && (-90.0..0.0).contains(&lat),
                "{structure:?}"
            );
            // CRS round trip: back to the model's metres within a centimetre.
            let [x, y] = crs.from_map(structure.position).unwrap();
            let expected_x = 500_000.0 + structure.station_m.unwrap();
            assert!(
                (x - expected_x).abs() < 0.01 && (y - 4_705_000.0).abs() < 0.01,
                "{x} {y}"
            );
        }
        assert_eq!(index.structures[0].station_m, Some(100.0));
        assert_eq!(index.structures[1].station_m, Some(300.0));

        // One alignment of three route nodes; the vertex stations are the
        // engine's chainage — cumulative route length in the model CRS.
        assert_eq!(index.alignments.len(), 1);
        let alignment = &index.alignments[0];
        assert_eq!(alignment.id, "aln-e9bc0619ae359761-1");
        let stations: Vec<f64> = alignment.vertices.iter().map(|v| v[2]).collect();
        assert_eq!(stations, vec![0.0, 200.0, 400.0]);
        let [x, _] = crs
            .from_map([alignment.vertices[1][0], alignment.vertices[1][1]])
            .unwrap();
        assert!((x - 500_200.0).abs() < 0.01);

        // The index is exactly what the kernel deserialises: no field lost.
        let json = serde_json::to_value(&index).unwrap();
        let back: ObjectIndex = serde_json::from_value(json).unwrap();
        assert_eq!(back, index);
    }

    #[test]
    fn a_missing_package_keeps_the_domain_s_own_refusal() {
        let error = index("package", "/nonexistent/model.dsgrid", None).unwrap_err();
        assert_eq!(error.code(), "model_not_found");
    }
}
