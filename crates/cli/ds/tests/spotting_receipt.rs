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
use ds_grid_model::{EntityId, ExchangeBindingRow, GridModelSnapshot, ModelCrs, SpottingSettings};
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

/// Four physical wires use two native section groups: three phase slots and
/// one independent slot. The middle suspension is not a section boundary until
/// the native retype changes its attachment duty.
fn write_binding_model(path: &Path, bindings: bool) {
    let mut snapshot = spotting_model();
    for point in &mut snapshot.attachment_points {
        point.dead_end = Some(point.structure_type_id.as_str() == "st-heavy");
    }
    for name in ["tangent", "heavy"] {
        let prototype = snapshot
            .attachment_points
            .iter()
            .find(|point| point.id.as_str() == format!("ap-{name}"))
            .unwrap()
            .clone();
        for slot in 1..=3 {
            let mut point = prototype.clone();
            point.id = id(&format!("ap-{name}-{slot}"));
            point.slot = slot;
            snapshot.attachment_points.push(point);
        }
    }
    let mut same_class = snapshot
        .structure_types
        .iter()
        .find(|row| row.id.as_str() == "st-tangent")
        .unwrap()
        .clone();
    same_class.id = id("st-tangent-copy");
    same_class.engineering_name = "other tangent pole".into();
    snapshot.structure_types.push(same_class);
    let points = snapshot
        .attachment_points
        .iter()
        .filter(|row| row.structure_type_id.as_str() == "st-tangent")
        .cloned()
        .collect::<Vec<_>>();
    for mut point in points {
        point.id = id(&format!("{}-copy", point.id.as_str()));
        point.structure_type_id = id("st-tangent-copy");
        snapshot.attachment_points.push(point);
    }
    let mut middle = existing_structure("str-middle", LINE_M / 2.0);
    middle.structure_type_id = id("st-tangent");
    snapshot.structures.push(middle);
    snapshot.tension_section_supports[1].sequence = 2;
    snapshot
        .tension_section_supports
        .push(TensionSectionSupportRow {
            section_id: id("ts-basis"),
            sequence: 1,
            structure_id: id("str-middle"),
            attachment_point_id: Some(id("ap-tangent")),
            insulator_model: None,
        });
    let section = snapshot.tension_sections[0].clone();
    for slot in 1..=3 {
        let mut row = section.clone();
        row.id = id(&format!("ts-basis-{slot}"));
        snapshot.tension_sections.push(row.clone());
        for (sequence, structure) in ["str-start", "str-end"].into_iter().enumerate() {
            snapshot
                .tension_section_supports
                .push(TensionSectionSupportRow {
                    section_id: row.id.clone(),
                    sequence: sequence as u32 * 2,
                    structure_id: id(structure),
                    attachment_point_id: Some(id(&format!("ap-heavy-{slot}"))),
                    insulator_model: None,
                });
        }
        snapshot
            .tension_section_supports
            .push(TensionSectionSupportRow {
                section_id: row.id,
                sequence: 1,
                structure_id: id("str-middle"),
                attachment_point_id: Some(id(&format!("ap-tangent-{slot}"))),
                insulator_model: None,
            });
    }
    let mut rows = Vec::new();
    if bindings {
        for slot in 0..=3 {
            let entity = if slot == 0 {
                "ts-basis".to_owned()
            } else {
                format!("ts-basis-{slot}")
            };
            let group = if slot == 3 { 8 } else { 7 };
            let group_slot = if slot == 3 { 0 } else { slot };
            let mut row = |namespace: &str, key: String| {
                rows.push(ExchangeBindingRow {
                    entity_id: id(&entity),
                    system: "pls_cadd".into(),
                    namespace: namespace.into(),
                    key,
                    raw: None,
                })
            };
            row("section_group_slot", format!("{group}:{group_slot}"));
            if slot == 0 || slot == 3 {
                row("section_number", group.to_string());
            }
            row("section_native_sag_token_1", "146.782".into());
            row("section_display_catenary_constant_m", "530.801".into());
        }
    }
    let bytes = pack(
        &snapshot,
        &PackOptions {
            model_id: id("binding-model"),
            model_revision: 31,
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
            exchange_bindings: if bindings {
                BTreeMap::from([("pls_cadd".into(), rows)])
            } else {
                BTreeMap::new()
            },
        },
    )
    .unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn prove_binding_split(path: &str, split: bool) {
    use ds_grid_engine::{CommandEnvelope, GridCommand, GridSession};
    use ds_grid_exchange::carry_forward_pls_section_bindings;
    for bound in [true, false] {
        let target = if split { "st-heavy" } else { "st-tangent-copy" };
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("source.dsgrid");
        let output = dir.path().join("result.dsgrid");
        write_binding_model(&model, bound);
        let original = std::fs::read(&model).unwrap();
        let package = unpack(&original).unwrap();
        let mut native = GridSession::open(package.snapshot.clone());
        let envelope = CommandEnvelope::new(
            "binding-split",
            native.current_revision().revision_id.clone(),
            GridCommand::RetypeStructure {
                id: id("str-middle"),
                structure_type_id: id(target),
            },
        );
        let outcome = native.apply_command(envelope.clone()).unwrap();
        assert_eq!(
            outcome.delta.tension_section_splits.len(),
            if split { 4 } else { 0 },
            "actual native splitting command"
        );
        let mut expected_bindings = package.exchange_bindings.clone();
        if let Some(rows) = expected_bindings.get_mut("pls_cadd") {
            let report = carry_forward_pls_section_bindings(rows, &[outcome.delta]).unwrap();
            assert_eq!(report.transfers.len(), if split { 6 } else { 0 });
            assert_eq!(
                report.invalidated_display_caches.len(),
                if split { 8 } else { 0 }
            );
            assert_eq!(rows.len(), if split { 6 } else { 14 });
        }
        let checkpoint = native.save_checkpoint().unwrap();
        let expected = pack(
            &checkpoint.snapshot,
            &PackOptions {
                model_id: package.manifest.model.model_id.clone(),
                model_revision: 32,
                presentation: package.manifest.model.presentation.clone(),
                coordinate_system: package.manifest.model.coordinate_system.clone(),
                library_pins: package.manifest.model.library_pins.clone(),
                library_needs: package.manifest.model.library_needs.clone(),
                assets: package.assets.clone(),
                exchange_bindings: expected_bindings,
            },
        )
        .unwrap();
        let commands = dir.path().join("commands.json");
        let payload = if path == "batch" {
            json!({"expected_revision": envelope.expected_revision, "commands": [{"command_id": envelope.command_id, "command": envelope.command}]})
        } else {
            serde_json::to_value(&envelope).unwrap()
        };
        std::fs::write(&commands, serde_json::to_vec(&payload).unwrap()).unwrap();
        let mut args = match path {
            "batch" => vec![
                "dsgrid",
                "apply-batch",
                "--model",
                text(&model),
                "--batch",
                text(&commands),
            ],
            "single" => vec![
                "dsgrid",
                "apply",
                "--model",
                text(&model),
                "--envelope",
                text(&commands),
            ],
            _ => vec![
                "dsgrid",
                "structure",
                "retype",
                "--package",
                text(&model),
                "--structure",
                "str-middle",
                "--type",
                target,
                "--yes",
            ],
        };
        args.extend(["--out", text(&output), "--output", "json"]);
        let (result, code) = common::json(&args);
        assert_eq!(code, 0, "{path}: {result}");
        assert_eq!(std::fs::read(&model).unwrap(), original);
        let actual = std::fs::read(&output).unwrap();
        assert!(
            actual == expected,
            "{path}: matched native checkpoint cadence; actual {}B/{} expected {}B/{}",
            actual.len(),
            sha256_hex(&actual),
            expected.len(),
            sha256_hex(&expected)
        );
    }
}

fn prove_binding_refusal(path: &str) {
    for dry in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("source.dsgrid");
        let output = dir.path().join("result.dsgrid");
        write_binding_model(&model, true);
        let mut package = unpack(&std::fs::read(&model).unwrap()).unwrap();
        package
            .exchange_bindings
            .get_mut("pls_cadd")
            .unwrap()
            .retain(|row| {
                !(row.entity_id.as_str() == "ts-basis"
                    && row.namespace == "section_native_sag_token_1")
            });
        let source = pack(
            &package.snapshot,
            &PackOptions {
                model_id: package.manifest.model.model_id,
                model_revision: 31,
                presentation: package.manifest.model.presentation,
                coordinate_system: package.manifest.model.coordinate_system,
                library_pins: package.manifest.model.library_pins,
                library_needs: package.manifest.model.library_needs,
                assets: package.assets,
                exchange_bindings: package.exchange_bindings,
            },
        )
        .unwrap();
        std::fs::write(&model, &source).unwrap();
        let session = ds_grid_engine::GridSession::open(package.snapshot);
        let envelope = ds_grid_engine::CommandEnvelope::new(
            "binding-refusal",
            session.current_revision().revision_id.clone(),
            ds_grid_engine::GridCommand::RetypeStructure {
                id: id("str-middle"),
                structure_type_id: id("st-heavy"),
            },
        );
        let commands = dir.path().join("commands.json");
        std::fs::write(&commands, serde_json::to_vec(&if path == "batch" {
                json!({"expected_revision": envelope.expected_revision, "commands": [{"command_id": envelope.command_id, "command": envelope.command}]})
            } else { serde_json::to_value(envelope).unwrap() }).unwrap()).unwrap();
        let mut args = match path {
            "batch" => vec![
                "dsgrid",
                "apply-batch",
                "--model",
                text(&model),
                "--batch",
                text(&commands),
            ],
            "single" => vec![
                "dsgrid",
                "apply",
                "--model",
                text(&model),
                "--envelope",
                text(&commands),
            ],
            _ => vec![
                "dsgrid",
                "structure",
                "retype",
                "--package",
                text(&model),
                "--structure",
                "str-middle",
                "--type",
                "st-heavy",
            ],
        };
        args.extend(["--out", text(&output), "--output", "json"]);
        if dry {
            args.push("--dry-run");
        } else if path == "typed" {
            args.push("--yes");
        }
        let (result, code) = common::json(&args);
        assert_ne!(code, 0, "{path}: {result}");
        assert_eq!(
            result["error"]["code"], "pls_section_identity_unresolved",
            "{path}: {result}"
        );
        assert_eq!(
            result["error"]["detail"]["reason"],
            "incomplete_native_display_cache"
        );
        assert!(!output.exists());
        assert_eq!(std::fs::read(&model).unwrap(), source);
    }
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn binding_emission_batch_split_matches_native_owner_exactly() {
    prove_binding_split("batch", true);
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn binding_emission_batch_refuses_incomplete_cache_before_dry_run_or_write() {
    prove_binding_refusal("batch");
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn binding_emission_single_split_matches_native_owner_exactly() {
    prove_binding_split("single", true);
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn binding_emission_single_refuses_incomplete_cache_before_dry_run_or_write() {
    prove_binding_refusal("single");
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn binding_emission_typed_split_matches_native_owner_exactly() {
    prove_binding_split("typed", true);
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn binding_emission_typed_refuses_incomplete_cache_before_dry_run_or_write() {
    prove_binding_refusal("typed");
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn binding_emission_same_class_preserves_every_binding_without_split_inference() {
    for path in ["batch", "single", "typed"] {
        prove_binding_split(path, false);
    }
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn binding_emission_spotting_receipt_matches_native_topology_and_retirement() {
    use ds_grid_engine::{
        CommandEnvelope, GridCommand, GridSession, SpottingLayoutApplyRequest,
        spotting_layout_application, spotting_layout_command_id,
    };
    use ds_grid_exchange::carry_forward_pls_section_bindings;
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("source.dsgrid");
    write_model(&model);
    let mut package = unpack(&std::fs::read(&model).unwrap()).unwrap();
    package.exchange_bindings.insert(
        "pls_cadd".into(),
        vec![
            ExchangeBindingRow {
                entity_id: id("ts-basis"),
                system: "pls_cadd".into(),
                namespace: "section_number".into(),
                key: "42".into(),
                raw: None,
            },
            ExchangeBindingRow {
                entity_id: id("ts-basis"),
                system: "pls_cadd".into(),
                namespace: "section_group_slot".into(),
                key: "42:0".into(),
                raw: None,
            },
        ],
    );
    let source = pack(
        &package.snapshot,
        &PackOptions {
            model_id: package.manifest.model.model_id.clone(),
            model_revision: 1,
            presentation: package.manifest.model.presentation.clone(),
            coordinate_system: package.manifest.model.coordinate_system.clone(),
            library_pins: package.manifest.model.library_pins.clone(),
            library_needs: package.manifest.model.library_needs.clone(),
            assets: package.assets.clone(),
            exchange_bindings: package.exchange_bindings.clone(),
        },
    )
    .unwrap();
    std::fs::write(&model, &source).unwrap();
    let planned = common::invoke(&[
        "dsgrid",
        "run",
        "--model",
        text(&model),
        "--operation",
        "plan_whole_model_spotting",
        "--output",
        "json",
    ]);
    assert_eq!(planned.code, 0, "{}", planned.stdout);
    let receipt: Value = serde_json::from_str(&planned.stdout).unwrap();
    let plans = receipt["data"]["result"]["batch"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| serde_json::from_value(item["plan"].clone()).unwrap())
        .collect();
    let request = SpottingLayoutApplyRequest {
        plans,
        fixed_structure_retypes: Default::default(),
        prior_warnings: Default::default(),
        warning_band_half_width_m: None,
    };
    let mut native = GridSession::open(package.snapshot.clone());
    let application =
        spotting_layout_application(native.snapshot(), native.current_revision(), &request)
            .unwrap();
    let outcome = native
        .apply_command(CommandEnvelope::new(
            spotting_layout_command_id(&request),
            native.current_revision().revision_id.clone(),
            GridCommand::ApplySpottingLayouts { application },
        ))
        .unwrap();
    let report = carry_forward_pls_section_bindings(
        package.exchange_bindings.get_mut("pls_cadd").unwrap(),
        std::slice::from_ref(&outcome.delta),
    )
    .unwrap();
    // Whole-layout replacement has its own native topology semantics. This
    // caller must not infer a split or invent lineage when its delta has none.
    assert!(outcome.delta.tension_section_splits.is_empty());
    assert!(report.transfers.is_empty());
    let checkpoint = native.save_checkpoint().unwrap();
    let expected = pack(
        &checkpoint.snapshot,
        &PackOptions {
            model_id: package.manifest.model.model_id,
            model_revision: 2,
            presentation: package.manifest.model.presentation,
            coordinate_system: package.manifest.model.coordinate_system,
            library_pins: package.manifest.model.library_pins,
            library_needs: package.manifest.model.library_needs,
            assets: package.assets,
            exchange_bindings: package.exchange_bindings,
        },
    )
    .unwrap();
    let receipt_path = dir.path().join("receipt.json");
    std::fs::write(&receipt_path, &planned.stdout).unwrap();
    let output = dir.path().join("result.dsgrid");
    let sha = sha256_hex(planned.stdout.as_bytes());
    let (result, code) =
        apply_receipt(text(&model), text(&receipt_path), &sha, Some(text(&output)));
    assert_eq!(code, 0, "{result}");
    assert_eq!(std::fs::read(&output).unwrap(), expected);
    assert_eq!(std::fs::read(&model).unwrap(), source);
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
#[ignore = "requires the explicitly selected compatible Network executable"]
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
