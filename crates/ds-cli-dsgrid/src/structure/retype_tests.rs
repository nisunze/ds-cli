//! Synthetic multiset stringing on a 45° bend; no project or production model.

use super::*;
use ds_cli_contract::{Format, Output, parse};
use ds_grid_engine::GridSession;
use ds_grid_exchange::{PackOptions, package::pack};
use ds_grid_model::*;

const SINGLE: &str = "l-w-stay-S255.012";
const REPLACEMENT: &str = "j-w-60d-S325.014";
const PEER: &str = "j-w-60d-S190.012";

fn type_id(name: &str) -> StructureTypeId {
    StructureTypeId::new(format!("st-{}", name.replace('.', "-"))).unwrap()
}

fn point(name: &str, label: &str, slot: u32) -> AttachmentPointRow {
    AttachmentPointRow {
        id: AttachmentPointId::new(format!("ap-{}-{label}-{slot}", name.replace('.', "-")))
            .unwrap(),
        structure_type_id: type_id(name),
        set_label: label.into(),
        slot,
        local_x_m: 0.0,
        local_y_m: 0.0,
        local_z_m: 10.0 - f64::from(slot) * 0.7,
        orientation_rad: None,
        semantic: AttachmentSemantic::DeadEnd,
        device_kind: None,
        dead_end: Some(true),
    }
}

fn fixture() -> GridModelSnapshot {
    let mut model = GridModelSnapshot::default();
    let alignment = AlignmentId::new("alignment-1").unwrap();
    model.alignments.push(AlignmentRow {
        id: alignment.clone(),
        parent_id: None,
        label: "Synthetic bend".into(),
        terrain_corridor_half_width_m: None,
        route_buffer_half_width_m: None,
        terrain_gap_tolerance_m: None,
        survey_note: None,
        delivery_phase: None,
        global_station_gap_m: None,
    });
    let leg = 100.0 / 2.0_f64.sqrt();
    for (id, x, y, role) in [
        ("n0", 0.0, 0.0, RouteNodeRole::Terminus),
        ("n1", 100.0, 0.0, RouteNodeRole::AnglePoint),
        ("n2", 100.0 + leg, leg, RouteNodeRole::Terminus),
    ] {
        model.route_nodes.push(RouteNodeRow {
            id: RouteNodeId::new(id).unwrap(),
            x_m: x,
            y_m: y,
            z_m: 0.0,
            role,
            associated_structure_id: None,
        });
    }
    for (sequence, from, to) in [(0, "n0", "n1"), (1, "n1", "n2")] {
        model.route_edges.push(RouteEdgeRow {
            id: RouteEdgeId::new(format!("edge-{sequence}")).unwrap(),
            alignment_id: alignment.clone(),
            sequence,
            from_node_id: RouteNodeId::new(from).unwrap(),
            to_node_id: RouteNodeId::new(to).unwrap(),
            length_m: 100.0,
        });
    }
    for name in [SINGLE, REPLACEMENT, PEER] {
        model.structure_types.push(StructureTypeRow {
            id: type_id(name),
            engineering_name: name.into(),
            description: None,
            height_m: Some(12.0),
            resource_id: None,
        });
    }
    for (id, name, station, x, y) in [
        ("str-1", SINGLE, 100.0, 100.0, 0.0),
        ("str-2", PEER, 200.0, 100.0 + leg, leg),
    ] {
        model.structures.push(StructureRow {
            id: StructureId::new(id).unwrap(),
            structure_type_id: type_id(name),
            network_role: StructureNetworkRole::Support,
            engineering_number: Some(id.trim_start_matches("str-").into()),
            network_name: None,
            description: None,
            alignment_id: Some(alignment.clone()),
            station_m: Some(station),
            profile_offset_m: Some(0.0),
            x_m: x,
            y_m: y,
            z_m: 0.0,
            orientation_rad: 0.0,
            additional_alignment_placements: Vec::new(),
            staking: Default::default(),
        });
    }
    model.cables.push(CableRow {
        id: CableId::new("cable-1").unwrap(),
        engineering_name: "Synthetic cable".into(),
        description: None,
        diameter_m: 0.01,
        cross_section_m2: 0.000_07,
        mass_per_length_kg_per_m: 0.3,
        rated_strength_n: 26_000.0,
        nominal_elastic_modulus_pa: Some(77.0e9),
        reference_temperature_c: 20.0,
        creep_model: None,
        creep_temperature_shift_c: None,
        creep_shift_tension_n: None,
        resource_id: None,
    });
    model.cable_curves.push(CableCurveRow {
        cable_id: CableId::new("cable-1").unwrap(),
        family: StrandFamily::Outer,
        initial_stress_strain_pa: vec![0.0, 70e9],
        creep_stress_strain_pa: vec![0.0, 60e9],
        final_modulus_pa: Some(65e9),
        thermal_expansion_per_k: 19e-6,
    });
    for (label, count) in [("phase-back", 3), ("phase-ahead", 3), ("GW", 1)] {
        for slot in 0..count {
            for name in [SINGLE, PEER] {
                model.attachment_points.push(point(name, label, slot));
            }
            // Only two used slots have exact matches on the chosen type.
            if label == "phase-back" && slot < 2 {
                model
                    .attachment_points
                    .push(point(REPLACEMENT, label, slot));
            }
            let section = TensionSectionId::new(format!("section-{label}-{slot}")).unwrap();
            model.tension_sections.push(TensionSectionRow {
                id: section.clone(),
                cable_id: CableId::new("cable-1").unwrap(),
                criterion_set_id: None,
                sag: AuthoredSag::HorizontalTension { tension_n: 4_000.0 },
                sag_condition: Some(CableCondition::Initial),
                sag_weather_state_id: None,
                ruling_span_m: None,
                details: Default::default(),
            });
            for (sequence, id, name) in [(0, "str-1", SINGLE), (1, "str-2", PEER)] {
                model
                    .tension_section_supports
                    .push(TensionSectionSupportRow {
                        section_id: section.clone(),
                        sequence,
                        structure_id: StructureId::new(id).unwrap(),
                        attachment_point_id: Some(point(name, label, slot).id),
                        insulator_model: Some("retained hardware".into()),
                    });
            }
        }
    }
    let session = GridSession::open(model);
    assert!(session.is_valid(), "{:?}", session.validation());
    session.snapshot().clone()
}

fn options() -> PackOptions {
    PackOptions {
        model_id: EntityId::new("synthetic-retype").unwrap(),
        model_revision: 1,
        presentation: Default::default(),
        coordinate_system: ModelCrs::new("EPSG:32736").unwrap(),
        library_pins: Vec::new(),
        library_needs: Vec::new(),
        assets: Vec::new(),
        exchange_bindings: Default::default(),
    }
}

fn context(writing: bool) -> Context {
    Context {
        confirmed: writing,
        output: Output {
            format: Format::Json,
            pretty: false,
            color: false,
        },
    }
}

fn inputs(
    path: &std::path::Path,
    out: &std::path::Path,
    revision: &str,
    name: &str,
    writing: bool,
) -> Inputs {
    parse(
        &COMMAND,
        &[
            "--package",
            path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--revision",
            revision,
            "--structure",
            "1",
            "--type",
            name,
            if writing { "--yes" } else { "--dry-run" },
        ]
        .map(str::to_owned),
    )
    .unwrap()
}

#[test]
fn retype_dry_run_and_failed_package_write_expose_exact_native_draft_delta() {
    let original = fixture();
    let mut session = GridSession::open(original.clone());
    let head = session.current_revision().revision_id.clone();
    let native = session
        .apply_transaction_at_head(
            head.clone(),
            vec![(
                "native-proof".into(),
                GridCommand::RetypeStructure {
                    id: StructureId::new("str-1").unwrap(),
                    structure_type_id: type_id(REPLACEMENT),
                },
            )],
        )
        .unwrap();
    let findings = &native.outcomes[0].delta.unresolved_retype_attachments;
    assert_eq!(findings.len(), 5);
    assert!(!session.is_valid());
    assert_eq!(
        session.snapshot().tension_sections,
        original.tension_sections
    );
    for (before, after) in original
        .tension_section_supports
        .iter()
        .zip(&session.snapshot().tension_section_supports)
    {
        let mut expected = before.clone();
        if before.structure_id.as_str() == "str-1" {
            let old = original
                .attachment_points
                .iter()
                .find(|p| Some(&p.id) == before.attachment_point_id.as_ref())
                .unwrap();
            expected.attachment_point_id = original
                .attachment_points
                .iter()
                .find(|p| {
                    p.structure_type_id == type_id(REPLACEMENT)
                        && p.set_label == old.set_label
                        && p.slot == old.slot
                })
                .map(|p| p.id.clone());
        }
        assert_eq!(
            after, &expected,
            "only native exact matches or unresolved attachments"
        );
    }
    assert!(matches!(
        pack(session.snapshot(), &options()),
        Err(ds_grid_exchange::PackageError::InvalidModel { .. })
    ));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.dsgrid");
    let out = dir.path().join("draft.dsgrid");
    let bytes = pack(&original, &options()).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let dry = run(
        &inputs(&path, &out, head.as_str(), REPLACEMENT, false),
        &context(false),
    )
    .unwrap();
    assert_eq!(
        dry["deltas"][0]["unresolved_retype_attachments"],
        json!(findings)
    );
    assert_eq!(dry["findings"]["cleared"].as_array().unwrap().len(), 1);
    assert_eq!(dry["refusal_on_write"], Value::Null);
    assert_eq!(dry["persisted"], false);
    assert_eq!(dry["warnings"][0]["code"], "unresolved_retype_attachments");
    let text = render(&dry);
    assert!(text.contains("full validation, package export and publication require repair"));
    for f in findings {
        assert!(text.contains(&format!(
            "section {} sequence {} structure {} previous attachment {} set {} slot {}",
            f.section_id,
            f.sequence,
            f.structure_id,
            f.previous_attachment_point_id,
            f.set_label,
            f.slot
        )));
    }
    let failure = run(
        &inputs(&path, &out, head.as_str(), REPLACEMENT, true),
        &context(true),
    )
    .unwrap_err();
    assert_eq!(failure.code(), "package_emit_failed");
    let written = &failure.detail_value().unwrap()["receipt"];
    assert_eq!(written["deltas"], dry["deltas"]);
    assert_eq!(written["warnings"], dry["warnings"]);
    assert_eq!(written["resulting_revision"], dry["resulting_revision"]);
    assert_eq!(written["dry_run"], false);
    assert_eq!(written["persisted"], false);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(!out.exists());
}

#[test]
fn retype_keeps_reg_revision_target_and_unrelated_validation_gates() {
    let mut model = fixture();
    // The replacement's matched set has an extra unstrung slot, an error
    // unrelated to the unmatched phase-ahead and GW attachments.
    for slot in 2..4 {
        model
            .attachment_points
            .push(point(REPLACEMENT, "phase-back", slot));
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.dsgrid");
    let out = dir.path().join("result.dsgrid");
    let bytes = pack(&model, &options()).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let head = GridSession::open(model)
        .current_revision()
        .revision_id
        .clone();
    for writing in [false, true] {
        let failure = run(
            &inputs(&path, &out, head.as_str(), REPLACEMENT, writing),
            &context(writing),
        )
        .unwrap_err();
        assert_eq!(failure.code(), "model_validation_failed");
        assert!(
            failure.detail_value().unwrap()["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["message"]
                    .as_str()
                    .unwrap()
                    .contains("complete set slots {0, 1, 2, 3}"))
        );
        let stale = run(
            &inputs(&path, &out, "stale-revision", REPLACEMENT, writing),
            &context(writing),
        )
        .unwrap_err();
        assert_eq!(stale.code(), "revision_conflict");
    }
    let reg = run(
        &inputs(&path, &out, head.as_str(), SINGLE, false),
        &context(false),
    )
    .unwrap();
    assert_eq!(reg["refusal_on_write"], FINDING_STRUCTURE_TYPE_NOT_ALLOWED);
    let failure = run(
        &inputs(&path, &out, head.as_str(), SINGLE, true),
        &context(true),
    )
    .unwrap_err();
    assert_eq!(failure.code(), FINDING_STRUCTURE_TYPE_NOT_ALLOWED);
    let both = parse(
        &COMMAND,
        &[
            "--package",
            path.to_str().unwrap(),
            "--model",
            "explicit-local-model",
            "--type",
            REPLACEMENT,
            "--dry-run",
        ]
        .map(str::to_owned),
    )
    .unwrap();
    assert_eq!(
        run(&both, &context(false)).unwrap_err().code(),
        "target_required"
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(!out.exists());
}

#[test]
fn retype_with_complete_slots_persists_without_draft_warning() {
    let mut model = fixture();
    for (label, count) in [("phase-back", 3), ("phase-ahead", 3), ("GW", 1)] {
        for slot in 0..count {
            if label != "phase-back" || slot == 2 {
                model
                    .attachment_points
                    .push(point(REPLACEMENT, label, slot));
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.dsgrid");
    let out = dir.path().join("result.dsgrid");
    let bytes = pack(&model, &options()).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let head = GridSession::open(model)
        .current_revision()
        .revision_id
        .clone();
    let dry = run(
        &inputs(&path, &out, head.as_str(), REPLACEMENT, false),
        &context(false),
    )
    .unwrap();
    let write = run(
        &inputs(&path, &out, head.as_str(), REPLACEMENT, true),
        &context(true),
    )
    .unwrap();
    assert_eq!(write["deltas"], dry["deltas"]);
    assert!(
        write["deltas"][0]
            .get("unresolved_retype_attachments")
            .is_none()
    );
    assert_eq!(write["warnings"], json!([]));
    assert_eq!(write["persisted"], true);
    let result =
        crate::package::decode(out.to_str().unwrap(), &std::fs::read(&out).unwrap()).unwrap();
    assert!(validate_snapshot(&result.snapshot).is_valid());
    assert_eq!(result.snapshot.tension_sections, fixture().tension_sections);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
