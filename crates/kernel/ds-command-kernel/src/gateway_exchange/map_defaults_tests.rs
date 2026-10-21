use super::*;
use crate::gateway_exchange::{resume, start, Facts, Method, Observed, Step};

fn answered<C: Exchange>(call: C, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status: 200,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    match resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn sent<C: Exchange>(call: C) -> Request {
    match start(call) {
        Step::Fetch { request, .. } => request,
        _ => panic!("expected a fetch"),
    }
}

#[test]
fn the_read_asks_for_the_config_and_admits_section_by_section() {
    let request = sent(Read);
    assert_eq!(
        (request.method, request.path.as_str(), request.timeout_ms),
        (Method::Post, "/api/v1/map_defaults", 120_000)
    );
    assert_eq!(request.body, Some(json!({"action": "get_config"})));

    let read = answered(
        Read,
        json!({"success": true, "data": {
            "hillshade": {"enabled": true, "exaggeration": 1.5},
            "scale_bar": {"enabled": true, "unit": "nautical"},
            "measure": {"line_color": "#ff0000", "line_width": "wide"},
            "gps_accuracy": {"warn_above_meters": 25},
            "camera": {"max_pitch": 60},
            "basemaps": [{"id": "osm"}]
        }}),
    )
    .unwrap();
    let defaults = MapDefaults::default();
    assert_eq!(
        read.hillshade,
        HillshadeDefaults {
            enabled: false,
            exaggeration: 1.5
        },
        "relief never starts enabled"
    );
    assert_eq!(
        read.scale_bar,
        ScaleBarDefaults {
            enabled: true,
            max_width: 120,
            unit: ScaleUnit::Nautical
        }
    );
    assert_eq!(
        read.measure, defaults.measure,
        "a section that does not fit is the default section"
    );
    assert_eq!(read.gps_accuracy.warn_above_meters, 25.0);
    assert!(read.gps_accuracy.survey_only);
    assert_eq!(read.zoom_level, defaults.zoom_level);
    assert_eq!(read.camera["max_pitch"], 60);
    assert_eq!(read.camera["default_pitch_3d"], 20);

    let none = answered(Read, json!({"success": true, "data": "map defaults"}));
    assert_eq!(none.unwrap_err().code, INVALID);
}

#[test]
fn a_publish_sends_the_document_without_a_basemap_catalogue() {
    let mut defaults = MapDefaults::default();
    defaults.hillshade.enabled = true;
    defaults.scale_bar.enabled = true;
    defaults.camera.insert("max_pitch".into(), json!(70));
    let request = sent(Publish {
        defaults: defaults.clone(),
    });
    let body = request.body.unwrap();
    assert_eq!(body["action"], "update");
    assert_eq!(body["data"]["hillshade"]["enabled"], false);
    assert_eq!(body["data"]["scale_bar"]["enabled"], true);
    assert_eq!(body["data"]["scale_bar"]["unit"], "metric");
    assert_eq!(body["data"]["camera"]["max_pitch"], 70);
    assert_eq!(body["data"]["camera"]["experimental_range_warning"], true);
    assert!(body["data"].get("basemaps").is_none());

    let written = json!({"success": true, "data": body["data"].clone()});
    let published = answered(Publish { defaults }, written).unwrap();
    assert!(published.scale_bar.enabled && !published.hillshade.enabled);
}

#[test]
fn a_stored_document_round_trips_and_garbage_is_the_defaults() {
    let mut defaults = MapDefaults::default();
    defaults.measure.halo_color = "#ffffff".into();
    defaults.zoom_level.decimals = 2;
    let stored = serde_json::to_value(&defaults).unwrap();
    assert_eq!(MapDefaults::admit(&stored), defaults);
    assert_eq!(MapDefaults::admit(&json!(null)), MapDefaults::default());
    assert_eq!(MapDefaults::admit(&json!([1, 2])), MapDefaults::default());
}

#[test]
fn the_nearest_preset_prefers_the_smaller_of_two_equally_near() {
    assert_eq!(nearest_preset(0.0), 0.3);
    assert_eq!(nearest_preset(0.7), 0.6);
    assert_eq!(nearest_preset(1.25), 1.0);
    assert_eq!(nearest_preset(2.0), 1.5);
    assert_eq!(nearest_preset(9.0), 2.5);
}

#[test]
fn a_colour_or_size_that_draws_nothing_takes_its_default() {
    let admitted = MapDefaults::admit(&json!({
        "hillshade": {"exaggeration": 0},
        "measure": {"line_color": " ", "halo_color": "#ffffff", "line_width": 0, "label_text_size": -2},
        "zoom_level": {"decimals": 2, "legacy": true}
    }));
    let defaults = MapDefaults::default();
    assert_eq!(admitted.hillshade.exaggeration, defaults.hillshade.exaggeration);
    assert_eq!(admitted.measure.line_color, defaults.measure.line_color);
    assert_eq!(admitted.measure.halo_color, "#ffffff");
    assert_eq!(admitted.measure.line_width, defaults.measure.line_width);
    assert_eq!(admitted.measure.label_text_size, defaults.measure.label_text_size);
    assert_eq!(
        admitted.zoom_level.decimals, 2,
        "an unknown field is dropped and the known one kept"
    );
}
