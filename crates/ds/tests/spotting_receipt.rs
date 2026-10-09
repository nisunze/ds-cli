//! `ds dsgrid spotting apply-receipt` lands the exact receipt `ds dsgrid run`
//! wrote for a whole-model spotting proposal, without planning again.
//!
//! The model is authored here from typed rows: one straight 400 m line over
//! flat ground between two existing dead-end structures, strung once, with a
//! design policy carrying its spotting settings. The receipt is the real
//! `plan_whole_model_spotting` output of the `ds` binary, applied by the same
//! binary; the tampered, foreign and truncated variants prove the gates.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use ds_grid_exchange::package::{AssetBytes, PackOptions, pack, unpack};
use ds_grid_model::capacity::{
    AngleConvention, AngleInterpolationPolicy, BoundSlot, CapacityArtifactRow, CapacityBasisRow,
    CapacityCaseBindingRow, CapacityFamilyKind, CapacityOrigin, CapacityVerification,
    CaseBindingResolution, OutsideDomainPolicy, SpanLimitAngleKnotRow, SpanLimitSetRow,
    SpanLimitWeightCaseSlotRow, SpanLimitWeightMaximumRow, WeightSpanBasis,
};
use ds_grid_model::tables::{
    AlignmentRow, AnalysisCaseRow, AttachmentPointRow, AttachmentSemantic, AuthoredSag,
    AvailableStructureRow, CableCondition, CableCurveRow, CableRow, CaseBindingRole,
    CriterionCaseBindingRow, CriterionQuantity, CriterionRuleRow, CriterionSetRow, DesignPolicyRow,
    ResourceRow, RouteEdgeRow, RouteNodeRole, RouteNodeRow, SolverCapability, StrandFamily,
    StructureDuty, StructureDutyProfileRow, StructureMaterialClass, StructureRow, StructureTypeRow,
    TensionSectionRow, TensionSectionSupportRow, TerrainPointRow, WeatherStateRow,
    WindDirectionPolicy,
};
use ds_grid_model::{EntityId, GridModelSnapshot, ModelCrs, SpottingSettings};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const LINE_M: f64 = 400.0;
const GROUND_Z_M: f64 = 100.0;
/// With a 2000 m catenary constant a 400 m span sags 10.008 m and leaves
/// 4.86 m over flat ground against a 6 m requirement: the line needs supports.
const ATTACHMENT_Z_M: f64 = 14.868_336_111_607;

fn id<T: TryFrom<String>>(raw: &str) -> T
where
    T::Error: std::fmt::Debug,
{
    T::try_from(raw.to_string()).unwrap()
}

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

/// The exact bytes an embedded definition resource carries in the package.
fn resource_bytes(raw: &str) -> Vec<u8> {
    format!("{raw} definition").into_bytes()
}

fn resource(raw: &str) -> ResourceRow {
    let bytes = resource_bytes(raw);
    ResourceRow {
        id: id(raw),
        invariant_leaf: format!("{raw}.def"),
        media: "application/octet-stream".to_string(),
        content_digest: format!("sha256:{}", sha256_hex(&bytes)),
        byte_len: bytes.len() as u64,
        provider: ds_grid_model::ResourceProvider::Embedded,
    }
}

/// One structure type: its conductor attachment and a verified span-limit
/// capacity table admitting 300 m wind spans from 0 to 0.6 rad.
fn push_type(snapshot: &mut GridModelSnapshot, name: &str, content_digest: &str) {
    let (type_id, resource_id) = (format!("st-{name}"), format!("res-{name}"));
    snapshot.structure_types.push(StructureTypeRow {
        id: id(&type_id),
        engineering_name: format!("{name} pole"),
        description: None,
        height_m: Some(16.0),
        resource_id: Some(id(&resource_id)),
    });
    snapshot.attachment_points.push(AttachmentPointRow {
        id: id(&format!("ap-{name}")),
        structure_type_id: id(&type_id),
        set_label: "conductor".to_string(),
        slot: 0,
        local_x_m: 0.0,
        local_y_m: 0.0,
        local_z_m: ATTACHMENT_Z_M,
        orientation_rad: None,
        semantic: AttachmentSemantic::Phase,
        device_kind: None,
        dead_end: None,
    });
    let (basis, artifact, set) = (
        format!("cbasis-{name}"),
        format!("ca-{name}"),
        format!("sls-{name}"),
    );
    snapshot.capacity_bases.push(CapacityBasisRow {
        id: id(&basis),
        angle_convention: AngleConvention::Magnitude,
        weight_span_basis: WeightSpanBasis::Conventional,
        generator: None,
    });
    snapshot.capacity_artifacts.push(CapacityArtifactRow {
        id: id(&artifact),
        structure_type_id: id(&type_id),
        realization_digest: content_digest.to_string(),
        capacity_basis_id: id(&basis),
        family: CapacityFamilyKind::SpanLimits,
        family_schema_version: 1,
        origin: CapacityOrigin::Authored,
        verification: CapacityVerification::Verified,
    });
    snapshot.span_limit_sets.push(SpanLimitSetRow {
        id: id(&set),
        capacity_artifact_id: id(&artifact),
        angle_policy: AngleInterpolationPolicy::PiecewiseLinear,
        outside_domain: OutsideDomainPolicy::Reject,
    });
    let slot = format!("{set}-slot-0");
    snapshot
        .span_limit_weight_case_slots
        .push(SpanLimitWeightCaseSlotRow {
            id: id(&slot),
            set_id: id(&set),
            sequence: 0,
        });
    snapshot
        .capacity_case_bindings
        .push(CapacityCaseBindingRow {
            id: id(&format!("{set}-bind-0")),
            capacity_artifact_id: id(&artifact),
            bound_slot: BoundSlot::WeightCase { slot_id: id(&slot) },
            source_identity: None,
            resolution: CaseBindingResolution::Resolved {
                analysis_case_id: id("ac-weight"),
                wind_sign: None,
            },
            evidence: None,
        });
    for (sequence, line_angle_rad) in [(0_u32, 0.0_f64), (1, 0.6)] {
        let knot = format!("{set}-knot-{sequence}");
        snapshot.span_limit_angle_knots.push(SpanLimitAngleKnotRow {
            id: id(&knot),
            set_id: id(&set),
            sequence,
            line_angle_rad,
            allowable_wind_span_m: 300.0,
            minimum_signed_weight_span_m: -1_000.0,
        });
        snapshot
            .span_limit_weight_maxima
            .push(SpanLimitWeightMaximumRow {
                angle_knot_id: id(&knot),
                weight_case_slot_id: id(&slot),
                maximum_signed_weight_span_m: 1_000.0,
            });
    }
}

fn existing_structure(raw: &str, station_m: f64) -> StructureRow {
    StructureRow {
        staking: Default::default(),
        id: id(raw),
        structure_type_id: id("st-heavy"),
        network_role: Default::default(),
        engineering_number: Some(raw.to_string()),
        network_name: None,
        description: None,
        alignment_id: Some(id("al-line")),
        station_m: Some(station_m),
        profile_offset_m: Some(0.0),
        x_m: station_m,
        y_m: 0.0,
        z_m: GROUND_Z_M,
        orientation_rad: 0.0,
        additional_alignment_placements: Vec::new(),
    }
}

fn duty_profile(sequence: u32, name: &str, duties: Vec<StructureDuty>) -> StructureDutyProfileRow {
    StructureDutyProfileRow {
        policy_id: id("policy-spotting"),
        sequence,
        structure_resource_id: id(&format!("res-{name}")),
        family: name.to_string(),
        material: StructureMaterialClass::Wood,
        portal_configuration: name == "heavy",
        duties,
        preferred_span_upper_m: None,
        review_span_upper_m: None,
        verification: CapacityVerification::Verified,
        evidence: Some("test policy".to_string()),
    }
}

fn spotting_model() -> GridModelSnapshot {
    let mut snapshot = GridModelSnapshot::default();
    snapshot.alignments.push(AlignmentRow {
        ground_profile_basis: None,
        id: id("al-line"),
        parent_id: None,
        label: "Spotting line".to_string(),
        terrain_corridor_half_width_m: Some(15.0),
        route_buffer_half_width_m: None,
        terrain_gap_tolerance_m: Some(25.0),
        survey_note: None,
        delivery_phase: None,
        global_station_gap_m: None,
        sequence: None,
    });
    for (raw, x_m) in [("nd-0", 0.0), ("nd-1", LINE_M)] {
        snapshot.route_nodes.push(RouteNodeRow {
            id: id(raw),
            x_m,
            y_m: 0.0,
            z_m: GROUND_Z_M,
            role: RouteNodeRole::Terminus,
            associated_structure_id: None,
        });
    }
    snapshot.route_edges.push(RouteEdgeRow {
        id: id("ed-0"),
        alignment_id: id("al-line"),
        sequence: 0,
        from_node_id: id("nd-0"),
        to_node_id: id("nd-1"),
        length_m: LINE_M,
    });
    for index in 0..=40 {
        snapshot.terrain_points.push(TerrainPointRow {
            id: id(&format!("tp-{index}")),
            x_m: index as f64 * 10.0,
            y_m: 0.0,
            z_m: GROUND_Z_M,
            feature_class: "GP".to_string(),
            description: None,
            required_clearance_m: None,
            source_id: None,
        });
    }
    snapshot.cables.push(CableRow {
        id: id("cb-1"),
        engineering_name: "Spotting conductor".to_string(),
        description: None,
        diameter_m: 0.011_7,
        cross_section_m2: 0.000_081_3,
        mass_per_length_kg_per_m: 0.276_738,
        rated_strength_n: 26_270.0,
        nominal_elastic_modulus_pa: Some(69.9e9),
        reference_temperature_c: 20.0,
        creep_model: None,
        creep_temperature_shift_c: None,
        creep_shift_tension_n: None,
        resource_id: None,
    });
    snapshot.cable_curves.push(CableCurveRow {
        cable_id: id("cb-1"),
        family: StrandFamily::Outer,
        initial_stress_strain_pa: vec![0.0, 60.0e9, -1.2e12],
        creep_stress_strain_pa: vec![0.0, 55.0e9],
        final_modulus_pa: Some(65.0e9),
        thermal_expansion_per_k: 18.9e-6,
    });
    snapshot.weather_states.push(WeatherStateRow {
        id: id("ws-ref"),
        label: "Reference".to_string(),
        temperature_c: 20.0,
        wind_pressure_pa: 400.0,
        ice_thickness_m: 0.0,
        ice_density_kg_per_m3: 0.0,
        resultant_adder_n_per_m: 0.0,
    });
    for (raw, label) in [
        ("ac-wind", "Maximum wind span"),
        ("ac-weight", "Maximum weight span"),
        ("ac-uplift", "Minimum weight"),
    ] {
        snapshot.analysis_cases.push(AnalysisCaseRow {
            id: id(raw),
            label: label.to_string(),
            weather_state_id: id("ws-ref"),
            condition: CableCondition::Initial,
            wind_direction: WindDirectionPolicy::TransverseBothSigns,
            wire_load_factor: 1.0,
            structure_load_factor: None,
            solver: SolverCapability::RulingSpanCableState,
        });
    }
    snapshot.criterion_sets.push(CriterionSetRow {
        id: id("cs-1"),
        label: "Spotting criteria".to_string(),
    });
    for (raw, role, slot_ordinal, case) in [
        ("ccb-wind", CaseBindingRole::MaximumWind, None, "ac-wind"),
        (
            "ccb-weight",
            CaseBindingRole::MaximumWeightSlot,
            Some(0),
            "ac-weight",
        ),
        (
            "ccb-uplift",
            CaseBindingRole::MinimumWeight,
            None,
            "ac-uplift",
        ),
    ] {
        snapshot
            .criterion_case_bindings
            .push(CriterionCaseBindingRow {
                id: id(raw),
                set_id: id("cs-1"),
                role,
                slot_ordinal,
                analysis_case_id: id(case),
            });
    }
    snapshot.criterion_rules.push(CriterionRuleRow {
        id: id("cr-clearance"),
        set_id: id("cs-1"),
        quantity: CriterionQuantity::MinimumVerticalClearanceM,
        condition: CableCondition::Initial,
        weather_state_id: Some(id("ws-ref")),
        cable_id: Some(id("cb-1")),
        value_si: 6.0,
    });
    snapshot.resources.push(resource("res-catalog"));
    let tangent = resource("res-tangent");
    let heavy = resource("res-heavy");
    let (tangent_digest, heavy_digest) =
        (tangent.content_digest.clone(), heavy.content_digest.clone());
    snapshot.resources.push(tangent);
    snapshot.resources.push(heavy);
    push_type(&mut snapshot, "tangent", &tangent_digest);
    push_type(&mut snapshot, "heavy", &heavy_digest);
    for (sequence, name, cost) in [(0_u32, "tangent", 100.0), (1, "heavy", 400.0)] {
        snapshot.available_structures.push(AvailableStructureRow {
            catalog_resource_id: id("res-catalog"),
            sequence,
            structure_resource_id: id(&format!("res-{name}")),
            cost_for_optimization: cost,
            use_for_automatic_spotting: true,
            automatic_spotting_min_line_angle_rad: 0.0,
            automatic_spotting_set: 1,
            automatic_spotting_max_line_angle_rad: None,
            uncharacterized_i32_05: 0,
            uncharacterized_f64_06: 0.0,
            uncharacterized_f64_07: 0.0,
            uncharacterized_f64_08: 0.0,
            uncharacterized_f64_09: 0.0,
            uncharacterized_i32_10: 0,
            uncharacterized_f64_11: 0.0,
            uncharacterized_f64_12: 0.0,
        });
    }
    // The existing line: two dead-end structures at the termini, strung once.
    snapshot
        .structures
        .push(existing_structure("str-start", 0.0));
    snapshot
        .structures
        .push(existing_structure("str-end", LINE_M));
    snapshot.tension_sections.push(TensionSectionRow {
        id: id("ts-basis"),
        cable_id: id("cb-1"),
        criterion_set_id: Some(id("cs-1")),
        sag: AuthoredSag::CatenaryConstant { c_m: 2_000.0 },
        sag_condition: Some(CableCondition::Initial),
        sag_weather_state_id: Some(id("ws-ref")),
        ruling_span_m: None,
        details: Default::default(),
    });
    for (sequence, structure) in ["str-start", "str-end"].into_iter().enumerate() {
        snapshot
            .tension_section_supports
            .push(TensionSectionSupportRow {
                section_id: id("ts-basis"),
                sequence: sequence as u32,
                structure_id: id(structure),
                attachment_point_id: Some(id("ap-heavy")),
                insulator_model: None,
            });
    }
    let settings: SpottingSettings = serde_json::from_value(json!({
        "design_policy_id": "policy-spotting",
        "criterion_set_id": "cs-1",
        "station_step_m": 50.0,
        "max_reported_rejections": 64,
        "conductor": {"default_set_label": "conductor"}
    }))
    .unwrap();
    snapshot.design_policies.push(DesignPolicyRow {
        transformer_protection_threshold_kva: Default::default(),
        forbid_uplift_on_inline_suspension: true,
        spotting_settings: Some(Box::new(settings)),
        id: id("policy-spotting"),
        label: "Spotting test policy".to_string(),
        release_identity: "spotting-test/r1".to_string(),
        content_digest: digest('7'),
        applicability: "test line".to_string(),
        verification: CapacityVerification::Verified,
        max_consecutive_suspension_structures: None,
        maximum_support_move_m: None,
        permitted_refinements: Vec::new(),
        source_note: None,
        max_consecutive_wood_structures: None,
        max_spans_between_strong_structures: None,
        strong_structure_materials: Vec::new(),
    });
    snapshot.structure_duty_profiles.push(duty_profile(
        0,
        "tangent",
        vec![
            StructureDuty::InlineSuspension,
            StructureDuty::RunningAngle,
            StructureDuty::Terminal,
        ],
    ));
    snapshot.structure_duty_profiles.push(duty_profile(
        1,
        "heavy",
        vec![
            StructureDuty::InlineSuspension,
            StructureDuty::RunningAngle,
            StructureDuty::StrainDeadEnd,
            StructureDuty::Terminal,
            StructureDuty::Tap,
        ],
    ));
    snapshot
}

fn write_model(path: &Path) {
    let bytes = pack(
        &spotting_model(),
        &PackOptions {
            model_id: EntityId::new("spotting-receipt-model").unwrap(),
            model_revision: 1,
            presentation: Default::default(),
            coordinate_system: ModelCrs::new("EPSG:32735").unwrap(),
            library_pins: vec![],
            library_needs: vec![],
            assets: ["res-catalog", "res-tangent", "res-heavy"]
                .into_iter()
                .map(|raw| AssetBytes {
                    invariant_leaf: format!("{raw}.def"),
                    bytes: resource_bytes(raw),
                })
                .collect(),
            exchange_bindings: BTreeMap::new(),
        },
    )
    .unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Write `receipt` beside the model and return its path and SHA-256.
fn write_receipt(dir: &Path, name: &str, receipt: &Value) -> (String, String) {
    let path = dir.join(name);
    let bytes = serde_json::to_vec(receipt).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    (path.display().to_string(), sha256_hex(&bytes))
}

fn apply_receipt(model: &str, receipt: &str, sha: &str, out: Option<&str>) -> (Value, i32) {
    let mut args = vec![
        "dsgrid",
        "spotting",
        "apply-receipt",
        "--model",
        model,
        "--receipt",
        receipt,
        "--receipt-sha256",
        sha,
        "--output",
        "json",
    ];
    match out {
        Some(out) => args.extend(["--out", out]),
        None => args.push("--dry-run"),
    }
    common::json(&args)
}

#[test]
fn a_whole_model_spotting_receipt_applies_without_planning_again() {
    let dir = tempfile::tempdir().unwrap();
    let model_path = dir.path().join("line.dsgrid");
    write_model(&model_path);
    let model = text(&model_path);

    // The receipt is exactly what `ds dsgrid run` writes for the proposal.
    let planned = common::invoke(&[
        "dsgrid",
        "run",
        "--model",
        model,
        "--operation",
        "plan_whole_model_spotting",
        "--output",
        "json",
    ]);
    assert_eq!(planned.code, 0, "{}", planned.stdout);
    assert!(
        planned.stderr.trim().is_empty(),
        "no progress unless asked: {}",
        planned.stderr
    );

    // Asked for, native progress streams on stderr as JSON lines and the
    // sealed plans on stdout are unchanged.
    let observed = common::invoke(&[
        "dsgrid",
        "run",
        "--model",
        model,
        "--operation",
        "plan_whole_model_spotting",
        "--progress",
        "--output",
        "json",
    ]);
    assert_eq!(observed.code, 0, "{}", observed.stdout);
    // The host memory budget is read live, so compare the sealed plans: the
    // answer the apply door verifies.
    let sealed = |stdout: &str| {
        let receipt: Value = serde_json::from_str(stdout).unwrap();
        receipt["data"]["result"]["batch"]["items"].clone()
    };
    assert_eq!(sealed(&observed.stdout), sealed(&planned.stdout));
    let events: Vec<Value> = observed
        .stderr
        .lines()
        .map(|line| serde_json::from_str(line).expect("each progress line is JSON"))
        .collect();
    assert!(!events.is_empty());
    for event in &events {
        assert_eq!(
            event["progress"]["run"]["operation_id"],
            "plan_whole_model_spotting"
        );
        assert!(event["elapsed_ms"].is_u64(), "{event}");
        assert!(event["eta"].is_object(), "{event}");
    }
    let stages: Vec<&str> = events
        .iter()
        .filter_map(|event| event["progress"]["stage"].as_str())
        .collect();
    assert!(stages.contains(&"search"), "{stages:?}");
    assert_eq!(stages.last(), Some(&"finished"), "{stages:?}");

    let receipt_path = dir.path().join("whole-model.json");
    std::fs::write(&receipt_path, planned.stdout.as_bytes()).unwrap();
    let receipt_sha = sha256_hex(planned.stdout.as_bytes());
    let receipt: Value = serde_json::from_str(&planned.stdout).unwrap();
    let batch = &receipt["data"]["result"]["batch"];
    assert_eq!(batch["requested_alignments"], 1, "{batch}");
    assert_eq!(batch["completed_plans"], 1, "{batch}");
    let plan = &batch["items"][0]["plan"];
    let plan_digest = plan["plan_digest"].as_str().unwrap().to_string();
    let proposed = plan["structures"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|structure| structure["fixed_existing"] != json!(true))
        .count();
    assert_ne!(plan["status"], "infeasible", "{}", plan["infeasibility"]);
    assert!(
        plan["spans"].as_array().unwrap().len() >= 2,
        "a 400 m span cannot clear 6 m, so the line takes supports: {plan}"
    );

    // Every gate, no write.
    let (dry, code) = apply_receipt(model, text(&receipt_path), &receipt_sha, None);
    assert_eq!(code, 0, "{dry}");
    let data = &dry["data"];
    assert_eq!(data["verification_level"], "digest_verified_plans");
    assert_eq!(data["would_apply"], true, "{data}");
    assert_eq!(data["proposal"]["plan_digests"], json!([plan_digest]));
    assert_eq!(data["proposal"]["alignment_count"], 1);
    assert!(
        data["proposal"]["proposed_structure_count"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert_eq!(data["receipt"]["sha256"], receipt_sha);
    assert!(!dir.path().join("spotted.dsgrid").exists());

    // The write: one new revision, the base package untouched.
    let before = std::fs::read(&model_path).unwrap();
    let out_path = dir.path().join("spotted.dsgrid");
    let (applied, code) = apply_receipt(
        model,
        text(&receipt_path),
        &receipt_sha,
        Some(text(&out_path)),
    );
    assert_eq!(code, 0, "{applied}");
    assert_eq!(std::fs::read(&model_path).unwrap(), before);
    let artifact = &applied["data"]["artifact"];
    let written = std::fs::read(&out_path).unwrap();
    assert_eq!(artifact["sha256"], sha256_hex(&written));
    assert_eq!(
        applied["data"]["resulting_revision"],
        dry["data"]["resulting_revision"]
    );
    let spotted = unpack(&written).unwrap().snapshot;
    let base = unpack(&before).unwrap().snapshot;
    assert!(
        spotted.structures.len() > base.structures.len(),
        "the planned supports stand in the new revision"
    );
    assert!(proposed >= 1 || spotted.structures.len() > base.structures.len());
    // Every section the layout strung carries the basis's criterion set.
    assert!(
        spotted.tension_sections.iter().all(|section| section
            .criterion_set_id
            .as_ref()
            .map(|id| id.as_str())
            == Some("cs-1"))
    );

    // A foreign package is refused before any plan is read.
    let (refused, code) =
        apply_receipt(&common::fixture(), text(&receipt_path), &receipt_sha, None);
    assert_ne!(code, 0);
    assert_eq!(
        refused["error"]["code"], "receipt_model_mismatch",
        "{refused}"
    );

    // A receipt whose bytes moved is refused by its pin.
    let (refused, code) = apply_receipt(model, text(&receipt_path), &"0".repeat(64), None);
    assert_ne!(code, 0);
    assert_eq!(
        refused["error"]["code"], "receipt_digest_mismatch",
        "{refused}"
    );

    // An edited plan is re-pinned by the caller but refused by its digest.
    let mut edited = receipt.clone();
    edited["data"]["result"]["batch"]["items"][0]["plan"]["objective"]["total_cost_for_optimization"] =
        json!(1.0);
    let (edited_path, edited_sha) = write_receipt(dir.path(), "edited.json", &edited);
    let (refused, code) = apply_receipt(model, &edited_path, &edited_sha, None);
    assert_ne!(code, 0);
    assert_eq!(
        refused["error"]["code"], "spotting_application_refused",
        "{refused}"
    );
    assert!(
        refused["error"]["detail"]["engine"]
            .as_str()
            .unwrap()
            .contains("digest"),
        "{refused}"
    );

    // A receipt that withholds rows can only be previewed.
    let mut truncated = receipt.clone();
    truncated["data"]["more"] = json!({"truncated": [{
        "field": "result.batch.items[0].plan.rejected.rows",
        "total": 10, "shown": 5, "withheld": 5, "limit": 5
    }]});
    let (truncated_path, truncated_sha) = write_receipt(dir.path(), "truncated.json", &truncated);
    let (refused, code) = apply_receipt(model, &truncated_path, &truncated_sha, None);
    assert_ne!(code, 0);
    assert_eq!(refused["error"]["code"], "receipt_truncated", "{refused}");
}
