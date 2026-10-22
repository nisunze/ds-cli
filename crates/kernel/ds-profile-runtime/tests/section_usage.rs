//! Native source facts and actual display-selector acceptance, with no UI calculation.
#[path = "../../../../../ds-network/crates/ds-grid-engine/tests/common/mod.rs"]
mod common;
use ds_grid_engine::GridSession;
use ds_grid_model::*;
use ds_profile_runtime::Runtime;
mod support;
use serde_json::json;
fn model() -> GridModelSnapshot {
    let mut model = common::fixture();
    // A continuous surveyed surface makes the complete physical support chain drawable.
    for (i, (x, y, z)) in [(0., 0., 100.), (300., 0., 102.), (600., 40., 98.)]
        .into_iter()
        .enumerate()
    {
        model.terrain_points.push(TerrainPointRow {
            id: TerrainPointId::new(format!("display-ground-{i}")).unwrap(),
            x_m: x,
            y_m: y,
            z_m: z,
            feature_class: "ground".into(),
            description: None,
            required_clearance_m: None,
            source_id: None,
        });
    }
    model.tension_sections.push(TensionSectionRow {
        id: TensionSectionId::new("display-section").unwrap(),
        cable_id: CableId::new("cb-1").unwrap(),
        criterion_set_id: None,
        sag: AuthoredSag::HorizontalTension { tension_n: 20_000. },
        sag_condition: Some(CableCondition::Initial),
        sag_weather_state_id: Some(WeatherStateId::new("ws-everyday").unwrap()),
        ruling_span_m: Some(300.),
        details: Default::default(),
    });
    for (sequence, structure) in ["str-1", "str-2", "str-3"].into_iter().enumerate() {
        model
            .tension_section_supports
            .push(TensionSectionSupportRow {
                section_id: TensionSectionId::new("display-section").unwrap(),
                sequence: sequence as u32,
                structure_id: StructureId::new(structure).unwrap(),
                attachment_point_id: Some(AttachmentPointId::new("ap-left").unwrap()),
                insulator_model: None,
            });
    }
    let template = model.analysis_cases[0].clone();
    // Keep the original criterion/capacity-bound cases; added display cases
    // cannot invalidate the packaged model or change its engineering envelope.
    for (id, temperature, ice) in [("hot", 60., 0.), ("ice", 20., 0.06)] {
        let weather_id = WeatherStateId::new(format!("weather-{id}")).unwrap();
        model.weather_states.push(WeatherStateRow {
            id: weather_id.clone(),
            label: if id == "hot" {
                "Hot conductor weather"
            } else {
                "Heavy ice weather"
            }
            .into(),
            temperature_c: temperature,
            wind_pressure_pa: 0.,
            ice_thickness_m: ice,
            ice_density_kg_per_m3: 900.,
            resultant_adder_n_per_m: 0.,
        });
        let mut case = template.clone();
        case.id = AnalysisCaseId::new(id).unwrap();
        case.label = if id == "hot" {
            "Maximum temperature display"
        } else {
            "Heavy ice display"
        }
        .into();
        case.weather_state_id = weather_id;
        model.analysis_cases.push(case);
    }
    model
}

#[test]
fn section_usage_label_reads_current_solver_basis_and_native_selector_controls_it() {
    let snapshot = model();
    let session = GridSession::open(snapshot.clone());
    let mut request = support::initialize(
        &session,
        "section-label-fixture",
        "section-labels",
        Default::default(),
        &[],
    );
    request.style_document = serde_json::from_str(include_str!(
        "../../../../../ds-brain/internal/services/profile_display_style.v1.json"
    ))
    .unwrap();
    request.patch = serde_json::from_value(json!({"visibility":{"section_labels":true}})).unwrap();
    let mut runtime = Runtime::default();
    let shown = runtime.execute(request.clone()).unwrap();
    let basis = shown["scene"]["evidence"]["section_basis"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["section_id"] == "display-section")
        .expect("native section basis");
    let percent = basis["maximum_support_tension_rts_percent"]
        .as_f64()
        .unwrap();
    let tension = basis["maximum_support_tension_n"].as_f64().unwrap();
    let rated = basis["rated_strength_n"].as_f64().unwrap();
    assert!((percent - tension / rated * 100.0).abs() < 1e-10);
    // The wires read the exact cable engineering name, never the opaque section id.
    let cable = shown["scene"]["conductor_spans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|span| span["section_id"] == "display-section")
        .expect("the fixture strings the section")["cable_engineering_name"]
        .as_str()
        .unwrap()
        .to_owned();
    let expected = format!("{cable} · Max support RTS {percent:.1}%");
    assert!(!expected.contains("display-section"));
    let labels = shown["scene"]["display"]["primitives"].as_array().unwrap();
    assert!(labels
        .iter()
        .any(|row| row["option"] == "section_labels" && row["labels"][0]["text"] == expected));
    request.patch = serde_json::from_value(json!({"visibility":{"section_labels":false}})).unwrap();
    let hidden = runtime
        .execute(support::refresh(&request, &shown, None))
        .unwrap();
    assert!(!hidden["scene"]["display"]["primitives"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["option"] == "section_labels"));
    assert_eq!(
        hidden["scene"]["evidence"]["section_basis"], shown["scene"]["evidence"]["section_basis"],
        "display switch cannot alter solver facts"
    );
}
