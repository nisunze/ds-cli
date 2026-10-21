//! Actual retained Gisagara naming-page content, not a synthetic replacement.
//! The element bound changes; all authored words, rectangles and logos remain.
use ds_command_kernel::{
    printing::{self, Layout},
    report_export::sha256_hex,
};
use serde_json::{json, Value};
const HELD: &[u8] = include_bytes!("fixtures/gisagara-mv-front-naming-held.json");
fn held() -> Layout {
    serde_json::from_slice(HELD).unwrap()
}
#[test]
fn retained_275_element_naming_page_is_admitted_without_content_change() {
    assert_eq!(
        sha256_hex(HELD),
        "e3f51db93dc0c2688fdc4151b0b9ef282672ab916b04f77ae3f81bfd6ebd124c"
    );
    let layout = held();
    assert_eq!(layout.elements.len(), 275);
    printing::validate(&layout).unwrap();
    let output: Value = serde_json::from_str(
        &printing::evaluate(
            &serde_json::to_vec(&json!({"op":"validate","layout":layout})).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(output["valid"], true);
    let admitted: Layout = serde_json::from_value(output["layout"].clone()).unwrap();
    assert_eq!(
        admitted, layout,
        "validator preserves every authored element/rectangle/text and held logo byte"
    );
    assert!(serde_json::to_vec(&admitted).unwrap().len() <= printing::MAX_LAYOUT_BYTES);
}
#[test]
fn named_400_element_limit_retains_page_and_byte_bounds() {
    let mut layout = held();
    let mut rectangle = layout.elements[0].clone();
    while layout.elements.len() < printing::MAX_LAYOUT_ELEMENTS {
        rectangle.id = format!("bounded-element-{}", layout.elements.len());
        layout.elements.push(rectangle.clone());
    }
    printing::validate(&layout).unwrap();
    let mut too_many = layout.clone();
    rectangle.id = "element-401".into();
    too_many.elements.push(rectangle);
    assert_eq!(
        printing::validate(&too_many).unwrap_err(),
        "print layout needs 1..400 page elements"
    );
    let mut empty = layout.clone();
    empty.elements.clear();
    assert_eq!(
        printing::validate(&empty).unwrap_err(),
        "print layout needs 1..400 page elements"
    );
    let mut page = layout.clone();
    page.page_mm[0] = 49.;
    assert_eq!(
        printing::validate(&page).unwrap_err(),
        "invalid page dimensions"
    );
    let mut outside = layout.clone();
    outside.elements[0].rect[0] = outside.page_mm[0];
    assert!(printing::validate(&outside)
        .unwrap_err()
        .contains("lies outside the page"));
    let mut oversized = layout;
    for element in &mut oversized.elements {
        element.text = "x".repeat(4000);
    }
    assert_eq!(
        printing::validate(&oversized).unwrap_err(),
        "print layout exceeds 750 KB including logos"
    );
}
