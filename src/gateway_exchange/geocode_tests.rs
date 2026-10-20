use super::*;
use crate::gateway_exchange::{resume, start, Facts, Method, Observed, Step};
use serde_json::json;

fn answered<C: Exchange>(call: C, status: u16, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status,
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

fn sent<C: Exchange>(call: C) -> Result<Request, Refusal> {
    match start(call) {
        Step::Fetch { request, .. } => Ok(request),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected a fetch"),
    }
}

fn not_configured() -> Value {
    json!({"success": false, "error": {"code": NOT_CONFIGURED,
        "message": "Place search is not configured on this deployment (no Google API key)."}})
}

fn search(query: &str) -> Search {
    Search {
        query: query.into(),
        language: Some("fr".into()),
        bias: None,
    }
}

#[test]
fn availability_is_the_config_flag_and_nothing_else() {
    let request = sent(Availability).unwrap();
    assert_eq!(
        (request.method, request.path.as_str()),
        (Method::Post, "/api/v1/geocode")
    );
    assert_eq!(request.body, Some(json!({"action": "get_config"})));
    let config = |data: Value| answered(Availability, 200, json!({"success": true, "data": data}));
    assert_eq!(config(json!({"available": true, "provider": "google_places"})), Ok(true));
    assert_eq!(config(json!({"available": false})), Ok(false));
    assert_eq!(config(json!({"available": "yes"})), Ok(false));
}

#[test]
fn a_search_sends_the_trimmed_query_its_language_and_a_wrapped_bias() {
    let call = Search {
        bias: Some(GeocodePoint {
            lat: -1.95,
            lng: 390.06,
        }),
        ..search("  Kigali Heights ")
    };
    let body = sent(call).unwrap().body.unwrap();
    assert_eq!(body["action"], "search");
    assert_eq!(body["query"], "Kigali Heights");
    assert_eq!(body["language"], "fr");
    assert_eq!(body["bias_lat"], -1.95);
    assert!((body["bias_lng"].as_f64().unwrap() - 30.06).abs() < 1e-9);

    let plain = sent(Search {
        language: Some("  ".into()),
        ..search("Huye")
    })
    .unwrap()
    .body
    .unwrap();
    assert_eq!(plain, json!({"action": "search", "query": "Huye"}));
}

#[test]
fn a_search_refuses_what_brain_would_refuse_before_sending() {
    for call in [
        search("   "),
        search(&"a".repeat(257)),
        Search {
            language: Some("fr; DROP".into()),
            ..search("Huye")
        },
        Search {
            bias: Some(GeocodePoint {
                lat: 91.0,
                lng: 0.0,
            }),
            ..search("Huye")
        },
        Search {
            bias: Some(GeocodePoint {
                lat: 0.0,
                lng: f64::NAN,
            }),
            ..search("Huye")
        },
    ] {
        assert_eq!(sent(call).unwrap_err().code, INPUT_INVALID);
    }
    assert!(sent(search(&"é".repeat(128))).is_ok());
    assert_eq!(sent(search(&"é".repeat(129))).unwrap_err().code, INPUT_INVALID);
}

#[test]
fn a_search_admits_only_named_places_at_a_point() {
    let places = answered(
        search("Huye"),
        200,
        json!({"success": true, "data": {"query": "Huye", "results": [
            {"name": " Huye ", "formatted_address": "Huye, Rwanda", "lat": -2.6, "lng": 29.74,
             "place_id": "abc", "types": ["locality"]},
            {"name": "", "lat": 1.0, "lng": 1.0},
            {"name": "No point", "lat": "x", "lng": 1.0},
            {"name": "Bare", "lat": -2.0, "lng": 30.0, "place_id": ""},
            "not a place"
        ]}}),
    )
    .unwrap();
    assert_eq!(
        places,
        [
            GeocodePlace {
                name: "Huye".into(),
                formatted_address: "Huye, Rwanda".into(),
                lat: -2.6,
                lng: 29.74,
                place_id: Some("abc".into()),
            },
            GeocodePlace {
                name: "Bare".into(),
                formatted_address: String::new(),
                lat: -2.0,
                lng: 30.0,
                place_id: None,
            },
        ]
    );
    let none = json!({"success": true, "data": {"query": "x", "results": null}});
    assert_eq!(answered(search("x"), 200, none), Ok(Vec::new()));
    let wrong = json!({"success": true, "data": {"results": {"name": "x"}}});
    assert_eq!(answered(search("x"), 200, wrong).unwrap_err().code, INVALID);
}

#[test]
fn a_deployment_without_a_key_is_unavailable_and_any_other_refusal_stays_one() {
    let refused = answered(search("Huye"), 503, not_configured()).unwrap_err();
    assert_eq!(refused.code, NOT_CONFIGURED);
    assert_eq!(
        places(Err(refused.clone())),
        Ok(GeocodePlaces {
            available: false,
            places: Vec::new()
        })
    );
    assert_eq!(address(Err(refused)), Ok(GeocodeAddress::default()));

    let upstream = json!({"success": false, "error": {"code": "GEOCODE_UPSTREAM_ERROR",
        "message": "Place search failed: quota"}});
    let refused = answered(search("Huye"), 502, upstream).unwrap_err();
    assert_eq!(refused.code, "GEOCODE_UPSTREAM_ERROR");
    assert_eq!(places(Err(refused.clone())), Err(refused));
    assert_eq!(
        places(Ok(Vec::new())),
        Ok(GeocodePlaces {
            available: true,
            places: Vec::new()
        })
    );
}

#[test]
fn a_reverse_lookup_sends_the_point_and_admits_the_address() {
    let call = Reverse {
        point: GeocodePoint {
            lat: -1.9441,
            lng: -329.9381,
        },
        language: Some("en".into()),
    };
    let body = sent(call.clone()).unwrap().body.unwrap();
    assert_eq!(body["action"], "reverse");
    assert_eq!(body["lat"], -1.9441);
    assert!((body["lng"].as_f64().unwrap() - 30.0619).abs() < 1e-9);
    assert_eq!(body["language"], "en");

    let found = answered(
        call.clone(),
        200,
        json!({"success": true, "data": {"found": true, "formatted_address": "KN 3 Rd, Kigali",
            "lat": -1.9441, "lng": 30.0619, "place_id": "p",
            "components": [{"long_name": "Kigali", "short_name": "KGL", "types": ["locality", 7]},
                           {"short_name": "RW"}, 3]}}),
    )
    .unwrap();
    assert_eq!(
        found,
        GeocodeAddress {
            available: true,
            found: true,
            formatted_address: "KN 3 Rd, Kigali".into(),
            components: vec![
                GeocodeAddressPart {
                    long_name: "Kigali".into(),
                    types: vec!["locality".into()],
                },
                GeocodeAddressPart {
                    long_name: String::new(),
                    types: Vec::new(),
                },
            ],
        }
    );
    let nothing = json!({"success": true, "data": {"found": false, "formatted_address": "",
        "lat": -1.9, "lng": 30.0}});
    let none = answered(call.clone(), 200, nothing).unwrap();
    assert!(none.available && !none.found && none.components.is_empty());

    let nowhere = Reverse {
        point: GeocodePoint {
            lat: -95.0,
            lng: 30.0,
        },
        language: None,
    };
    assert_eq!(sent(nowhere).unwrap_err().code, INPUT_INVALID);
}
