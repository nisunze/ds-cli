use super::*;
use serde_json::json;

const CSV: &[u8] = b"lon,lat,name\n30.06,-1.94,Kigali\n29.74,-2.60,Huye\n";

fn reading(attributes_only: bool) -> Value {
    json!({"sheetKey":"s0","separator":null,"headerRow":null,"xColumn":"lon",
        "yColumn":"lat","crs":"wgs84_lonlat","utmZone":null,"utmSouthern":null,
        "attributesOnly":attributes_only,"layerName":null,"columnRenames":[],"dropColumns":[]})
}

#[test]
fn identity_is_stable_for_an_unchanged_source_and_parameters() {
    let id = || conversion_identity(&digest(CSV), "p.csv", Some(&reading(false)), None, None);
    assert_eq!(id().unwrap(), id().unwrap());
    assert_eq!(id().unwrap().len(), 64);
}

#[test]
fn identity_changes_with_any_parameter_that_changes_the_output() {
    let base = conversion_identity(&digest(CSV), "p.csv", Some(&reading(false)), None, None);
    for other in [
        conversion_identity(&digest(CSV), "p.csv", Some(&reading(true)), None, None),
        conversion_identity(&digest(b"x"), "p.csv", Some(&reading(false)), None, None),
        conversion_identity(
            &digest(CSV),
            "p.csv",
            Some(&reading(false)),
            Some("a"),
            None,
        ),
        conversion_identity(
            &digest(CSV),
            "p.csv",
            Some(&reading(false)),
            None,
            Some(&json!({"zColumn": "z"})),
        ),
    ] {
        assert_ne!(base.clone().unwrap(), other.unwrap());
    }
}

#[test]
fn styled_formats_fold_in_their_style_contract() {
    let kml = conversion_identity(&digest(CSV), "a.kml", None, None, None).unwrap();
    let json = conversion_identity(&digest(CSV), "a.json", None, None, None).unwrap();
    assert_ne!(kml, json);
}
