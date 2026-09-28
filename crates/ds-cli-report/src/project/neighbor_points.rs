//! Point-only context for other active transformers on an individual sheet.
//! Rooms remain the authority for their own networks and schedules.  This
//! module projects only their transformer markers into the print context.

use std::collections::BTreeMap;

use ds_cli_contract::outcome::Failure;
use ds_project_data::room_hold::Room;
use ds_report_host::PrintContextBytes;
use serde_json::{Value, json};

use super::hold::{Hold, Link, head_version};

pub(super) const LAYER: &str = "neighbor_transformers";

/// An explicit governed print-style binding selects this print-only layer.
/// It leaves existing layouts, MV context and report data untouched.
pub(super) fn selected(
    receipt: &ds_command_kernel::report_export::InputReceipt,
    held_setups: &[Value],
) -> Result<bool, String> {
    let sheets = receipt.sheets()?;
    let setups = sheets["printing_setups"]
        .as_array()
        .map_or(held_setups, Vec::as_slice);
    Ok(setups.iter().any(|setup| {
        setup["layout"]["style_refs"][LAYER]
            .as_str()
            .is_some_and(|reference| !reference.is_empty())
    }))
}

fn unavailable(message: impl Into<String>) -> Failure {
    Failure::unavailable("neighbor_transformer_points_unavailable", message.into()).remedy(
        "Refresh the active transformer rooms for this project, then retry the sheet; the neighboring points layer requires one verified point per active room",
    )
}

/// A batch reads each active room at most once when its point is not already
/// held at the reported head. The checksum-verified project hold is the cache;
/// a later sheet in this batch and a later export reuse those same rooms.
#[allow(clippy::too_many_arguments)]
pub(super) fn catalogue(
    lane: &str,
    project: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    names: &[String],
    status: Option<&BTreeMap<String, Value>>,
    hold: &Hold,
    link: &mut Link,
) -> Result<BTreeMap<String, Value>, Failure> {
    if names.len() > ds_cli_auth::PROJECT_REPORT_MAX_TRANSFORMERS {
        return Err(unavailable(
            "the project has more active transformer rooms than the print batch admits",
        ));
    }
    let mut points = BTreeMap::new();
    let mut fetch = Vec::new();
    for name in names {
        let expected = status
            .map(|rows| {
                rows.get(name)
                    .map(|row| head_version(row).as_i64())
                    .ok_or_else(|| unavailable(format!("{name} is absent from project status")))
            })
            .transpose()?;
        let held = hold.room(name).map_err(unavailable)?;
        if let Some(room) = held.filter(|room| {
            expected.is_none_or(|head| head.is_some_and(|version| room.version == Some(version)))
        }) {
            points.insert(name.clone(), marker(&room).map_err(unavailable)?);
        } else {
            fetch.push(name.clone());
        }
    }
    for chunk in fetch.chunks(8) {
        let response = link
            .read(|| ds_cli_auth::transformer_contexts_for_project(lane, project, chunk))?
            .ok_or_else(|| unavailable(format!(
                "{} active transformer point rooms are not held while the service is unavailable",
                fetch.len()
            )))?;
        if response.identity() != identity || response.project_id() != project {
            return Err(unavailable(
                "neighbor point response belongs to another project or identity",
            ));
        }
        let snapshots = response.into_result();
        if snapshots.len() != chunk.len() {
            return Err(unavailable(
                "neighbor point response omitted a requested room",
            ));
        }
        for (name, snapshot) in chunk.iter().zip(snapshots) {
            if snapshot.ds_project() != project || snapshot.transformer_name() != name {
                return Err(unavailable(format!(
                    "neighbor point response changed scope for {name}"
                )));
            }
            let version = snapshot
                .metadata()
                .version()
                .and_then(|version| i64::try_from(version).ok())
                .filter(|version| *version > 0)
                .ok_or_else(|| unavailable(format!("{name} has no saved room revision")))?;
            if let Some(expected) = status
                .and_then(|rows| rows.get(name))
                .and_then(|row| head_version(row).as_i64())
                && version != expected
            {
                return Err(unavailable(format!(
                    "{name} changed after project status was read"
                )));
            }
            let room = Room {
                transformer: name.clone(),
                version: Some(version),
                content_digest: snapshot.metadata().content_digest().map(str::to_string),
                layers: snapshot.layers().clone(),
            };
            let feature = marker(&room).map_err(unavailable)?;
            hold.hold_room(&room).map_err(unavailable)?;
            points.insert(name.clone(), feature);
        }
    }
    if points.len() != names.len() {
        return Err(unavailable("project marker catalogue is incomplete"));
    }
    Ok(points)
}

/// One saved room contributes exactly one geographical transformer point.
/// No neighboring design attributes are copied into the print document.
pub(super) fn marker(room: &Room) -> Result<Value, String> {
    let features = room
        .layers
        .get("tr")
        .and_then(|layer| layer.get("features"))
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{} has no transformer point layer", room.transformer))?;
    let [feature] = features.as_slice() else {
        return Err(format!(
            "{} needs exactly one saved transformer point, found {}",
            room.transformer,
            features.len()
        ));
    };
    let coordinates = feature["geometry"]["coordinates"]
        .as_array()
        .filter(|coordinates| feature["geometry"]["type"] == "Point" && coordinates.len() >= 2)
        .ok_or_else(|| format!("{} has no point geometry", room.transformer))?;
    let lon = coordinates[0]
        .as_f64()
        .filter(|value| value.is_finite() && (-180.0..=180.0).contains(value))
        .ok_or_else(|| format!("{} has an invalid transformer longitude", room.transformer))?;
    let lat = coordinates[1]
        .as_f64()
        .filter(|value| value.is_finite() && (-90.0..=90.0).contains(value))
        .ok_or_else(|| format!("{} has an invalid transformer latitude", room.transformer))?;
    Ok(json!({
        "type": "Feature",
        "id": room.transformer,
        "geometry": {"type": "Point", "coordinates": [lon, lat]},
        "properties": {"transfo": room.transformer},
    }))
}

fn point(feature: &Value) -> Result<[f64; 2], String> {
    let coordinates = feature["geometry"]["coordinates"]
        .as_array()
        .ok_or("neighbor marker has no coordinates")?;
    Ok([
        coordinates[0].as_f64().ok_or("invalid marker longitude")?,
        coordinates[1].as_f64().ok_or("invalid marker latitude")?,
    ])
}

/// Add every other project point to a print-only context. The renderer clips
/// the layer to the actual map viewport, whose fit is based on the current
/// room's design before this context joins it.
pub(super) fn attach(
    current: &str,
    markers: &BTreeMap<String, Value>,
    context: Option<PrintContextBytes>,
) -> Result<Option<PrintContextBytes>, String> {
    let other = markers
        .iter()
        .filter(|(name, _)| name.as_str() != current)
        .map(|(_, feature)| feature.clone())
        .collect::<Vec<_>>();
    if other.is_empty() {
        return Ok(context);
    }
    let mut document = context
        .as_ref()
        .map(|held| serde_json::from_slice::<Value>(&held.bytes).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_else(
            || json!({"schema":"ds.print-context/v1","coverage":null,"sources":[],"layers":{}}),
        );
    let mut layers: BTreeMap<String, Value> =
        serde_json::from_value(document["layers"].clone()).map_err(|error| error.to_string())?;
    if layers.contains_key(LAYER) {
        return Err(format!("print context already contains {LAYER}"));
    }
    // Held geographic sources keep their exact acquisition coverage. This
    // project-room marker source is independent of that area. When it is the
    // only context, its own points define the document's coverage.
    if document["coverage"].is_null() {
        let mut bounds: [f64; 4] = [180.0, 90.0, -180.0, -90.0];
        for feature in markers.values() {
            let [lon, lat] = point(feature)?;
            bounds[0] = bounds[0].min(lon);
            bounds[1] = bounds[1].min(lat);
            bounds[2] = bounds[2].max(lon);
            bounds[3] = bounds[3].max(lat);
        }
        if bounds[0] == bounds[2] {
            bounds[0] = (bounds[0] - 0.00001).max(-180.0);
            bounds[2] = (bounds[2] + 0.00001).min(180.0);
        }
        if bounds[1] == bounds[3] {
            bounds[1] = (bounds[1] - 0.00001).max(-90.0);
            bounds[3] = (bounds[3] + 0.00001).min(90.0);
        }
        let [w, s, e, n] = bounds;
        document["coverage"] =
            json!({"type":"Polygon","coordinates":[[[w,s],[e,s],[e,n],[w,n],[w,s]]]});
    }
    let sources = document["sources"]
        .as_array_mut()
        .ok_or("print context has no source inventory")?;
    sources.push(
        json!({"layer":LAYER,"kind":"project_transformer_points","feature_count":other.len()}),
    );
    layers.insert(
        LAYER.into(),
        json!({"type":"FeatureCollection","features":other}),
    );
    let bytes = ds_command_kernel::report_export::print_context_document(
        &document["coverage"],
        &document["sources"],
        &layers,
    )?;
    Ok(Some(PrintContextBytes {
        sha256: ds_command_kernel::report_export::sha256_hex(&bytes),
        bytes,
        layers: layers.into_keys().collect(),
        omitted: context.map_or_else(Vec::new, |held| held.omitted),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(name: &str, lon: f64) -> Room {
        Room {
            transformer: name.into(),
            version: Some(1),
            content_digest: None,
            layers: BTreeMap::from([
                (
                    "tr".into(),
                    json!({"type":"FeatureCollection","features":[{
                        "type":"Feature","geometry":{"type":"Point","coordinates":[lon,-2.0]},
                        "properties":{"transfo":name,"tr_size":"100 kVA","private_note":"never print"}
                    }]}),
                ),
                (
                    "lv_lines".into(),
                    json!({"features":[{"type":"Feature","geometry":null}]}),
                ),
            ]),
        }
    }

    #[test]
    fn neighbor_context_contains_only_other_points_and_public_names() {
        let markers = BTreeMap::from([
            ("own".into(), marker(&room("own", 29.7)).unwrap()),
            ("other".into(), marker(&room("other", 29.701)).unwrap()),
        ]);
        let attached = attach("own", &markers, None).unwrap().unwrap();
        let document: Value = serde_json::from_slice(&attached.bytes).unwrap();
        assert_eq!(
            document["layers"][LAYER]["features"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            document["layers"][LAYER]["features"][0]["properties"],
            json!({"transfo":"other"})
        );
        assert!(document["layers"]["lv_lines"].is_null());
        assert!(document["layers"]["lv_poles"].is_null());
        assert!(document["layers"]["lv_print_info"].is_null());
        assert!(
            !String::from_utf8(attached.bytes)
                .unwrap()
                .contains("private_note")
        );
    }

    #[test]
    fn governed_layout_accepts_the_neighbor_layer_binding() {
        use ds_command_kernel::printing::{default_layout, elements};

        let layout = elements::set_layer_style_ref(
            default_layout(),
            LAYER,
            "master/neighbor_transformers_print",
            &[elements::StyleFacts {
                style_ref: "master/neighbor_transformers_print".into(),
                style_target: "print".into(),
            }],
        )
        .unwrap();
        assert_eq!(
            layout.style_refs[LAYER],
            "master/neighbor_transformers_print"
        );
        ds_command_kernel::printing::validate(&layout).unwrap();
    }

    #[test]
    fn a_solo_project_adds_no_layer_and_malformed_points_refuse() {
        let own = marker(&room("own", 29.7)).unwrap();
        assert!(
            attach("own", &BTreeMap::from([("own".into(), own)]), None)
                .unwrap()
                .is_none()
        );
        let mut malformed = room("bad", 29.7);
        malformed.layers.get_mut("tr").unwrap()["features"][0]["geometry"]["type"] =
            json!("LineString");
        assert!(marker(&malformed).unwrap_err().contains("point geometry"));
    }

    #[test]
    fn current_project_rooms_supply_one_catalogue_without_network_reads() {
        let temp = tempfile::tempdir().unwrap();
        let hold = Hold::at(temp.path().into(), "principal", "project-a");
        hold.hold_room(&room("own", 29.7)).unwrap();
        hold.hold_room(&room("other", 29.701)).unwrap();
        let status = BTreeMap::from([
            ("own".into(), json!({"metadata":{"version":1}})),
            ("other".into(), json!({"metadata":{"version":1}})),
        ]);
        let identity =
            ds_cli_auth::ProviderIdentity::new("canary", &"a".repeat(64), "principal").unwrap();
        let points = catalogue(
            "canary",
            "project-a",
            &identity,
            &["own".into(), "other".into()],
            Some(&status),
            &hold,
            &mut Link::default(),
        )
        .unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points["other"]["properties"], json!({"transfo":"other"}));
        assert!(hold.room("other").unwrap().is_some());
    }

    #[test]
    fn adding_neighbors_keeps_other_context_coverage_and_layers() {
        let coverage = json!({"type":"Polygon","coordinates":[[[29.69,-2.01],[29.71,-2.01],[29.71,-1.99],[29.69,-1.99],[29.69,-2.01]]]});
        let sources = json!([{"layer":"roads","kind":"catalog"}]);
        let layers = BTreeMap::from([(
            "roads".into(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"LineString","coordinates":[[29.69,-2.0],[29.71,-2.0]]},"properties":{}}]}),
        )]);
        let bytes =
            ds_command_kernel::report_export::print_context_document(&coverage, &sources, &layers)
                .unwrap();
        let held = PrintContextBytes {
            sha256: ds_command_kernel::report_export::sha256_hex(&bytes),
            bytes,
            layers: vec!["roads".into()],
            omitted: vec!["rivers".into()],
        };
        let markers = BTreeMap::from([
            ("own".into(), marker(&room("own", 29.7)).unwrap()),
            ("other".into(), marker(&room("other", 29.701)).unwrap()),
        ]);
        let attached = attach("own", &markers, Some(held)).unwrap().unwrap();
        let document: Value = serde_json::from_slice(&attached.bytes).unwrap();
        assert_eq!(document["coverage"], coverage);
        assert_eq!(document["layers"]["roads"], layers["roads"]);
        assert_eq!(attached.omitted, ["rivers"]);
        assert!(attached.layers.contains(&LAYER.to_string()));
    }
}
