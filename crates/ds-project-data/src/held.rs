//! Layers a host already holds, merged into the one print-context document.
//!
//! [`read_print_context`](crate::read_print_context) supplies what the
//! project's rooms hold and names what it cannot: survey and local-layer
//! captures are refused, project MV is omitted. A host that holds those
//! layers itself — the desktop's captures and retained derived context, the
//! CLI's projected MV models — attaches them here, through the same kernel
//! recipe, so both hosts stage one `ds.print-context/v1` document under one
//! digest. This module never reads a room, never acquires and never decides
//! what a held layer contains; it merges what it is handed.
//!
//! It is the host-neutral form of the CLI's `mv_context::attach`, minus the
//! model loop: the caller projects and clips, this module merges.

use ds_command_kernel::project_dataset_cache::{Scope, REFERENCE_CACHE_PROVIDER};
use ds_command_kernel::report_export::{print_context_document, sha256_hex};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

use crate::catalog::ReferenceResource;
use crate::print_context::{Omission, PrintContext};
use crate::Failure;

/// One layer the host holds and wants printed: its document id, its
/// `FeatureCollection`, and the source-inventory entry that records where it
/// came from (kind, model versions, capture identity). The entry's `layer`
/// field is set to `id` on merge, so an entry that names another layer cannot
/// slip in under this one.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldLayer {
    pub id: String,
    pub collection: Value,
    pub source: Value,
}

fn invalid(message: impl std::fmt::Display) -> Failure {
    Failure::Unsupported(message.to_string())
}

/// The design's extent buffered by `buffer_m` through the kernel's own metric
/// buffering, as the coverage polygon a fresh document starts from.
fn buffered_coverage(transformer: &str, network: &Value, buffer_m: f64) -> Result<Value, Failure> {
    let request = json!({
        "schema": ds_command_kernel::project_design_extent::SCHEMA,
        "action": "extent",
        "name": transformer,
        "layers": network,
        "buffer_m": buffer_m,
    });
    let bytes = serde_json::to_vec(&request).map_err(crate::store)?;
    let answer: Value = serde_json::from_str(
        &ds_command_kernel::project_design_extent::evaluate(&bytes).map_err(crate::store)?,
    )
    .map_err(crate::store)?;
    let [w, s, e, n]: [f64; 4] = serde_json::from_value(answer["buffered"].clone())
        .map_err(|_| invalid("the design has no geographic extent"))?;
    Ok(json!({"type": "Polygon", "coordinates": [[[w, s], [e, s], [e, n], [w, n], [w, s]]]}))
}

/// Merge held layers into one transformer's print context.
///
/// The read document's coverage and layers are kept as they are; without a
/// read document the coverage is the design's extent buffered by `buffer_m`.
/// Each held layer is added under its own id with its source entry appended
/// to the inventory. A held id that the document already carries is refused
/// (`print_context_unsupported`): the engine's layer identity is one per
/// document, and a silent overwrite would print one source as another. An
/// empty held collection is not attached — the engine drops an empty
/// collection — and every omission the read named for an id the host now
/// holds is dropped, whether or not the held collection had features inside
/// the extent; the other omissions, warnings and the `reused` fact survive.
///
/// `Ok(None)` only when there was no read context and nothing to attach.
pub fn attach_layers(
    context: Option<PrintContext>,
    extra: Vec<HeldLayer>,
    transformer: &str,
    network: &Value,
    buffer_m: f64,
) -> Result<Option<PrintContext>, Failure> {
    let coverage = buffered_coverage(transformer, network, buffer_m)?;
    let mut document = match context.as_ref().and_then(|c| c.document.as_deref()) {
        Some(bytes) => serde_json::from_slice::<Value>(bytes).map_err(crate::store)?,
        None => json!({"coverage": coverage, "layers": {}, "sources": []}),
    };
    let mut layers: BTreeMap<String, Value> =
        serde_json::from_value(document["layers"].clone()).map_err(crate::store)?;
    let held_ids: Vec<String> = extra.iter().map(|layer| layer.id.clone()).collect();
    for HeldLayer {
        id,
        collection,
        mut source,
    } in extra
    {
        let features = collection["features"]
            .as_array()
            .ok_or_else(|| invalid(format!("held layer {id} is not a FeatureCollection")))?;
        if features.is_empty() {
            continue;
        }
        if layers.contains_key(&id) {
            return Err(invalid(format!(
                "held layer {id} collides with another print source"
            )));
        }
        let entry = source
            .as_object_mut()
            .ok_or_else(|| invalid(format!("held layer {id} has no source entry")))?;
        entry.insert("layer".to_owned(), json!(id));
        document["sources"]
            .as_array_mut()
            .ok_or_else(|| crate::store("print context has no source inventory"))?
            .push(source);
        layers.insert(
            id,
            json!({"type": "FeatureCollection", "features": features}),
        );
    }
    let (mut omitted, warnings, reused) = match context {
        Some(context) => (context.omitted, context.warnings, context.reused),
        None => (Vec::new(), Vec::new(), true),
    };
    omitted.retain(|omission: &Omission| !held_ids.contains(&omission.layer));
    if layers.is_empty() {
        // Nothing to print with: the read's own facts come back untouched,
        // or nothing when there was no read.
        return Ok(if omitted.is_empty() && warnings.is_empty() {
            None
        } else {
            Some(PrintContext {
                document: None,
                sha256: None,
                layers: Vec::new(),
                omitted,
                warnings,
                reused,
            })
        });
    }
    let bytes = print_context_document(&document["coverage"], &document["sources"], &layers)
        .map_err(|error| {
            if error.contains("over the") {
                Failure::TooLarge(error)
            } else {
                Failure::Store(error)
            }
        })?;
    let sha256 = sha256_hex(&bytes);
    Ok(Some(PrintContext {
        layers: layers.into_keys().collect(),
        document: Some(bytes),
        sha256: Some(sha256),
        omitted,
        warnings,
        reused,
    }))
}

/// The catalogue rows this machine's held rooms stand in for, when the
/// catalogue itself cannot be read: identity and layer only, from the room's
/// own dataset parameters. Never downloadable, never verified — a read-only
/// stand-in that lets held subsets print. An unreadable inventory stands in
/// for nothing.
pub fn held_catalog_rooms(root: &Path, scope: &Scope) -> Vec<ReferenceResource> {
    let Ok(inventory) = ds_layer_store::project_dataset_cache::project_inventory(
        root,
        &scope.principal,
        &scope.project,
    ) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for room in inventory["rooms"].as_array().into_iter().flatten() {
        if room["provider"] != REFERENCE_CACHE_PROVIDER {
            continue;
        }
        let (Some(id), Some(layer), Some(country)) = (
            room["dataset_id"].as_str(),
            room["parameters"]["layer"].as_str(),
            room["parameters"]["country"].as_str(),
        ) else {
            continue;
        };
        rows.push(ReferenceResource {
            id: id.to_string(),
            version: room["source_version"].as_str().unwrap_or("").to_string(),
            label: layer.to_string(),
            layer: layer.to_string(),
            country: country.to_string(),
            kind: "bigquery".to_string(),
            active: true,
            ..Default::default()
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datasets::{buildings_dataset, catalog_dataset};
    use ds_command_kernel::project_dataset_cache::{dataset_key, BufferPolicy, Dataset};

    fn network() -> Value {
        json!({"tr": {"type": "FeatureCollection", "features": [{
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [30.0, -2.0]},
            "properties": {"transfo": "tx_a"}
        }]}})
    }

    fn feature(name: &str) -> Value {
        json!({"type": "Feature", "properties": {"name": name},
            "geometry": {"type": "Point", "coordinates": [30.0005, -2.0005]}})
    }

    fn collection(features: Vec<Value>) -> Value {
        json!({"type": "FeatureCollection", "features": features})
    }

    fn held(id: &str, features: Vec<Value>, kind: &str) -> HeldLayer {
        HeldLayer {
            id: id.into(),
            collection: collection(features),
            source: json!({"kind": kind, "models": [{"id": "mv-model", "revision": "rev-1"}]}),
        }
    }

    /// A read context over one catalogue layer under its own coverage.
    fn read_context() -> (PrintContext, Value) {
        let coverage = json!({"type": "Polygon", "coordinates": [[
            [29.9, -2.1], [30.1, -2.1], [30.1, -1.9], [29.9, -1.9], [29.9, -2.1]]]});
        let layers = BTreeMap::from([("buildings".to_owned(), collection(vec![feature("b1")]))]);
        let sources = json!([{"layer": "buildings", "kind": "catalog", "dataset_id": "d"}]);
        let bytes = print_context_document(&coverage, &sources, &layers).unwrap();
        let context = PrintContext {
            sha256: Some(sha256_hex(&bytes)),
            document: Some(bytes),
            layers: vec!["buildings".into()],
            omitted: vec![
                Omission {
                    layer: "dsgrid_mv_lines".into(),
                    reason: "desktop_only".into(),
                },
                Omission {
                    layer: "dsgrid_mv_structures".into(),
                    reason: "desktop_only".into(),
                },
                Omission {
                    layer: "contours_index".into(),
                    reason: "no features in the design's extent".into(),
                },
            ],
            warnings: vec!["contours: thin".into()],
            reused: true,
        };
        (context, coverage)
    }

    #[test]
    fn attach_layers_merges_held_layers_under_one_coverage_and_refuses_an_id_collision() {
        let (context, coverage) = read_context();
        let extra = vec![
            held(
                "dsgrid_mv_lines",
                vec![feature("line")],
                "project_dsgrid_mv",
            ),
            // Empty held collections are not attached, yet the read's
            // omission for them is dropped: the host held the layer.
            held("dsgrid_mv_structures", vec![], "project_dsgrid_mv"),
        ];
        let merged = attach_layers(Some(context), extra, "tx_a", &network(), 200.0)
            .unwrap()
            .unwrap();
        let document: Value = serde_json::from_slice(merged.document.as_ref().unwrap()).unwrap();
        assert_eq!(document["schema"], "ds.print-context/v1");
        assert_eq!(document["coverage"], coverage, "the read coverage is kept");
        assert_eq!(merged.layers, vec!["buildings", "dsgrid_mv_lines"]);
        assert_eq!(
            document["layers"]["dsgrid_mv_lines"]["features"][0]["properties"]["name"],
            "line"
        );
        assert_eq!(
            document["layers"]["buildings"]["features"][0]["properties"]["name"],
            "b1"
        );
        assert!(document["layers"].get("dsgrid_mv_structures").is_none());
        assert_eq!(document["sources"].as_array().unwrap().len(), 2);
        assert_eq!(document["sources"][1]["layer"], "dsgrid_mv_lines");
        assert_eq!(document["sources"][1]["kind"], "project_dsgrid_mv");
        assert_eq!(document["sources"][1]["models"][0]["revision"], "rev-1");
        assert_eq!(
            merged.sha256.as_deref(),
            Some(sha256_hex(merged.document.as_ref().unwrap()).as_str())
        );
        assert_eq!(
            merged.omitted,
            vec![Omission {
                layer: "contours_index".into(),
                reason: "no features in the design's extent".into(),
            }]
        );
        assert_eq!(merged.warnings, vec!["contours: thin".to_string()]);
        assert!(merged.reused);

        // Without a read document the coverage is the buffered design extent.
        let fresh = attach_layers(
            None,
            vec![held(
                "dsgrid_mv_lines",
                vec![feature("line")],
                "project_dsgrid_mv",
            )],
            "tx_a",
            &network(),
            200.0,
        )
        .unwrap()
        .unwrap();
        let document: Value = serde_json::from_slice(fresh.document.as_ref().unwrap()).unwrap();
        let ring = document["coverage"]["coordinates"][0].as_array().unwrap();
        assert_eq!(ring.len(), 5);
        assert!(ring[0][0].as_f64().unwrap() < 30.0 && ring[1][0].as_f64().unwrap() > 30.0);
        assert!(ring[0][1].as_f64().unwrap() < -2.0 && ring[2][1].as_f64().unwrap() > -2.0);
        assert_eq!(fresh.layers, vec!["dsgrid_mv_lines"]);
        assert!(fresh.omitted.is_empty());

        // Nothing read and nothing held is no context at all.
        assert_eq!(
            attach_layers(None, Vec::new(), "tx_a", &network(), 200.0)
                .unwrap()
                .map(|c| c.layers),
            None
        );

        // A held id the document already carries is refused, never overwritten.
        let (context, _) = read_context();
        let refused = attach_layers(
            Some(context),
            vec![held("buildings", vec![feature("other")], "local_layer")],
            "tx_a",
            &network(),
            200.0,
        )
        .unwrap_err();
        assert_eq!(refused.code(), "print_context_unsupported");
        assert!(refused.message().contains("collides"), "{refused}");

        // A design with no geographic extent cannot be given a coverage.
        let unlocated = attach_layers(
            None,
            vec![held(
                "dsgrid_mv_lines",
                vec![feature("line")],
                "project_dsgrid_mv",
            )],
            "tx_a",
            &json!({"tr": {"type": "FeatureCollection", "features": []}}),
            200.0,
        )
        .unwrap_err();
        assert_eq!(unlocated.code(), "print_context_unsupported");
    }

    fn open_room(root: &Path, scope: &Scope, dataset: &Dataset, source_version: &str) {
        let document = serde_json::to_value(dataset).unwrap();
        ds_layer_store::project_dataset_cache::execute_at(
            root,
            json!({
                "action": "open",
                "dataset_key": dataset_key(scope, dataset),
                "principal": scope.principal,
                "project": scope.project,
                "dataset_id": document["id"],
                "provider": document["provider"],
                "quality": document["quality"],
                "parameters": document["parameters"],
                "crs": document["crs"],
                "source_version": source_version,
                "buffer_policy": BufferPolicy::default(),
            }),
        )
        .unwrap();
    }

    #[test]
    fn held_catalog_rooms_stand_in_for_an_unreadable_catalogue() {
        let temp = tempfile::tempdir().unwrap();
        let scope = Scope {
            principal: "uid-1".into(),
            project: "project-a".into(),
        };
        // No inventory yet: nothing stands in.
        assert!(held_catalog_rooms(temp.path(), &scope).is_empty());

        let resource = ReferenceResource {
            id: "a".repeat(64),
            version: "b".repeat(64),
            layer: "roads".into(),
            country: "RW".into(),
            ..Default::default()
        };
        open_room(temp.path(), &scope, &catalog_dataset(&resource), "v7");
        // A derived room is not a catalogue resource.
        open_room(temp.path(), &scope, &buildings_dataset(), "v1");
        // Another project's held subset stays that project's.
        let other = Scope {
            principal: "uid-1".into(),
            project: "project-b".into(),
        };
        let foreign = ReferenceResource {
            id: "c".repeat(64),
            layer: "rivers".into(),
            country: "RW".into(),
            ..Default::default()
        };
        open_room(temp.path(), &other, &catalog_dataset(&foreign), "v2");

        let rows = held_catalog_rooms(temp.path(), &scope);
        assert_eq!(
            rows,
            vec![ReferenceResource {
                id: "a".repeat(64),
                version: "v7".into(),
                label: "roads".into(),
                layer: "roads".into(),
                country: "RW".into(),
                kind: "bigquery".into(),
                active: true,
                ..Default::default()
            }]
        );
        // The stand-in resolves the `catalog` source the way a read row does,
        // and carries no bundle to download or verify.
        assert!(crate::catalog::find(&rows, "RW", "roads").is_some());
        assert!(rows[0].download_url.is_none());
        assert_eq!(held_catalog_rooms(temp.path(), &other).len(), 1);
    }
}
