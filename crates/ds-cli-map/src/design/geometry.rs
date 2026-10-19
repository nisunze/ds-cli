//! `ds map design geometry` — stage a geometry replacement on ONE feature.
//!
//! Drafting means drawing: moving a mis-placed pole, re-routing a line
//! segment, straightening a service cable. The contract is deliberately one
//! feature per call — a geometry write that could fan out over a selector is
//! how a whole layer gets dragged to one coordinate. The application enforces
//! the same exactly-one rule on its side.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::design::TRANSFORMER_ARG;
use crate::{DESCRIPTOR_ARG, TARGET_ARG};

const GEOMETRY_ID_ARG: Arg = Arg {
    name: "id",
    kind: ArgKind::Value,
    value: "<feature-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The one feature whose geometry to replace: its id, or `<layer>#<id>` when the id repeats across layers.",
};

const GEOMETRY_LAYER_ARG: Arg = Arg::value(
    "layer",
    "<layer>",
    "Design layer the --id belongs to (ids repeat across layers); same as `--id <layer>#<id>`.",
);

const GEOMETRY_ARG: Arg = Arg {
    name: "geometry",
    kind: ArgKind::Value,
    value: "<geojson>",
    required: true,
    default: None,
    choices: &[],
    summary: "GeoJSON geometry object: Point, LineString, Polygon, or their Multi forms.",
};

pub static COMMAND: Command = Command {
    id: "map.design.geometry",
    path: &["map", "design", "geometry"],
    contract: 1,
    summary: "Stage a geometry replacement on one design feature.",
    purpose: "\
Replaces the geometry of exactly one design feature — move a pole, re-route a \
line — in the transformer's local room and marks it dirty; the project is \
untouched until `ds map design save`. One feature per call, always: an id \
that matches more than one feature is refused, never guessed. Ids repeat \
across layers (a pole and a line can both be 41): qualify one with --layer or \
as `<layer>#<id>`. Coordinates are [lon, lat] degrees.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        GEOMETRY_ID_ARG,
        GEOMETRY_LAYER_ARG,
        GEOMETRY_ARG,
        Arg::switch(
            "dry-run",
            "Validate the addressing and geometry; stage nothing.",
        ),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
The layer and id addressed, the geometry type written, and `staged` and \
`persisted` separately — `persisted` is false here always.",
    examples: &[
        Example {
            command: r#"ds map design geometry --transformer T-1042 --id lv_poles#41 --geometry '{"type":"Point","coordinates":[30.06,-1.95]}'"#,
            note: "Move pole 41 of lv_poles. Stage only; save is a separate confirmed push.",
            runnable: false,
        },
        Example {
            command: r#"ds map design geometry --transformer T-1042 --layer lv_lines --id 41 --geometry '{"type":"LineString","coordinates":[[30.06,-1.95],[30.07,-1.95]]}'"#,
            note: "Re-route line 41 of lv_lines when a pole also carries id 41.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::PROJECT_NOT_OPEN,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        Refusal {
            code: "invalid_geometry",
            when: "the --geometry value is not a bounded GeoJSON geometry object",
            remedy: "pass a JSON object with type and [lon, lat] coordinates in degrees",
        },
        Refusal {
            code: "ambiguous_feature",
            when: "the id matched zero features, or more than one",
            remedy: AMBIGUOUS_FEATURE_REMEDY,
        },
    ],
    reference: Some("docs/reference/map.md"),
    search: &[],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub(super) const AMBIGUOUS_FEATURE_REMEDY: &str = "read ids from `ds map design select`; qualify an id that repeats across layers with --layer or `<layer>#<id>`; a geometry write addresses exactly one";

/// The closed `design.features.set_geometry` arguments. The id travels
/// verbatim — the window resolves `<layer>#<id>` against its own room layers,
/// so an id that itself contains `#` is never split blindly here.
fn bridge_arguments(
    transformer: &str,
    id: &str,
    layer: Option<&str>,
    geometry: Value,
    dry_run: bool,
) -> Value {
    let mut arguments = Map::new();
    arguments.insert("transformer".into(), json!(transformer));
    if let Some(layer) = layer {
        arguments.insert("layers".into(), json!([layer]));
    }
    arguments.insert("ids".into(), json!([id]));
    arguments.insert("geometry".into(), geometry);
    arguments.insert("dryRun".into(), json!(dry_run));
    Value::Object(arguments)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let id = inputs.require("id")?;
    let raw_geometry = inputs.require("geometry")?;
    let dry_run = inputs.switch("dry-run");

    let geometry: Value = serde_json::from_str(raw_geometry).map_err(|error| {
        Failure::invalid(
            "invalid_geometry",
            format!("--geometry is not JSON: {error}"),
        )
        .remedy("pass a GeoJSON geometry object, quoted for your shell")
    })?;
    if !geometry.is_object() {
        return Err(Failure::invalid(
            "invalid_geometry",
            "--geometry must be a GeoJSON geometry OBJECT",
        )
        .remedy(r#"e.g. '{"type":"Point","coordinates":[30.06,-1.95]}'"#));
    }

    let arguments = bridge_arguments(transformer, id, inputs.value("layer"), geometry, dry_run);

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_GEOMETRY,
        arguments,
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(|failure| super::classify_geometry_failure(crate::classify_design_failure(failure)))?;

    Ok(json!({
        "transformer": transformer,
        "project": result["project"],
        "layer": result["layer"],
        "id": result["id"],
        "geometry_type": result["geometryType"],
        "dry_run": result["dryRun"].as_bool().unwrap_or(dry_run),
        "staged": result["staged"].as_bool().unwrap_or(false),
        "persisted": result["persisted"].as_bool().unwrap_or(false),
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} · {}  ←  {}\n",
        data["layer"].as_str().unwrap_or("?"),
        data["id"].as_str().unwrap_or("?"),
        data["geometry_type"].as_str().unwrap_or("geometry"),
    );
    if data["dry_run"].as_bool().unwrap_or(false) {
        out.push_str("\ndry run; nothing was staged\n");
        return out;
    }
    out.push('\n');
    out.push_str(super::staging_note(data));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// e9c31772: the documented `--id lv_poles#41` reaches the window
    /// verbatim (it resolves the layer against its room), and `--layer`
    /// travels as the closed `layers` argument the window admits.
    #[test]
    fn a_layer_qualified_address_crosses_the_bridge_inside_the_closed_contract() {
        let point = json!({"type":"Point","coordinates":[30.06,-1.95]});
        let qualified = bridge_arguments("T-1042", "lv_poles#41", None, point.clone(), true);
        assert_eq!(qualified["ids"], json!(["lv_poles#41"]));
        assert!(qualified.get("layers").is_none());
        let layered = bridge_arguments("T-1042", "41", Some("lv_lines"), point, false);
        assert_eq!(layered["layers"], json!(["lv_lines"]));
        assert_eq!(layered["ids"], json!(["41"]));
        for arguments in [&qualified, &layered] {
            for key in arguments.as_object().unwrap().keys() {
                assert!(
                    crate::DESIGN_GEOMETRY.arguments.contains(&key.as_str()),
                    "{key} is outside design.features.set_geometry"
                );
            }
        }
        assert!(COMMAND.args.iter().any(|arg| arg.name == "layer"));
        assert!(
            COMMAND
                .examples
                .iter()
                .any(|example| example.command.contains("--id lv_poles#41"))
        );
    }

    #[test]
    fn an_ambiguous_window_refusal_names_the_qualified_way_out() {
        let failure = super::super::classify_geometry_failure(
            Failure::failed("desktop_refused", "refused").detail(json!({
                "detail": "ids must address exactly one feature; matched 2."
            })),
        );
        assert_eq!(failure.code(), "ambiguous_feature");
        assert!(failure.remedy_text().unwrap().contains("--layer"));
    }
}
