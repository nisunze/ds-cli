use super::*;

fn line(id: &str, coordinates: Value) -> Value {
    json!({"type":"Feature","id":id,"properties":{"_layer":"alignments"},
        "geometry":{"type":"LineString","coordinates":coordinates}})
}

#[test]
fn every_projected_alignment_is_a_route_with_its_own_bounds() {
    let projection = json!({"type":"FeatureCollection","features":[
        line("aln-1", json!([[29.70,-2.62,1500.0],[29.72,-2.60,1510.0]])),
        {"type":"Feature","id":"str-1","properties":{"_layer":"structures"},
         "geometry":{"type":"Point","coordinates":[29.71,-2.61]}},
        {"type":"Feature","id":"aln-2","properties":{"_layer":"alignments"},
         "geometry":{"type":"MultiLineString","coordinates":[[[29.80,-2.70],[29.81,-2.69]],[[29.82,-2.68],[29.83,-2.71]]]}}
    ]});
    let routes = routes(&projection).unwrap();
    assert_eq!(
        routes.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["aln-1", "aln-2"]
    );
    assert_eq!(routes[0].bounds, [29.70, -2.62, 29.72, -2.60]);
    assert_eq!(routes[1].bounds, [29.80, -2.71, 29.83, -2.68]);
    assert_eq!(routes[1].feature["id"], "aln-2");
    assert_eq!(union(&routes).unwrap(), [29.70, -2.71, 29.83, -2.60]);
    let missing = json!({"features":[{"properties":{"_layer":"alignments"},
        "geometry":{"type":"LineString","coordinates":[[29.7,-2.6]]}}]});
    assert!(routes_error(&missing).contains("alignment has no id"));
}

fn routes_error(projection: &Value) -> String {
    match routes(projection) {
        Ok(_) => panic!("an alignment without an id must refuse"),
        Err(error) => error,
    }
}

#[test]
fn design_rooms_lend_only_the_booklet_families_near_the_model() {
    let point = |x: f64| {
        json!({"type":"Feature","properties":{"name":"T"},
        "geometry":{"type":"Point","coordinates":[x,-2.6]}})
    };
    let room = json!({
        "tr":{"type":"FeatureCollection","features":[point(29.70), point(30.50)]},
        "lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},
            "geometry":{"type":"LineString","coordinates":[[29.60,-2.6],[29.80,-2.6]]}}]},
        "customers":{"type":"FeatureCollection","features":[point(29.70)]},
        "service_cables":{"type":"FeatureCollection","features":[]}
    });
    let kept = design_layers(&room, [29.65, -2.65, 29.75, -2.55]).unwrap();
    let ids = kept
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(ids, ["lv_lines", "tr"]);
    // A line crossing the area is kept whole; a distant transformer is not.
    assert_eq!(kept["lv_lines"]["features"].as_array().unwrap().len(), 1);
    assert_eq!(kept["tr"]["features"].as_array().unwrap().len(), 1);
    assert_eq!(expand([1.0, 2.0, 3.0, 4.0], 0.5), [0.5, 1.5, 3.5, 4.5]);
}
