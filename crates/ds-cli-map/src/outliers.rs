//! `ds map outliers` — flag geometry that does not belong with the rest.
//!
//! Three independent detectors, each switchable: spatial isolation (a feature
//! far from every other), size (a feature much longer or larger than its
//! neighbours) and extent (a feature whose bounding box is out of scale).
//! The application scores them together and puts the flagged features on the
//! map as a new layer.
//!
//! The bounded default matters here more than anywhere else in the domain.
//! The application's answer carries the entire scored feature collection —
//! every feature, every property — and returning that verbatim would make one
//! call the most expensive thing in the CLI. So the default is the counts and
//! the score summary, the flagged features are already visible on the map,
//! and their individual findings are an explicit `--limit` projection whose
//! truncation is reported in `more`.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{BOOL_CHOICES, DESCRIPTOR_ARG};

/// The application's own defaults, as its tool dock opens with them.
const DEFAULT_THRESHOLD: &str = "3.5";
const DEFAULT_MIN_FEATURES: &str = "5";
/// Enough findings to see the shape of the problem, not so many that reading
/// the answer costs more than fixing it.
const DEFAULT_LIMIT: &str = "10";

pub static COMMAND: Command = Command {
    id: "map.outliers",
    path: &["map", "outliers"],
    contract: 1,
    summary: "Flag geometry outliers in a layer as a new layer.",
    purpose: "\
Scores every feature in a layer against its neighbours and flags the ones that \
do not belong: isolated in space, out of scale in size, or out of scale in \
extent. Flagged features are added to the map as a new layer. The default \
answer is counts and the score summary; --limit adds individual findings, and \
the full scored collection is never returned — it is on the map instead.",
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "layer",
            "<analysis-id>",
            "The `analysis_id` from `ds map view`.",
        )
        .required(),
        Arg::value(
            "threshold",
            "<n>",
            "Score above which a feature is flagged; 1..20.",
        )
        .default(DEFAULT_THRESHOLD),
        Arg::value(
            "min-features",
            "<n>",
            "Refuse to score a layer smaller than this; 3..100.",
        )
        .default(DEFAULT_MIN_FEATURES),
        Arg::value(
            "spatial-isolation",
            "<bool>",
            "Detect features far from every other.",
        )
        .default("true")
        .choices(BOOL_CHOICES),
        Arg::value(
            "size-outliers",
            "<bool>",
            "Detect features out of scale in size.",
        )
        .default("true")
        .choices(BOOL_CHOICES),
        Arg::value(
            "extent-outliers",
            "<bool>",
            "Detect features out of scale in extent.",
        )
        .default("true")
        .choices(BOOL_CHOICES),
        Arg::value("limit", "<n>", "Return at most this many findings; 0..200.")
            .default(DEFAULT_LIMIT),
        DESCRIPTOR_ARG,
    ],
    output: "\
How many features were analysable, how many were invalid, how many were \
flagged and by which detector, the median distance, size and extent the scores \
were taken against, and the new layer holding the flagged features. Up to \
--limit findings, with `more.omitted` when the list was cut.",
    examples: &[Example {
        command: "ds map outliers --layer lv:poles --threshold 4 --output json",
        note: "A design layer is addressable too, not only a drawn one.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "no such analysis layer, it is empty, or its geometry is unusable",
            remedy: "run `ds map view`; the layer must carry features with geometry",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let layer = inputs.require("layer")?;
    let threshold = crate::number(inputs.require("threshold")?, "threshold", 1.0, 20.0)?;
    let min_features = crate::integer(inputs.require("min-features")?, "min-features", 3, 100)?;
    let limit = crate::integer(inputs.require("limit")?, "limit", 0, 200)? as usize;

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DETECT_OUTLIERS,
        json!({
            "layerId": layer,
            "settings": {
                "threshold": threshold,
                "minFeatures": min_features,
                "spatialIsolation": crate::boolean(inputs.value("spatial-isolation"), true),
                "sizeOutliers": crate::boolean(inputs.value("size-outliers"), true),
                "extentOutliers": crate::boolean(inputs.value("extent-outliers"), true),
            },
        }),
        crate::TOOL_TIMEOUT,
    )?;

    let output = &result["output"];
    let summary = &output["summary"];
    let empty = Vec::new();
    let findings = output["findings"].as_array().unwrap_or(&empty);
    let shown: Vec<Value> = findings.iter().take(limit).map(project).collect();
    let omitted = findings.len().saturating_sub(shown.len());

    let mut data = json!({
        "source": layer,
        "threshold": threshold,
        "features": output["feature_count"].as_u64().unwrap_or(0),
        "analyzable": output["analyzable_count"].as_u64().unwrap_or(0),
        "invalid": output["invalid_count"].as_u64().unwrap_or(0),
        "flagged": output["outlier_count"].as_u64().unwrap_or(0),
        "geometry": output["geometry_type"],
        "units": output["coordinate_units"],
        // True when the layer was smaller than --min-features: nothing was
        // scored, which is a different answer from "nothing was flagged".
        "insufficient_features": summary["insufficient_features"].as_bool().unwrap_or(false),
        "by_detector": {
            "spatial_isolation": summary["spatial_isolation_count"].as_u64().unwrap_or(0),
            "size": summary["size_outlier_count"].as_u64().unwrap_or(0),
            "extent": summary["extent_outlier_count"].as_u64().unwrap_or(0),
        },
        "medians": {
            "nearest_distance": summary["nearest_distance_median"],
            "size": summary["size_metric_median"],
            "extent": summary["extent_metric_median"],
        },
        "layer": result["layerId"],
        "name": result["layerName"],
        "findings": shown,
    });
    if omitted > 0 {
        data["more"] = json!({
            "omitted": omitted,
            "remedy": format!("re-run with --limit {}", findings.len().min(200)),
        });
    }
    Ok(data)
}

/// One finding, without the scored geometry — that is on the map already.
fn project(finding: &Value) -> Value {
    json!({
        "feature": finding["feature_id"],
        "index": finding["feature_index"],
        "score": finding["score"],
        "reasons": finding["reasons"],
        "nearest_distance": finding["nearest_distance"],
    })
}

pub fn render(data: &Value) -> String {
    if data["insufficient_features"].as_bool().unwrap_or(false) {
        return format!(
            "not scored  — {} has too few features\n  → lower --min-features, or use a fuller layer\n",
            data["source"].as_str().unwrap_or("")
        );
    }
    let flagged = data["flagged"].as_u64().unwrap_or(0);
    let detectors = &data["by_detector"];
    let mut out = format!(
        "{} of {} analysable\n  isolated {}  ·  size {}  ·  extent {}\n",
        crate::plural(flagged, "outlier"),
        data["analyzable"],
        detectors["spatial_isolation"],
        detectors["size"],
        detectors["extent"],
    );
    if let Some(invalid) = data["invalid"].as_u64().filter(|count| *count > 0) {
        out.push_str(&format!("  {} with unusable geometry\n", invalid));
    }
    if flagged > 0 {
        out.push_str(&format!(
            "  new layer  {}\n",
            data["name"].as_str().unwrap_or("")
        ));
    }
    if let Some(findings) = data["findings"].as_array().filter(|list| !list.is_empty()) {
        out.push('\n');
        for finding in findings {
            let empty = Vec::new();
            let reasons: Vec<&str> = finding["reasons"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .filter_map(Value::as_str)
                .collect();
            out.push_str(&format!(
                "  {:<28} {:>7.2}  {}\n",
                finding["feature"].as_str().unwrap_or(""),
                finding["score"].as_f64().unwrap_or(0.0),
                reasons.join(", "),
            ));
        }
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
