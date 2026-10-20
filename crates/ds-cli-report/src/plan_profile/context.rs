//! The MV booklet's default geographic context, read from what this machine holds.
//!
//! The Kernel's `printing::mv_context` names the families and assembles each
//! capture; this host only executes the reads. Every source is held: the
//! project's dataset rooms through `Mode::Read`, and its saved design rooms
//! through the report hold. Nothing reaches a provider or the service, and an
//! unheld family is a named omission in the receipt, never a reason to fetch.
use super::*;
use ds_command_kernel::printing::{
    context::CatalogResource, mv_context, project_sources::Sources, resolved_styles::VectorLayer,
};
use ds_command_kernel::project_dataset_cache::Scope;
use ds_command_kernel::style_resolution::Snapshot;

pub(super) const CONTEXT_UNAVAILABLE: Refusal = Refusal {
    code: "mv_context_unavailable",
    when: "held geographic context or design rooms cannot be read or assembled for the booklet",
    remedy: "repair the named held room, or pass explicit --context-pages",
};

/// The captures one print consumes, alive until the Reporter has read them.
pub(super) struct Acquired {
    pub files: Vec<PathBuf>,
    pub key_plan: Option<PathBuf>,
    pub receipt: Value,
    _stage: tempfile::TempDir,
}

fn unavailable(message: impl std::fmt::Display) -> Failure {
    Failure::failed(CONTEXT_UNAVAILABLE.code, message.to_string())
        .remedy(CONTEXT_UNAVAILABLE.remedy)
}

/// One printable route: its native id, its WGS84 feature and bounds.
pub(super) struct Route {
    pub id: String,
    pub feature: Value,
    pub bounds: [f64; 4],
}

/// Every alignment of the projected model with its WGS84 route bounds.
pub(super) fn routes(projection: &Value) -> Result<Vec<Route>, String> {
    let mut routes = Vec::new();
    for feature in projection["features"]
        .as_array()
        .ok_or("MV projection has no features")?
    {
        if feature["properties"]["_layer"] != "alignments" {
            continue;
        }
        let id = feature["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("MV projection alignment has no id")?;
        if let Some(bounds) = ds_geo_lite::viewport::geojson_bounds(feature) {
            routes.push(Route {
                id: id.to_owned(),
                feature: feature.clone(),
                bounds,
            });
        }
    }
    Ok(routes)
}

pub(super) fn expand(bounds: [f64; 4], margin: f64) -> [f64; 4] {
    [
        bounds[0] - margin,
        bounds[1] - margin,
        bounds[2] + margin,
        bounds[3] + margin,
    ]
}

fn union_bounds(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn union(routes: &[Route]) -> Option<[f64; 4]> {
    routes.iter().map(|route| route.bounds).reduce(union_bounds)
}

/// The design families the booklet prints, kept only where they meet `area`.
fn design_layers(layers: &Value, area: [f64; 4]) -> Result<Value, String> {
    let [w, s, e, n] = area;
    let polygon = json!({"type":"Polygon","coordinates":[[[w,s],[e,s],[e,n],[w,n],[w,s]]]});
    let mut kept = serde_json::Map::new();
    for id in mv_context::SHEET_DESIGN
        .iter()
        .chain(mv_context::KEY_PLAN_DESIGN.iter())
    {
        if kept.contains_key(*id) {
            continue;
        }
        let features = layers[*id]["features"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|feature| {
                !feature["geometry"].is_null()
                    && ds_geo_lite::spatial_predicate::intersects_area(
                        &feature["geometry"],
                        &polygon,
                    )
                    .unwrap_or(false)
            })
            .cloned()
            .collect::<Vec<_>>();
        kept.insert(
            (*id).to_owned(),
            json!({"type":"FeatureCollection","features":features}),
        );
    }
    Ok(Value::Object(kept))
}

fn vector_layers(rows: Vec<Value>, families: &[&str]) -> Result<Vec<VectorLayer>, String> {
    rows.into_iter()
        .filter(|row| row["id"].as_str().is_some_and(|id| families.contains(&id)))
        .map(|row| serde_json::from_value(row).map_err(|e| e.to_string()))
        .collect()
}

fn counts(layers: &[VectorLayer]) -> Value {
    layers
        .iter()
        .map(|layer| {
            (
                layer.id.clone(),
                json!(layer.geojson["features"].as_array().map_or(0, Vec::len)),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

fn omissions(context: &ds_project_data::PrintContext) -> Value {
    context
        .omitted
        .iter()
        .map(|omission| json!({"layer":omission.layer,"reason":omission.reason}))
        .collect()
}

/// Read the held context of every route and assemble the Kernel's captures.
pub(super) fn acquire(
    lane: &str,
    project: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    projection: &Value,
    styles: &Snapshot,
) -> Result<Acquired, Failure> {
    let routes = routes(projection).map_err(unavailable)?;
    let Some(model_bounds) = union(&routes) else {
        return Err(unavailable("the model has no geographic route"));
    };
    // The project dataset rooms live under the machine-shared report root.
    let root = ds_report_host::shared_root().map_err(unavailable)?;
    let scope = Scope {
        principal: identity.uid().into(),
        project: project.into(),
    };
    // The held rooms stand in for the catalogue: no catalogue read is made.
    let held = ds_project_data::held::held_catalog_rooms(&root, &scope);
    let resources = held
        .iter()
        .map(|row| CatalogResource {
            layer: row.layer.clone(),
            label: row.label.clone(),
            country: row.country.clone(),
            ready: true,
            ..Default::default()
        })
        .collect::<Vec<_>>();
    // The Kernel frames the key plan; the same rectangle bounds what is read.
    let key_plan_area = mv_context::key_plan_extent(model_bounds);
    let design_area = union_bounds(
        key_plan_area,
        expand(model_bounds, mv_context::SHEET_MARGIN_DEG),
    );
    // Saved design rooms this machine holds; no head is read from the service.
    let names = crate::project::hold::transformer_room_inventory(identity, project)?
        .iter()
        .filter_map(|row| row["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let mut design = Sources::default();
    let mut design_error = None;
    let rooms = crate::project::hold::acquire_transformer_rooms(
        lane,
        project,
        identity,
        &names,
        None,
        |room, _source| {
            let layers = design_layers(&json!(room.layers), design_area).map_err(unavailable)?;
            if let Err(error) = design.transformer(&room.transformer, &layers) {
                design_error.get_or_insert(error);
            }
            Ok(())
        },
    )?;
    if let Some(error) = design_error {
        return Err(unavailable(error));
    }
    let stage = tempfile::tempdir().map_err(unavailable)?;
    let mut files = Vec::with_capacity(routes.len());
    let mut alignments = Vec::with_capacity(routes.len());
    let sheet_sources = mv_context::sheet_sources(&resources);
    for (index, route) in routes.iter().enumerate() {
        let network = json!({"mv_route":{"type":"FeatureCollection","features":[route.feature]}});
        let context = ds_project_data::read_print_context(
            &root,
            &scope,
            &route.id,
            &network,
            &sheet_sources,
            &held,
            ds_project_data::Mode::Read,
        )
        .map_err(|error| unavailable(format!("{}: {error}", route.id)))?;
        let area = expand(route.bounds, mv_context::SHEET_MARGIN_DEG);
        let mut sources = Sources::default();
        if let Some(bytes) = &context.document {
            let document: Value = serde_json::from_slice(bytes).map_err(unavailable)?;
            sources
                .collections(&document["layers"])
                .map_err(unavailable)?;
        }
        let mut layers = vector_layers(
            sources.vectors_in_area(area).map_err(unavailable)?,
            &[
                &mv_context::SHEET_CATALOG[..],
                &[
                    "buildings_context",
                    "contours_intermediate",
                    "contours_index",
                ],
            ]
            .concat(),
        )
        .map_err(unavailable)?;
        layers.extend(
            vector_layers(
                design.vectors_in_area(area).map_err(unavailable)?,
                &mv_context::SHEET_DESIGN,
            )
            .map_err(unavailable)?,
        );
        let carried = counts(&layers);
        let capture = mv_context::assemble(project, &route.id, layers, styles)
            .map_err(|error| unavailable(format!("{}: {error}", route.id)))?;
        let path = stage.path().join(format!("alignment-{index:04}.json"));
        std::fs::write(&path, serde_json::to_vec(&capture).map_err(unavailable)?)
            .map_err(unavailable)?;
        alignments.push(json!({"alignment_id":route.id,"context_sha256":capture.context_sha256,"layers":carried,"omitted":omissions(&context)}));
        files.push(path);
    }
    let network = json!({"mv_route":{"type":"FeatureCollection",
        "features":routes.iter().map(|route| route.feature.clone()).collect::<Vec<_>>()}});
    let key_sources = mv_context::key_plan_sources(&resources);
    let key_context = ds_project_data::read_print_context(
        &root,
        &scope,
        "key_plan",
        &network,
        &key_sources,
        &held,
        ds_project_data::Mode::Read,
    )
    .map_err(|error| unavailable(format!("key plan: {error}")))?;
    let mut sources = Sources::default();
    if let Some(bytes) = &key_context.document {
        let document: Value = serde_json::from_slice(bytes).map_err(unavailable)?;
        sources
            .collections(&document["layers"])
            .map_err(unavailable)?;
    }
    let mut layers = vector_layers(
        sources
            .vectors_in_area(key_plan_area)
            .map_err(unavailable)?,
        &mv_context::KEY_PLAN_CATALOG,
    )
    .map_err(unavailable)?;
    layers.extend(
        vector_layers(
            design.vectors_in_area(key_plan_area).map_err(unavailable)?,
            &mv_context::KEY_PLAN_DESIGN,
        )
        .map_err(unavailable)?,
    );
    let carried = counts(&layers);
    let key_plan = mv_context::assemble_key_plan(project, layers, styles)
        .map_err(|error| unavailable(format!("key plan: {error}")))?;
    let key_plan_path = stage.path().join("key-plan.json");
    std::fs::write(
        &key_plan_path,
        serde_json::to_vec(&key_plan).map_err(unavailable)?,
    )
    .map_err(unavailable)?;
    let failures = rooms
        .failures
        .iter()
        .map(|(name, failure)| json!({"transformer":name,"code":failure.code()}))
        .collect::<Vec<_>>();
    let receipt = json!({
        "source": "held",
        "catalog_rooms": held.len(),
        "design_rooms": {"held": names.len(), "unreadable": failures},
        "alignments": alignments,
        "key_plan": {"context_sha256": key_plan.context_sha256, "layers": carried, "omitted": omissions(&key_context)},
    });
    Ok(Acquired {
        files,
        key_plan: Some(key_plan_path),
        receipt,
        _stage: stage,
    })
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
