//! Read-only native model acquisition. Engineering projection and CRS belong
//! to the captured Network engine; clipping and print provenance belong to the kernel.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Refusal;
use ds_command_kernel::{printing::map, report_export};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) const NETWORK_ENGINE_MISSING: Refusal = Refusal {
    code: "network_engine_missing",
    when: "MV context is requested but the Network executable is absent",
    remedy: "Install or activate a compatible Network engine for MV geographic projection",
};
pub(crate) const NETWORK_ENGINE_INCOMPATIBLE: Refusal = Refusal {
    code: "network_engine_incompatible",
    when: "MV context's captured Network executable or encoding profile cannot be admitted",
    remedy: "Install or activate a compatible Network engine for MV geographic projection",
};

pub(super) struct Model {
    identity: map::Model,
    projection: Value,
}
pub(super) fn overview(models: &[Model]) -> Result<Value, Failure> {
    let mut sources = ds_command_kernel::printing::project_sources::Sources::default();
    for model in models {
        let identity = map::Model {
            id: model.identity.id.clone(),
            label: model.identity.label.clone(),
            revision_id: model.identity.revision_id.clone(),
            digest: model.identity.digest.clone(),
        };
        // Complete projected model, including routes outside the LV transformers' envelope.
        let bounds = ds_command_kernel::project_design_extent::design_extent(
            "mv_data",
            &json!({"mv":model.projection}),
            0.00001,
        )
        .map_err(fail)?
        .bounds
        .ok_or_else(|| fail("MV model contains no geographic geometry"))?;
        sources
            .mv(&map::mv_layers(model.projection.clone(), identity, bounds).map_err(fail)?)
            .map_err(fail)?;
    }
    Ok(sources.layers())
}
/// The MV projections an earlier connected run kept beside this account's
/// project rooms. A copy whose checksum does not match reads as not held.
const HELD: &str = "mv-projections.json";
const HELD_SCHEMA: &str = "ds.report-mv-hold/v1";

pub(super) fn held(hold: &super::hold::Hold) -> Option<Vec<Model>> {
    let held: Value = serde_json::from_slice(&hold.read_file(HELD)?).ok()?;
    let models = held["models"].as_array()?;
    let body = serde_json::to_vec(models).ok()?;
    if held["schema"] != HELD_SCHEMA || held["sha256"] != report_export::sha256_hex(&body) {
        return None;
    }
    models
        .iter()
        .map(|model| {
            let text = |key: &str| model[key].as_str().map(str::to_string);
            Some(Model {
                identity: map::Model {
                    id: text("id")?,
                    label: text("label")?,
                    revision_id: text("revision_id")?,
                    digest: text("digest")?,
                },
                projection: model["projection"].clone(),
            })
        })
        .collect()
}

/// Keep the projections a connected run read, for the prints after it.
pub(super) fn hold_models(hold: &super::hold::Hold, models: &[Model]) -> Result<(), String> {
    let rows: Vec<Value> = models
        .iter()
        .map(|model| {
            json!({
                "id": model.identity.id, "label": model.identity.label,
                "revision_id": model.identity.revision_id, "digest": model.identity.digest,
                "projection": model.projection,
            })
        })
        .collect();
    let body = serde_json::to_vec(&rows).map_err(|error| error.to_string())?;
    let document =
        json!({"schema": HELD_SCHEMA, "sha256": report_export::sha256_hex(&body), "models": rows});
    let bytes = serde_json::to_vec(&document).map_err(|error| error.to_string())?;
    hold.keep_file(HELD, &bytes)
}

pub(super) fn provenance(models: &[Model]) -> Vec<Value> {
    models.iter().map(|model| json!({"model_id":model.identity.id,"revision_id":model.identity.revision_id,"sha256":model.identity.digest,"label":model.identity.label})).collect()
}
pub(super) fn load_local(path: &str, lane: &str) -> Result<Model, Failure> {
    let local = ds_project_data::mv_projection::load_local(Path::new(path), lane)
        .map_err(|error| projection_fail(&error))?;
    Ok(Model {
        identity: map::Model {
            id: local.model_id,
            label: "Local MV draft".into(),
            revision_id: local.revision_id,
            digest: local.sha256,
        },
        projection: local.projection,
    })
}
fn fail(e: impl std::fmt::Display) -> Failure {
    Failure::failed("print_context_invalid", e.to_string())
        .remedy("Check the exact project's promoted MV model and declared CRS; no model is modified by printing")
}
fn projection_fail(error: &ds_project_data::mv_projection::ProjectionError) -> Failure {
    if error.unavailable() {
        if error.code() == "network_engine_missing" {
            Failure::unavailable(NETWORK_ENGINE_MISSING.code, error.to_string())
        } else {
            Failure::unavailable(NETWORK_ENGINE_INCOMPATIBLE.code, error.to_string())
        }
        .remedy("Install or activate a compatible Network engine for MV geographic projection")
    } else {
        fail(error)
    }
}
/// The project's active MV models at their heads. A model `held` already
/// projects at the same revision and digest is reused: it is neither
/// downloaded nor projected again.
pub(super) fn load(
    lane: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    project: &str,
    held: &[Model],
) -> Result<Vec<Model>, Failure> {
    let mut cursor = None;
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    loop {
        let response = ds_cli_auth::grid_models_for_project(
            lane,
            project,
            &ds_cli_auth::GridModelsCommand::List {
                limit: 100,
                cursor: cursor.clone(),
                include_deleted: false,
            },
        )?;
        let data = response.data;
        rows.extend(
            data["models"]
                .as_array()
                .ok_or_else(|| fail("MV catalog has no model rows"))?
                .iter()
                .cloned(),
        );
        if rows.len() > 100 {
            return Err(fail("Print context supports up to 100 project MV models"));
        }
        if data["more"] == false {
            break;
        }
        let next = data["next_cursor"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| fail("MV catalog pagination is incomplete"))?
            .to_string();
        if !seen.insert(next.clone()) {
            return Err(fail("MV catalog repeated a cursor"));
        }
        cursor = Some(next);
    }
    let mut models = Vec::new();
    let mut features_total = 0;
    let mut projection_session = None;
    for row in rows {
        if row["model_kind"] != "mv_line" || row["state"] != "active" {
            continue;
        }
        let text = |key: &str| {
            row[key]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .ok_or_else(|| fail(format!("MV catalog lacks {key}")))
        };
        let id = text("model_id")?;
        let revision = text("head_revision_id")?;
        let digest = text("head_model_digest")?;
        if let Some(kept) = held.iter().find(|model| {
            model.identity.id == id
                && model.identity.revision_id == revision
                && model.identity.digest == digest
        }) {
            features_total += kept.projection["features"].as_array().map_or(0, Vec::len);
            if features_total > 200_000 {
                return Err(fail("MV print projections exceed 200,000 features"));
            }
            models.push(Model {
                identity: map::Model {
                    id,
                    label: text("display_name")?,
                    revision_id: revision,
                    digest,
                },
                projection: kept.projection.clone(),
            });
            continue;
        }
        let response = ds_cli_auth::grid_models_for_project(
            lane,
            project,
            &ds_cli_auth::GridModelsCommand::Download {
                model: id.clone(),
                revision: revision.clone(),
            },
        )?;
        let result = response;
        if result.data["sha256"] != digest || result.data["verified"] != true {
            return Err(fail(
                "Downloaded MV model differs from the pinned catalog head",
            ));
        }
        let bytes = result
            .bytes
            .ok_or_else(|| fail("MV owner returned no verified model bytes"))?;
        let session = projection_session
            .get_or_insert_with(|| {
                ds_project_data::mv_projection::ProjectionSession::capture_for_lane(lane)
            })
            .as_ref()
            .map_err(projection_fail)?;
        let projection = session
            .project(&bytes, &revision)
            .map_err(|error| projection_fail(&error))?;
        features_total += projection["features"].as_array().map_or(0, Vec::len);
        if features_total > 200_000 {
            return Err(fail("MV print projections exceed 200,000 features"));
        }
        models.push(Model {
            identity: map::Model {
                id,
                label: text("display_name")?,
                revision_id: revision,
                digest,
            },
            projection,
        });
    }
    let current = ds_cli_auth::probe_headless_identity_for_named_project(lane)?
        .ok_or_else(|| fail("native identity disappeared during MV model acquisition"))?;
    if &current != identity {
        return Err(fail("native identity changed during MV model acquisition"));
    }
    Ok(models)
}

pub(super) fn attach(
    mut context: Option<ds_report_host::PrintContextBytes>,
    models: &[Model],
    name: &str,
    network: &Value,
    buffer: f64,
) -> Result<Option<ds_report_host::PrintContextBytes>, Failure> {
    let request = json!({"schema":ds_command_kernel::project_design_extent::SCHEMA,"action":"extent","name":name,"layers":network,"buffer_m":buffer});
    let answer: Value = serde_json::from_str(
        &ds_command_kernel::project_design_extent::evaluate(
            &serde_json::to_vec(&request).map_err(fail)?,
        )
        .map_err(fail)?,
    )
    .map_err(fail)?;
    let bounds: [f64; 4] = serde_json::from_value(answer["buffered"].clone()).map_err(fail)?;
    let [w, s, e, n] = bounds;
    let mut document = context.as_ref().map(|c| serde_json::from_slice::<Value>(&c.bytes).map_err(fail)).transpose()?.unwrap_or_else(|| json!({"coverage":{"type":"Polygon","coordinates":[[[w,s],[e,s],[e,n],[w,n],[w,s]]]},"layers":{},"sources":[]}));
    let mut layers: BTreeMap<String, Value> =
        serde_json::from_value(document["layers"].clone()).map_err(fail)?;
    let mut mv = BTreeMap::<String, Vec<Value>>::new();
    for model in models {
        let identity = map::Model {
            id: model.identity.id.clone(),
            label: model.identity.label.clone(),
            revision_id: model.identity.revision_id.clone(),
            digest: model.identity.digest.clone(),
        };
        let selected = map::mv_layers(model.projection.clone(), identity, bounds).map_err(fail)?;
        for (id, collection) in selected
            .as_object()
            .ok_or_else(|| fail("MV print selection has no layers"))?
        {
            mv.entry(id.clone()).or_default().extend(
                collection["features"]
                    .as_array()
                    .ok_or_else(|| fail("MV print selection has no features"))?
                    .iter()
                    .cloned(),
            );
        }
    }
    suppress_angle_symbols_at_poles(&mut mv, network);
    let versions = models.iter().map(|m| json!({"id":m.identity.id,"revision":m.identity.revision_id,"sha256":m.identity.digest})).collect::<Vec<_>>();
    for (id, features) in mv {
        if features.is_empty() {
            continue;
        }
        if layers.contains_key(&id) {
            return Err(fail("MV layer identity collides with another print source"));
        }
        document["sources"]
            .as_array_mut()
            .ok_or_else(|| fail("Print context has no source inventory"))?
            .push(json!({"layer":id,"kind":"project_dsgrid_mv","models":versions}));
        layers.insert(id, json!({"type":"FeatureCollection","features":features}));
    }
    if layers.is_empty() {
        return Ok(None);
    }
    let bytes =
        report_export::print_context_document(&document["coverage"], &document["sources"], &layers)
            .map_err(fail)?;
    let omitted = context
        .take()
        .map(|c| {
            c.omitted
                .into_iter()
                .filter(|id| !id.starts_with("dsgrid_mv_"))
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(ds_report_host::PrintContextBytes {
        sha256: report_export::sha256_hex(&bytes),
        bytes,
        layers: layers.into_keys().collect(),
        omitted,
    }))
}

/// An angle point and a pole at the same site are one mark on an individual
/// transformer sheet. Keep the model projection intact: MV overviews still
/// print every angle point, and an unmatched point remains visible here.
fn suppress_angle_symbols_at_poles(mv: &mut BTreeMap<String, Vec<Value>>, network: &Value) {
    const COINCIDENT_M: f64 = 1.0;
    let mut poles = ["lv_poles", "tapping_poles"]
        .into_iter()
        .filter_map(|layer| network[layer]["features"].as_array())
        .flat_map(|features| features.iter())
        .filter_map(point_coords)
        .collect::<Vec<_>>();
    if let Some(structures) = mv.get("dsgrid_mv_structures") {
        poles.extend(structures.iter().filter_map(point_coords));
    }
    if poles.is_empty() {
        return;
    }
    if let Some(angles) = mv.get_mut("dsgrid_mv_angle_points") {
        angles.retain(|angle| {
            point_coords(angle).is_none_or(|[lon, lat]| {
                !poles.iter().any(|[pole_lon, pole_lat]| {
                    ds_geo_lite::haversine_m(lon, lat, *pole_lon, *pole_lat) <= COINCIDENT_M
                })
            })
        });
    }
}

fn point_coords(feature: &Value) -> Option<[f64; 2]> {
    if feature["geometry"]["type"] != "Point" {
        return None;
    }
    let coordinates = feature["geometry"]["coordinates"].as_array()?;
    let lon = coordinates.first()?.as_f64()?;
    let lat = coordinates.get(1)?.as_f64()?;
    (lon.is_finite()
        && lat.is_finite()
        && (-180.0..=180.0).contains(&lon)
        && (-90.0..=90.0).contains(&lat))
    .then_some([lon, lat])
}

#[cfg(test)]
#[path = "mv_context_tests.rs"]
mod tests;
