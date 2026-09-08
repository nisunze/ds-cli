//! Design-geometry extents for the project dataset room.
//!
//! `project_dataset_cache` decides how far around an extent a project acquires;
//! this module decides what the extent *is*. Splitting them keeps the coverage
//! mathematics free of GeoJSON shapes, and it puts the one remaining piece of
//! geometry reading — "these design layers occupy this rectangle" — in Rust,
//! where the browser, the desktop and `ds` read it identically. A host that
//! re-derived an extent in its own language would eventually disagree with the
//! coverage plan built from it, and a coverage that disagrees with the design
//! it was planned for is exactly the failure the room exists to prevent.
//!
//! Two decisions here are load-bearing:
//!
//! * **An extent is exact, not decorated.** A transformer with a single point
//!   and no drawn network yields a zero-area rectangle, and `plan` reads that
//!   zero area as "undesigned" and applies the wider isolated buffer. Padding a
//!   point to make it look like a rectangle — which a renderer legitimately
//!   wants, so `fitBounds` has something to fit — would silently downgrade
//!   every bare transformer to the narrow design buffer. Padding is therefore an
//!   explicit, caller-supplied renderer concern that defaults to none.
//! * **The design in hand outranks nothing and is missed by nothing.** A print
//!   runs against unsaved layers that no cached room has yet seen. `merge`
//!   folds those held designs into the cached roster by union, so a plan can
//!   neither miss the design being printed nor drop coverage the saved design
//!   still needs.
//!
//! Nothing here performs I/O, and nothing here decides what to acquire.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::project_dataset_cache::{Extent, MAX_EXTENTS};

pub const SCHEMA: &str = "ds.project-design-extent/v1";

/// A design is a bounded set of named layers, not an arbitrary document store.
pub const MAX_LAYERS: usize = 512;
/// How many unsaved designs one caller may hold at once. A print holds one.
pub const MAX_HELD_DESIGNS: usize = 64;
/// Renderer padding is a nudge for `fitBounds`, never a coverage decision.
pub const MAX_POINT_PADDING_DEGREES: f64 = 1.0;
const MAX_NAME_BYTES: usize = 256;

fn union_bounds(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

/// One layer's extent.
///
/// Design layers reach this module in two shapes: a complete GeoJSON document,
/// and the renderer's bare `{ "features": [...] }` collection with no `type`.
/// Both are read without copying the geometry — a project's layers are large,
/// and normalising them into a new document first would be the most expensive
/// thing this module does.
fn layer_bounds(layer: &Value) -> Option<[f64; 4]> {
    if layer.get("type").is_some() {
        return ds_geo::viewport::geojson_bounds(layer);
    }
    layer
        .get("features")?
        .as_array()?
        .iter()
        .filter_map(member_bounds)
        .reduce(union_bounds)
}

/// One member of a bare `{ "features": [...] }` collection.
///
/// Read through its `geometry` exactly as a declared FeatureCollection's walk
/// would. Design layers routinely omit `"type": "Feature"` on their members, and
/// requiring it here would silently return "no extent" for a real design — the
/// worst possible answer, because an empty extent reads as an undesigned
/// transformer rather than as an error.
fn member_bounds(feature: &Value) -> Option<[f64; 4]> {
    match feature.get("geometry") {
        Some(geometry) => ds_geo::viewport::geojson_bounds(geometry),
        None => ds_geo::viewport::geojson_bounds(feature),
    }
}

/// Widen a degenerate axis so a renderer has something to fit.
///
/// Applied only where the extent has no width at all, and only when a caller
/// asks for it. An extent with real width is returned untouched, so this can
/// never quietly enlarge what a project acquires.
fn padded(bounds: [f64; 4], degrees: f64) -> [f64; 4] {
    if degrees <= 0.0 {
        return bounds;
    }
    let [mut w, mut s, mut e, mut n] = bounds;
    if w == e {
        w = (w - degrees).max(-180.0);
        e = (e + degrees).min(180.0);
    }
    if s == n {
        s = (s - degrees).max(-90.0);
        n = (n + degrees).min(90.0);
    }
    [w, s, e, n]
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("a design extent needs the transformer it belongs to".into());
    }
    if name.len() > MAX_NAME_BYTES {
        return Err("transformer name is too long".into());
    }
    Ok(())
}

fn validate_padding(degrees: f64) -> Result<(), String> {
    if !degrees.is_finite() || !(0.0..=MAX_POINT_PADDING_DEGREES).contains(&degrees) {
        return Err(format!(
            "point padding must be between 0 and {MAX_POINT_PADDING_DEGREES} degrees"
        ));
    }
    Ok(())
}

/// Read the rectangle a set of design layers occupies.
///
/// `None` means the design holds no usable WGS84 coordinate at all — an empty
/// roster, empty collections, or a file whose coordinates were never projected
/// into WGS84. That is the undesigned case, reported rather than guessed at.
pub fn layers_bounds(layers: &Value, point_padding_degrees: f64) -> Result<Option<[f64; 4]>, String> {
    validate_padding(point_padding_degrees)?;
    if layers.is_null() {
        return Ok(None);
    }
    let map = layers
        .as_object()
        .ok_or("design layers must be an object of layer name to feature collection")?;
    if map.len() > MAX_LAYERS {
        return Err(format!("a design carries at most {MAX_LAYERS} layers"));
    }
    Ok(map
        .values()
        .filter_map(layer_bounds)
        .reduce(union_bounds)
        .map(|bounds| padded(bounds, point_padding_degrees)))
}

/// One transformer's extent, in the shape a coverage plan consumes.
pub fn design_extent(
    name: &str,
    layers: &Value,
    point_padding_degrees: f64,
) -> Result<Extent, String> {
    validate_name(name)?;
    Ok(Extent {
        name: name.to_string(),
        bounds: layers_bounds(layers, point_padding_degrees)?,
    })
}

/// An unsaved design the caller is holding — the transformer being printed.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeldDesign {
    pub name: String,
    #[serde(default)]
    pub layers: Value,
}

#[derive(Debug, Serialize)]
pub struct MergedExtents {
    pub schema: &'static str,
    pub extents: Vec<Extent>,
    /// Held designs that no cached room knew about at all.
    pub added: Vec<String>,
    /// Held designs whose geometry reaches past what the cached room holds.
    pub widened: Vec<String>,
    pub cached_count: usize,
    pub held_count: usize,
}

/// Fold the designs in hand into the project's cached roster.
///
/// The union is deliberate in both directions. A held design that grew must not
/// be missed, and a held design that *shrank* must not retract coverage the
/// saved design still needs — another window may be printing that saved version
/// right now, and the room is shared. Surplus coverage is already reported
/// separately by `assess`, so over-covering here costs a plan nothing.
pub fn merge_extents(cached: &[Extent], held: &[HeldDesign]) -> Result<MergedExtents, String> {
    if cached.len() > MAX_EXTENTS {
        return Err(format!(
            "a project coverage plan reads at most {MAX_EXTENTS} transformer extents"
        ));
    }
    if held.len() > MAX_HELD_DESIGNS {
        return Err(format!(
            "at most {MAX_HELD_DESIGNS} held designs can join one acquisition plan"
        ));
    }
    let mut merged: BTreeMap<String, Option<[f64; 4]>> = BTreeMap::new();
    for extent in cached {
        validate_name(&extent.name)?;
        let entry = merged.entry(extent.name.clone()).or_default();
        *entry = match (*entry, extent.bounds) {
            (Some(a), Some(b)) => Some(union_bounds(a, b)),
            (a, b) => a.or(b),
        };
    }
    let mut added = Vec::new();
    let mut widened = Vec::new();
    for design in held {
        let extent = design_extent(&design.name, &design.layers, 0.0)?;
        let known = merged.get(&extent.name).copied();
        match (known.flatten(), extent.bounds) {
            (_, None) => {
                // A held design with no geometry adds nothing and hides nothing.
                merged.entry(extent.name.clone()).or_default();
                if known.is_none() {
                    added.push(extent.name.clone());
                }
            }
            (None, Some(bounds)) => {
                merged.insert(extent.name.clone(), Some(bounds));
                if known.is_none() {
                    added.push(extent.name.clone());
                } else {
                    widened.push(extent.name.clone());
                }
            }
            (Some(cached_bounds), Some(bounds)) => {
                let union = union_bounds(cached_bounds, bounds);
                if union != cached_bounds {
                    widened.push(extent.name.clone());
                }
                merged.insert(extent.name.clone(), Some(union));
            }
        }
    }
    let extents: Vec<Extent> = merged
        .into_iter()
        .map(|(name, bounds)| Extent { name, bounds })
        .collect();
    if extents.len() > MAX_EXTENTS {
        return Err(format!(
            "a project coverage plan reads at most {MAX_EXTENTS} transformer extents"
        ));
    }
    Ok(MergedExtents {
        schema: SCHEMA,
        cached_count: cached.len(),
        held_count: held.len(),
        added,
        widened,
        extents,
    })
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Extent {
        schema: String,
        name: String,
        #[serde(default)]
        layers: Value,
        /// Renderer-only. A coverage plan always leaves this at zero.
        #[serde(default)]
        point_padding_degrees: f64,
        /// Metres to grow the located extent by, through the same helper the
        /// coverage policy buffers rooms with. A print context asks for this;
        /// nothing in TypeScript converts metres to degrees.
        #[serde(default)]
        buffer_m: Option<f64>,
    },
    Merge {
        schema: String,
        #[serde(default)]
        cached: Vec<Extent>,
        #[serde(default)]
        held: Vec<HeldDesign>,
    },
}

fn require_schema(schema: &str) -> Result<(), String> {
    if schema != SCHEMA {
        return Err(format!("project design extent request must use {SCHEMA}"));
    }
    Ok(())
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    let request: Request =
        serde_json::from_slice(input).map_err(|error| format!("invalid request: {error}"))?;
    let value = match request {
        Request::Extent {
            schema,
            name,
            layers,
            point_padding_degrees,
            buffer_m,
        } => {
            require_schema(&schema)?;
            if let Some(metres) = buffer_m {
                if !metres.is_finite() || !(0.0..=50_000.0).contains(&metres) {
                    return Err("buffer_m must be between 0 and 50000 metres".into());
                }
            }
            let extent = design_extent(&name, &layers, point_padding_degrees)?;
            let located = extent.bounds.is_some();
            let degenerate = extent
                .bounds
                .is_some_and(|[w, s, e, n]| w == e || s == n);
            let buffered = match (extent.bounds, buffer_m) {
                (Some([w, s, e, n]), Some(metres)) => {
                    let rect = crate::project_dataset_cache::buffered(
                        crate::project_dataset_cache::Rect { w, s, e, n },
                        metres,
                    );
                    Some([rect.w, rect.s, rect.e, rect.n])
                }
                _ => None,
            };
            json!({
                "schema": SCHEMA,
                "extent": serde_json::to_value(&extent).map_err(|error| error.to_string())?,
                "located": located,
                "degenerate": degenerate,
                "buffered": buffered,
            })
        }
        Request::Merge {
            schema,
            cached,
            held,
        } => {
            require_schema(&schema)?;
            serde_json::to_value(merge_extents(&cached, &held)?)
                .map_err(|error| error.to_string())?
        }
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffered_extent_grows_through_the_coverage_policy_helper() {
        let request = json!({
            "action": "extent", "schema": SCHEMA, "name": "t1", "buffer_m": 200.0,
            "layers": {"lv_lines": {"features": [
                {"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.001,-1.999]]}}
            ]}}
        });
        let value: Value =
            serde_json::from_str(&evaluate(request.to_string().as_bytes()).unwrap()).unwrap();
        let buffered = value["buffered"].as_array().unwrap();
        let [w, s, e, n]: [f64; 4] =
            [0, 1, 2, 3].map(|i| buffered[i].as_f64().unwrap());
        // 200 m is ~0.0018° of latitude; the west/east growth is at least that.
        assert!((n - -1.999) > 0.0017 && (n - -1.999) < 0.0019);
        assert!((-2.0 - s) > 0.0017 && (-2.0 - s) < 0.0019);
        assert!(30.0 - w > 0.0017 && e - 30.001 > 0.0017);
        assert!(value["extent"]["bounds"].as_array().unwrap().len() == 4);
        let none = json!({"action": "extent", "schema": SCHEMA, "name": "t1", "layers": {}});
        let value: Value =
            serde_json::from_str(&evaluate(none.to_string().as_bytes()).unwrap()).unwrap();
        assert!(value["buffered"].is_null());
        let bad = json!({"action": "extent", "schema": SCHEMA, "name": "t1", "layers": {}, "buffer_m": -1.0});
        assert!(evaluate(bad.to_string().as_bytes()).is_err());
    }

    fn point(lon: f64, lat: f64) -> Value {
        json!({"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[lon,lat]}})
    }

    fn layers(features: Vec<Value>) -> Value {
        json!({"lv_lines": {"features": features}})
    }

    #[test]
    fn a_design_with_no_geometry_reports_no_extent_instead_of_guessing() {
        assert_eq!(layers_bounds(&json!({}), 0.0).unwrap(), None);
        assert_eq!(layers_bounds(&Value::Null, 0.0).unwrap(), None);
        assert_eq!(layers_bounds(&layers(vec![]), 0.0).unwrap(), None);
        // Coordinates that were never projected into WGS84 are dropped, not
        // turned into a nonsensical rectangle.
        assert_eq!(
            layers_bounds(&layers(vec![point(521_000., 9_770_000.)]), 0.0).unwrap(),
            None
        );
        let extent = design_extent("t1", &json!({}), 0.0).unwrap();
        assert_eq!(extent.name, "t1");
        assert!(extent.bounds.is_none());
        assert!(design_extent(" ", &json!({}), 0.0).is_err());
        assert!(layers_bounds(&json!([]), 0.0).is_err());
    }

    #[test]
    fn a_single_point_transformer_stays_zero_area_so_it_reads_as_undesigned() {
        let bounds = layers_bounds(&layers(vec![point(30.06, -1.94)]), 0.0)
            .unwrap()
            .unwrap();
        assert_eq!(bounds, [30.06, -1.94, 30.06, -1.94]);
        // `project_dataset_cache::plan` calls exactly this comparison to decide
        // that a transformer has no designed network and earns the wider
        // isolated buffer. Padding it here would silently narrow that buffer.
        assert!(bounds[0] == bounds[2] && bounds[1] == bounds[3]);
        // A renderer that needs something to fit asks for padding explicitly.
        let padded = layers_bounds(&layers(vec![point(30.06, -1.94)]), 0.0005)
            .unwrap()
            .unwrap();
        for (got, want) in padded.iter().zip([30.0595, -1.9405, 30.0605, -1.9395]) {
            assert!((got - want).abs() < 1e-9, "{padded:?}");
        }
        assert!(layers_bounds(&layers(vec![point(30., -2.)]), 2.0).is_err());
        assert!(layers_bounds(&layers(vec![point(30., -2.)]), f64::NAN).is_err());
    }

    #[test]
    fn every_layer_and_geometry_kind_contributes_to_one_union() {
        let design = json!({
            "lv_poles": {"features": [point(30.0, -2.0), point(30.002, -1.998)]},
            // The complete-document shape, read without normalising it first.
            "lv_lines": {"type": "FeatureCollection", "features": [
                {"type":"Feature","properties":{},"geometry":{"type":"LineString",
                    "coordinates":[[29.995,-2.004],[30.001,-1.999]]}}
            ]},
            "service_areas": {"features": [
                {"type":"Feature","properties":{},"geometry":{"type":"MultiPolygon",
                    "coordinates":[[[[30.0,-2.0],[30.004,-2.0],[30.004,-1.996],[30.0,-1.996],[30.0,-2.0]]]]}}
            ]},
            // A feature with no geometry is skipped rather than failing the read.
            "annotations": {"features": [{"type":"Feature","properties":{},"geometry":null}]},
            // The shape design layers actually carry: a member with a geometry
            // and no `type` of its own.
            "sketch": {"features": [{"geometry":{"type":"Point","coordinates":[30.006,-1.995]}}]},
            "empty": {"features": []}
        });
        // The type-less member widens the union, so it is provably read.
        assert_eq!(
            layers_bounds(&design, 0.0).unwrap().unwrap(),
            [29.995, -2.004, 30.006, -1.995]
        );
        // Padding never touches an extent that already has width.
        assert_eq!(
            layers_bounds(&design, 0.0005).unwrap().unwrap(),
            [29.995, -2.004, 30.006, -1.995]
        );
    }

    #[test]
    fn the_design_in_hand_joins_the_cached_roster_and_is_never_missed() {
        let cached = vec![
            Extent {
                name: "t1".into(),
                bounds: Some([30.0, -2.0, 30.002, -1.998]),
            },
            Extent {
                name: "t2".into(),
                bounds: None,
            },
        ];
        // An unsaved design that reaches past its saved room, plus one the
        // project has never cached at all.
        let held = vec![
            HeldDesign {
                name: "t1".into(),
                layers: layers(vec![point(30.010, -1.990)]),
            },
            HeldDesign {
                name: "t9".into(),
                layers: layers(vec![point(31.0, -2.0), point(31.002, -1.998)]),
            },
        ];
        let merged = merge_extents(&cached, &held).unwrap();
        assert_eq!(
            merged.extents.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["t1", "t2", "t9"]
        );
        // Union, not replacement: the saved extent another window may still be
        // printing stays covered.
        assert_eq!(merged.extents[0].bounds, Some([30.0, -2.0, 30.010, -1.990]));
        assert_eq!(merged.extents[1].bounds, None);
        assert_eq!(merged.extents[2].bounds, Some([31.0, -2.0, 31.002, -1.998]));
        assert_eq!(merged.widened, ["t1"]);
        assert_eq!(merged.added, ["t9"]);
        assert_eq!((merged.cached_count, merged.held_count), (2, 2));

        // A held design inside its cached extent changes nothing.
        let quiet = merge_extents(
            &cached,
            &[HeldDesign {
                name: "t1".into(),
                layers: layers(vec![point(30.001, -1.999)]),
            }],
        )
        .unwrap();
        assert_eq!(quiet.extents[0].bounds, cached[0].bounds);
        assert!(quiet.widened.is_empty() && quiet.added.is_empty());

        // A cached transformer with no design that is now being drawn.
        let drawn = merge_extents(
            &cached,
            &[HeldDesign {
                name: "t2".into(),
                layers: layers(vec![point(30.5, -2.5)]),
            }],
        )
        .unwrap();
        assert_eq!(drawn.extents[1].bounds, Some([30.5, -2.5, 30.5, -2.5]));
        assert_eq!(drawn.widened, ["t2"]);
        assert!(drawn.added.is_empty());

        // A held design with nothing drawn yet is still a named transformer.
        let bare = merge_extents(
            &[],
            &[HeldDesign {
                name: "t5".into(),
                layers: json!({}),
            }],
        )
        .unwrap();
        assert_eq!(bare.extents.len(), 1);
        assert!(bare.extents[0].bounds.is_none());
        assert_eq!(bare.added, ["t5"]);

        assert!(merge_extents(&[], &[HeldDesign { name: "".into(), layers: json!({}) }]).is_err());
    }

    #[test]
    fn the_dispatcher_answers_both_actions_and_refuses_a_foreign_schema() {
        let request = json!({"schema": SCHEMA, "action": "extent", "name": "t1",
            "layers": {"lv": {"features": [{"type":"Feature","properties":{},
                "geometry":{"type":"Point","coordinates":[30.,-2.]}}]}}});
        let result: Value =
            serde_json::from_str(&evaluate(request.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(result["schema"], SCHEMA);
        assert_eq!(result["extent"]["name"], "t1");
        assert_eq!(result["extent"]["bounds"], json!([30., -2., 30., -2.]));
        assert_eq!(result["located"], json!(true));
        assert_eq!(result["degenerate"], json!(true));

        let merge = json!({"schema": SCHEMA, "action": "merge",
            "cached": [{"name": "t1", "bounds": [30., -2., 30.002, -1.998]}],
            "held": [{"name": "t2", "layers": {"lv": {"features": [
                {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[31.,-2.]}}]}}}]});
        let merged: Value =
            serde_json::from_str(&evaluate(merge.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(merged["extents"].as_array().unwrap().len(), 2);
        assert_eq!(merged["added"], json!(["t2"]));

        assert!(evaluate(b"{}").is_err());
        let wrong = json!({"schema": "ds.other/v1", "action": "extent", "name": "t1", "layers": {}});
        assert!(evaluate(wrong.to_string().as_bytes()).is_err());
        let unknown = json!({"schema": SCHEMA, "action": "extent", "name": "t1", "layers": {}, "wat": 1});
        assert!(evaluate(unknown.to_string().as_bytes()).is_err());
    }
}
