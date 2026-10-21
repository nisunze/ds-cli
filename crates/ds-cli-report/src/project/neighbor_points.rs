//! Acquire verified active rooms for point-only or complete circuit context.
//! The kernel owns the print projection; each room owns its engineering data.

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
    selection: &ds_command_kernel::report_formats::DesignOutputSelection,
) -> Result<bool, String> {
    let sheets = receipt.sheets()?;
    let setups = sheets["printing_setups"]
        .as_array()
        .map_or(held_setups, Vec::as_slice);
    Ok(setups
        .iter()
        .filter(|setup| selected_setup(setup, selection))
        .any(|setup| {
            setup["layout"]["style_refs"][LAYER]
                .as_str()
                .is_some_and(|reference| !reference.is_empty())
        }))
}

pub(super) fn networks_selected(
    receipt: &ds_command_kernel::report_export::InputReceipt,
    held_setups: &[Value],
    selection: &ds_command_kernel::report_formats::DesignOutputSelection,
) -> Result<bool, String> {
    let sheets = receipt.sheets()?;
    let setups = sheets["printing_setups"]
        .as_array()
        .map_or(held_setups, Vec::as_slice);
    setups
        .iter()
        .filter(|setup| selected_setup(setup, selection))
        .try_fold(false, |selected, setup| {
            let layout: ds_command_kernel::printing::Layout =
                serde_json::from_value(setup["layout"].clone())
                    .map_err(|e| format!("invalid adjacent-network layout: {e}"))?;
            ds_command_kernel::printing::validate(&layout)?;
            Ok(selected || ds_command_kernel::printing::adjacent_networks::selected(&layout))
        })
}

fn selected_setup(
    setup: &Value,
    selection: &ds_command_kernel::report_formats::DesignOutputSelection,
) -> bool {
    selection
        .prints
        .iter()
        .any(|print| print.enabled && setup["id"].as_str() == Some(print.layout_id.as_str()))
}

/// The rooms the point catalogue already read and verified, at the exact saved
/// versions it took each point from; the kernel projects print-only data.
pub(super) fn networks(
    rooms: Vec<Room>,
) -> Result<Vec<ds_command_kernel::printing::adjacent_networks::Room>, Failure> {
    rooms
        .into_iter()
        .map(|room| {
            let version = room.version.filter(|v| *v > 0).ok_or_else(|| {
                unavailable(format!("{} has no saved revision", room.transformer))
            })?;
            Ok(ds_command_kernel::printing::adjacent_networks::Room {
                transformer: room.transformer,
                version,
                layers: room.layers,
            })
        })
        .collect()
}

pub(super) fn attach_networks(
    current: &str,
    rooms: &[ds_command_kernel::printing::adjacent_networks::Room],
    context: Option<PrintContextBytes>,
    fields: &BTreeMap<String, std::collections::BTreeSet<String>>,
) -> Result<Option<PrintContextBytes>, String> {
    let held = context
        .as_ref()
        .map(|c| serde_json::from_slice::<Value>(&c.bytes).map_err(|e| e.to_string()))
        .transpose()?;
    let Some(doc) = ds_command_kernel::printing::adjacent_networks::attach(
        current,
        rooms,
        held.as_ref(),
        fields,
    )?
    else {
        return Ok(None);
    };
    let layers: BTreeMap<String, Value> =
        serde_json::from_value(doc["layers"].clone()).map_err(|e| e.to_string())?;
    let bytes = ds_command_kernel::report_export::print_context_document(
        &doc["coverage"],
        &doc["sources"],
        &layers,
    )?;
    Ok(Some(PrintContextBytes {
        sha256: ds_command_kernel::report_export::sha256_hex(&bytes),
        bytes,
        layers: layers.into_keys().collect(),
        omitted: context.map_or_else(Vec::new, |c| c.omitted),
    }))
}

pub(super) fn network_fields(
    receipt: &ds_command_kernel::report_export::InputReceipt,
    held_setups: &[Value],
    selection: &ds_command_kernel::report_formats::DesignOutputSelection,
    capture: Option<&ds_command_kernel::printing::style_capture::Capture>,
) -> Result<BTreeMap<String, std::collections::BTreeSet<String>>, String> {
    let sheets = receipt.sheets()?;
    let setups = sheets["printing_setups"]
        .as_array()
        .map_or(held_setups, Vec::as_slice);
    let layouts: Vec<ds_command_kernel::printing::Layout> = setups
        .iter()
        .filter(|setup| selected_setup(setup, selection))
        .map(|setup| serde_json::from_value(setup["layout"].clone()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut fields: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for mut layout in layouts {
        let held = capture
            .map(|capture| capture.for_layout(&layout))
            .transpose()?;
        let documents = match &held {
            Some(held) => {
                layout.style_refs.extend(
                    serde_json::from_value::<BTreeMap<String, String>>(held["style_refs"].clone())
                        .map_err(|e| e.to_string())?,
                );
                &held["print_styles"]
            }
            None => &sheets["printing_styles"],
        };
        for (layer, required) in ds_command_kernel::printing::adjacent_networks::required_fields(
            std::slice::from_ref(&layout),
            documents,
        )? {
            fields.entry(layer).or_default().extend(required);
        }
    }
    Ok(fields)
}

fn unavailable(message: impl Into<String>) -> Failure {
    Failure::unavailable("neighbor_transformer_points_unavailable", message.into()).remedy(
        "Refresh the active transformer rooms for this project, then retry the sheet; the neighboring points layer requires one verified point per active room",
    )
}

/// A batch reads each active room at most once when its point is not already
/// held at the reported head. The checksum-verified project hold is the cache;
/// a later sheet in this batch and a later export reuse those same rooms. With
/// `keep_rooms` the rooms read come back too, so circuits read none again.
#[allow(clippy::too_many_arguments)]
pub(super) fn catalogue(
    lane: &str,
    project: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    names: &[String],
    status: Option<&BTreeMap<String, Value>>,
    hold: &Hold,
    link: &mut Link,
    keep_rooms: bool,
) -> Result<(BTreeMap<String, Value>, Vec<Room>), Failure> {
    if names.len() > ds_cli_auth::PROJECT_REPORT_MAX_TRANSFORMERS {
        return Err(unavailable(
            "the project has more active transformer rooms than the print batch admits",
        ));
    }
    let mut points = BTreeMap::new();
    let mut rooms = Vec::new();
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
            if keep_rooms {
                rooms.push(room);
            }
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
            if keep_rooms {
                rooms.push(room);
            }
        }
    }
    if points.len() != names.len() {
        return Err(unavailable("project marker catalogue is incomplete"));
    }
    Ok((points, rooms))
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
#[path = "neighbor_points_tests.rs"]
mod tests;
