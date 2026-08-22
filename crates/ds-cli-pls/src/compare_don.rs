//! `ds pls compare-don` — reconcile a DON against an authority.
//!
//! Two `.don` files that describe the same line should assign the same
//! structure at the same position. When they do not, the interesting question
//! is never "are these files different" — it is *which positions disagree, and
//! is the disagreement a real substitution or just a naming difference*.
//!
//! The task answers exactly that, and separates the three outcomes:
//! agreeing, name-equivalent (reconciled by a declared equivalence), and
//! genuinely different. `ds` passes the equivalences through as repeated
//! `--equivalent from=to` pairs rather than making the caller author a request
//! file for what is usually one or two entries.

use std::collections::BTreeMap;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_tasks::{CompareDonAssignmentRequest, compare_don_assignment};
use serde_json::{Value, json};

use crate::{file_digest, numeric, source_path, task_failure};

/// The station tolerance below which two ordinals are judged to be the same
/// position. Stated as a default rather than hidden: a caller comparing
/// resurveyed alignments will want to raise it.
const DEFAULT_TOLERANCE_M: &str = "1.0";

pub static COMMAND: Command = Command {
    id: "pls.compare-don",
    path: &["pls", "compare-don"],
    contract: 1,
    summary: "Reconcile a DON's structure assignment against an authority.",
    purpose: "\
Compares two .don files position by position and separates three outcomes: \
positions that agree, positions reconciled by a declared name equivalence, and \
positions that genuinely disagree. Positions are matched by station within a \
tolerance, so a resurveyed alignment does not read as a wholesale \
substitution. Read-only.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "baseline",
            "<path>",
            "The authority: the issued or signed .don.",
        )
        .required(),
        Arg::value("candidate", "<path>", "The .don being reconciled.").required(),
        Arg::value(
            "baseline-sha256",
            "<sha256:…>",
            "The baseline's expected digest. The task refuses if the bytes changed.",
        ),
        Arg::value(
            "candidate-sha256",
            "<sha256:…>",
            "The candidate's expected digest.",
        ),
        Arg::value("baseline-block", "<n>", "Block index within the baseline.").default("0"),
        Arg::value(
            "candidate-block",
            "<n>",
            "Block index within the candidate.",
        )
        .default("0"),
        Arg::value(
            "tolerance",
            "<metres>",
            "Maximum station difference still judged the same position.",
        )
        .default(DEFAULT_TOLERANCE_M),
        Arg::repeated(
            "equivalent",
            "<candidate=baseline>",
            "Declare a deliberate naming harmonisation; repeatable.",
        ),
    ],
    output: "\
Both sources with their digests, the alignment evidence, and the three counts \
— agreeing, name-equivalent, differing — with the differing positions listed.",
    examples: &[Example {
        command: "ds pls compare-don --baseline ./issued.don --candidate ./revised.don --output json",
        note: "Without digests this refuses and reports both, ready to pin.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "source_not_found",
            when: "--baseline or --candidate does not name a file",
            remedy: "check both paths; each takes a .don",
        },
        Refusal {
            code: "invalid_number",
            when: "a block index or the tolerance is not a number",
            remedy: "block indices are whole numbers; tolerance is metres",
        },
        Refusal {
            code: "missing_digest_pin",
            when: "--baseline-sha256 or --candidate-sha256 was not given",
            remedy: "the refusal carries each file's current digest; pin those values",
        },
        Refusal {
            code: "invalid_equivalence",
            when: "an --equivalent value is not `candidate=baseline`",
            remedy: "write it as `--equivalent old-name=new-name`",
        },
        Refusal {
            code: "task_refused",
            when: "the task ran and refused — an unreadable .don, or a block index that does not exist",
            remedy: "read detail.code and detail.detail for the task's own reason",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let baseline_raw = inputs.require("baseline")?;
    let candidate_raw = inputs.require("candidate")?;

    let mut name_equivalences: BTreeMap<String, String> = BTreeMap::new();
    for pair in inputs.repeated("equivalent") {
        let Some((candidate, baseline)) = pair.split_once('=') else {
            return Err(Failure::invalid(
                "invalid_equivalence",
                format!("`{pair}` is not a `candidate=baseline` pair"),
            )
            .remedy("write it as `--equivalent old-name=new-name`"));
        };
        if candidate.is_empty() || baseline.is_empty() {
            return Err(Failure::invalid(
                "invalid_equivalence",
                format!("`{pair}` has an empty side"),
            )
            .remedy("both sides of the `=` must name a structure leaf"));
        }
        name_equivalences.insert(candidate.to_string(), baseline.to_string());
    }

    let tolerance_raw = inputs.value("tolerance").unwrap_or(DEFAULT_TOLERANCE_M);
    let station_tolerance_m: f64 = tolerance_raw.parse().map_err(|_| {
        Failure::invalid("invalid_number", "--tolerance must be a number of metres")
            .remedy("pass a value such as `--tolerance 1.0`")
    })?;
    if !station_tolerance_m.is_finite() || station_tolerance_m < 0.0 {
        return Err(Failure::invalid(
            "invalid_number",
            "--tolerance must be a finite, non-negative distance",
        )
        .remedy("pass a value such as `--tolerance 1.0`"));
    }

    let baseline_path = source_path(baseline_raw, "baseline")?;
    let candidate_path = source_path(candidate_raw, "candidate")?;

    // The task requires a digest pin for each source and refuses without one.
    // That is the guard working: it forces a caller to state what they think
    // the file is. When either is absent, the refusal carries the digests
    // observed right now, so obtaining a pin never means shelling out.
    let (Some(baseline_sha), Some(candidate_sha)) = (
        inputs.value("baseline-sha256"),
        inputs.value("candidate-sha256"),
    ) else {
        return Err(Failure::invalid(
            "missing_digest_pin",
            "this comparison is digest-pinned; both sources need an expected SHA-256",
        )
        .remedy("pin the digests below with --baseline-sha256 and --candidate-sha256")
        .detail(json!({
            "observed": {
                "baseline": file_digest(&baseline_path),
                "candidate": file_digest(&candidate_path),
            },
        })));
    };

    let request = CompareDonAssignmentRequest {
        baseline_path,
        expected_baseline_sha256: baseline_sha.to_string(),
        baseline_block_index: numeric(inputs.value("baseline-block"), 0)?,
        candidate_path,
        expected_candidate_sha256: candidate_sha.to_string(),
        candidate_block_index: numeric(inputs.value("candidate-block"), 0)?,
        station_tolerance_m,
        name_equivalences,
    };

    let result = compare_don_assignment(&request)
        .map_err(|error| task_failure(&error.code, &error.detail))?;

    serde_json::to_value(&result).map_err(|error| {
        Failure::internal(
            "result_unserializable",
            "the task result could not be encoded",
        )
        .detail(json!({ "detail": error.to_string() }))
    })
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "baseline   {} ({} structures)\ncandidate  {} ({} structures)\n\n",
        data["baseline_leaf"].as_str().unwrap_or(""),
        data["baseline_structure_count"],
        data["candidate_leaf"].as_str().unwrap_or(""),
        data["candidate_structure_count"],
    );
    out.push_str(&format!(
        "agreeing        {}\nname-equivalent {}\ndiffering       {}\n",
        data["agreeing"], data["name_equivalent"], data["differing"],
    ));

    let differences = data["differences"].as_array();
    if let Some(differences) = differences.filter(|list| !list.is_empty()) {
        out.push_str("\nDIFFERENCES\n");
        for difference in differences {
            out.push_str(&format!(
                "  #{:<5} {:<28} → {}\n",
                difference["structure_number"],
                difference["baseline_leaf"].as_str().unwrap_or(""),
                difference["candidate_leaf"].as_str().unwrap_or(""),
            ));
        }
    }
    out
}
