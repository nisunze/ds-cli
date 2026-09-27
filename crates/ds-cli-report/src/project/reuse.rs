//! Which of one transformer's requested outputs `report project export` may
//! reuse, and which it must generate — decided per output id, never per row.
//!
//! The rule is one sentence: an output is reused only when the project owns a
//! copy of THAT output id whose producing origin names this run's exact input
//! base fingerprint and this run's exact engine build; `force` generates every
//! output without consulting evidence. Nothing else counts. A fresh `xlsx`,
//! `shp` or `kmz` is evidence about itself and never about `pdf__<layout>`; a
//! row whose `report_metadata.status` is not `stale`, or whose download plan
//! says `fresh`, says a transformer has some files, not which ones — the
//! download plan's own contract keeps source freshness apart from artifact
//! availability for exactly this reason.
//!
//! A print is never reused. Its bytes also depend on the print context, the
//! layout and the style recipe, and no origin records their freshness, so the
//! input base and engine build cannot prove a print current.
//!
//! The evidence is the project's own: the transformer status row ds-brain
//! returns unchanged through `design.status`, the same read the export
//! already makes for room heads. Each `report_artifacts[]` entry names its
//! `output_id`, stored `sha256` and the `origin` that produced it —
//! `work_id`, `input_base_fingerprint`, `engine_build_manifest_sha256` — kept
//! per output because a merged head holds outputs of several runs. Nothing
//! here reads a machine-local cache or infers an origin from the row.
//!
//! `export` plans only a connected publishing run that is not `--force`d and
//! appends no survey form (the input base fingerprints no survey row); every
//! other run generates the full policy and its receipt names why.

use std::collections::{BTreeMap, BTreeSet};

use ds_command_kernel::report_export::{
    InputReceipt, engine_identity, exact_lower_sha256, input_base_fingerprint,
};
use ds_command_kernel::report_formats::{
    self, DesignOutputSelection, normalize_output_selection, output_class,
};
use serde::Serialize;
use serde_json::Value;

/// The project-owned current copy of one output, exactly as its status row
/// states it: the declared output (`output_id`, stored `sha256`) and the
/// origin that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputEvidence {
    pub output_id: String,
    pub sha256: String,
    pub work_id: String,
    pub input_base_fingerprint: String,
    pub engine_build_manifest_sha256: String,
}

/// The per-output evidence one transformer status row carries.
///
/// An artifact without an origin, or whose origin names no engine build,
/// proves nothing about its producer and is left out: that output is then
/// generated. A row whose artifacts are not the shape ds-brain writes is
/// refused whole, because picking the entries that happen to parse would
/// be inference.
pub fn evidence_from_status_row(row: &Value) -> Result<Vec<OutputEvidence>, String> {
    let artifacts = match row.get("report_artifacts") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(artifacts)) => artifacts,
        Some(_) => return Err("report_artifacts is not an array".into()),
    };
    let mut evidence = Vec::with_capacity(artifacts.len());
    for (index, artifact) in artifacts.iter().enumerate() {
        let at = |error: String| format!("report_artifacts[{index}]: {error}");
        if !artifact.is_object() {
            return Err(at("is not an object".into()));
        }
        let output_id = text(artifact, "output_id")
            .map_err(at)?
            .ok_or_else(|| at("names no output_id".into()))?;
        let origin = match artifact.get("origin") {
            None | Some(Value::Null) => continue,
            Some(origin) if origin.is_object() => origin,
            Some(_) => return Err(at("origin is not an object".into())),
        };
        let sha256 = text(artifact, "sha256").map_err(at)?;
        let work_id = text(origin, "work_id").map_err(at)?;
        let input_base = text(origin, "input_base_fingerprint").map_err(at)?;
        let engine_build = text(origin, "engine_build_manifest_sha256").map_err(at)?;
        if let (Some(sha256), Some(work_id), Some(input_base), Some(engine_build)) =
            (sha256, work_id, input_base, engine_build)
        {
            evidence.push(OutputEvidence {
                output_id,
                sha256,
                work_id,
                input_base_fingerprint: input_base,
                engine_build_manifest_sha256: engine_build,
            });
        }
    }
    Ok(evidence)
}

/// A present string member; absent, null or empty is `None` (ds-brain omits
/// empty members), any other type is a shape violation.
fn text(value: &Value, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.is_empty() => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("{key} is not a string")),
    }
}

/// The output ids a full run of this project's policy produces, in the
/// policy's order: what a run asks for when nothing is reused.
pub fn policy_outputs(receipt: &InputReceipt) -> Result<Vec<String>, String> {
    Ok(
        report_formats::selected_plan(&receipt.sheets()?, false, None)?
            .into_iter()
            .map(|output| output.output_id)
            .collect(),
    )
}

/// This run's input base for one transformer at the saved revision it will
/// read: the kernel's fingerprint, the one the run receipt carries.
pub fn current_input_base(
    transformer: &str,
    server_version: i64,
    receipt: &InputReceipt,
) -> Result<String, String> {
    input_base_fingerprint(
        transformer,
        server_version,
        &receipt.sheets_sha256,
        &receipt.country,
        &receipt.reference_semantic_sha256,
    )
}

/// The installed engine's build manifest digest, from its `build-info`.
pub fn current_engine_build(build_info: &Value) -> Result<String, String> {
    Ok(engine_identity(build_info)?.build_manifest_sha256)
}

/// What this run would produce from: the kernel's `input_base_fingerprint`
/// for the transformer and the installed engine's build manifest digest.
#[derive(Debug, Clone, Copy)]
pub struct CurrentInputs<'a> {
    pub input_base_fingerprint: &'a str,
    pub engine_build_manifest_sha256: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerateReason {
    /// The caller asked for regeneration; evidence was not consulted.
    Forced,
    /// A print: its context, layout and style recipe have no proven freshness.
    PrintFreshnessUnproven,
    /// The project holds no copy of this exact output id with an origin.
    NoEvidence,
    /// More than one copy claims this output id; neither is chosen.
    EvidenceAmbiguous,
    /// A digest in the evidence is not an exact lowercase sha256.
    EvidenceMalformed,
    /// The copy was produced from a different input base.
    InputsChanged,
    /// The copy was produced by a different engine build.
    EngineChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    /// Skip generation; the project's copy with these bytes, from this run
    /// of the project's, is current.
    Reuse {
        sha256: String,
        work_id: String,
    },
    Generate {
        reason: GenerateReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputPlan {
    pub output_id: String,
    #[serde(flatten)]
    pub decision: Decision,
}

/// Decide every requested output of one transformer, in request order.
///
/// Refuses a request that names an output twice and current inputs that are
/// not exact digests: a comparison against an unproven "current" would make
/// any evidence look fresh or stale at random.
pub fn plan_outputs(
    requested: &[String],
    current: CurrentInputs<'_>,
    evidence: &[OutputEvidence],
    force: bool,
) -> Result<Vec<OutputPlan>, String> {
    let mut seen = BTreeSet::new();
    if let Some(repeated) = requested.iter().find(|id| !seen.insert(id.as_str())) {
        return Err(format!("output {repeated:?} is requested more than once"));
    }
    if !exact_lower_sha256(current.input_base_fingerprint)
        || !exact_lower_sha256(current.engine_build_manifest_sha256)
    {
        return Err("current input base and engine build must be exact sha256 digests".into());
    }
    let mut held: BTreeMap<&str, Vec<&OutputEvidence>> = BTreeMap::new();
    for entry in evidence {
        held.entry(entry.output_id.as_str())
            .or_default()
            .push(entry);
    }
    Ok(requested
        .iter()
        .map(|output_id| {
            let decision = if force {
                Decision::Generate {
                    reason: GenerateReason::Forced,
                }
            } else if output_class(output_id) == "print" {
                Decision::Generate {
                    reason: GenerateReason::PrintFreshnessUnproven,
                }
            } else {
                decide(held.get(output_id.as_str()).map(Vec::as_slice), current)
            };
            OutputPlan {
                output_id: output_id.clone(),
                decision,
            }
        })
        .collect())
}

fn decide(copies: Option<&[&OutputEvidence]>, current: CurrentInputs<'_>) -> Decision {
    let generate = |reason| Decision::Generate { reason };
    let copy = match copies {
        None | Some([]) => return generate(GenerateReason::NoEvidence),
        Some([copy]) => copy,
        Some(_) => return generate(GenerateReason::EvidenceAmbiguous),
    };
    if !exact_lower_sha256(&copy.sha256)
        || !exact_lower_sha256(&copy.input_base_fingerprint)
        || !exact_lower_sha256(&copy.engine_build_manifest_sha256)
    {
        return generate(GenerateReason::EvidenceMalformed);
    }
    if copy.input_base_fingerprint != current.input_base_fingerprint {
        return generate(GenerateReason::InputsChanged);
    }
    if copy.engine_build_manifest_sha256 != current.engine_build_manifest_sha256 {
        return generate(GenerateReason::EngineChanged);
    }
    Decision::Reuse {
        sha256: copy.sha256.clone(),
        work_id: copy.work_id.clone(),
    }
}

/// The output ids a run must hand the engine: everything not reused.
pub fn to_generate(plan: &[OutputPlan]) -> Vec<&str> {
    plan.iter()
        .filter(|output| matches!(output.decision, Decision::Generate { .. }))
        .map(|output| output.output_id.as_str())
        .collect()
}

/// What one transformer's run hands the engine, and so what it publishes: a
/// run seals only the outputs it produced, and ds-brain's merge keeps the
/// project-owned copies of the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunScope {
    /// Nothing is reused: the run is the full policy run, no selection.
    Full,
    /// Only these outputs are generated and published.
    Subset(DesignOutputSelection),
    /// Every requested output is reused: no engine run, no publication.
    AllReused,
}

/// The scope of one transformer's run under its plan, as the kernel's own
/// local output selection so the engine request plans exactly these ids.
pub fn run_scope(plan: &[OutputPlan]) -> Result<RunScope, String> {
    let generate = to_generate(plan);
    if generate.len() == plan.len() {
        return Ok(RunScope::Full);
    }
    if generate.is_empty() {
        return Ok(RunScope::AllReused);
    }
    let tokens = generate.iter().map(|id| id.to_string()).collect();
    normalize_output_selection(None, Some(tokens), None).map(RunScope::Subset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const INPUTS: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const ENGINE: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const BYTES: &str = "3333333333333333333333333333333333333333333333333333333333333333";
    const OTHER: &str = "4444444444444444444444444444444444444444444444444444444444444444";

    fn current() -> CurrentInputs<'static> {
        CurrentInputs {
            input_base_fingerprint: INPUTS,
            engine_build_manifest_sha256: ENGINE,
        }
    }

    fn held(output_id: &str) -> OutputEvidence {
        OutputEvidence {
            output_id: output_id.into(),
            sha256: BYTES.into(),
            work_id: "work-1".into(),
            input_base_fingerprint: INPUTS.into(),
            engine_build_manifest_sha256: ENGINE.into(),
        }
    }

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn reason(plan: &[OutputPlan], output_id: &str) -> Option<GenerateReason> {
        match &plan.iter().find(|o| o.output_id == output_id)?.decision {
            Decision::Generate { reason } => Some(*reason),
            Decision::Reuse { .. } => None,
        }
    }

    /// A row as ds-brain's `TransformerStatus` marshals it.
    fn artifact(output_id: &str, origin: Value) -> Value {
        json!({
            "origin": origin,
            "sha256": BYTES,
            "size_bytes": 10,
            "url": format!("https://example.invalid/{output_id}"),
            "output_id": output_id,
            "format": output_id,
        })
    }

    fn origin() -> Value {
        json!({
            "work_id": "work-1",
            "input_base_fingerprint": INPUTS,
            "engine_version": "1.2.3",
            "engine_build_manifest_sha256": ENGINE,
        })
    }

    #[test]
    fn a_status_row_yields_each_output_with_its_exact_origin() {
        let row = json!({
            "name": "TR-1",
            "metadata": {"version": 7},
            "report_artifacts": [
                artifact("xlsx", origin()),
                artifact("kmz", Value::Null),
                artifact("shp", json!({"work_id": "work-0", "input_base_fingerprint": INPUTS})),
            ],
        });
        assert_eq!(evidence_from_status_row(&row).unwrap(), [held("xlsx")]);
        assert!(
            evidence_from_status_row(&json!({"name": "TR-1"}))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_row_not_shaped_as_ds_brain_writes_it_is_refused_whole() {
        for row in [
            json!({"report_artifacts": {"xlsx": {}}}),
            json!({"report_artifacts": ["xlsx"]}),
            json!({"report_artifacts": [artifact("xlsx", json!("work-1"))]}),
            json!({"report_artifacts": [{"sha256": BYTES, "origin": origin()}]}),
            json!({"report_artifacts": [artifact("xlsx", json!({"work_id": 1}))]}),
        ] {
            assert!(evidence_from_status_row(&row).is_err(), "{row}");
        }
    }

    #[test]
    fn the_current_input_base_is_the_kernels_fingerprint() {
        let receipt = InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: "RW".into(),
            sheets_json: "{}".into(),
            sheets_sha256: BYTES.into(),
            reference_semantic_sha256: OTHER.into(),
        };
        assert_eq!(
            current_input_base("tr_1", 7, &receipt).unwrap(),
            input_base_fingerprint("tr_1", 7, BYTES, "RW", OTHER).unwrap()
        );
        assert_ne!(
            current_input_base("tr_1", 8, &receipt).unwrap(),
            current_input_base("tr_1", 7, &receipt).unwrap()
        );
        assert!(current_engine_build(&json!({"build_manifest_sha256": ENGINE})).is_err());
    }

    #[test]
    fn fresh_tabular_and_geospatial_outputs_never_stand_in_for_a_missing_pdf() {
        let requested = ids(&["xlsx", "shp", "kmz", "pdf__a3_sheet"]);
        let evidence = [held("xlsx"), held("shp"), held("kmz")];
        let plan = plan_outputs(&requested, current(), &evidence, false).unwrap();
        assert_eq!(to_generate(&plan), ["pdf__a3_sheet"]);
        for reused in ["xlsx", "shp", "kmz"] {
            assert_eq!(
                plan.iter()
                    .find(|o| o.output_id == reused)
                    .unwrap()
                    .decision,
                Decision::Reuse {
                    sha256: BYTES.into(),
                    work_id: "work-1".into(),
                }
            );
        }
        // Only the pdf reaches the engine, and so only the pdf is published.
        let RunScope::Subset(selection) = run_scope(&plan).unwrap() else {
            panic!("a mixed plan runs a subset");
        };
        assert_eq!(selection.tokens().unwrap(), ["pdf__a3_sheet"]);
        assert!(selection.geospatial.is_empty() && selection.tabular.is_empty());
    }

    #[test]
    fn a_print_always_generates_even_over_perfect_evidence() {
        let requested = ids(&["pdf__a3_sheet", "png__a3_sheet", "pdf_a3"]);
        let evidence = [held("pdf__a3_sheet"), held("png__a3_sheet"), held("pdf_a3")];
        let plan = plan_outputs(&requested, current(), &evidence, false).unwrap();
        assert_eq!(
            to_generate(&plan),
            ["pdf__a3_sheet", "png__a3_sheet", "pdf_a3"]
        );
        assert!(
            requested
                .iter()
                .all(|id| { reason(&plan, id) == Some(GenerateReason::PrintFreshnessUnproven) })
        );
        assert_eq!(run_scope(&plan).unwrap(), RunScope::Full);
    }

    #[test]
    fn an_output_is_matched_by_its_exact_output_id_never_by_format_or_class() {
        let requested = ids(&["shp", "gpkg"]);
        let evidence = [held("shp.zip"), held("SHP"), held("geospatial")];
        let plan = plan_outputs(&requested, current(), &evidence, false).unwrap();
        assert_eq!(to_generate(&plan), ["shp", "gpkg"]);
        assert_eq!(reason(&plan, "gpkg"), Some(GenerateReason::NoEvidence));
    }

    #[test]
    fn an_all_reused_row_runs_nothing_and_a_cold_row_runs_the_full_policy() {
        let requested = ids(&["xlsx", "shp", "kmz"]);
        let evidence = [held("xlsx"), held("shp"), held("kmz")];
        let warm = plan_outputs(&requested, current(), &evidence, false).unwrap();
        assert_eq!(run_scope(&warm).unwrap(), RunScope::AllReused);
        let cold = plan_outputs(&requested, current(), &[], false).unwrap();
        assert_eq!(run_scope(&cold).unwrap(), RunScope::Full);
    }

    #[test]
    fn force_generates_every_output_even_over_perfect_evidence() {
        let requested = ids(&["xlsx", "pdf__a3_sheet"]);
        let evidence = [held("xlsx"), held("pdf__a3_sheet")];
        let plan = plan_outputs(&requested, current(), &evidence, true).unwrap();
        assert_eq!(to_generate(&plan), ["xlsx", "pdf__a3_sheet"]);
        assert!(plan.iter().all(|o| o.decision
            == Decision::Generate {
                reason: GenerateReason::Forced
            }));
        assert_eq!(run_scope(&plan).unwrap(), RunScope::Full);
    }

    #[test]
    fn a_copy_from_other_inputs_or_another_engine_build_is_regenerated() {
        let mut moved_inputs = held("xlsx");
        moved_inputs.input_base_fingerprint = OTHER.into();
        let mut moved_engine = held("kmz");
        moved_engine.engine_build_manifest_sha256 = OTHER.into();
        let plan = plan_outputs(
            &ids(&["xlsx", "kmz"]),
            current(),
            &[moved_inputs, moved_engine],
            false,
        )
        .unwrap();
        assert_eq!(reason(&plan, "xlsx"), Some(GenerateReason::InputsChanged));
        assert_eq!(reason(&plan, "kmz"), Some(GenerateReason::EngineChanged));
    }

    #[test]
    fn ambiguous_or_malformed_evidence_is_never_reused() {
        let mut malformed = held("kmz");
        malformed.sha256 = "A".repeat(64);
        let plan = plan_outputs(
            &ids(&["xlsx", "kmz"]),
            current(),
            &[held("xlsx"), held("xlsx"), malformed],
            false,
        )
        .unwrap();
        assert_eq!(
            reason(&plan, "xlsx"),
            Some(GenerateReason::EvidenceAmbiguous)
        );
        assert_eq!(
            reason(&plan, "kmz"),
            Some(GenerateReason::EvidenceMalformed)
        );
    }

    #[test]
    fn a_repeated_request_or_an_unproven_current_input_is_refused() {
        assert!(plan_outputs(&ids(&["xlsx", "xlsx"]), current(), &[], false).is_err());
        let unproven = CurrentInputs {
            input_base_fingerprint: "",
            engine_build_manifest_sha256: ENGINE,
        };
        assert!(plan_outputs(&ids(&["xlsx"]), unproven, &[held("xlsx")], false).is_err());
    }

    #[test]
    fn the_plan_serializes_one_decision_per_output() {
        let plan = plan_outputs(
            &ids(&["xlsx", "pdf__a3_sheet"]),
            current(),
            &[held("xlsx")],
            false,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&plan).unwrap(),
            json!([
                {"output_id": "xlsx", "decision": "reuse", "sha256": BYTES, "work_id": "work-1"},
                {"output_id": "pdf__a3_sheet", "decision": "generate", "reason": "print_freshness_unproven"}
            ])
        );
    }
}
