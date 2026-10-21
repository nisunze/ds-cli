use ds_kml::{layer_document, member_reference, parse, Styles};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn namespaces_inline_normal_pair_and_partial_override() {
    let root = parse(r##"<k:kml xmlns:k="http://www.opengis.net/kml/2.2"><k:Document>
      <k:Style id="base"><k:LineStyle><k:color>ff010203</k:color><k:width>2</k:width></k:LineStyle></k:Style>
      <k:StyleMap id="map"><k:Pair><k:key>normal</k:key><k:styleUrl>#base</k:styleUrl></k:Pair></k:StyleMap>
      <k:Placemark><k:styleUrl>#map</k:styleUrl><k:Style><k:LineStyle><k:width>4</k:width></k:LineStyle></k:Style></k:Placemark>
    </k:Document></k:kml>"##).unwrap();
    let mut placemarks = Vec::new();
    root.descendants("Placemark", &mut placemarks);
    let documents = BTreeMap::from([("doc.kml".into(), root.clone())]);
    let (id, style) = Styles::new(&documents)
        .placemark("doc.kml", placemarks[0])
        .unwrap()
        .unwrap();
    assert!(id.starts_with("doc.kml#map@inline-"));
    assert_eq!(style["LineStyle"], json!({"color":"ff010203", "width":4.0}));
}

#[test]
fn cycles_bad_colours_and_bad_widths_are_named() {
    for body in [
        r##"<StyleMap id="bad"><Pair><key>normal</key><styleUrl>#bad</styleUrl></Pair></StyleMap>"##,
        r##"<Style id="bad"><LineStyle><color>z1234567</color></LineStyle></Style>"##,
        r##"<Style id="bad"><LineStyle><width>NaN</width></LineStyle></Style>"##,
    ] {
        let root = parse(&format!(
            "<kml>{body}<Placemark><styleUrl>#bad</styleUrl></Placemark></kml>"
        ))
        .unwrap();
        let mut placemarks = Vec::new();
        root.descendants("Placemark", &mut placemarks);
        let documents = BTreeMap::from([("doc.kml".into(), root.clone())]);
        assert!(Styles::new(&documents)
            .placemark("doc.kml", placemarks[0])
            .is_err());
    }
    assert_eq!(
        member_reference("styles/a.kml", "../icons/p.png").unwrap(),
        "icons/p.png"
    );
    assert!(member_reference("doc.kml", "../outside.kml").is_err());
    assert!(member_reference("doc.kml", "https://example.org/a.kml").is_err());
}

#[test]
fn categorical_documents_keep_distinct_colour_width_and_label_values() {
    let features = vec![
        json!({"properties":{"kml_style_id":"one", "kml_style":{"LineStyle":{"color":"ff0000ff","width":1.0}}}}),
        json!({"properties":{"kml_style_id":"two", "kml_style":{"LineStyle":{"color":"ff00ff00","width":5.0}}}}),
    ];
    let doc = layer_document(&features, "LineString").unwrap();
    assert_eq!(doc["metadata"]["kml_images"], json!([]));
    assert_eq!(
        doc["paint"]["line-width"],
        json!(["match", ["get", "kml_style_id"], "one", 1.0, "two", 5.0, 1])
    );
    assert_eq!(
        doc["metadata"]["kml_styles"]["one"]["LineStyle"]["color"],
        "ff0000ff"
    );
}

#[test]
fn inline_icon_href_replaces_the_shared_resource_origin() {
    let shared = parse(r#"<kml><Style id="pin"><IconStyle><Icon><href>../icons/a.png</href></Icon></IconStyle></Style></kml>"#).unwrap();
    let root = parse(r#"<kml><Placemark><styleUrl>styles/shared.kml#pin</styleUrl><Style><IconStyle><Icon><href>https://example.org/pin.png</href></Icon></IconStyle></Style></Placemark></kml>"#).unwrap();
    let mut placemarks = Vec::new();
    root.descendants("Placemark", &mut placemarks);
    let documents = BTreeMap::from([
        ("doc.kml".into(), root.clone()),
        ("styles/shared.kml".into(), shared),
    ]);
    let (_, style) = Styles::new(&documents)
        .placemark("doc.kml", placemarks[0])
        .unwrap()
        .unwrap();
    assert_eq!(style["IconStyle"]["href"], "https://example.org/pin.png");
    assert!(style["IconStyle"].get("member").is_none());
}

#[test]
fn xml_boolean_style_flags_are_decoded() {
    let root =
        parse("<Style><PolyStyle><fill>false</fill><outline>true</outline></PolyStyle></Style>")
            .unwrap();
    assert_eq!(
        ds_kml::decode_style(&root.children[0]).unwrap()["PolyStyle"],
        json!({"fill":false,"outline":true})
    );
}

#[test]
fn uri_encoded_archive_names_and_style_fragments_are_resolved() {
    let shared = parse(r#"<kml><Style id="π"><IconStyle><Icon><href>../icons/pin%20file.png</href></Icon></IconStyle></Style></kml>"#).unwrap();
    let root = parse(r#"<kml><Placemark><styleUrl>styles/shared%20styles.kml#%CF%80</styleUrl></Placemark></kml>"#).unwrap();
    let mut placemarks = Vec::new();
    root.descendants("Placemark", &mut placemarks);
    let documents = BTreeMap::from([
        ("doc.kml".into(), root.clone()),
        ("styles/shared styles.kml".into(), shared),
    ]);
    let (id, style) = Styles::new(&documents)
        .placemark("doc.kml", placemarks[0])
        .unwrap()
        .unwrap();
    assert_eq!(id, "styles/shared styles.kml#π");
    assert_eq!(style["IconStyle"]["member"], "icons/pin file.png");
    assert!(ds_kml::uri_member_reference("doc.kml", "%2e%2e/outside.kml").is_err());
}
