//! `ds map view` — what the paired map is showing, and what is on it.
//!
//! This is the command every other one in the domain starts from. It answers
//! where the map is looking, and it names the layers: the `layer` id that
//! `map remove` takes, and the `analysis_id` that the vector tools take.
//! Reporting both is what keeps a caller from composing an identifier itself.
//!
//! It reads the session view the application already publishes, and projects
//! the map out of it. The session also carries capabilities and folder
//! grants; none of that is here, because a caller asking what is on the map
//! should not pay for the application's whole self-description.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::bridge;
use serde_json::{Value, json};

use crate::{ANALYSIS_SKETCH_PREFIX, DESCRIPTOR_ARG};

/// The cheapest useful answer. A working map carries a handful of temporary
/// layers; the application publishes up to two hundred, and printing all of
/// them by default would make the most-called command in the domain the most
/// expensive one.
const DEFAULT_LIMIT: &str = "20";

pub static COMMAND: Command = Command {
    id: "map.view",
    path: &["map", "view"],
    contract: 1,
    summary: "Read the paired map: view state and its local layers.",
    purpose: "\
Reports whether the map is open, where it is looking — centre, zoom and the \
visible bounding box — and the local layers on it. Each layer is reported with \
both identifiers a caller needs: `layer`, which `ds map remove` takes, and \
`analysis_id`, which the vector tools take. Start here: every other command in \
this domain acts on something this one names.",
    chapter: Chapter::Survey,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("limit", "<n>", "Report at most this many layers; 1..200.")
            .default(DEFAULT_LIMIT),
        DESCRIPTOR_ARG,
    ],
    output: "\
`open`, and when open the centre, zoom and bounding box. Then `layer_count` \
and the layers, each with its name, geometry type, feature count, visibility, \
source, whether this session created it, and both identifiers. `more.omitted` \
when --limit cut the list.",
    examples: &[
        Example {
            command: "ds map view",
            note: "",
            runnable: true,
        },
        Example {
            command: "ds map view --output json",
            note: "Read .data.layers[].analysis_id to feed a vector tool.",
            runnable: true,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::REFUSED,
        crate::UNREADABLE,
        Refusal {
            code: "desktop_contract_mismatch",
            when: "the session reply does not match this build's contract",
            remedy: "update DS GridDesign and `ds` to matching releases",
        },
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::integer(inputs.require("limit")?, "limit", 1, 200)? as usize;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let session = bridge::session(&descriptor)?;

    let map = &session["map"];
    let empty = Vec::new();
    let layers = map[crate::SNAPSHOT_LAYERS].as_array().unwrap_or(&empty);
    let shown: Vec<Value> = layers.iter().take(limit).map(project).collect();
    let omitted = layers.len().saturating_sub(shown.len());

    let open = map[crate::SNAPSHOT_OPEN].as_bool().unwrap_or(false);
    let mut data = json!({
        "open": open,
        "layer_count": layers.len(),
        "layers": shown,
    });
    if open {
        for field in crate::SNAPSHOT_VIEW_FIELDS {
            data[*field] = map[*field].clone();
        }
    }
    if omitted > 0 {
        data["more"] = json!({
            "omitted": omitted,
            "remedy": format!("re-run with --limit {}", layers.len().min(200)),
        });
    }
    Ok(data)
}

/// One layer, as the two identifiers plus what a caller decides on.
///
/// The field names are read from the declared snapshot contract rather than
/// written inline, so the parity test's proof that the application still
/// publishes them is a proof about this function.
fn project(layer: &Value) -> Value {
    let id = layer[crate::SNAPSHOT_LAYER_ID].as_str().unwrap_or_default();
    let mut projected = json!({
        "layer": id,
        "analysis_id": format!("{ANALYSIS_SKETCH_PREFIX}{id}"),
    });
    for (reported, published) in crate::SNAPSHOT_LAYER_FIELDS {
        projected[*reported] = layer[*published].clone();
    }
    projected
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    if data["open"].as_bool().unwrap_or(false) {
        let center = &data["center"];
        out.push_str(&format!(
            "map open  centre {}, {}  zoom {}\n",
            center[0], center[1], data["zoom"]
        ));
    } else {
        out.push_str("map not open\n  → open a project map in DS GridDesign\n");
    }

    let empty = Vec::new();
    let layers = data["layers"].as_array().unwrap_or(&empty);
    if layers.is_empty() {
        out.push_str("\nno local layers\n  → ds map draw --name <name> --geometry Polygon --features <file>\n");
        return out;
    }

    out.push_str(&format!(
        "\n{} on the map:\n",
        crate::plural(data["layer_count"].as_u64().unwrap_or(0), "local layer")
    ));
    for layer in layers {
        out.push_str(&format!(
            "  {:<28} {:<11} {:>6}  {}{}\n",
            layer["name"].as_str().unwrap_or(""),
            layer["geometry"].as_str().unwrap_or(""),
            layer["features"],
            layer["analysis_id"].as_str().unwrap_or(""),
            if layer["this_session"].as_bool().unwrap_or(false) {
                "  (this session)"
            } else {
                ""
            },
        ));
    }
    if let Some(more) = data["more"].as_object() {
        out.push_str(&format!(
            "\n{} more not shown\n  → {}\n",
            more["omitted"],
            more["remedy"].as_str().unwrap_or(""),
        ));
    }
    out
}
