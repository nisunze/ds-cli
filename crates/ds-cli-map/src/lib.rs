//! Map presentation, native layer management and project GIS uploads.
//! Layer/catalogue, tile and style operations use native project authentication.
//! Local overlays use ds-web's shared Rust filesystem store without a desktop.
//! Rendering, sketch editing and interactive design commands retain their
//! named desktop bridge operations. This crate is a host adapter, not an engine.

pub mod data;
pub mod design;
pub mod draw;
pub mod evidence;
pub mod layer;
pub mod line_difference;
pub mod outliers;
pub mod points_along;
pub mod random_points;
pub mod remove;
pub mod survey;
pub mod ui;
pub mod view;
pub mod zoom;

use std::collections::BTreeSet;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Domain, Refusal};
use serde_json::{Map, Value, json};

// The paired-application primitives every bridge domain shares, declared once
// in `ds-cli-desktop` and re-exported here so a caller of this domain — and
// every command in it — keeps naming them as `crate::…`.
pub use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, INVALID_NUMBER, NOT_PAIRED, PAIRING_REJECTED, REFUSED,
    SIGNED_OUT, SIGNED_OUT_MARKERS, UNREACHABLE, UNREADABLE, UNSUPPORTED,
    classify_signed_out as classify_design_failure, integer, invoke, paired, paired_availability,
    plural,
};

pub static DOMAIN: Domain = Domain {
    id: "map",
    summary: "Local data, layer ordering, remote overlays, and design edits.",
    commands: &[
        &view::COMMAND,
        &draw::COMMAND,
        &remove::COMMAND,
        &zoom::COMMAND,
        &data::inspect::COMMAND,
        &data::upload::COMMAND,
        &data::list::COMMAND,
        &data::remove::COMMAND,
        &layer::list::COMMAND,
        &layer::reorder::COMMAND,
        &layer::remote_list::COMMAND,
        &layer::add::COMMAND,
        &layer::remove::COMMAND,
        &layer::visibility::COMMAND,
        &ui::open::COMMAND,
        &evidence::capture::COMMAND,
        &points_along::COMMAND,
        &random_points::COMMAND,
        &outliers::COMMAND,
        &line_difference::COMMAND,
        &survey::download::COMMAND,
        &survey::plan::COMMAND,
        &survey::apply::COMMAND,
        &design::open::COMMAND,
        &design::read::COMMAND,
        &design::discard::COMMAND,
        &design::layer_to_local::COMMAND,
        &design::upload_to_local::COMMAND,
        &design::select::COMMAND,
        &design::set::COMMAND,
        &design::create::COMMAND,
        &design::delete::COMMAND,
        &design::geometry::COMMAND,
        &design::process_setup::COMMAND,
        &design::version_create::COMMAND,
        &design::version_list::COMMAND,
        &design::version_play::COMMAND,
        &design::version_compare::COMMAND,
        &design::process::COMMAND,
        &design::batch_process::COMMAND,
        &design::batch_report::COMMAND,
        &design::batch_save::COMMAND,
        &design::attach_print::COMMAND,
        &design::save::COMMAND,
        &design::list::COMMAND,
        &design::pin::COMMAND,
        &design::report::COMMAND,
        &design::upload::COMMAND,
        &design::upload_stage::COMMAND,
    ],
};

// ---------------------------------------------------------------------------
// The declared wire contract
// ---------------------------------------------------------------------------

pub const LAYER_ADD: BridgeOp = BridgeOp {
    operation: "map.temporary_layer.add",
    arguments: &["name", "geometryType", "features"],
};
pub const LAYER_REMOVE: BridgeOp = BridgeOp {
    operation: "map.temporary_layer.remove",
    arguments: &["layerId"],
};
pub const LAYERS_LIST: BridgeOp = BridgeOp {
    operation: "map.layers.list",
    arguments: &["scope", "refresh", "limit"],
};
pub const LAYERS_REORDER: BridgeOp = BridgeOp {
    operation: "map.layers.reorder",
    arguments: &["orders", "apply"],
};
pub const REMOTE_LAYER_ADD: BridgeOp = BridgeOp {
    operation: "map.remote_layer.add",
    arguments: &["name", "kind", "url", "tileSize", "attribution", "visible"],
};
pub const REMOTE_LAYER_REMOVE: BridgeOp = BridgeOp {
    operation: "map.remote_layer.remove",
    arguments: &["layerId"],
};
pub const REMOTE_LAYER_VISIBILITY: BridgeOp = BridgeOp {
    operation: "map.remote_layer.visibility",
    arguments: &["layerId", "visible"],
};
pub const ZOOM_TO: BridgeOp = BridgeOp {
    operation: "map.zoom_to",
    arguments: &["bbox", "layerId", "padding"],
};
pub const UI_OPEN: BridgeOp = BridgeOp {
    operation: "map.ui.open",
    arguments: &["target", "ref"],
};
pub const EVIDENCE_CAPTURE: BridgeOp = BridgeOp {
    operation: "map.evidence.capture",
    arguments: &["scope", "path", "replace"],
};
pub const POINTS_ALONG: BridgeOp = BridgeOp {
    operation: "gis.points_along",
    arguments: &["layerId", "settings.intervalM", "settings.includeEnds"],
};
pub const RANDOM_POINTS: BridgeOp = BridgeOp {
    operation: "gis.random_points",
    arguments: &[
        "layerId",
        "settings.minSpacingM",
        "settings.maxSpacingM",
        "settings.bufferDistanceM",
        "settings.sampleElevation",
        "settings.seed",
    ],
};
pub const DETECT_OUTLIERS: BridgeOp = BridgeOp {
    operation: "gis.detect_geometry_outliers",
    arguments: &[
        "layerId",
        "settings.threshold",
        "settings.minFeatures",
        "settings.spatialIsolation",
        "settings.sizeOutliers",
        "settings.extentOutliers",
    ],
};
pub const LINE_EXTENSION_DIFFERENCE: BridgeOp = BridgeOp {
    operation: "gis.line_extension_difference",
    arguments: &[
        "sourceLayer",
        "baseLayer",
        "name",
        "coverageToleranceM",
        "healToleranceM",
    ],
};
pub const DESIGN_READ: BridgeOp = BridgeOp {
    operation: "design.transformer.read",
    arguments: &["transformer", "layers", "property"],
};
pub const DESIGN_OPEN: BridgeOp = BridgeOp {
    operation: "design.transformer.open",
    arguments: &["transformer"],
};
pub const DESIGN_DISCARD: BridgeOp = BridgeOp {
    operation: "design.transformer.discard",
    arguments: &["transformer"],
};
pub const DESIGN_LAYER_TO_LOCAL: BridgeOp = BridgeOp {
    operation: "design.layer.copy_to_local",
    arguments: &["transformer", "layer", "name"],
};
pub const DESIGN_UPLOAD_TO_LOCAL: BridgeOp = BridgeOp {
    operation: "design.upload.layer_to_local",
    arguments: &["path", "sourceLayer", "name"],
};
pub const DESIGN_SELECT: BridgeOp = BridgeOp {
    operation: "design.features.select",
    arguments: &["transformer", "layers", "where", "bbox", "ids", "sample"],
};
pub const DESIGN_SET: BridgeOp = BridgeOp {
    operation: "design.features.set_properties",
    arguments: &[
        "transformer",
        "layers",
        "where",
        "bbox",
        "ids",
        "properties",
        "dryRun",
    ],
};
pub const DESIGN_CREATE: BridgeOp = BridgeOp {
    operation: "design.features.create",
    arguments: &[
        "transformer",
        "targetLayer",
        "features",
        "sourceLayer",
        "carryProperties",
        "properties",
        "dryRun",
    ],
};
pub const DESIGN_PROCESS: BridgeOp = BridgeOp {
    operation: "design.process.run",
    arguments: &[
        "transformer",
        // The application fixes the differential's `layers` to `lv_lines`
        // itself, so this domain must not send one — a declared key is the
        // only key that can be sent, which is what stops it.
        "differential.where",
        "differential.ids",
        "differential.bbox",
    ],
};
pub const DESIGN_PROCESS_CONFIGURE: BridgeOp = BridgeOp {
    operation: "design.process.configure",
    arguments: &[
        "configOnly",
        "surveyLayers",
        "temporaryLayers",
        "includeDesignCustomers",
        "poleSurveyLayers",
        "poleTemporaryLayers",
        "preset",
        "settings",
        "resetSettings",
        "scope",
        "dryRun",
    ],
};
pub const DESIGN_VERSION_BEGIN: BridgeOp = BridgeOp {
    operation: "design.version.begin",
    arguments: &["transformers", "reason"],
};
pub const DESIGN_VERSION_LIST: BridgeOp = BridgeOp {
    operation: "design.version.list",
    arguments: &["transformer"],
};
pub const DESIGN_VERSION_PLAY: BridgeOp = BridgeOp {
    operation: "design.version.play",
    arguments: &["transformer", "version"],
};
pub const DESIGN_VERSION_COMPARE: BridgeOp = BridgeOp {
    operation: "design.version.compare",
    arguments: &["transformer", "from", "to"],
};
pub const DESIGN_PROCESS_BATCH: BridgeOp = BridgeOp {
    operation: "design.process.batch",
    arguments: &["transformers", "settings", "parallel"],
};
pub const DESIGN_SAVE: BridgeOp = BridgeOp {
    operation: "design.transformer.save",
    arguments: &["transformer"],
};
pub const DESIGN_SAVE_BATCH: BridgeOp = BridgeOp {
    operation: "design.transformer.save_batch",
    arguments: &["transformers", "parallel"],
};
pub const DESIGN_DELETE: BridgeOp = BridgeOp {
    operation: "design.features.delete",
    arguments: &["transformer", "layers", "where", "bbox", "ids", "dryRun"],
};
pub const DESIGN_GEOMETRY: BridgeOp = BridgeOp {
    operation: "design.features.set_geometry",
    arguments: &["transformer", "ids", "geometry", "dryRun"],
};
pub const DESIGN_LIST: BridgeOp = BridgeOp {
    operation: "design.transformer.list",
    arguments: &["limit"],
};
pub const DESIGN_PIN: BridgeOp = BridgeOp {
    operation: "design.pin.apply",
    arguments: &["transformers", "selection", "mode"],
};
pub const DESIGN_REPORT: BridgeOp = BridgeOp {
    operation: "design.report.export",
    arguments: &["transformer"],
};
pub const DESIGN_REPORT_BATCH: BridgeOp = BridgeOp {
    operation: "design.report.export_batch",
    arguments: &["transformers", "fileLevel", "combinePerDistrict"],
};
pub const DESIGN_ATTACH_PRINT: BridgeOp = BridgeOp {
    operation: "design.report.attach_print",
    arguments: &[
        "path",
        "scope",
        "transformer",
        "mapFamily",
        "layoutName",
        "paperSize",
        "orientation",
        "pageRole",
        "sourceReceiptSha256",
    ],
};
pub const DESIGN_UPLOAD_INSPECT: BridgeOp = BridgeOp {
    operation: "design.upload.inspect",
    arguments: &["paths", "network", "parallel"],
};
pub const DESIGN_UPLOAD_STAGE_BATCH: BridgeOp = BridgeOp {
    operation: "design.upload.stage_batch",
    arguments: &[
        "items.transformer",
        "items.path",
        "parallel",
        "replaceLocal",
    ],
};
pub const SURVEY_MIGRATE_PLAN: BridgeOp = BridgeOp {
    operation: "survey.migrate.plan",
    arguments: &["sourceProject"],
};
pub const SURVEY_MIGRATE_APPLY: BridgeOp = BridgeOp {
    operation: "survey.migrate.apply",
    arguments: &["sourceProject"],
};
pub const SURVEY_WORKING_AREA_DOWNLOAD: BridgeOp = BridgeOp {
    operation: "survey.working_area.download",
    arguments: &["entireProject"],
};

/// Every operation this domain can send, for the parity test to walk. A new
/// operation that is not listed here cannot be sent: [`invoke`] takes a
/// `BridgeOp`, and the test requires each one to be an operation the
/// application actually implements.
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &LAYER_ADD,
    &LAYER_REMOVE,
    &ZOOM_TO,
    &UI_OPEN,
    &EVIDENCE_CAPTURE,
    &POINTS_ALONG,
    &RANDOM_POINTS,
    &DETECT_OUTLIERS,
    &LINE_EXTENSION_DIFFERENCE,
    &DESIGN_OPEN,
    &DESIGN_READ,
    &DESIGN_DISCARD,
    &DESIGN_LAYER_TO_LOCAL,
    &DESIGN_UPLOAD_TO_LOCAL,
    &DESIGN_SELECT,
    &DESIGN_SET,
    &DESIGN_CREATE,
    &DESIGN_DELETE,
    &DESIGN_GEOMETRY,
    &DESIGN_PROCESS_CONFIGURE,
    &DESIGN_VERSION_BEGIN,
    &DESIGN_VERSION_LIST,
    &DESIGN_VERSION_PLAY,
    &DESIGN_VERSION_COMPARE,
    &DESIGN_PROCESS,
    &DESIGN_PROCESS_BATCH,
    &DESIGN_SAVE,
    &DESIGN_SAVE_BATCH,
    &DESIGN_LIST,
    &DESIGN_REPORT,
    &DESIGN_REPORT_BATCH,
    &DESIGN_ATTACH_PRINT,
    &DESIGN_UPLOAD_INSPECT,
    &DESIGN_UPLOAD_STAGE_BATCH,
    &SURVEY_MIGRATE_PLAN,
    &SURVEY_MIGRATE_APPLY,
    &SURVEY_WORKING_AREA_DOWNLOAD,
];

/// The application's bound on one temporary layer. Hand-copied from
/// `MAX_AGENT_FEATURES`, and checked against it by the parity test.
///
/// It is enforced here as well as there so an operator who exported a
/// 40,000-feature file learns that from a local refusal naming the bound,
/// rather than from the application rejecting a payload it already received.
pub const MAX_LAYER_FEATURES: usize = 10_000;

/// The application's bound on one creation from selection
/// (`MAX_CREATE_FROM_SELECTION`).
pub const MAX_CREATE_FEATURES: usize = 5_000;

/// The application's bound on a returned feature sample
/// (`MAX_DESIGN_FEATURE_SAMPLE`).
pub const MAX_FEATURE_SAMPLE: u64 = 200;

/// What `map view` reads out of the application's published map snapshot.
///
/// These are hand copies of the CLI map session projection in the application, and they are
/// the quietest kind of hand copy there is: a renamed field does not fail, it
/// reports `null`, and a caller sees a map with no layers on it rather than
/// an error. So they are declared here, read from here, and proved against
/// the application's source by `tests/bridge_parity.rs`.
pub const SNAPSHOT_OPEN: &str = "open";
pub const SNAPSHOT_LAYERS: &str = "temporaryLayers";
pub const SNAPSHOT_VIEW_FIELDS: &[&str] = &["center", "zoom", "bbox"];

/// One snapshot layer, as (what `ds` reports, what the application
/// publishes). The left column is this CLI's contract; the right is the
/// application's.
pub const SNAPSHOT_LAYER_FIELDS: &[(&str, &str)] = &[
    ("name", "name"),
    ("geometry", "geometryType"),
    ("features", "featureCount"),
    ("visible", "visible"),
    ("source", "source"),
    ("this_session", "cliOwned"),
];

/// The layer's own identifier, from which both reported ids are made.
pub const SNAPSHOT_LAYER_ID: &str = "id";

/// The prefix the application's analysis-layer catalogue gives a temporary
/// layer (`sketch:${layer.id}` in `loadOutlierLayerOptions`).
///
/// This is the one identifier `ds` composes rather than receives, and it
/// exists because the bridge publishes no operation that lists analysis
/// options. `map view` and `map draw` therefore report the composed id as
/// `analysis_id`, so a caller passes a value it was given rather than one it
/// built — and the parity test holds the prefix to the application's.
pub const ANALYSIS_SKETCH_PREFIX: &str = "sketch:";

/// What `ds map ui open` reports, as (reported, published).
///
/// `resolvedRef` is the one worth having: the application resolves a layer id
/// or style ref to the thing it actually opened — a style key, a survey table
/// key, a `sketch-` tab id — and reporting it means the next command in a
/// sequence passes a value it was given rather than one it guessed.
pub const UI_OPEN_REPLY_FIELDS: &[(&str, &str)] = &[
    ("target", "target"),
    ("ref", "ref"),
    ("resolved_ref", "resolvedRef"),
    ("opened", "opened"),
];

/// The evidence receipt fields carried straight through, as (reported,
/// published).
///
/// Declared here rather than written inline for the same reason the map
/// snapshot fields are: a renamed field in the application does not fail, it
/// reports `null`, and a caller ends up with a receipt whose digest is missing
/// rather than an error. Declaring the pairs makes the projection one table
/// `tests/bridge_parity.rs` can hold to the application's own reply.
pub const EVIDENCE_RECEIPT_FIELDS: &[(&str, &str)] = &[
    ("path", "path"),
    ("bytes", "bytes"),
    ("sha256", "sha256"),
    ("scope", "scope"),
    ("view", "view"),
    ("ui", "ui"),
];

/// The frame size, which the application publishes as two top-level numbers
/// from its native `EvidenceCaptureReceipt`.
///
/// `ds` reports them as one `dimensions` object because the receipt is read as
/// a whole — a width without its height beside it is not a fact anyone uses —
/// and because that keeps the receipt at exactly seven keys. It is a shape
/// change and nothing more: no number here is derived, scaled or rounded.
pub const EVIDENCE_WIDTH: &str = "width";
pub const EVIDENCE_HEIGHT: &str = "height";

/// The whole receipt, in the order it is written. Seven keys, fixed: a
/// screenshot is evidence only if what is written beside it does not vary.
pub const EVIDENCE_RECEIPT_KEYS: &[&str] = &[
    "path",
    "bytes",
    "sha256",
    "dimensions",
    "scope",
    "view",
    "ui",
];

// ---------------------------------------------------------------------------
// Timeouts
// ---------------------------------------------------------------------------

/// Adding, removing or moving is a redraw. Anything slower is a hung webview.
pub const UI_TIMEOUT: Duration = Duration::from_secs(60);
pub const API_TIMEOUT: Duration = Duration::from_secs(2 * 60);
/// A capture has to wait for tiles, labels and the panel to settle before the
/// frame is worth keeping, then write and digest a PNG. Longer than a redraw,
/// far shorter than a vector tool.
pub const EVIDENCE_TIMEOUT: Duration = Duration::from_secs(3 * 60);
/// A vector tool runs real geometry over a whole layer, in WASM.
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Reading a transformer room may have to fetch it from the project first.
pub const DESIGN_READ_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Staging walks every feature in the room and rewrites the cache.
pub const DESIGN_STAGE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The Fast LV process generates a network. It is the long one, and it is
/// held just inside the application's own 31-minute invocation bound on
/// purpose: `ds` gives up first, with a typed refusal naming the operation,
/// rather than waiting for the bridge's bare gateway timeout.
pub const DESIGN_PROCESS_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// The survey migration API has a ten-minute server timeout. Give the
/// frontend enough time to invalidate affected local caches before answering.
pub const SURVEY_MIGRATION_TIMEOUT: Duration = Duration::from_secs(11 * 60);
/// A full-project Working Area refresh walks every survey form sequentially.
pub const SURVEY_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

// ---------------------------------------------------------------------------
// Reading GeoJSON a caller supplies
// ---------------------------------------------------------------------------

/// A features file, bounded and already checked for the shape it claims.
#[derive(Debug)]
pub struct Supplied {
    pub features: Vec<Value>,
    /// `[west, south, east, north]` over every coordinate read, when there
    /// was at least one finite pair. Computed while validating, so it costs
    /// nothing extra, and reported so a caller can move the map to what it
    /// just drew without a second pass over its own file.
    pub bbox: Option<[f64; 4]>,
    /// Every distinct `geometry.type` seen, in order.
    pub kinds: BTreeSet<String>,
}

/// The largest features file this will read. Ten thousand features of real
/// geometry are comfortably inside it; anything larger is a data export that
/// belongs in a project layer, not a temporary one.
pub const MAX_FEATURES_BYTES: u64 = 64 * 1024 * 1024;

pub const FEATURES_NOT_FOUND: Refusal = Refusal {
    code: "features_not_found",
    when: "the features path does not name a readable file",
    remedy: "check the path; it takes a GeoJSON file",
};
pub const FEATURES_NOT_GEOJSON: Refusal = Refusal {
    code: "features_not_geojson",
    when: "the file is not JSON, or holds no recognisable feature array",
    remedy: "pass a FeatureCollection, an array of Features, or one Feature",
};
pub const FEATURES_EMPTY: Refusal = Refusal {
    code: "features_empty",
    when: "the file parsed but carries no features",
    remedy: "a layer needs at least one feature",
};
pub const FEATURES_OVER_BOUND: Refusal = Refusal {
    code: "features_over_bound",
    when: "the file carries more features than the application accepts",
    remedy: "split the file; the refusal names the bound",
};

pub const GEOMETRY_MISMATCH: Refusal = Refusal {
    code: "geometry_mismatch",
    when: "a feature's geometry is not the declared --geometry",
    remedy: "one layer holds one geometry type; split the file or fix --geometry",
};

/// Read and bound a GeoJSON file, accepting the three shapes an export
/// actually arrives in: a `FeatureCollection`, a bare array of features, or a
/// single feature.
pub fn load_features(raw: &str, flag: &str, max: usize) -> Result<Supplied, Failure> {
    let path = std::path::Path::new(raw);
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::invalid(
            "features_not_found",
            format!("`{raw}` is not a readable file"),
        )
        .remedy(format!("check the path passed to --{flag}"))
        .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    if !metadata.is_file() {
        return Err(
            Failure::invalid("features_not_found", format!("`{raw}` is not a file"))
                .remedy(format!("check the path passed to --{flag}")),
        );
    }
    if metadata.len() > MAX_FEATURES_BYTES {
        return Err(Failure::invalid(
            "features_over_bound",
            format!("`{raw}` is larger than this command reads"),
        )
        .remedy("split the file, or import it as a project layer instead")
        .detail(json!({ "bytes": metadata.len(), "max_bytes": MAX_FEATURES_BYTES })));
    }

    let text = std::fs::read_to_string(path).map_err(|error| {
        Failure::invalid("features_not_found", format!("`{raw}` could not be read"))
            .remedy(format!("check the path passed to --{flag}"))
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    let value: Value = serde_json::from_str(&text).map_err(|error| {
        Failure::invalid("features_not_geojson", format!("`{raw}` is not valid JSON"))
            .remedy("pass a FeatureCollection, an array of Features, or one Feature")
            .detail(json!({ "detail": error.to_string().chars().take(120).collect::<String>() }))
    })?;

    let features: Vec<Value> = match &value {
        Value::Array(items) => items.clone(),
        Value::Object(object) => match object.get("features") {
            Some(Value::Array(items)) => items.clone(),
            _ if object.contains_key("geometry") => vec![value.clone()],
            _ => {
                return Err(Failure::invalid(
                    "features_not_geojson",
                    format!("`{raw}` holds no feature array"),
                )
                .remedy("pass a FeatureCollection, an array of Features, or one Feature"));
            }
        },
        _ => {
            return Err(Failure::invalid(
                "features_not_geojson",
                format!("`{raw}` is not a GeoJSON document"),
            )
            .remedy("pass a FeatureCollection, an array of Features, or one Feature"));
        }
    };

    if features.is_empty() {
        return Err(
            Failure::invalid("features_empty", format!("`{raw}` carries no features"))
                .remedy("a layer needs at least one feature"),
        );
    }
    if features.len() > max {
        return Err(Failure::invalid(
            "features_over_bound",
            format!("`{raw}` carries more features than the application accepts"),
        )
        .remedy(format!("split the file into runs of at most {max}"))
        .detail(json!({ "features": features.len(), "max": max })));
    }

    let mut kinds = BTreeSet::new();
    let mut bounds: Option<[f64; 4]> = None;
    for feature in &features {
        if let Some(kind) = feature["geometry"]["type"].as_str() {
            kinds.insert(kind.to_string());
        }
        extend_bounds(&mut bounds, &feature["geometry"]["coordinates"]);
    }

    Ok(Supplied {
        features,
        bbox: bounds,
        kinds,
    })
}

/// Require every feature to carry the declared geometry type — the same check
/// the application makes, made here so the caller hears which feature is
/// wrong instead of which payload was rejected.
pub fn require_geometry(supplied: &Supplied, declared: &str) -> Result<(), Failure> {
    for (index, feature) in supplied.features.iter().enumerate() {
        let kind = feature["geometry"]["type"].as_str().unwrap_or("");
        if kind != declared {
            return Err(Failure::invalid(
                "geometry_mismatch",
                format!("feature {index} is a {} in a {declared} layer", spell(kind)),
            )
            .remedy(GEOMETRY_MISMATCH.remedy)
            .detail(json!({
                "index": index,
                "declared": declared,
                "found": kinds_of(supplied),
            })));
        }
    }
    Ok(())
}

fn spell(kind: &str) -> &str {
    if kind.is_empty() {
        "feature with no geometry"
    } else {
        kind
    }
}

pub fn kinds_of(supplied: &Supplied) -> Vec<String> {
    supplied.kinds.iter().cloned().collect()
}

/// Walk a nested GeoJSON coordinate array, widening `bounds`.
///
/// This is arithmetic over the caller's own input, not geometry: it derives
/// no length, no area and no projection, and asks no engine anything. It
/// exists so `map draw --zoom` can move the map to what it just drew without
/// the caller computing an extent by hand.
fn extend_bounds(bounds: &mut Option<[f64; 4]>, node: &Value) {
    let Some(items) = node.as_array() else { return };
    if let (Some(x), Some(y)) = (
        items.first().and_then(Value::as_f64),
        items.get(1).and_then(Value::as_f64),
    ) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        match bounds {
            Some(box_) => {
                box_[0] = box_[0].min(x);
                box_[1] = box_[1].min(y);
                box_[2] = box_[2].max(x);
                box_[3] = box_[3].max(y);
            }
            None => *bounds = Some([x, y, x, y]),
        }
        return;
    }
    for item in items {
        extend_bounds(bounds, item);
    }
}

// ---------------------------------------------------------------------------
// Flag shapes shared across the domain
// ---------------------------------------------------------------------------

pub const INVALID_BBOX: Refusal = Refusal {
    code: "invalid_bbox",
    when: "--bbox is not four finite degrees, or west/south are not below east/north",
    remedy: "pass --bbox west,south,east,north in degrees",
};
pub const INVALID_PAIR: Refusal = Refusal {
    code: "invalid_pair",
    when: "a key=value flag has no `=`, or an empty key",
    remedy: "write it as --<flag> name=value",
};

/// Parse `west,south,east,north`, applying the same bounds the application
/// applies, so a wrong box is a local refusal rather than a round trip.
pub fn bbox(raw: &str) -> Result<[f64; 4], Failure> {
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    let refuse = |message: &str| {
        Failure::invalid("invalid_bbox", message.to_string())
            .remedy(INVALID_BBOX.remedy)
            .detail(json!({ "given": raw }))
    };
    if parts.len() != 4 {
        return Err(refuse("--bbox takes four comma-separated degrees"));
    }
    let mut values = [0f64; 4];
    for (slot, part) in values.iter_mut().zip(&parts) {
        *slot = part
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| refuse("--bbox values must be finite numbers"))?;
    }
    let [west, south, east, north] = values;
    if !(-180.0..=180.0).contains(&west) || !(-180.0..=180.0).contains(&east) {
        return Err(refuse("--bbox longitudes must be within -180..180"));
    }
    if !(-90.0..=90.0).contains(&south) || !(-90.0..=90.0).contains(&north) {
        return Err(refuse("--bbox latitudes must be within -90..90"));
    }
    if west >= east || south >= north {
        return Err(refuse("--bbox needs west below east and south below north"));
    }
    Ok(values)
}

/// A number flag, held to the bound stated in its own summary.
pub fn number(raw: &str, flag: &str, min: f64, max: f64) -> Result<f64, Failure> {
    let parsed = raw
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| {
            Failure::invalid("invalid_number", format!("`--{flag}` must be a number"))
                .remedy(format!("pass {min}..{max}"))
        })?;
    if parsed < min || parsed > max {
        return Err(
            Failure::invalid("invalid_number", format!("`--{flag}` is outside its bound"))
                .remedy(format!("pass {min}..{max}"))
                .detail(json!({ "given": parsed, "min": min, "max": max })),
        );
    }
    Ok(parsed)
}

/// A `true|false` flag. Declared as a value rather than a switch wherever the
/// application's own default is on: a switch can only ever mean "turn this
/// on", so a switch for a setting that starts on cannot express the caller's
/// actual choice, and help would document a default the flag cannot restore.
pub fn boolean(raw: Option<&str>, default: bool) -> bool {
    match raw {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

pub const BOOL_CHOICES: &[&str] = &["true", "false"];

/// Collect repeated `key=value` flags into one JSON object.
///
/// An empty value becomes JSON `null` rather than `""`, and that is the
/// selector semantic the application publishes: a `null` predicate matches a
/// feature whose property is absent, null or blank — which is the real shape
/// of an unmarked as-built row, since it carries no key at all rather than an
/// explicit `"draft"`. `""` and `null` are already the same thing to the
/// matcher, so nothing is lost and the useful case is reachable.
pub fn pairs(raw: &[String], flag: &str) -> Result<Map<String, Value>, Failure> {
    let mut object = Map::new();
    for entry in raw {
        let Some((key, value)) = entry.split_once('=') else {
            return Err(
                Failure::invalid("invalid_pair", format!("`--{flag} {entry}` has no `=`"))
                    .remedy(format!("write it as --{flag} name=value")),
            );
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(Failure::invalid(
                "invalid_pair",
                format!("`--{flag} {entry}` has an empty name"),
            )
            .remedy(format!("write it as --{flag} name=value")));
        }
        object.insert(
            key.to_string(),
            if value.is_empty() {
                Value::Null
            } else {
                Value::String(value.to_string())
            },
        );
    }
    Ok(object)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, body: &str) -> String {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, body).expect("temp file is writable");
        path.display().to_string()
    }

    fn load(name: &str, body: &str) -> Result<Supplied, Failure> {
        load_features(&write(name, body), "features", MAX_LAYER_FEATURES)
    }

    const ONE_LINE: &str = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{},"geometry":
         {"type":"LineString","coordinates":[[30.0,-1.9],[30.4,-2.2],[29.8,-1.7]]}}]}"#;

    #[test]
    fn a_features_file_is_read_in_all_three_shapes_an_export_arrives_in() {
        // A FeatureCollection is what a GIS tool writes; a bare array is what
        // a script writes; a single feature is what a person pastes. Reading
        // only the first would refuse two of the three with
        // `features_not_geojson`, which reads like the file is broken.
        let collection = load("ds-map-shape-collection.geojson", ONE_LINE).expect("collection");
        let array = load(
            "ds-map-shape-array.geojson",
            r#"[{"type":"Feature","properties":{},"geometry":
                {"type":"LineString","coordinates":[[30.0,-1.9],[30.4,-2.2],[29.8,-1.7]]}}]"#,
        )
        .expect("array");
        let single = load(
            "ds-map-shape-single.geojson",
            r#"{"type":"Feature","properties":{},"geometry":
                {"type":"LineString","coordinates":[[30.0,-1.9],[30.4,-2.2],[29.8,-1.7]]}}"#,
        )
        .expect("single feature");

        for supplied in [&collection, &array, &single] {
            assert_eq!(supplied.features.len(), 1);
            assert_eq!(kinds_of(supplied), vec!["LineString".to_string()]);
        }
    }

    #[test]
    fn the_extent_is_the_full_span_of_every_coordinate_not_the_first_pair() {
        // The bbox `--zoom` moves the map to. Walking only the outer array,
        // or stopping at the first coordinate pair, both produce a plausible
        // box that is wrong — and a map that moves somewhere near the data is
        // harder to notice than one that does not move at all.
        let supplied = load("ds-map-extent.geojson", ONE_LINE).expect("read");
        let bbox = supplied.bbox.expect("a line has an extent");
        assert_eq!(bbox, [29.8, -2.2, 30.4, -1.7]);
    }

    #[test]
    fn a_polygon_extent_reaches_through_both_levels_of_nesting() {
        // Polygon coordinates are rings of pairs, one array deeper than a
        // line's. A walker that assumed one depth would return no extent at
        // all for every polygon layer.
        let supplied = load(
            "ds-map-extent-polygon.geojson",
            r#"{"type":"Feature","properties":{},"geometry":{"type":"Polygon",
                "coordinates":[[[30.0,-1.9],[30.5,-1.9],[30.5,-2.4],[30.0,-2.4],[30.0,-1.9]]]}}"#,
        )
        .expect("read");
        assert_eq!(supplied.bbox, Some([30.0, -2.4, 30.5, -1.9]));
    }

    #[test]
    fn a_mixed_file_is_refused_by_naming_every_geometry_it_holds() {
        let supplied = load(
            "ds-map-mixed.geojson",
            r#"{"type":"FeatureCollection","features":[
                {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[30.0,-1.9]}},
                {"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[30.0,-1.9],[30.1,-2.0]]}}]}"#,
        )
        .expect("read");
        assert_eq!(
            kinds_of(&supplied),
            vec!["LineString".to_string(), "Point".to_string()]
        );
        let refusal = require_geometry(&supplied, "Point").expect_err("mixed must refuse");
        assert_eq!(refusal.code(), "geometry_mismatch");
        assert_eq!(
            refusal.detail_value().expect("detail")["index"],
            1,
            "the second feature is the one that does not fit"
        );
    }

    #[test]
    fn an_empty_file_and_a_non_geojson_file_are_told_apart() {
        // Both are "this file will not do", and they need different fixes.
        assert_eq!(
            load(
                "ds-map-empty.geojson",
                r#"{"type":"FeatureCollection","features":[]}"#
            )
            .expect_err("empty")
            .code(),
            "features_empty"
        );
        assert_eq!(
            load("ds-map-notjson.geojson", "not json at all")
                .expect_err("not json")
                .code(),
            "features_not_geojson"
        );
        assert_eq!(
            load("ds-map-nofeatures.geojson", r#"{"type":"Topology"}"#)
                .expect_err("no features")
                .code(),
            "features_not_geojson"
        );
    }

    #[test]
    fn a_bbox_is_held_to_the_bounds_the_application_holds_it_to() {
        assert_eq!(
            bbox("29.9,-2.1,30.2,-1.85").expect("valid"),
            [29.9, -2.1, 30.2, -1.85]
        );
        // Transposed: the commonest mistake, and the one that would otherwise
        // travel to the application before being refused.
        assert_eq!(
            bbox("30.2,-1.85,29.9,-2.1").expect_err("transposed").code(),
            "invalid_bbox"
        );
        for bad in ["1,2,3", "a,b,c,d", "200,-2,201,-1", "29,-95,30,-94", ""] {
            assert_eq!(
                bbox(bad).expect_err("must refuse").code(),
                "invalid_bbox",
                "`{bad}` was accepted as a bounding box"
            );
        }
    }

    #[test]
    fn an_empty_selector_value_means_unset_not_empty_string() {
        // `--where drafting_status=` has to reach the application as JSON
        // null, because null is its predicate for "absent, null or blank" —
        // which is the real shape of an unmarked as-built row. Sending "" for
        // it would be the difference between finding those rows and finding
        // none of them.
        let parsed = pairs(
            &[
                "drafting_status=".to_string(),
                "phase=three".to_string(),
                " spaced =value".to_string(),
            ],
            "where",
        )
        .expect("valid pairs");
        assert_eq!(parsed["drafting_status"], Value::Null);
        assert_eq!(parsed["phase"], Value::String("three".into()));
        assert!(parsed.contains_key("spaced"), "the key is trimmed");

        assert_eq!(
            pairs(&["novalue".to_string()], "where")
                .expect_err("no equals")
                .code(),
            "invalid_pair"
        );
        assert_eq!(
            pairs(&["=orphan".to_string()], "where")
                .expect_err("no key")
                .code(),
            "invalid_pair"
        );
    }

    #[test]
    fn a_boolean_flag_falls_back_to_the_applications_own_default() {
        // `--include-ends` is a value flag rather than a switch precisely so
        // a caller can turn off something that starts on. If an unparseable
        // value silently flipped the default, that would be worse than
        // refusing — the parser's `choices` is what refuses.
        assert!(boolean(None, true));
        assert!(!boolean(None, false));
        assert!(!boolean(Some("false"), true));
        assert!(boolean(Some("true"), false));
    }
}
