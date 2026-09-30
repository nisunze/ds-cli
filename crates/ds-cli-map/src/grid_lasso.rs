//! A transient Desktop selection lens. Only bounded input admission lives
//! here; the held native scene owns geometry predicates, filters and identity.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_geo::table::{ColumnFilterInput, FilterOp, FilterSortInput, filter_sort_indices};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const FAMILIES: &[&str] = &[
    "structures",
    "terrain_points",
    "alignments",
    "tension_sections",
    "attachment_points",
];
const MAX_JSON_BYTES: usize = 64 * 1024;
const MAX_POINTS: usize = 256;

const INVALID: Refusal = Refusal {
    code: "invalid_grid_lasso",
    when: "an ID, coordinate space, polygon, predicate, family, filter or selection mode is invalid",
    remedy: "use the polygon/filter schema in --help and exact held model/revision IDs",
};
const STALE: Refusal = Refusal {
    code: "grid_selection_stale",
    when: "the model, revision or held scene changed during selection",
    remedy: "read the held scene (map profile view for Profile), then retry its exact pins",
};
const CLOSED: Refusal = Refusal {
    code: "grid_scene_closed",
    when: "the window has no held scene for the requested space",
    remedy: "open the exact model and Plan/Profile surface in the paired Desktop",
};

pub static COMMAND: Command = Command {
    id: "map.grid.lasso",
    path: &["map", "grid", "lasso"],
    contract: 1,
    summary: "Select Grid Plan or Profile elements by lasso and attribute filters.",
    purpose: "Changes transient selection only. The window fences model/revision and supplies its axis pin to the native spatial AND attribute query. CLI validates inputs before pairing; native topology/column/scene checks remain authoritative. No model effect, inferred pins/project or server route. Requires Desktop map.grid.lasso support.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<model-id>", "Exact open model ID; nonblank <=200 bytes, no surrounding whitespace/controls.").required(),
        Arg::value("revision", "<revision-id>", "Exact held revision ID; --model's bounds apply.").required(),
        Arg::value("space", "<profile|plan>", "Coordinate space of the held scene.").choices(&["profile", "plan"]).required(),
        Arg::value("polygon", "<json>", "<=64 KiB, finite exact pairs. Profile: 3..256 [scene_x,scene_y] engineering pairs (not pixels), open/closed once, abs <=1e9; or GeoJSON in those units. Plan: WGS84 GeoJSON, lon [-180,180], lat [-90,90]. GeoJSON: only {type:Polygon,coordinates:[ring]}, closed 4..256 pairs; no holes/extra keys/ordinates.").required(),
        Arg::value("predicate", "<intersects|within>", "Boundary-inclusive; within requires the whole entity.").choices(&["intersects", "within"]).default("intersects"),
        Arg::repeated("family", "<family>", "Repeat distinct scene families; omitted = all five. terrain_points = native ground points.").choices(FAMILIES),
        Arg::value("filter", "<json>", "Closed ProfileTableFilterQuery <=64 KiB: {filters?:[{column,op,value?,value2?}],stats_columns?:[]}. Lists default empty; <=64 AND filters, <=8 distinct stats names. Names: nonblank <=200 bytes, no surrounding whitespace/controls. Values: strings <=4096 bytes; value2 may be null. op: contains|equals|not_equals|starts_with|ends_with|gt|gte|lt|lte|between|is_empty|not_empty. Unknown/duplicate keys refused."),
        Arg::value("mode", "<replace|add|remove|intersect>", "Combine native hits with the current selection.").choices(&["replace", "add", "remove", "intersect"]).default("replace"),
        crate::DESCRIPTOR_ARG,
    ],
    output: "Exact window receipt: held model/revision, native identities and primary focus. Empty hits succeed; CLI computes nothing.",
    examples: &[
        Example {
            command: "ds map grid lasso --model <id> --revision <rev> --space profile --polygon '[[0,0],[100,0],[100,100]]' --family structures --output json",
            note: "Use the held engineering axis.",
            runnable: false,
        },
    ],
    refusals: &[
        INVALID, STALE, CLOSED, crate::NOT_PAIRED, crate::AMBIGUOUS,
        crate::UNREACHABLE, crate::PAIRING_REJECTED, crate::UNSUPPORTED,
        crate::UNREADABLE, crate::REFUSED,
    ],
    reference: Some("docs/reference/map.md"),
    search: &["selection", "polygon", "attributes", "structures", "terrain"],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

#[derive(Serialize)]
struct GridLassoRequest<'a> {
    model_id: &'a str,
    expected_revision: &'a str,
    space: &'a str,
    polygon: Value,
    predicate: &'a str,
    families: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<Value>,
    mode: &'a str,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PolygonInput {
    Pairs(Vec<[f64; 2]>),
    GeoJson(GeoJsonPolygon),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeoJsonPolygon {
    #[serde(rename = "type")]
    kind: String,
    coordinates: Vec<Vec<[f64; 2]>>,
}

// The wasm ProfileTableFilterQuery is private. Its closed control envelope is
// admitted here, with the operator enum and ordered-bound validation delegated
// to the same ds-geo kernel. No rows, candidates or hits are computed here.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileTableFilterQuery {
    #[serde(default)]
    filters: Vec<ClosedColumnFilter>,
    #[serde(default)]
    stats_columns: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClosedColumnFilter {
    column: String,
    op: FilterOp,
    #[serde(default)]
    value: String,
    #[serde(default)]
    value2: Option<String>,
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = request(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(&descriptor, &crate::GRID_LASSO, request, crate::UI_TIMEOUT)
}

fn request(inputs: &Inputs) -> Result<Value, Failure> {
    let model_id = id(inputs.require("model")?, "model")?;
    let expected_revision = id(inputs.require("revision")?, "revision")?;
    let space = choice(inputs.require("space")?, &["profile", "plan"], "space")?;
    let predicate = choice(
        inputs.value("predicate").unwrap_or("intersects"),
        &["intersects", "within"],
        "predicate",
    )?;
    let mode = choice(
        inputs.value("mode").unwrap_or("replace"),
        &["replace", "add", "remove", "intersect"],
        "mode",
    )?;
    let mut families = Vec::new();
    for family in inputs.repeated("family") {
        let family = choice(family, FAMILIES, "family")?;
        if families.contains(&family) {
            return Err(invalid("repeat only distinct --family values"));
        }
        families.push(family);
    }
    if families.is_empty() {
        families.extend_from_slice(FAMILIES);
    }
    let polygon = polygon(inputs.require("polygon")?, space)?;
    let filter = inputs.value("filter").map(filter).transpose()?;
    serde_json::to_value(GridLassoRequest {
        model_id,
        expected_revision,
        space,
        polygon,
        predicate,
        families,
        filter,
        mode,
    })
    .map_err(|_| invalid("request cannot be represented as finite JSON"))
}

fn polygon(raw: &str, space: &str) -> Result<Value, Failure> {
    bounded_json(raw, "polygon")?;
    let parsed: PolygonInput = serde_json::from_str(raw).map_err(|_| {
        invalid(
            "polygon must contain exact numeric pairs or a closed type/coordinates GeoJSON Polygon",
        )
    })?;
    let (points, geojson) = match parsed {
        PolygonInput::Pairs(points) if space == "profile" => (points, false),
        PolygonInput::Pairs(_) => return Err(invalid("Plan requires WGS84 GeoJSON Polygon")),
        PolygonInput::GeoJson(mut polygon) => {
            if polygon.kind != "Polygon" || polygon.coordinates.len() != 1 {
                return Err(invalid("GeoJSON must be Polygon with exactly one ring"));
            }
            (polygon.coordinates.remove(0), true)
        }
    };
    if !(3..=MAX_POINTS).contains(&points.len()) {
        return Err(invalid(
            "polygon needs 3..256 pairs (including any closing pair)",
        ));
    }
    let closed = points.first() == points.last();
    if geojson && (!closed || points.len() < 4) {
        return Err(invalid(
            "GeoJSON ring must be explicitly closed with at least four pairs",
        ));
    }
    let vertices = &points[..points.len() - usize::from(closed)];
    if vertices.len() < 3
        || vertices
            .iter()
            .enumerate()
            .any(|(i, point)| *point == vertices[(i + 1) % vertices.len()])
    {
        return Err(invalid(
            "polygon needs at least three distinct adjacent vertices",
        ));
    }
    for [x, y] in &points {
        if !x.is_finite() || !y.is_finite() {
            return Err(invalid("polygon coordinates must be finite"));
        }
        if space == "profile" {
            if x.abs() > 1e9 || y.abs() > 1e9 {
                return Err(invalid("Profile coordinates must have abs <=1e9"));
            }
        } else if !(-180.0..=180.0).contains(x) || !(-90.0..=90.0).contains(y) {
            return Err(invalid(
                "Plan requires WGS84 longitude [-180,180] and latitude [-90,90]",
            ));
        }
    }
    // Normalize only representation: never project coordinates or mint a pin.
    if space == "profile" {
        Ok(json!(points))
    } else {
        let geometry = json!({"type":"Polygon", "coordinates":[points]});
        ds_geo::admission::admit_geometry(&geometry)
            .map_err(|error| invalid(format!("invalid native Plan polygon: {error}")))?;
        Ok(geometry)
    }
}

fn filter(raw: &str) -> Result<Value, Failure> {
    bounded_json(raw, "filter")?;
    let query: ProfileTableFilterQuery = serde_json::from_str(raw).map_err(|error| {
        invalid(format!(
            "filter must be a closed native ProfileTableFilterQuery: {error}"
        ))
    })?;
    if query.filters.len() > 64 || query.stats_columns.len() > 8 {
        return Err(invalid(
            "filter is bounded to 64 predicates and 8 stats_columns",
        ));
    }
    for (index, name) in query.stats_columns.iter().enumerate() {
        id(name, "stats_columns")?;
        if query.stats_columns[..index].contains(name) {
            return Err(invalid("stats_columns must be distinct"));
        }
    }
    let mut filters = Vec::new();
    for filter in query.filters {
        id(&filter.column, "filter column")?;
        if filter.value.len() > 4096
            || filter
                .value2
                .as_ref()
                .is_some_and(|value| value.len() > 4096)
        {
            return Err(invalid("filter values must be <=4096 bytes"));
        }
        filters.push(ColumnFilterInput {
            column: filter.column,
            op: filter.op,
            value: filter.value,
            value2: filter.value2,
        });
    }
    filter_sort_indices(FilterSortInput {
        rows: vec![],
        filters,
        sort: None,
    })
    .map_err(|error| invalid(format!("invalid native filter: {error}")))?;
    // Preserve the caller's admitted query, including omitted optional fields.
    serde_json::from_str(raw).map_err(|_| invalid("filter must be JSON"))
}

fn bounded_json(raw: &str, field: &str) -> Result<(), Failure> {
    if raw.len() > MAX_JSON_BYTES {
        return Err(invalid(format!("{field} JSON must be <=64 KiB")));
    }
    Ok(())
}

fn id<'a>(raw: &'a str, field: &str) -> Result<&'a str, Failure> {
    if raw.is_empty() || raw.trim() != raw || raw.len() > 200 || raw.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{field} must be a nonblank exact ID/name <=200 bytes without surrounding whitespace or control characters"
        )));
    }
    Ok(raw)
}

fn choice<'a>(raw: &'a str, choices: &[&str], field: &str) -> Result<&'a str, Failure> {
    if !choices.contains(&raw) {
        return Err(invalid(format!(
            "invalid {field}; use {}",
            choices.join(", ")
        )));
    }
    Ok(raw)
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("invalid_grid_lasso", message.into())
}

pub fn render(data: &Value) -> String {
    format!("grid selection {data}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(space: &str, polygon: &str, extra: &[&str]) -> Result<Inputs, Failure> {
        let mut args = vec![
            "--model",
            "model-a",
            "--revision",
            "rev-b",
            "--space",
            space,
            "--polygon",
            polygon,
        ];
        args.extend_from_slice(extra);
        ds_cli_contract::args::parse(
            &COMMAND,
            &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn profile_payload_has_exact_pins_families_filter_and_set_mode() {
        let polygon = "[[0,0],[100,0],[100,100],[0,100]]";
        let filter = r#"{"filters":[{"column":"number","op":"gte","value":"10"}],"stats_columns":["number"]}"#;
        let inputs = inputs(
            "profile",
            polygon,
            &[
                "--family",
                "structures",
                "--family",
                "terrain_points",
                "--filter",
                filter,
                "--predicate",
                "within",
                "--mode",
                "intersect",
            ],
        )
        .unwrap();
        let payload = request(&inputs).unwrap();
        assert_eq!(
            payload,
            json!({
                "model_id":"model-a", "expected_revision":"rev-b", "space":"profile",
                "polygon":[[0.0,0.0],[100.0,0.0],[100.0,100.0],[0.0,100.0]],
                "predicate":"within", "families":["structures","terrain_points"],
                "filter":serde_json::from_str::<Value>(filter).unwrap(), "mode":"intersect"
            })
        );
        assert_eq!(crate::GRID_LASSO.operation, "map.grid.lasso");
        assert_eq!(
            ds_cli_desktop::ops::undeclared_key(&crate::GRID_LASSO, &payload),
            None
        );
        let keys = payload
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(keys, crate::GRID_LASSO.arguments.iter().copied().collect());
    }

    #[test]
    fn plan_payload_preserves_wgs84_and_explicit_defaults() {
        let polygon =
            r#"{"type":"Polygon","coordinates":[[[30,-2],[30.1,-2],[30.1,-1.9],[30,-2]]]}"#;
        let payload = request(&inputs("plan", polygon, &[]).unwrap()).unwrap();
        assert_eq!(
            payload,
            json!({
                "model_id":"model-a", "expected_revision":"rev-b", "space":"plan",
                "polygon":{"type":"Polygon","coordinates":[[[30.0,-2.0],[30.1,-2.0],[30.1,-1.9],[30.0,-2.0]]]},
                "predicate":"intersects", "families":FAMILIES, "mode":"replace"
            })
        );
        assert_eq!(
            ds_cli_desktop::ops::undeclared_key(&crate::GRID_LASSO, &payload),
            None
        );
    }

    #[test]
    fn profile_geojson_is_only_representation_normalization() {
        let polygon =
            r#"{"type":"Polygon","coordinates":[[[1000,500],[1200,500],[1200,600],[1000,500]]]}"#;
        assert_eq!(
            request(&inputs("profile", polygon, &[]).unwrap()).unwrap()["polygon"],
            json!([
                [1000.0, 500.0],
                [1200.0, 500.0],
                [1200.0, 600.0],
                [1000.0, 500.0]
            ])
        );
    }

    #[test]
    fn malformed_polygons_are_rejected_before_pairing() {
        for polygon in [
            "null",
            "{}",
            "[]",
            "[[0,0],[1,1]]",
            "[[0,0],[1,1],[0,0]]",
            "[[0,0],[0,0],[1,1]]",
            "[[0,0,0],[1,0],[1,1]]",
            "[[0,0],[1,0],[1,\"1\"]]",
            "[[0,0],[1,0],[1,1e309]]",
            "[[0,0],[1,0],[1,1000000001]]",
            r#"{"type":"Feature","geometry":{"type":"Polygon","coordinates":[]}}"#,
            r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1]]]}"#,
            r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]],"crs":"scene"}"#,
            r#"{"type":"Polygon","type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}"#,
            r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]],[[0,0],[1,0],[1,1],[0,0]]]}"#,
        ] {
            assert_eq!(
                request(&inputs("profile", polygon, &[]).unwrap())
                    .unwrap_err()
                    .code(),
                "invalid_grid_lasso",
                "{polygon}"
            );
        }
        let too_many = serde_json::to_string(&vec![[0.0, 0.0]; MAX_POINTS + 1]).unwrap();
        assert!(polygon(&too_many, "profile").is_err());
        assert!(polygon(&" ".repeat(MAX_JSON_BYTES + 1), "profile").is_err());
        for polygon in [
            "[[0,0],[1,0],[1,1]]",
            r#"{"type":"Polygon","coordinates":[[[181,0],[1,0],[1,1],[181,0]]]}"#,
            r#"{"type":"Polygon","coordinates":[[[0,91],[1,0],[1,1],[0,91]]]}"#,
            r#"{"type":"Polygon","coordinates":[[[0,0],[1,1],[0,1],[1,0],[0,0]]]}"#,
            r#"{"type":"Polygon","coordinates":[[[0,0],[1,0],[2,0],[0,0]]]}"#,
        ] {
            assert!(super::polygon(polygon, "plan").is_err(), "{polygon}");
        }
    }

    #[test]
    fn closed_native_filters_are_validated_without_rows() {
        for raw in [
            "{}",
            r#"{"filters":[],"stats_columns":[]}"#,
            r#"{"filters":[{"column":"name","op":"contains","value":"Pole"}]}"#,
            r#"{"filters":[{"column":"date","op":"between","value":"2026-01-01","value2":"2026-09-30"}]}"#,
        ] {
            assert_eq!(
                filter(raw).unwrap(),
                serde_json::from_str::<Value>(raw).unwrap()
            );
        }
        for raw in [
            "null",
            r#"{"rows":[]}"#,
            r#"{"sort":null}"#,
            r#"{"or":[]}"#,
            r#"{"filters":[],"filters":[]}"#,
            r#"{"filters":null}"#,
            r#"{"filters":[{"column":"x","op":"sql","value":"1"}]}"#,
            r#"{"filters":[{"column":"x","op":"equals","value":1}]}"#,
            r#"{"filters":[{"column":"x","op":"equals","extra":true}]}"#,
            r#"{"filters":[{"column":"x","op":"equals","column":"y"}]}"#,
            r#"{"filters":[{"column":" ","op":"equals"}]}"#,
            r#"{"filters":[{"column":"x","op":"gt","value":"NaN"}]}"#,
            r#"{"filters":[{"column":"x","op":"between","value":"1","value2":"tomorrow"}]}"#,
            r#"{"stats_columns":["x","x"]}"#,
        ] {
            assert_eq!(
                filter(raw).unwrap_err().code(),
                "invalid_grid_lasso",
                "{raw}"
            );
        }
        assert!(filter(&" ".repeat(MAX_JSON_BYTES + 1)).is_err());
        assert!(
            filter(&json!({"filters":vec![json!({"column":"x","op":"equals"});65]}).to_string())
                .is_err()
        );
        assert!(filter(&json!({"stats_columns":vec!["x";9]}).to_string()).is_err());
        assert!(
            filter(
                &json!({"filters":[{"column":"x","op":"equals","value":"x".repeat(4097)}]})
                    .to_string()
            )
            .is_err()
        );
    }

    #[test]
    fn choices_and_exact_ids_refuse_before_pairing() {
        let polygon = "[[0,0],[1,0],[1,1]]";
        for extra in [
            ["--mode", "toggle"],
            ["--predicate", "touches"],
            ["--family", "ground_points"],
        ] {
            assert_eq!(
                inputs("profile", polygon, &extra).unwrap_err().code(),
                "invalid_choice"
            );
        }
        assert!(inputs("pixels", polygon, &[]).is_err());
        for name in ["", " ", " model", "model ", "model\n", &"x".repeat(201)] {
            assert!(id(name, "model").is_err());
        }
        for mode in ["replace", "add", "remove", "intersect"] {
            assert_eq!(
                request(&inputs("profile", polygon, &["--mode", mode]).unwrap()).unwrap()["mode"],
                mode
            );
        }
        assert!(
            request(
                &inputs(
                    "profile",
                    polygon,
                    &["--family", "structures", "--family", "structures"]
                )
                .unwrap()
            )
            .is_err()
        );
    }
}
