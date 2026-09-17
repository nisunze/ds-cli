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
//!
//! [`preview_context`] is the one resolver a preview goes through, on every
//! host: the host reads what its stores hold for the draft's context layers
//! — captures, retained preparations, rooms — and hands it over as
//! [`HeldPreviewState`]; every decision over it is a kernel call
//! (`held_context::admit`, `retained_admission`) or a `Mode::Read` of the
//! rooms, and nothing is ever acquired for a preview.

use ds_command_kernel::printing::context::{
    printed_layer_ids, retained_admission, RetainedContext, RetainedVerdict, RETAINED_SCHEMA,
};
use ds_command_kernel::printing::held_context::{admit, HeldCapture};
use ds_command_kernel::printing::preview::Refusal;
use ds_command_kernel::printing::{PrintContextLayer, PrintContextSource};
use ds_command_kernel::project_dataset_cache::{Scope, REFERENCE_CACHE_PROVIDER};
use ds_command_kernel::report_export::{print_context_document, sha256_hex};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

use crate::catalog::ReferenceResource;
use crate::ensure::Mode;
use crate::print_context::{
    buffer_of, document_failure, read_print_context, Omission, PrintContext, SourceKind,
};
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
/// buffering, as `[west, south, east, north]`.
fn buffered_bounds(transformer: &str, network: &Value, buffer_m: f64) -> Result<[f64; 4], Failure> {
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
    serde_json::from_value(answer["buffered"].clone())
        .map_err(|_| invalid("the design has no geographic extent"))
}

/// The buffered design extent as the coverage polygon a fresh document
/// starts from.
fn buffered_coverage(transformer: &str, network: &Value, buffer_m: f64) -> Result<Value, Failure> {
    let [w, s, e, n] = buffered_bounds(transformer, network, buffer_m)?;
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
        .map_err(document_failure)?;
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

/// What a host holds for a preview's context layers, read from its own
/// stores without a decision: the survey and local-layer captures, by the
/// context layer id each answers, and the derived preparations it retained,
/// keyed by context layer id. A layer with no entry here is simply not held.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeldPreviewState {
    pub captures: Vec<HeldCapture>,
    pub retained: BTreeMap<String, RetainedContext>,
}

/// Why a preview's context could not be resolved: a kernel refusal over a
/// handed capture (its code and message key travel unchanged), or a failure
/// of the holdings read or the document merge.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewContextRefusal {
    Kernel(Refusal),
    Holdings(Failure),
}

impl PreviewContextRefusal {
    /// The refusal code a command declares for this refusal.
    pub fn code(&self) -> &str {
        match self {
            PreviewContextRefusal::Kernel(refusal) => &refusal.code,
            PreviewContextRefusal::Holdings(failure) => failure.code(),
        }
    }
}

impl From<Failure> for PreviewContextRefusal {
    fn from(failure: Failure) -> Self {
        PreviewContextRefusal::Holdings(failure)
    }
}

/// The reason a layer is omitted when the host hands no capture for it.
const NOT_CAPTURED: &str = "not captured on this host";
/// The reason every room-held layer is omitted when the host has no holdings
/// root to read from.
const NO_HOLDINGS_ROOT: &str = "no holdings root on this host";

/// The source-inventory entry an admitted capture prints under: the source
/// as the draft spells it (kind and identity) with the capture's declared
/// count beside it.
fn capture_source(source: &PrintContextSource, capture: &HeldCapture) -> Result<Value, Failure> {
    let mut entry = serde_json::to_value(source).map_err(crate::store)?;
    let fields = entry
        .as_object_mut()
        .ok_or_else(|| crate::store("a context source is not an object"))?;
    fields.insert("declared_count".to_owned(), json!(capture.declared_count));
    if let Some(project) = capture.source_project.as_deref() {
        fields.insert("source_project".to_owned(), json!(project));
    }
    Ok(entry)
}

/// Resolve the print context one preview prints with, from what this host
/// holds and nothing else.
///
/// For each wanted layer, by its source kind: a survey or local layer prints
/// from the capture handed under its id, admitted by
/// `held_context::admit` — a capture the kernel refuses ends the preview with
/// that refusal, a layer with no capture is a named omission; an elevation
/// contours or project MV layer prints from the preparation retained under
/// its id when `retained_admission` lets it stand in for the design's
/// buffered extent — otherwise the MV projection is a named omission and
/// contours fall back to the project's rooms; a catalogue, building or
/// unretained contours layer is read from the rooms under `root` in
/// `Mode::Read`, which omits what is not held and never acquires; without a
/// root every such layer is omitted as unreadable here. What was read and
/// what was held merge through [`attach_layers`] into one
/// `ds.print-context/v1` document under one digest.
///
/// `Ok(None)` when nothing was wanted, or nothing could be supplied and
/// nothing needs naming.
#[allow(clippy::too_many_arguments)]
pub fn preview_context(
    root: Option<&Path>,
    scope: &Scope,
    project: &str,
    transformer: &str,
    layers: &Value,
    wanted: &[PrintContextLayer],
    catalog: &[ReferenceResource],
    held: &HeldPreviewState,
) -> Result<Option<PrintContext>, PreviewContextRefusal> {
    if wanted.is_empty() {
        return Ok(None);
    }
    let mut held_layers: Vec<HeldLayer> = Vec::new();
    let mut to_read: Vec<PrintContextLayer> = Vec::new();
    let mut omitted: Vec<Omission> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut widest = 0.0_f64;
    for layer in wanted {
        widest = widest.max(buffer_of(&layer.source));
        match &layer.source {
            PrintContextSource::Survey { .. } | PrintContextSource::LocalLayer { .. } => {
                let capture = held
                    .captures
                    .iter()
                    .find(|capture| capture.context_id == layer.id);
                let Some(capture) = capture else {
                    omitted.push(Omission {
                        layer: layer.id.clone(),
                        reason: NOT_CAPTURED.into(),
                    });
                    continue;
                };
                let collection = admit(project, &layer.source, capture)
                    .map_err(PreviewContextRefusal::Kernel)?;
                held_layers.push(HeldLayer {
                    id: layer.id.clone(),
                    collection,
                    source: capture_source(&layer.source, capture)?,
                });
            }
            PrintContextSource::ElevationContours { .. }
            | PrintContextSource::ProjectDsgridMv { .. } => {
                let bounds = buffered_bounds(transformer, layers, buffer_of(&layer.source))?;
                let retained = held.retained.get(&layer.id);
                match retained_admission(layer, retained, bounds) {
                    RetainedVerdict::Use {
                        layers: prepared,
                        warnings: retained_warnings,
                    } => {
                        let source_version = retained
                            .map(|entry| entry.source_version.clone())
                            .unwrap_or_default();
                        for (id, collection) in prepared {
                            held_layers.push(HeldLayer {
                                id,
                                collection,
                                source: json!({
                                    "kind": layer.source_kind(),
                                    "retained": RETAINED_SCHEMA,
                                    "source_version": source_version,
                                }),
                            });
                        }
                        warnings.extend(retained_warnings);
                    }
                    RetainedVerdict::Omit { reason } => {
                        if matches!(layer.source, PrintContextSource::ProjectDsgridMv { .. }) {
                            omitted.extend(printed_layer_ids(layer).into_iter().map(|id| {
                                Omission {
                                    layer: id,
                                    reason: reason.clone(),
                                }
                            }));
                            continue;
                        }
                        if retained.is_some() {
                            warnings.push(format!("{}: {reason}", layer.label));
                        }
                        to_read.push(layer.clone());
                    }
                }
            }
            PrintContextSource::Catalog { .. } | PrintContextSource::GoogleOpenBuildings { .. } => {
                to_read.push(layer.clone());
            }
        }
    }
    let read = match (root, to_read.is_empty()) {
        (_, true) => None,
        (Some(root), false) => Some(read_print_context(
            root,
            scope,
            transformer,
            layers,
            &to_read,
            catalog,
            Mode::Read,
        )?),
        (None, false) => {
            omitted.extend(
                to_read
                    .iter()
                    .flat_map(printed_layer_ids)
                    .map(|id| Omission {
                        layer: id,
                        reason: NO_HOLDINGS_ROOT.into(),
                    }),
            );
            None
        }
    };
    let read = match read {
        Some(mut context) => {
            context.omitted.extend(omitted);
            context.warnings.extend(warnings);
            Some(context)
        }
        None if omitted.is_empty() && warnings.is_empty() => None,
        None => Some(PrintContext {
            document: None,
            sha256: None,
            layers: Vec::new(),
            omitted,
            warnings,
            reused: true,
        }),
    };
    Ok(attach_layers(
        read,
        held_layers,
        transformer,
        layers,
        widest,
    )?)
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

    #[test]
    fn a_merge_over_the_engines_feature_bound_is_too_large_not_a_defect() {
        let over = ds_command_kernel::report_export::MAX_PRINT_CONTEXT_FEATURES + 1;
        let refused = attach_layers(
            None,
            vec![held("survey_poles", vec![feature("p"); over], "survey")],
            "tx_a",
            &network(),
            200.0,
        )
        .unwrap_err();
        assert_eq!(refused.code(), "print_context_too_large", "{refused}");
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

    fn scope() -> Scope {
        Scope {
            principal: "uid-1".into(),
            project: "project-a".into(),
        }
    }

    fn survey_layer() -> PrintContextLayer {
        PrintContextLayer {
            id: "survey_poles".into(),
            label: "Poles".into(),
            source: PrintContextSource::Survey {
                form: "poles".into(),
            },
        }
    }

    fn local_layer() -> PrintContextLayer {
        PrintContextLayer {
            id: "sketch_1".into(),
            label: "Sketch".into(),
            source: PrintContextSource::LocalLayer {
                layer_id: "sketch_1".into(),
            },
        }
    }

    fn mv_layer() -> PrintContextLayer {
        PrintContextLayer {
            id: "project_dsgrid_mv".into(),
            label: "Proposed MV".into(),
            source: PrintContextSource::ProjectDsgridMv {
                buffer_m: 200.0,
                corridor: None,
            },
        }
    }

    fn contours_layer() -> PrintContextLayer {
        PrintContextLayer {
            id: "elevation_contours".into(),
            label: "Contours".into(),
            source: PrintContextSource::ElevationContours {
                buffer_m: 200.0,
                minor_interval_m: 10,
                index_interval_m: 50,
                sample_spacing_m: 10.0,
            },
        }
    }

    fn catalog_layer() -> PrintContextLayer {
        PrintContextLayer {
            id: "roads".into(),
            label: "Roads".into(),
            source: PrintContextSource::Catalog {
                country: "RW".into(),
                layer: "roads".into(),
            },
        }
    }

    fn capture(context_id: &str, count: usize) -> HeldCapture {
        HeldCapture {
            context_id: context_id.into(),
            features: collection((0..count).map(|i| feature(&format!("f{i}"))).collect()),
            declared_count: count as u64,
            render_mode: Some("rows".into()),
            source_project: Some("project-a".into()),
            features_loaded: true,
        }
    }

    /// A retained preparation of `layer` over the whole design, as the
    /// browser's v2 entry spells it.
    fn retained_for(layer: &PrintContextLayer, layers: Value) -> RetainedContext {
        RetainedContext {
            schema: RETAINED_SCHEMA.into(),
            parameters: ds_command_kernel::printing::context::retained_parameters(&layer.source)
                .unwrap(),
            coverage: [29.0, -3.0, 31.0, -1.0],
            layers,
            source_version: "mv-rev-7".into(),
            warnings: vec!["mv: one model skipped".into()],
        }
    }

    fn resolve(
        root: Option<&Path>,
        wanted: &[PrintContextLayer],
        held: &HeldPreviewState,
    ) -> Result<Option<PrintContext>, PreviewContextRefusal> {
        preview_context(
            root,
            &scope(),
            "project-a",
            "tx_a",
            &network(),
            wanted,
            &[],
            held,
        )
    }

    fn omission(layer: &str, reason: &str) -> Omission {
        Omission {
            layer: layer.into(),
            reason: reason.into(),
        }
    }

    #[test]
    fn a_capture_the_kernel_refuses_ends_the_preview_with_that_code() {
        // A survey capture with nothing synchronised.
        let mut empty = capture("survey_poles", 0);
        empty.declared_count = 0;
        let held = HeldPreviewState {
            captures: vec![empty],
            ..Default::default()
        };
        let refused = resolve(None, &[survey_layer()], &held).unwrap_err();
        assert_eq!(refused.code(), "print_context_survey_empty");
        let PreviewContextRefusal::Kernel(refusal) = refused else {
            panic!("a kernel refusal, not a holdings failure");
        };
        assert_eq!(refusal.message_key, "printing_context_survey_empty");
        assert_eq!(refusal.params["name"], "poles");

        // A local layer held as a display preview is not printable data.
        let mut preview = capture("sketch_1", 2);
        preview.render_mode = Some("preview".into());
        let held = HeldPreviewState {
            captures: vec![preview],
            ..Default::default()
        };
        let refused = resolve(None, &[local_layer()], &held).unwrap_err();
        assert_eq!(refused.code(), "print_context_too_large");
        assert!(matches!(refused, PreviewContextRefusal::Kernel(_)));

        // A capture from another project is refused under its own key.
        let mut foreign = capture("sketch_1", 2);
        foreign.source_project = Some("project-b".into());
        let held = HeldPreviewState {
            captures: vec![foreign],
            ..Default::default()
        };
        assert_eq!(
            resolve(None, &[local_layer()], &held).unwrap_err().code(),
            "print_context_foreign_project"
        );
    }

    #[test]
    fn a_missing_capture_is_an_omission_and_an_admitted_one_prints() {
        // Nothing handed for the survey: named, never refused.
        let none = resolve(None, &[survey_layer()], &HeldPreviewState::default())
            .unwrap()
            .unwrap();
        assert!(none.document.is_none());
        assert_eq!(none.omitted, vec![omission("survey_poles", NOT_CAPTURED)]);

        // The capture handed under the layer's id prints under that id, with
        // the source spelled as the draft spells it.
        let held = HeldPreviewState {
            captures: vec![capture("other", 1), capture("survey_poles", 2)],
            ..Default::default()
        };
        let context = resolve(None, &[survey_layer()], &held).unwrap().unwrap();
        assert_eq!(context.layers, vec!["survey_poles"]);
        assert!(context.omitted.is_empty());
        let document: Value = serde_json::from_slice(context.document.as_ref().unwrap()).unwrap();
        assert_eq!(
            document["layers"]["survey_poles"]["features"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            document["sources"][0],
            json!({"layer": "survey_poles", "kind": "survey", "form": "poles", "declared_count": 2, "source_project": "project-a"})
        );
        assert_eq!(
            context.sha256.as_deref(),
            Some(sha256_hex(context.document.as_ref().unwrap()).as_str())
        );
        assert!(context.reused);
    }

    #[test]
    fn a_retained_preparation_prints_when_the_kernel_lets_it_stand_in() {
        let mv = mv_layer();
        let prepared = json!({
            "dsgrid_mv_lines": collection(vec![feature("line")]),
            "dsgrid_mv_structures": collection(vec![feature("pole")]),
            "dsgrid_mv_angle_points": collection(vec![]),
        });
        let held = HeldPreviewState {
            retained: BTreeMap::from([(mv.id.clone(), retained_for(&mv, prepared))]),
            ..Default::default()
        };
        let context = resolve(None, &[mv.clone()], &held).unwrap().unwrap();
        // Each emitted id is merged under its own id; the empty one is not
        // attached and not named — the host held it.
        assert_eq!(
            context.layers,
            vec!["dsgrid_mv_lines", "dsgrid_mv_structures"]
        );
        assert!(context.omitted.is_empty());
        assert_eq!(context.warnings, vec!["mv: one model skipped".to_string()]);
        let document: Value = serde_json::from_slice(context.document.as_ref().unwrap()).unwrap();
        assert_eq!(
            document["layers"]["dsgrid_mv_lines"]["features"][0]["properties"]["name"],
            "line"
        );
        assert_eq!(document["sources"].as_array().unwrap().len(), 2);
        assert_eq!(
            document["sources"][0],
            json!({"layer": "dsgrid_mv_lines", "kind": "project_dsgrid_mv", "retained": RETAINED_SCHEMA, "source_version": "mv-rev-7"})
        );

        // Retained with other parameters: the MV projection is omitted under
        // every id it prints as, with the kernel's reason.
        let mut other = retained_for(&mv, json!({}));
        other.parameters = "{\"buffer_m\":500,\"kind\":\"project_dsgrid_mv\"}".into();
        let held = HeldPreviewState {
            retained: BTreeMap::from([(mv.id.clone(), other)]),
            ..Default::default()
        };
        let context = resolve(None, &[mv.clone()], &held).unwrap().unwrap();
        assert!(context.document.is_none());
        assert_eq!(
            context.omitted,
            vec![
                omission("dsgrid_mv_lines", "retained with other parameters"),
                omission("dsgrid_mv_structures", "retained with other parameters"),
                omission("dsgrid_mv_angle_points", "retained with other parameters"),
            ]
        );

        // Nothing retained for the MV: omitted, never acquired.
        let context = resolve(None, &[mv], &HeldPreviewState::default())
            .unwrap()
            .unwrap();
        assert_eq!(
            context.omitted[0],
            omission("dsgrid_mv_lines", "not retained on this computer")
        );
        assert_eq!(context.omitted.len(), 3);
    }

    #[test]
    fn contours_fall_back_to_the_rooms_and_room_layers_need_a_root() {
        // Retained contours stand in without a room read.
        let contours = contours_layer();
        let prepared = json!({
            "contours_intermediate": collection(vec![feature("c10")]),
            "contours_index": collection(vec![feature("c50")]),
        });
        let held = HeldPreviewState {
            retained: BTreeMap::from([(contours.id.clone(), retained_for(&contours, prepared))]),
            ..Default::default()
        };
        let context = resolve(None, &[contours.clone()], &held).unwrap().unwrap();
        assert_eq!(
            context.layers,
            vec!["contours_index", "contours_intermediate"]
        );
        assert!(context.omitted.is_empty());

        // Retained under other parameters, contours fall through to the
        // rooms; with no holdings root on this host they are omitted as
        // such, and the retained entry's reason is kept as a warning.
        let mut other = retained_for(&contours, json!({}));
        other.parameters = "{}".into();
        let held = HeldPreviewState {
            retained: BTreeMap::from([(contours.id.clone(), other)]),
            ..Default::default()
        };
        let context = resolve(None, &[contours.clone(), catalog_layer()], &held)
            .unwrap()
            .unwrap();
        assert!(context.document.is_none());
        assert_eq!(
            context.omitted,
            vec![
                omission("contours_intermediate", NO_HOLDINGS_ROOT),
                omission("contours_index", NO_HOLDINGS_ROOT),
                omission("roads", NO_HOLDINGS_ROOT),
            ]
        );
        assert_eq!(
            context.warnings,
            vec!["Contours: retained with other parameters".to_string()]
        );
        assert!(context.reused);

        // With a root and empty rooms, `Mode::Read` names what is not held
        // and touches no provider — the resolver has none to touch.
        let temp = tempfile::tempdir().unwrap();
        let context = resolve(
            Some(temp.path()),
            &[contours, catalog_layer()],
            &HeldPreviewState::default(),
        )
        .unwrap()
        .unwrap();
        assert!(context.document.is_none());
        assert_eq!(context.omitted.len(), 3);
        assert_eq!(context.omitted[0].layer, "contours_intermediate");
        assert_eq!(context.omitted[0].reason, "project_dataset_not_held");
        assert_eq!(
            context.omitted[2],
            omission("roads", "this layer is absent from the geographic catalog")
        );
        assert!(context.reused);

        // Nothing wanted is no context, whatever the design's extent.
        assert!(resolve(None, &[], &HeldPreviewState::default())
            .unwrap()
            .is_none());
    }

    #[test]
    fn held_and_read_layers_merge_into_one_document() {
        // A survey capture and a retained MV projection, with a catalogue layer
        // omitted by the read, under one digest with every omission named.
        let mv = mv_layer();
        let temp = tempfile::tempdir().unwrap();
        let held = HeldPreviewState {
            captures: vec![capture("survey_poles", 1)],
            retained: BTreeMap::from([(
                mv.id.clone(),
                retained_for(
                    &mv,
                    json!({"dsgrid_mv_lines": collection(vec![feature("line")])}),
                ),
            )]),
        };
        let context = resolve(
            Some(temp.path()),
            &[survey_layer(), mv, catalog_layer()],
            &held,
        )
        .unwrap()
        .unwrap();
        assert_eq!(context.layers, vec!["dsgrid_mv_lines", "survey_poles"]);
        assert_eq!(
            context.omitted,
            vec![omission(
                "roads",
                "this layer is absent from the geographic catalog"
            )]
        );
        let document: Value = serde_json::from_slice(context.document.as_ref().unwrap()).unwrap();
        assert_eq!(document["schema"], "ds.print-context/v1");
        assert_eq!(document["sources"].as_array().unwrap().len(), 2);
        // A holdings failure surfaces as such: a design without an extent
        // cannot be given a coverage.
        let unlocated = preview_context(
            None,
            &scope(),
            "project-a",
            "tx_a",
            &json!({"tr": {"type": "FeatureCollection", "features": []}}),
            &[survey_layer()],
            &[],
            &held,
        )
        .unwrap_err();
        assert!(matches!(
            unlocated,
            PreviewContextRefusal::Holdings(Failure::Unsupported(_))
        ));
        assert_eq!(unlocated.code(), "print_context_unsupported");
    }
}
