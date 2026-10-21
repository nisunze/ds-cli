use ds_profile_view::{labels::*, model::Screen};
use serde_json::{json, Value};
use std::cmp::Ordering;

fn number(value: &Value) -> f64 {
    if let Some(value) = value.as_f64() {
        return value;
    }
    match value["$number"].as_str().unwrap() {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        "-0" => -0.,
        other => panic!("unexpected numeric fixture {other}"),
    }
}

fn wire_number(value: f64) -> Value {
    if value.is_nan() {
        json!({"$number":"NaN"})
    } else if value == f64::INFINITY {
        json!({"$number":"Infinity"})
    } else if value == f64::NEG_INFINITY {
        json!({"$number":"-Infinity"})
    } else if value == 0. && value.is_sign_negative() {
        json!({"$number":"-0"})
    } else {
        json!(value)
    }
}

fn normalized(value: Value) -> Value {
    match value {
        Value::Number(n) => wire_number(n.as_f64().unwrap()),
        Value::Array(values) => Value::Array(values.into_iter().map(normalized).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, normalized(value)))
                .collect(),
        ),
        value => value,
    }
}

fn request(value: &Value) -> LabelRequest {
    LabelRequest {
        id: value["id"].as_str().unwrap().into(),
        label: value["label"].as_str().unwrap().into(),
        anchor: Screen {
            x: number(&value["anchor"]["x"]),
            y: number(&value["anchor"]["y"]),
        },
        orientation: orientation(value["orientation"].as_str().unwrap()).unwrap(),
        priority: number(&value["priority"]),
        required: value["required"].as_bool().unwrap_or(false),
    }
}

fn fixture_options(value: &Value) -> Options {
    let mut options = options(number(&value["width"]), number(&value["height"]));
    for (key, destination) in [
        ("lineHeightPx", &mut options.line_height),
        ("fontHeightPx", &mut options.font_height),
        ("paddingPx", &mut options.padding),
        ("viewportMarginPx", &mut options.margin),
        ("cellSizePx", &mut options.cell_size),
    ] {
        if let Some(value) = value.get(key) {
            *destination = number(value);
        }
    }
    options
}

// Captured from the old browser's localeCompare service. Canonically equivalent
// Unicode IDs share a rank; stable input order still distinguishes occurrences.
fn captured_collation(a: &str, b: &str) -> Ordering {
    let rank = |s| match s {
        "😀" => 0,
        "a" => 1,
        "A" => 2,
        "Å" => 3,
        "ä" => 4,
        "duplicate" => 5,
        "é" | "e\u{301}" => 6,
        "pole-10" => 7,
        "pole-2" => 8,
        "z" => 9,
        _ => 10,
    };
    rank(a).cmp(&rank(b))
}

fn placement_value(label: &PlacedLabel, original: &Value) -> Value {
    let mut value = original.clone();
    value["lines"] = json!(label.lines);
    value["placement"] = json!({
        "x": wire_number(label.placement.x), "y": wire_number(label.placement.y),
        "align": match label.placement.align { Align::Left => "left", Align::Right => "right", Align::Center => "center" },
        "baseline": match label.placement.baseline { Baseline::Bottom => "bottom", Baseline::Top => "top" },
        "rotationRad": wire_number(label.placement.rotation_rad),
    });
    value["bounds"] = json!({
        "left": wire_number(label.bounds.left), "top": wire_number(label.bounds.top),
        "right": wire_number(label.bounds.right), "bottom": wire_number(label.bounds.bottom),
    });
    value
}

#[test]
fn captured_922_requests_preserve_placements_unicode_ties_and_metric_order() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/profile-label-layout-parity.json")).unwrap();
    let mut request_count = 0;
    let mut placed_count = 0;
    for (index, sample) in cases.iter().enumerate() {
        let originals = sample["requests"].as_array().unwrap();
        let requests: Vec<_> = originals.iter().map(request).collect();
        let mut measurements = Vec::new();
        let result = layout_labels(
            &requests,
            fixture_options(&sample["options"]),
            |_, text| {
                measurements.push(text.to_owned());
                text.encode_utf16().count() as f64 * 6.
            },
            captured_collation,
        );
        let actual: Vec<_> = result
            .iter()
            .map(|p| placement_value(p, &originals[p.input_index]))
            .collect();
        assert_eq!(
            normalized(json!(actual)),
            normalized(sample["expected"].clone()),
            "fixture {index}"
        );
        assert_eq!(
            json!(measurements),
            sample["measurements"],
            "metric order in fixture {index}"
        );
        request_count += requests.len();
        placed_count += result.len();
    }
    assert_eq!(request_count, 922);
    assert_eq!(placed_count, 344);
}

#[test]
fn nonfinite_metrics_viewports_and_signed_zero_match_twenty_captured_controls() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "fixtures/profile-label-layout-numeric-parity.json"
    ))
    .unwrap();
    for sample in &cases {
        let originals = sample["requests"].as_array().unwrap();
        let requests: Vec<_> = originals.iter().map(request).collect();
        let mut measurements = Vec::new();
        let result = layout_labels(
            &requests,
            fixture_options(&sample["options"]),
            |_, text| {
                measurements.push(text.to_owned());
                number(&sample["metric"])
            },
            captured_collation,
        );
        let actual: Vec<_> = result
            .iter()
            .map(|p| placement_value(p, &originals[p.input_index]))
            .collect();
        assert_eq!(
            normalized(json!(actual)),
            normalized(sample["expected"].clone()),
            "{}",
            sample["name"]
        );
        assert_eq!(
            json!(measurements),
            sample["measurements"],
            "{}",
            sample["name"]
        );
    }
    assert_eq!(cases.len(), 20);
}

fn label(id: &str, required: bool) -> LabelRequest {
    LabelRequest {
        id: id.into(),
        label: "text".into(),
        anchor: Screen { x: 100., y: 80. },
        orientation: Orientation::Right,
        priority: 0.,
        required,
    }
}

#[test]
fn selected_labels_keep_the_native_overlap_fallback_and_offscreen_anchors_skip_metrics() {
    let ordinary = label("first", false);
    let selected = label("second", true);
    assert_eq!(
        layout_labels(
            &[ordinary.clone(), selected],
            options(300., 180.),
            |_, _| 24.,
            str::cmp
        )
        .len(),
        2
    );
    let mut offscreen = ordinary;
    offscreen.anchor.x = -1.;
    assert!(layout_labels(
        &[offscreen],
        options(300., 180.),
        |_, _| panic!("offscreen label was measured"),
        str::cmp
    )
    .is_empty());
}

#[test]
fn blank_lines_are_removed_without_trimming_authored_whitespace() {
    let mut request = label("multiline", false);
    request.label = "\n  name\n\nsecond\n".into();
    let result = layout_labels(
        &[request],
        options(300., 180.),
        |_, text| text.len() as f64 * 6.,
        str::cmp,
    );
    assert_eq!(result[0].lines, ["  name", "second"]);
    assert!(orientation("diagonal").is_none());
}
