//! `ds map draw` — put a GeoJSON file on the paired map as a local layer.
//!
//! A local layer is the same temporary sketch layer a person draws by hand:
//! it lives in the running session, it is not project data, and it disappears
//! when the session does. `persisted` is reported on every call and is always
//! false, so a caller can never mistake drawing for saving.
//!
//! The file is validated here before anything is sent. That is not
//! duplicated work: the application checks the same rules, but it checks them
//! on a payload it has already received, and reports "features[413] geometry
//! does not match Polygon" as an operation refusal. Checking first turns that
//! into a local, typed refusal naming the feature — the difference between
//! fixing a file and reading an error inside an error.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, MAX_LAYER_FEATURES};

/// The geometry types a temporary layer can hold, as the application spells
/// them. One layer holds one type — a mixed file is two layers.
const GEOMETRIES: &[&str] = &["Point", "LineString", "Polygon"];

pub static COMMAND: Command = Command {
    id: "map.draw",
    path: &["map", "draw"],
    contract: 1,
    summary: "Add a GeoJSON file to the paired map as a local layer.",
    purpose: "\
Draws the features in a GeoJSON file onto the running map as one local layer, \
the same kind a person draws by hand. Takes a FeatureCollection, a bare array \
of features, or a single feature. The layer is session-only: it is never \
written to the project, and `persisted` says so on every call. Pass --zoom to \
move the map to what was just drawn.",
    chapter: Chapter::Survey,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("name", "<text>", "Layer name, as it appears in the app.").required(),
        Arg::value("geometry", "<type>", "The one geometry type in the file.")
            .required()
            .choices(GEOMETRIES),
        Arg::value("features", "<path>", "GeoJSON file to draw.").required(),
        Arg::switch("zoom", "Move the map to the drawn layer's extent."),
        DESCRIPTOR_ARG,
    ],
    output: "\
The new layer's `layer` id for `ds map remove`, its `analysis_id` for the \
vector tools, the feature count the application accepted, the extent read from \
the file, and `persisted: false`. With --zoom, whether the map moved.",
    examples: &[
        Example {
            command: "ds map draw --name Survey --geometry Point --features ./points.geojson --zoom",
            note: "Draw and look at it.",
            runnable: false,
        },
        Example {
            command: "ds map draw --name Boundary --geometry Polygon --features ./aoi.geojson --output json",
            note: "Read .data.analysis_id to run a vector tool on it next.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::FEATURES_NOT_FOUND,
        crate::FEATURES_NOT_GEOJSON,
        crate::FEATURES_EMPTY,
        crate::FEATURES_OVER_BOUND,
        crate::GEOMETRY_MISMATCH,
        Refusal {
            code: "layer_not_returned",
            when: "the application accepted the layer but named no id for it",
            remedy: "run `ds map view` to find the layer; report the build",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let name = inputs.require("name")?;
    let geometry = inputs.require("geometry")?;
    let path = inputs.require("features")?;

    let supplied = crate::load_features(path, "features", MAX_LAYER_FEATURES)?;
    crate::require_geometry(&supplied, geometry)?;

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let added = crate::invoke(
        &descriptor,
        &crate::LAYER_ADD,
        json!({
            "name": name,
            "geometryType": geometry,
            "features": supplied.features,
        }),
        crate::UI_TIMEOUT,
    )?;

    let Some(layer) = added["layerId"].as_str() else {
        return Err(Failure::failed(
            "layer_not_returned",
            "the application accepted the layer but named no id for it",
        )
        .remedy("run `ds map view` to find the layer; report the build")
        .next("ds map view"));
    };

    let mut data = json!({
        "layer": layer,
        "analysis_id": format!("{}{layer}", crate::ANALYSIS_SKETCH_PREFIX),
        "name": added["name"].as_str().unwrap_or(name),
        "geometry": geometry,
        "features": added["featureCount"],
        "bbox": supplied.bbox.map(Vec::from),
        // Stated on every call. A temporary layer is never project data, and
        // a caller must never have to infer that from the absence of a field.
        "persisted": added["persistedToProject"].as_bool().unwrap_or(false),
    });

    if inputs.switch("zoom") {
        data["zoom"] = zoom_to_extent(&descriptor, supplied.bbox);
    }
    Ok(data)
}

/// Move the map to the drawn extent, and report rather than fail if it will
/// not go.
///
/// The layer is already on the map by this point. Turning "the map is not
/// open" into a failed `ds map draw` would report that the draw did not
/// happen, which is the one thing that is not true.
fn zoom_to_extent(
    descriptor: &ds_cli_desktop::discover::Descriptor,
    bbox: Option<[f64; 4]>,
) -> Value {
    let Some(bbox) = bbox else {
        return json!({
            "moved": false,
            "reason": "the file carries no finite coordinates to zoom to",
        });
    };
    match crate::invoke(
        descriptor,
        &crate::ZOOM_TO,
        json!({ "bbox": bbox }),
        crate::UI_TIMEOUT,
    ) {
        Ok(_) => json!({ "moved": true, "bbox": Vec::from(bbox) }),
        Err(failure) => json!({
            "moved": false,
            "reason": failure.message(),
            "code": failure.code(),
        }),
    }
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "drew  {}  ({}, {})\n  {}\n",
        data["name"].as_str().unwrap_or(""),
        crate::plural(data["features"].as_u64().unwrap_or(0), "feature"),
        data["geometry"].as_str().unwrap_or(""),
        data["analysis_id"].as_str().unwrap_or(""),
    );
    if let Some(bbox) = data["bbox"].as_array() {
        out.push_str(&format!(
            "  extent  {}, {} .. {}, {}\n",
            bbox[0], bbox[1], bbox[2], bbox[3]
        ));
    }
    out.push_str("  session-only; nothing was written to the project\n");
    if let Some(zoom) = data["zoom"].as_object() {
        if zoom["moved"].as_bool().unwrap_or(false) {
            out.push_str("  map moved to it\n");
        } else {
            out.push_str(&format!(
                "  map did not move — {}\n",
                zoom["reason"].as_str().unwrap_or("")
            ));
        }
    }
    out
}
