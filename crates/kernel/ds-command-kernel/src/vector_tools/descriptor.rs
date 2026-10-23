//! The vector tool catalogue: one descriptor per tool, built from the
//! checked-in tool list. Every default, range and refusal a request meets is
//! read from these schemas; hosts never restate them.
use ds_geo::layer::ipc::{GEOMETRY, ID};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;
pub const BUFFER_SUMMARY: &str = "Buffer each feature by a fixed distance into a polygon zone.";
pub const SAMPLE_SUMMARY: &str = "Place points along each line at a fixed interval.";
pub const INTERSECT_SUMMARY: &str = "Find the points where two line documents cross.";
pub const MEASURE_SUMMARY: &str = "Append geometry measures and report length, area and vertices.";
pub const OUTLIERS_SUMMARY: &str = "Flag isolated, oversized or undersized geometries.";
pub const COLLISIONS_SUMMARY: &str = "Read and rank an existing project collision report.";
pub const RANDOM_SUMMARY: &str = "Scatter reproducible points in polygons or buffered corridors.";
pub const DESCRIPTOR_VERSION: &str = "ds.vector-tool/v1";

#[derive(Debug, Clone, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    pub remedy: &'static str,
}
pub(crate) fn refuse(code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        message: message.into(),
        remedy: "Read ds data vector describe --tool <id>; use its schema and ranges.",
    }
}

#[derive(Clone, Deserialize)]
pub(super) struct Tool {
    pub(super) id: String,
    pub(super) name: String,
    category: String,
    category_name: String,
    pub(super) command: String,
    description: String,
}
fn catalogue() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        serde_json::from_str(include_str!("catalogue.json")).expect("checked-in catalogue")
    })
}
fn collision_tool() -> &'static Tool {
    static TOOL: OnceLock<Tool> = OnceLock::new();
    TOOL.get_or_init(|| Tool {
        id: "analysis::Collisions".into(),
        name: "Collisions".into(),
        category: "analysis".into(),
        category_name: "Vector analysis".into(),
        command: "collisions".into(),
        description:
            "Read the existing reporter's collision regions; detection remains a project operation."
                .into(),
    })
}
pub(super) fn tool(id: &str) -> Result<&'static Tool, Refusal> {
    catalogue()
        .iter()
        .chain(std::iter::once(collision_tool()))
        .find(|t| t.id == id || t.command == id)
        .ok_or_else(|| refuse("vector_tool_unknown", format!("Unknown vector tool {id}.")))
}
pub(super) fn available(command: &str) -> bool {
    matches!(
        command,
        "buffer"
            | "sample"
            | "intersect"
            | "measure"
            | "outliers"
            | "random-points-area"
            | "collisions"
    )
}
pub(super) fn field(name: &str, kind: &str, unit: &str) -> Value {
    json!({"name":name,"type":kind,"unit":unit,"nullable":matches!(name,"source_id"|"against_id"|"elevation_m")})
}
pub(super) fn output_fields(command: &str) -> Vec<Value> {
    match command {
        "collisions" => vec![
            field("id", "string", ""),
            field("transformerCount", "integer", "count"),
            field("pairCount", "integer", "count"),
            field("coveragePct", "number", "percent"),
            field("overlapClass", "string", ""),
            field("canConsolidate", "boolean", ""),
            field("lvCrossings", "integer", "count"),
            field("serviceCrossings", "integer", "count"),
            field("sharedCustomers", "integer", "count"),
        ],
        "measure" => vec![
            field("length_m", "number", "m"),
            field("area_m2", "number", "m2"),
            field("vertices", "integer", "count"),
        ],
        "sample" => vec![
            field("source_index", "integer", "index"),
            field("source_id", "string", ""),
            field("part_index", "integer", "zero_based"),
            field("distance_m", "number", "m"),
        ],
        "buffer" => vec![
            field("source_index", "integer", "index"),
            field("source_id", "string", ""),
            field("source_kind", "string", ""),
            field("buffer_radius_m", "number", "m"),
        ],
        "intersect" => vec![
            field("source_index", "integer", "index"),
            field("source_id", "string", ""),
            field("against_index", "integer", "index"),
            field("against_id", "string", ""),
        ],
        "random-points-area" => vec![
            field("source_feature_id", "string", ""),
            field("source_layer", "string", ""),
            field("point_index", "integer", "one_based"),
        ],
        "outliers" => [
            "_ds_outlier_score",
            "_ds_outlier_reasons",
            "_ds_outlier_source_id",
            "_ds_outlier_source_layer",
            "_ds_outlier_nearest_distance",
            "_ds_outlier_size_value",
            "_ds_outlier_size_kind",
            "_ds_outlier_extent_value",
        ]
        .iter()
        .map(|n| {
            field(
                n,
                if n.ends_with("score") || n.ends_with("distance") || n.ends_with("value") {
                    "number"
                } else {
                    "string"
                },
                "",
            )
        })
        .collect(),
        "mean-coordinates" => vec![field("x", "number", "deg"), field("y", "number", "deg")],
        "count-points-in-polygon" => vec![field("point_count", "integer", "count")],
        "sum-line-lengths" => vec![field("length_m", "number", "m")],
        "overlap-analysis" => vec![
            field("area_m2", "number", "m2"),
            field("overlap_percent", "number", "percent"),
        ],
        "check-validity" => vec![field("valid", "boolean", ""), field("reason", "string", "")],
        "extract-vertices" => vec![
            field("vertex_index", "integer", "zero_based"),
            field("part_index", "integer", "zero_based"),
        ],
        "nearest-hub" | "hub-distance-to-points" | "hub-lines" => vec![
            field("hub_id", "string", ""),
            field("distance_m", "number", "m"),
        ],
        "join-by-nearest" => vec![field("distance_m", "number", "m")],
        "random-points-on-lines" | "regular-points" => {
            vec![field("point_index", "integer", "one_based")]
        }
        "create-grid" => vec![field("grid_id", "string", "")],
        "shortest-path-point-to-point" | "shortest-path-layer-to-point" => vec![
            field("cost", "number", "cost"),
            field("length_m", "number", "m"),
        ],
        "service-area-from-point" | "service-area-from-layer" => {
            vec![field("cost", "number", "cost")]
        }
        _ => vec![],
    }
}

mod generation {
    use super::*;
    fn object(properties: Value) -> Value {
        json!({"type":"object", "additionalProperties":false, "properties":properties, "default":{}})
    }
    fn number(default: f64, min: f64, max: f64, unit: &str) -> Value {
        json!({"type":"number", "default":default,"minimum":min,"maximum":max,"unit":unit,"x-refusal":if unit=="m" {"vector_distance_out_of_range"}else{"vector_request_invalid"}})
    }
    fn integer(default: usize, min: usize, max: usize) -> Value {
        json!({"type":"integer","default":default,"minimum":min,"maximum":max})
    }
    fn arc_segments() -> Value {
        let mut schema = integer(8, 1, 64);
        schema["x-refusal"] = json!("vector_distance_out_of_range");
        schema
    }
    fn limit_schema() -> Value {
        let mut schema = integer(500, 1, 20000);
        schema["x-refusal"] = json!("vector_limit_out_of_range");
        schema
    }
    fn text(default: &str) -> Value {
        json!({"type":"string","default":default})
    }
    fn choice(default: &str, values: &[&str]) -> Value {
        json!({"type":"string","enum":values,"default":default})
    }
    fn boolean(default: bool) -> Value {
        json!({"type":"boolean","default":default})
    }
    fn distance(default: f64) -> Value {
        number(default, 0.01, 1_000_000., "m")
    }
    fn fields() -> Value {
        json!({"type":"array","items":{"type":"string"},"default":[]})
    }

    /// Roadmap schemas are proposed contracts, not executable algorithms. All
    /// parameters live here before their form or implementation is introduced.
    fn parameters(command: &str) -> Value {
        let p = match command {
            "buffer" => json!({"radius_m":number(25.,0.01,100_000.,"m"),"segments":arc_segments()}),
            "sample" => json!({"interval_m":distance(100.),"include_ends":boolean(false)}),
            "outliers" => {
                json!({"threshold":number(3.5,1.,20.,"robust_z_score"),"min_features":integer(5,3,100),"spatial_isolation":boolean(true),"size_outliers":boolean(true),"extent_outliers":boolean(true)})
            }
            "collisions" => {
                json!({"loaded":boolean(true),"computed":boolean(true),"last_read_computed":json!({"type":["boolean","null"],"default":null})})
            }
            "random-points-area" => {
                json!({"min_spacing_m":distance(50.),"max_spacing_m":distance(100.),"buffer_distance_m":number(25.,0.,1_000_000.,"m"),"seed":json!({"type":"integer","default":0,"minimum":0,"maximum":9_007_199_254_740_991u64,"unit":"seed"}),"sample_elevation":boolean(false),"elevations_m":json!({"type":"array","items":{"type":["number","null"],"unit":"m"},"default":[],"description":"Host terrain readings in returned point order; null means unavailable."})})
            }
            "variable-distance-buffer" => {
                json!({"field":text("distance_m"),"segments":arc_segments()})
            }
            "concave-hull" => {
                json!({"ratio":number(0.5,0.,1.,"ratio"),"allow_holes":boolean(false)})
            }
            "dissolve" | "collect-geometries" | "promote-to-multipart" => {
                json!({"fields":fields()})
            }
            "eliminate-sliver-polygons" => {
                json!({"area_threshold_m2":number(1.,0.,1e12,"m2"),"method":choice("largest_area",&["largest_area","longest_boundary"])})
            }
            "subdivide" => json!({"max_vertices":integer(256,4,100000)}),
            "densify-by-count" => json!({"count":integer(1,1,10000)}),
            "densify-by-interval" | "regular-points" | "create-grid" | "snap-points-to-grid" => {
                json!({"spacing_m":distance(100.),"grid_type":choice("rectangular",&["rectangular","hexagonal"])})
            }
            "simplify" | "snap-geometries-to-layer" => json!({"tolerance_m":distance(1.)}),
            "smooth" => json!({"iterations":integer(1,1,10),"offset":number(0.25,0.,0.5,"ratio")}),
            "points-to-path" => json!({"order_field":text("id"),"group_field":text("")}),
            "delete-holes" => json!({"max_area_m2":number(0.,0.,1e12,"m2")}),
            "extend-lines" => json!({"start_m":number(0.,0.,1e6,"m"),"end_m":distance(10.)}),
            "rotate" => {
                json!({"angle_deg":number(0.,-360.,360.,"deg"),"anchor":choice("centroid",&["centroid","origin"])})
            }
            "translate" => json!({"dx_m":number(0.,-1e6,1e6,"m"),"dy_m":number(0.,-1e6,1e6,"m")}),
            "affine-transform" => {
                json!({"scale_x":number(1.,-1e6,1e6,"ratio"),"scale_y":number(1.,-1e6,1e6,"ratio"),"rotation_deg":number(0.,-360.,360.,"deg"),"shear_x":number(0.,-100.,100.,"ratio"),"shear_y":number(0.,-100.,100.,"ratio"),"dx_m":number(0.,-1e6,1e6,"m"),"dy_m":number(0.,-1e6,1e6,"m")})
            }
            "select-by-location" | "extract-by-location" | "join-attributes-by-location" => {
                json!({"predicate":choice("intersects",&["intersects","within","contains","touches","disjoint"])})
            }
            "select-by-attribute" | "extract-by-attribute" => {
                json!({"field":text("id"),"operator":choice("equals",&["equals","not_equals","contains","greater_than","less_than"]),"value":text("example")})
            }
            "select-by-expression" | "extract-by-expression" => json!({"expression":text("id")}),
            "field-calculator" => {
                json!({"expression":text("id"),"output_field":text("calculated"),"output_type":choice("number",&["string","number","integer","boolean"])})
            }
            "random-selection" | "random-selection-within-subsets" | "random-points-on-lines" => {
                json!({"count":integer(10,1,100000),"seed":integer(0,0,1000000),"group_field":text("")})
            }
            "select-within-distance" => json!({"distance_m":distance(100.)}),
            "distance-matrix" | "join-by-nearest" => json!({"neighbours":integer(1,1,1000)}),
            "basic-statistics-for-fields" | "list-unique-values" | "drop-field" => {
                json!({"fields":fields()})
            }
            "split-vector-layer" => json!({"field":text("category")}),
            "reproject-layer" | "assign-projection" => json!({"crs":text("EPSG:4326")}),
            "join-attributes-by-value" => {
                json!({"source_field":text("id"),"against_field":text("id"),"fields":fields()})
            }
            "add-autoincremental-field" => {
                json!({"field":text("sequence"),"start":integer(1,0,1000000000)})
            }
            "add-field" => {
                json!({"field":text("new_field"),"field_type":choice("string",&["string","number","integer","boolean"])})
            }
            "refactor-fields" => {
                json!({"mapping":json!({"type":"array","items":object(json!({"source":text("id"),"name":text("identifier"),"type":choice("string",&["string","number","integer","boolean"])})),"default":[]})})
            }
            "rename-field" => json!({"field":text("id"),"new_name":text("identifier")}),
            "points-from-table" => {
                json!({"x_field":text("longitude"),"y_field":text("latitude"),"crs":text("EPSG:4326")})
            }
            "shortest-path-point-to-point"
            | "shortest-path-layer-to-point"
            | "service-area-from-point"
            | "service-area-from-layer" => {
                json!({"cost_field":text("length_m"),"budget":number(1000.,0.,1e12,"cost"),"origin":json!({"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2,"default":[30.,-2.]}),"destination":json!({"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2,"default":[30.001,-2.]})})
            }
            "split-lines-by-maximum-length" => json!({"max_length_m":distance(100.)}),
            "set-z-value" => json!({"z_m":number(0.,-12000.,100000.,"m")}),
            _ => json!({}),
        };
        object(p)
    }
    fn against(command: &str, category: &str) -> bool {
        category == "overlay"
            || matches!(
                command,
                "intersect"
                    | "select-by-location"
                    | "select-within-distance"
                    | "extract-by-location"
                    | "distance-matrix"
                    | "sum-line-lengths"
                    | "count-points-in-polygon"
                    | "overlap-analysis"
                    | "nearest-hub"
                    | "join-by-nearest"
                    | "join-attributes-by-value"
                    | "join-attributes-by-location"
                    | "hub-distance-to-points"
                    | "hub-lines"
                    | "snap-geometries-to-layer"
            )
    }
    fn source_schema() -> Value {
        json!({"type":"object","additionalProperties":false,"anyOf":[{"required":["layer_id"]},{"required":["file"]},{"required":["layer"]},{"required":["geojson"]}],"properties":{
            "layer_id":{"type":"string","description":"Reference to an admitted typed/IPC layer; hosts resolve it before running."},
            "file":{"type":"string","description":"Host-owned input file reference. Read GeoJSON/KML or Arrow IPC at the import boundary."},
            "name":{"type":"string","default":"Source"},
            "layer":{"type":"object","description":"Bound typed layer reference; features stay in the layer store."},
            "geojson":{"type":"object","description":"Inline GeoJSON import at the native file boundary only; WASM runners receive IPC, never this value."}
        }})
    }
    fn output_kind(command: &str) -> &'static str {
        match command {
            "collisions"
            | "distance-matrix"
            | "nearest-neighbour-analysis"
            | "basic-statistics-for-fields"
            | "list-unique-values"
            | "check-validity" => "report",
            "measure"
            | "sum-line-lengths"
            | "count-points-in-polygon"
            | "overlap-analysis"
            | "join-by-nearest"
            | "join-attributes-by-value"
            | "join-attributes-by-location"
            | "add-autoincremental-field"
            | "add-field"
            | "refactor-fields"
            | "rename-field"
            | "drop-field"
            | "field-calculator" => "appended_fields",
            _ => "new_layer",
        }
    }
    fn roadmap_report_schema(command: &str) -> Option<Value> {
        let columns = match command {
            "distance-matrix" => vec![
                field("source_id", "string", ""),
                field("target_id", "string", ""),
                field("distance_m", "number", "m"),
            ],
            "nearest-neighbour-analysis" => vec![
                field("count", "integer", "count"),
                field("observed_mean_distance_m", "number", "m"),
                field("expected_mean_distance_m", "number", "m"),
                field("nearest_neighbour_index", "number", "ratio"),
                field("z_score", "number", "z_score"),
            ],
            "basic-statistics-for-fields" => vec![
                field("field", "string", ""),
                field("count", "integer", "count"),
                field("missing", "integer", "count"),
                field("min", "number", "source_field"),
                field("max", "number", "source_field"),
                field("mean", "number", "source_field"),
                field("sum", "number", "source_field"),
                field("stddev", "number", "source_field"),
            ],
            "check-validity" => vec![
                field("source_id", "string", ""),
                field("valid", "boolean", ""),
                field("reason", "string", ""),
            ],
            "list-unique-values" => {
                return Some(
                    json!({"type":"object","properties":{"fields":{"type":"array","items":{"type":"object","properties":{"field":{"type":"string"},"values":{"type":"array","items":{"type":["string","number","boolean","null"]}}}}}}}),
                );
            }
            _ => return None,
        };
        let properties: serde_json::Map<String, Value> = columns
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap().into(),
                json!({"type":if f["nullable"] == true {json!([f["type"],"null"])}else{f["type"].clone()},"unit":f["unit"]}),
            )
        })
        .collect();
        Some(
            json!({"type":"object","x-fields":columns,"properties":{"rows":{"type":"array","items":{"type":"object","properties":properties}}}}),
        )
    }
    pub(super) fn descriptor(t: &Tool) -> Value {
        let params = parameters(&t.command);
        let mut input = object(
            json!({"source":source_schema(),"parameters":params,"output":object(json!({"limit":limit_schema(),"projection":choice("inline",&["inline","complete"]),"name":text("")}))}),
        );
        input["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
        input["required"] = json!(["source"]);
        let accepted_geometry = match t.command.as_str() {
            "sample" | "intersect" => json!(["LineString", "MultiLineString"]),
            _ => json!([
                "Point",
                "MultiPoint",
                "LineString",
                "MultiLineString",
                "Polygon",
                "MultiPolygon"
            ]),
        };
        input["properties"]["source"]["properties"]["geojson"]["x-value-kind"] = json!("layer");
        input["properties"]["source"]["properties"]["geojson"]["x-geometry-types"] =
            accepted_geometry.clone();
        if against(&t.command, &t.category) {
            input["properties"]["against"] = source_schema();
            input["properties"]["against"]["properties"]["geojson"]["x-value-kind"] =
                json!("layer");
            input["properties"]["against"]["properties"]["geojson"]["x-geometry-types"] =
                accepted_geometry.clone();
            input["required"] = json!(["source", "against"]);
        }
        for port in ["source", "against"] {
            if input["properties"].get(port).is_some() {
                input["properties"][port]["properties"]["layer"]["x-value-kind"] = json!("layer");
                input["properties"][port]["properties"]["layer"]["x-geometry-types"] =
                    accepted_geometry.clone();
            }
        }
        let summary = match t.command.as_str() {
            "buffer" => BUFFER_SUMMARY,
            "sample" => SAMPLE_SUMMARY,
            "intersect" => INTERSECT_SUMMARY,
            "measure" => MEASURE_SUMMARY,
            "outliers" => OUTLIERS_SUMMARY,
            "random-points-area" => RANDOM_SUMMARY,
            "collisions" => COLLISIONS_SUMMARY,
            _ => &t.description,
        };
        let mut example = json!({"source":{"name":"Example","geojson":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line-1","properties":{},"geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.001,-2.]]}}]}},"parameters":{},"output":{}});
        if against(&t.command, &t.category) {
            example["against"] = json!({"geojson":{"type":"LineString","coordinates":[[30.0005,-2.001],[30.0005,-1.999]]}});
        }
        if t.command == "collisions" {
            example["source"]["geojson"] = json!({"type":"FeatureCollection","features":[]});
        }
        normalize(&mut example, &input, "request").expect("descriptor example matches schema");
        let mut output = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","x-output-kind":output_kind(&t.command),"x-fields":output_fields(&t.command),"properties":{
            "tool_id":{"type":"string"},"dry_run":{"type":"boolean"},"source_features":{"type":"integer"},"produced":{"type":"integer"},"processed":{"type":"integer"},
            "fields_added":{"type":"array","items":{"type":"object","required":["name","type","unit"],"properties":{"name":{"type":"string"},"type":{"type":"string","enum":["string","number","integer","boolean"]},"unit":{"type":"string"}}}},
            "output_layer":{"type":"object","required":["name","geometry_type","fields"],"properties":{"name":{"type":"string"},"geometry_type":{"type":"string","enum":["Point","LineString","Polygon"]},"fields":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"type":{"type":"string","enum":["text","number","integer","boolean"]}}}}}},
            "result":{"type":["object","null"],"description":"Layer is delivered separately as typed Rust/Arrow IPC; metadata has no feature data."},
            "preview":{"type":"object","properties":{"counts":{"type":"object"},"fields_added":{"type":"array","items":{"type":"object"}},"sample":{"type":"array","maxItems":5},"warnings":{"type":"array","items":{"type":"string"}}}},
            "totals":{"type":"object"},"report":{"type":"object","description":"Tool-specific report; collision reports retain reporter metadata and distinguish absence, zero and held answers."}
        }});
        output["properties"]["output_layer"]["properties"]["render_mode"] = json!({"type":"string","enum":["geojson","vector_tiles"],"description":"Rust selects bounded GeoJSON rendering or the existing vector-tile file pipeline."});
        output["x-layer-geometry"] = match t.command.as_str() {
            "buffer" => json!(["Polygon", "MultiPolygon"]),
            "sample"
            | "intersect"
            | "random-points-area"
            | "centroids"
            | "point-on-surface"
            | "extract-vertices"
            | "mean-coordinates"
            | "regular-points"
            | "random-points-on-lines"
            | "points-from-table" => json!(["Point"]),
            "convex-hull"
            | "concave-hull"
            | "variable-distance-buffer"
            | "voronoi-polygons"
            | "delaunay-triangulation"
            | "create-grid" => json!(["Polygon", "MultiPolygon"]),
            "polygons-to-lines"
            | "points-to-path"
            | "merge-lines"
            | "explode-lines"
            | "hub-lines"
            | "shortest-path-point-to-point"
            | "shortest-path-layer-to-point"
            | "service-area-from-point"
            | "service-area-from-layer" => json!(["LineString", "MultiLineString"]),
            "lines-to-polygons" => json!(["Polygon", "MultiPolygon"]),
            "collisions" => json!([]),
            _ => json!("source"),
        };
        if matches!(
            t.command.as_str(),
            "buffer" | "sample" | "intersect" | "random-points-area"
        ) {
            output["properties"]["report"] = json!({"type":"null"});
        }
        if t.command == "measure" {
            output["properties"]["report"] = json!({"type":"object","properties":{
                "totals":{"type":"object","properties":{
                    "features":{"type":"integer"},"by_kind":{"type":"object","properties":{"point":{"type":"integer"},"line":{"type":"integer"},"polygon":{"type":"integer"}}},
                    "length_m":{"type":"number","unit":"m"},"area_m2":{"type":"number","unit":"m2"},"vertices":{"type":"integer"}}},
                "features":{"type":"array","items":{"type":"object","properties":{
                    "index":{"type":"integer"},"id":{"type":["string","null"]},"kind":{"type":"string","enum":["point","line","polygon"]},
                    "parts":{"type":"integer"},"holes":{"type":"integer"},"vertices":{"type":"integer"},
                    "length_m":{"type":"number","unit":"m"},"area_m2":{"type":"number","unit":"m2"}}}}
            }});
            output["properties"]["totals"] =
                output["properties"]["report"]["properties"]["totals"].clone();
        }
        if t.command == "outliers" {
            output["properties"]["report"] = json!({"type":"object","properties":{
                "layer_name":{"type":"string"},"source_layer":{"type":["string","null"]},"geometry_type":{"type":["string","null"]},
                "feature_count":{"type":"integer"},"analyzable_count":{"type":"integer"},"invalid_count":{"type":"integer"},"outlier_count":{"type":"integer"},"coordinate_units":{"type":"string"},
                "summary":{"type":"object","properties":{
                    "threshold":{"type":"number"},"min_features":{"type":"integer"},"spatial_isolation_count":{"type":"integer"},
                    "size_outlier_count":{"type":"integer"},"extent_outlier_count":{"type":"integer"},"insufficient_features":{"type":"boolean"},
                    "nearest_distance_median":{"type":"number","unit":"m"},"size_metric_median":{"type":"number","description":"Length in metres or area in square metres, according to geometry kind."},"extent_metric_median":{"type":"number","unit":"m"}}},
                "findings":{"type":"array","items":{"type":"object","properties":{
                    "feature_index":{"type":"integer"},"feature_id":{"type":"string"},"geometry_type":{"type":["string","null"]},
                    "score":{"type":"number"},"reasons":{"type":"array","items":{"type":"string"}},
                    "nearest_distance":{"type":["number","null"],"unit":"m"},"size_value":{"type":["number","null"]},
                    "extent_value":{"type":["number","null"],"unit":"m"},"detail":{"type":["string","null"]}}}}
            }});
        }

        if !matches!(
            t.command.as_str(),
            "buffer" | "sample" | "intersect" | "random-points-area" | "collisions"
        ) {
            output["x-inherited-properties"] = json!({"source":"source.layer.fields","description":"Source property names and types are retained unless a declared field replaces them."});
        }
        if let Some(report) = roadmap_report_schema(&t.command) {
            output["properties"]["report"] = report;
        }
        let dynamic_fields = match t.command.as_str() {
            "add-autoincremental-field" => {
                Some(json!({"name_from":"parameters.field","type":"integer"}))
            }
            "add-field" => {
                Some(json!({"name_from":"parameters.field","type_from":"parameters.field_type"}))
            }
            "field-calculator" => Some(
                json!({"name_from":"parameters.output_field","type_from":"parameters.output_type"}),
            ),
            "rename-field" => Some(
                json!({"name_from":"parameters.new_name","type_from":"source.properties[parameters.field]","replaces":"parameters.field"}),
            ),
            "refactor-fields" => Some(json!({"mapping_from":"parameters.mapping"})),
            "join-attributes-by-value" | "join-attributes-by-location" | "join-by-nearest" => {
                Some(json!({"fields_from":"against.layer.fields","prefix":"joined_"}))
            }
            "drop-field" => Some(json!({"remove_from":"parameters.fields"})),
            _ => None,
        };
        if let Some(fields) = dynamic_fields {
            output["x-dynamic-fields"] = fields;
        }
        let feature_properties: serde_json::Map<String, Value> = output_fields(&t.command)
        .into_iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap().to_owned(),
                json!({"type":if f["nullable"] == true {json!([f["type"],"null"])}else{f["type"].clone()},"unit":f["unit"]}),
            )
        })
        .collect();
        output["properties"]["result"] = json!({"type":"null","description":"Feature data is delivered separately as typed Rust/Arrow IPC."});
        output["x-data-plane"] = json!({"native":"ds_geo::layer::Layer","wasm":"Arrow IPC","id_column":ID,"geometry_column":GEOMETRY,"geometry_encoding":"WKB XY","fields":feature_properties,"samples":"preview.sample only"});
        output["properties"]["preview"]["properties"]["counts"] = json!({"type":"object","properties":{"source_features":{"type":"integer"},"processed":{"type":"integer","unit":"count"},"produced":{"type":"integer","unit":"count"}}});
        if t.command == "collisions" {
            let mut region_properties = serde_json::Map::new();
            for f in output_fields("collisions") {
                region_properties.insert(f["name"].as_str().unwrap().into(),json!({"type": if matches!(f["name"].as_str(),Some("coveragePct"|"overlapClass")) {json!([f["type"],"null"])} else {f["type"].clone()},"unit":f["unit"]}));
            }
            region_properties.insert(
                "transformers".into(),
                json!({"type":"array","items":{"type":"string"}}),
            );
            region_properties.insert(
            "feature_index".into(),
            json!({"type":"integer","description":"Row index into the typed input layer for rendering."}),
        );
            output["properties"]["report"] = json!({"type":"object","properties":{"phase":{"type":"string","enum":["idle","never","clear","found","held"]},"total":{"type":"integer"},"ranked":{"type":"boolean"},"more":{"type":"boolean"},"transformerCount":{"type":"integer"},"consolidatableCount":{"type":"integer"},"computedAtMs":{"type":["integer","null"]},"regions":{"type":"array","items":{"type":"object","properties":region_properties}},"phaseCopy":{"type":"object"}}});
        }
        if t.command == "random-points-area" {
            output["x-optional-fields"] = json!([field("elevation_m", "number", "m")]);
        }

        json!({"id":t.id,"version":DESCRIPTOR_VERSION,"name":t.name,"category":t.category,"category_name":t.category_name,"command":t.command,"summary":summary,"description":t.description,"status":if available(&t.command){"available"}else{"roadmap"},"input_schema":input,"output_schema":output,"examples":[{"summary":"Portable request; copy directly to --request.","request":example}],"refusals":[
            {"code":"vector_tool_unknown","when":"No descriptor has this id or command.","remedy":"List ds data vector describe."},
            {"code":"vector_tool_roadmap","when":"The shape is agreed but no runner is shipped.","remedy":"Inspect the schema; choose an available tool."},
            {"code":"vector_request_invalid","when":"The JSON does not match the schema or parameter relationships.","remedy":"Use the descriptor's types, enums, ranges and required fields."},
            {"code":"vector_document_malformed","when":"GeoJSON, KML or Arrow IPC is malformed or unsupported.","remedy":"Import a supported layer file; inspect its format and geometry."},
            {"code":"vector_document_empty","when":"The source has no features.","remedy":"Supply a nonempty source."},
            {"code":"vector_no_eligible_feature","when":"No geometry is eligible.","remedy":"Use measure to inspect geometry types."},
            {"code":"vector_distance_out_of_range","when":"An engine distance is out of range.","remedy":"Use the schema's distance range."},
            {"code":"vector_limit_out_of_range","when":"A limit is out of range.","remedy":"Use 1 through 20000."},
            {"code":"vector_external_data_required","when":"Requested terrain readings are missing or do not match returned points.","remedy":"Acquire elevations and supply elevations_m in point order, using null for missing readings."},
            {"code":"vector_data_bound_exceeded","when":"Input or projected output exceeds the 32 MiB/20000-feature bound.","remedy":"Split the source into bounded chunks or increase spacing."},
            {"code":"vector_engine_empty","when":"The buffer engine produced nothing.","remedy":"Check WGS84 coordinates and radius."}
        ]})
    }
}
/// Full per-tool disclosure; the unfiltered registry deliberately includes roadmap.
pub fn describe(id: Option<&str>) -> Result<Value, Refusal> {
    match id {
        Some(id) => Ok(generation::descriptor(tool(id)?)),
        None => Ok(Value::Array(
            catalogue()
                .iter()
                .chain(std::iter::once(collision_tool()))
                .map(generation::descriptor)
                .collect(),
        )),
    }
}
pub(super) fn normalize(v: &mut Value, s: &Value, path: &str) -> Result<(), Refusal> {
    let invalid = || {
        refuse(
            match s["x-refusal"].as_str() {
                Some("vector_distance_out_of_range") => "vector_distance_out_of_range",
                Some("vector_limit_out_of_range") => "vector_limit_out_of_range",
                _ => "vector_request_invalid",
            },
            format!("{path} does not match its schema."),
        )
    };
    if let Some(types) = s["type"].as_array() {
        let valid = types
            .iter()
            .filter_map(Value::as_str)
            .any(|kind| match kind {
                "null" => v.is_null(),
                "number" => v.is_number(),
                "boolean" => v.is_boolean(),
                "string" => v.is_string(),
                _ => false,
            });
        if !valid {
            return Err(invalid());
        }
    }
    match s["type"].as_str() {
        Some("object") => {
            let obj = v.as_object_mut().ok_or_else(invalid)?;
            if let Some(required) = s["required"].as_array() {
                for key in required {
                    if !obj.contains_key(key.as_str().unwrap()) {
                        return Err(refuse(
                            "vector_request_invalid",
                            format!("{path}.{} is required.", key.as_str().unwrap()),
                        ));
                    }
                }
            }
            if let Some(props) = s["properties"].as_object() {
                if s["additionalProperties"] == false && obj.keys().any(|k| !props.contains_key(k))
                {
                    return Err(invalid());
                }
                for (key, schema) in props {
                    if !obj.contains_key(key) {
                        if let Some(default) = schema.get("default") {
                            obj.insert(key.clone(), default.clone());
                        }
                    }
                    if let Some(value) = obj.get_mut(key) {
                        normalize(value, schema, &format!("{path}.{key}"))?;
                    }
                }
            }
        }
        Some("number" | "integer") => {
            let n = v.as_f64().ok_or_else(invalid)?;
            if !n.is_finite()
                || (s["type"] == "integer" && n.fract() != 0.)
                || s["minimum"].as_f64().is_some_and(|min| n < min)
                || s["maximum"].as_f64().is_some_and(|max| n > max)
            {
                return Err(invalid());
            }
            if s["type"] == "integer" {
                *v = json!(n as i64);
            }
        }
        Some("boolean") if !v.is_boolean() => return Err(invalid()),
        Some("string") if !v.is_string() => return Err(invalid()),
        Some("array") => {
            let a = v.as_array_mut().ok_or_else(invalid)?;
            if s["minItems"]
                .as_u64()
                .is_some_and(|n| a.len() < (n as usize))
                || s["maxItems"]
                    .as_u64()
                    .is_some_and(|n| a.len() > (n as usize))
            {
                return Err(invalid());
            }
            for (i, value) in a.iter_mut().enumerate() {
                normalize(value, &s["items"], &format!("{path}[{i}]"))?;
            }
        }
        _ => {}
    }
    if s["enum"]
        .as_array()
        .is_some_and(|values| !values.contains(v))
    {
        return Err(invalid());
    }
    Ok(())
}
