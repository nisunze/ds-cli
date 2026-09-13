//! Read-only native model acquisition. Engineering projection and CRS belong
//! to ds-grid-exchange/ds-geo; clipping and print provenance belong to the kernel.
use ds_cli_contract::outcome::Failure;
use ds_command_kernel::{printing::map, report_export};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Model {
    identity: map::Model,
    projection: Value,
}
pub(super) fn provenance(models: &[Model]) -> Vec<Value> {
    models.iter().map(|model| json!({"model_id":model.identity.id,"revision_id":model.identity.revision_id,"sha256":model.identity.digest,"label":model.identity.label})).collect()
}
fn fail(e: impl std::fmt::Display) -> Failure {
    Failure::failed("print_context_invalid", e.to_string())
        .remedy("Check the exact project's promoted MV model and declared CRS; no model is modified by printing")
}
pub(super) fn load(
    lane: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    project: &str,
) -> Result<Vec<Model>, Failure> {
    let mut cursor = None;
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    loop {
        let response = ds_cli_auth::grid_models(
            lane,
            &ds_cli_auth::GridModelsCommand::List {
                limit: 100,
                cursor: cursor.clone(),
            },
        )?;
        if response.identity() != identity || response.project_id() != project {
            return Err(fail("MV model acquisition changed project or identity"));
        }
        let data = response.into_result().data;
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
        let response = ds_cli_auth::grid_models(
            lane,
            &ds_cli_auth::GridModelsCommand::Download {
                model: id.clone(),
                revision: revision.clone(),
            },
        )?;
        if response.identity() != identity || response.project_id() != project {
            return Err(fail("MV model acquisition changed project or identity"));
        }
        let result = response.into_result();
        if result.data["sha256"] != digest || result.data["verified"] != true {
            return Err(fail(
                "Downloaded MV model differs from the pinned catalog head",
            ));
        }
        let bytes = result
            .bytes
            .ok_or_else(|| fail("MV owner returned no verified model bytes"))?;
        let projection =
            ds_project_data::mv_projection::project(&bytes, &revision).map_err(fail)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mv_context_uses_metric_crs_and_records_model_identity_without_changing_geometry_source() {
        let projection = json!({"type":"FeatureCollection","features":[{"type":"Feature","id":"route-1","geometry":{"type":"LineString","coordinates":[[29.999,-2.],[30.001,-2.]]},"properties":{"_layer":"alignments","label":"MV A"}}]});
        let models = [Model {
            identity: map::Model {
                id: "mv-model".into(),
                label: "Proposed MV".into(),
                revision_id: "rev-1".into(),
                digest: "a".repeat(64),
            },
            projection: projection.clone(),
        }];
        let network = json!({"tr":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30.,-2.]},"properties":{"transfo":"tx_a"}}]}});
        let attached = attach(None, &models, "tx_a", &network, 200.)
            .unwrap()
            .unwrap();
        let document: Value = serde_json::from_slice(&attached.bytes).unwrap();
        let feature = &document["layers"]["dsgrid_mv_lines"]["features"][0];
        assert_eq!(feature["properties"]["model_revision_id"], "rev-1");
        assert_eq!(feature["geometry"], projection["features"][0]["geometry"]);
        assert_eq!(attached.sha256, report_export::sha256_hex(&attached.bytes));
        assert_eq!(models[0].projection, projection);
    }
}
