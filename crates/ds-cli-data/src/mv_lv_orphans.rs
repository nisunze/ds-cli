//! `ds data mv-lv-orphans` — MV line ends without a transformer, and
//! transformers without MV, from local files.
//!
//! The kernel (`ds_command_kernel::mv_lv_orphans`) owns the join: which
//! features count, which line end is a junction, every distance and every
//! verdict. This surface reads the declared files and writes the optional
//! GeoJSON of orphans; it decides nothing.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::mv_lv_orphans::{self as orphans, Code, MvDocument, Request};
use serde_json::Value;
use std::io::Write;

const MV: Arg = Arg::repeated(
    "mv",
    "<path>",
    "MV lines as GeoJSON, e.g. from `ds dsgrid project geojson`; repeat per file.",
)
.required();
const TRANSFORMERS: Arg = Arg::value(
    "transformers",
    "<path>",
    "Transformer points as GeoJSON, or the JSON `ds design status --output json` wrote.",
)
.required();
const TOLERANCE: Arg = Arg::value(
    "tolerance-m",
    "<metres>",
    "Farther than this from its partner, a line end or transformer is an orphan.",
)
.required();
const JUNCTION: Arg = Arg::value(
    "junction-m",
    "<metres>",
    "A line end this close to another MV line is a junction, not a tip.",
)
.default("1");
const EXCLUDE: Arg = Arg::repeated(
    "exclude-prefix",
    "<text>",
    "Never judge transformers whose names start with this; repeat for more.",
);
const LIMIT: Arg = Arg::value(
    "limit",
    "<1..5000>",
    "Entries per list in the receipt; counts are always complete.",
)
.default("50");
const OUT: Arg = Arg::value(
    "out",
    "<path>",
    "Write every orphan as GeoJSON points here, for `ds map local register`.",
);
const OVERWRITE: Arg = Arg::switch("overwrite", "Replace --out if it already exists.");

pub const DISTANCE_OUT_OF_RANGE: Refusal = Refusal {
    code: "orphans_distance_out_of_range",
    when: "--tolerance-m or --junction-m is not a number, out of range, or not positive.",
    remedy: "Pass --tolerance-m above 0 and --junction-m of 0 or more, both at most 100000.",
};
pub const LIMIT_OUT_OF_RANGE: Refusal = Refusal {
    code: "orphans_limit_out_of_range",
    when: "--limit is 0, above 5000, or not a number.",
    remedy: "Pass a limit between 1 and 5000, or omit it for 50.",
};
pub const MV_MALFORMED: Refusal = Refusal {
    code: "orphans_mv_malformed",
    when: "An --mv file is not a GeoJSON FeatureCollection or Feature.",
    remedy: "Pass MV lines as GeoJSON, for example from `ds dsgrid project geojson`.",
};
pub const MV_EMPTY: Refusal = Refusal {
    code: "orphans_mv_empty",
    when: "No --mv file holds a usable LineString or MultiLineString.",
    remedy: "Pass the MV alignments as GeoJSON lines with WGS84 coordinates.",
};
pub const TRANSFORMERS_MALFORMED: Refusal = Refusal {
    code: "orphans_transformers_malformed",
    when: "--transformers is neither GeoJSON points nor a `ds design status` envelope.",
    remedy: "Pass named GeoJSON points or the JSON `ds design status --output json` writes.",
};
pub const TRANSFORMERS_EMPTY: Refusal = Refusal {
    code: "orphans_transformers_empty",
    when: "No transformer in --transformers carries a name and a WGS84 position.",
    remedy: "Pass named points, or design status rows carrying metadata.spatial.representative_point.",
};

pub static COMMAND: Command = Command {
    id: "data.mv-lv-orphans",
    path: &["data", "mv-lv-orphans"],
    contract: 1,
    summary: "Find MV line ends without a transformer and transformers without MV.",
    purpose: "\
Joins MV lines to transformers within a tolerance, offline. A line end that \
touches another MV line is a junction; every other end is a tip, and a tip \
farther than the tolerance from every transformer is an orphan. A transformer \
farther than the tolerance from every MV line is an orphan too, and an \
alignment none of whose tips reaches a transformer is listed. Distances are \
geodesic metres on WGS84 input; no projection or country is assumed. \
Exclusions (e.g. a fill-in prefix) still locate a transformer but never judge it.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        MV,
        TRANSFORMERS,
        TOLERANCE,
        JUNCTION,
        EXCLUDE,
        LIMIT,
        OUT,
        OVERWRITE,
    ],
    output: "\
`counts` over everything read (lines, line ends, junctions, tips, orphan tips, \
transformers located/excluded/judged, orphan transformers, skipped features), \
then `orphan_tips`, `orphan_transformers` and `alignments_ending_at_no_transformer`, \
each farthest first and bounded by --limit, which `more` reports. `skipped` \
counts every dropped feature by reason. --out writes every orphan as a point.",
    examples: &[Example {
        command: "ds data mv-lv-orphans --mv ./model-1.geojson --transformers ./design-status.json --tolerance-m 30 --output json",
        note: "MV tips and transformers more than 30 m from their partner.",
        runnable: false,
    }],
    refusals: &[
        crate::UNREADABLE,
        DISTANCE_OUT_OF_RANGE,
        LIMIT_OUT_OF_RANGE,
        MV_MALFORMED,
        MV_EMPTY,
        TRANSFORMERS_MALFORMED,
        TRANSFORMERS_EMPTY,
        crate::OUTPUT_REFUSED,
    ],
    reference: Some("docs/reference/data.md"),
    search: &[
        "orphan",
        "orphans",
        "mv tip",
        "transformer without mv",
        "unconnected transformer",
        "mv lv join",
        "dangling",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

/// The kernel's closed refusal vocabulary, mapped exhaustively: a code the
/// kernel grows fails to compile here instead of reaching a caller undeclared.
fn refuse(refusal: orphans::Refusal) -> Failure {
    let message = refusal.message;
    match refusal.code {
        Code::DistanceOutOfRange => Failure::invalid(DISTANCE_OUT_OF_RANGE.code, message)
            .remedy(DISTANCE_OUT_OF_RANGE.remedy),
        Code::LimitOutOfRange => {
            Failure::invalid(LIMIT_OUT_OF_RANGE.code, message).remedy(LIMIT_OUT_OF_RANGE.remedy)
        }
        Code::MvMalformed => {
            Failure::invalid(MV_MALFORMED.code, message).remedy(MV_MALFORMED.remedy)
        }
        Code::MvEmpty => Failure::invalid(MV_EMPTY.code, message).remedy(MV_EMPTY.remedy),
        Code::TransformersMalformed => Failure::invalid(TRANSFORMERS_MALFORMED.code, message)
            .remedy(TRANSFORMERS_MALFORMED.remedy),
        Code::TransformersEmpty => {
            Failure::invalid(TRANSFORMERS_EMPTY.code, message).remedy(TRANSFORMERS_EMPTY.remedy)
        }
    }
}

/// A file's JSON, or the kernel's malformed refusal for that input.
fn read_json(path: &str, malformed: Code) -> Result<Value, Failure> {
    let bytes = crate::read_source(path)?;
    serde_json::from_slice(&bytes).map_err(|error| {
        refuse(orphans::Refusal {
            code: malformed,
            message: format!("{path} is not JSON: {error}"),
            remedy: "",
        })
    })
}

fn metres(inputs: &Inputs, name: &str) -> Result<f64, Failure> {
    let raw = inputs.require(name)?;
    raw.trim().parse::<f64>().map_err(|_| {
        Failure::invalid(
            DISTANCE_OUT_OF_RANGE.code,
            format!("--{name} `{raw}` is not a number of metres"),
        )
        .remedy(DISTANCE_OUT_OF_RANGE.remedy)
    })
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let tolerance_m = metres(inputs, "tolerance-m")?;
    let junction_m = metres(inputs, "junction-m")?;
    let raw_limit = inputs.require("limit")?;
    // An unreadable limit is the kernel's out-of-range refusal, as 0 is.
    let limit =
        orphans::limit(Some(raw_limit.trim().parse::<usize>().unwrap_or(0))).map_err(refuse)?;
    let paths = inputs.repeated("mv");
    let documents: Vec<Value> = paths
        .iter()
        .map(|path| read_json(path, Code::MvMalformed))
        .collect::<Result<_, _>>()?;
    let transformers = read_json(inputs.require("transformers")?, Code::TransformersMalformed)?;
    let labels: Vec<String> = paths.iter().map(|path| crate::file_name(path)).collect();
    let mv: Vec<MvDocument<'_>> = labels
        .iter()
        .zip(&documents)
        .map(|(label, document)| MvDocument {
            source: label,
            document,
        })
        .collect();
    let outcome = orphans::join(&Request {
        mv: &mv,
        transformers: &transformers,
        tolerance_m,
        junction_m,
        exclude_prefixes: inputs.repeated("exclude-prefix"),
    })
    .map_err(refuse)?;
    let mut receipt = outcome.receipt(limit);
    if let Some(path) = inputs.value("out") {
        write_out(path, &outcome.orphan_features(), inputs.switch("overwrite"))?;
        receipt["out"] = Value::String(path.to_owned());
    }
    Ok(receipt)
}

fn write_out(path: &str, document: &Value, overwrite: bool) -> Result<(), Failure> {
    let refused = |message: String| {
        Failure::invalid(crate::OUTPUT_REFUSED.code, message).remedy(crate::OUTPUT_REFUSED.remedy)
    };
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if overwrite {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options
        .open(path)
        .map_err(|error| refused(format!("Could not create {path}: {error}")))?;
    let bytes = serde_json::to_vec(document).map_err(|error| refused(error.to_string()))?;
    file.write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|error| refused(format!("Could not write {path}: {error}")))
}

pub fn render(data: &Value) -> String {
    let counts = &data["counts"];
    let mut out = format!(
        "{} MV line(s), {} tip(s), {} junction(s); {} orphan tip(s)\n{} transformer(s) judged ({} excluded); {} orphan transformer(s)\n",
        counts["mv_lines"],
        counts["tips"],
        counts["junctions"],
        counts["orphan_tips"],
        counts["transformers_judged"],
        counts["transformers_excluded"],
        counts["orphan_transformers"],
    );
    for tip in data["orphan_tips"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  tip  {} {}  nearest {} at {} m\n",
            tip["alignment"].as_str().unwrap_or("?"),
            tip["end"].as_str().unwrap_or("?"),
            tip["nearest_transformer"].as_str().unwrap_or("?"),
            tip["distance_m"],
        ));
    }
    for transformer in data["orphan_transformers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  transformer  {}  nearest MV {} at {} m\n",
            transformer["name"].as_str().unwrap_or("?"),
            transformer["nearest_mv_alignment"].as_str().unwrap_or("?"),
            transformer["distance_to_mv_m"],
        ));
    }
    if !data["more"].is_null() {
        out.push_str("  more entries were omitted; raise --limit or read --out\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::output::{Format, Output};
    use serde_json::json;

    fn inputs(tokens: &[&str]) -> Inputs {
        let tokens: Vec<String> = tokens.iter().map(|t| (*t).to_owned()).collect();
        ds_cli_contract::args::parse(&COMMAND, &tokens).expect("declared inputs")
    }

    fn context() -> Context {
        Context {
            confirmed: false,
            output: Output::resolve(Format::Json, false, true),
        }
    }

    /// A fresh scratch directory under the process temp root, removed on drop.
    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("ds-mv-lv-orphans-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn declared_codes_are_the_kernels_vocabulary() {
        for code in Code::ALL {
            let refused = refuse(orphans::Refusal {
                code,
                message: String::new(),
                remedy: "",
            });
            assert_eq!(refused.code(), code.as_str());
            assert!(
                COMMAND.refusals.iter().any(|d| d.code == code.as_str()),
                "{} is not declared",
                code.as_str()
            );
        }
    }

    #[test]
    fn files_in_receipt_and_geojson_out() {
        let dir = Scratch::new("files");
        let mv = dir.path().join("model.geojson");
        let trs = dir.path().join("status.json");
        std::fs::write(
            &mv,
            json!({"type": "FeatureCollection", "features": [
                {"type": "Feature", "properties": {"alignment_id": "a1"},
                 "geometry": {"type": "LineString", "coordinates": [[0.0, 0.0], [0.004, 0.0]]}}
            ]})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            &trs,
            json!({"data": {"transformers": [
                {"name": "east", "kind": "individual",
                 "metadata": {"spatial": {"representative_point": [0.004, 0.0001], "source_layer": "tr"}}},
                {"name": "stray", "kind": "individual",
                 "metadata": {"spatial": {"representative_point": [0.001, 0.01], "source_layer": "tr"}}}
            ]}})
            .to_string(),
        )
        .unwrap();
        let out = dir.path().join("orphans.geojson");
        let parsed = inputs(&[
            "--mv",
            mv.to_str().unwrap(),
            "--transformers",
            trs.to_str().unwrap(),
            "--tolerance-m",
            "30",
            "--out",
            out.to_str().unwrap(),
        ]);
        let receipt = run(&parsed, &context()).expect("join");
        assert_eq!(receipt["counts"]["orphan_tips"], 1, "{receipt}");
        assert_eq!(receipt["orphan_tips"][0]["source"], "model.geojson");
        assert_eq!(receipt["counts"]["orphan_transformers"], 1, "{receipt}");
        assert_eq!(receipt["orphan_transformers"][0]["name"], "stray");
        let written: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(written["features"].as_array().unwrap().len(), 2);
        // The same --out is refused without --overwrite.
        let again = run(&parsed, &context()).unwrap_err();
        assert_eq!(again.code(), crate::OUTPUT_REFUSED.code);
        assert!(!render(&receipt).is_empty());
    }

    #[test]
    fn kernel_refusals_reach_the_caller_as_declared_codes() {
        let dir = Scratch::new("refusals");
        let mv = dir.path().join("model.geojson");
        std::fs::write(&mv, "{\"rows\": []}").unwrap();
        let parsed = inputs(&[
            "--mv",
            mv.to_str().unwrap(),
            "--transformers",
            mv.to_str().unwrap(),
            "--tolerance-m",
            "30",
        ]);
        let refused = run(&parsed, &context()).unwrap_err();
        assert_eq!(refused.code(), MV_MALFORMED.code);
        let parsed = inputs(&[
            "--mv",
            mv.to_str().unwrap(),
            "--transformers",
            mv.to_str().unwrap(),
            "--tolerance-m",
            "zero",
        ]);
        let refused = run(&parsed, &context()).unwrap_err();
        assert_eq!(refused.code(), DISTANCE_OUT_OF_RANGE.code);
    }
}
