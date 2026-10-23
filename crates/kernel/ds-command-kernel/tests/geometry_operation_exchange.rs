//! The typed geometry operation answers exactly what the retired Network
//! packet exports answered, refuses malformed requests, and its decoder
//! refuses replies that do not answer the request.
use ds_command_kernel::geometry_operation::{
    decode_request, decode_response, execute, BufferGeometryInput, ExtendLineInput,
    GeometryOperationRequest as Request, GeometryOperationResponse as Response, LineLengthInput,
    OffsetLineInput, OrientedExtentInput, PointsAlongLineInput, Position, ProfileStationRole,
    ProfileStationsInput, SplitLineAtPointInput, SplitLineWithCutterInput, TrimLineInput,
};
use serde_json::{json, Value};

fn flat(line: &[Position]) -> Vec<f64> {
    line.iter().flatten().copied().collect()
}

fn pairs(flat: &[f64]) -> Vec<Position> {
    flat.chunks_exact(2)
        .map(|pair| [pair[0], pair[1]])
        .collect()
}

/// The packed segment format the retired split exports returned.
fn unpack_segments(packed: &[f64]) -> Option<Vec<Vec<Position>>> {
    if packed.is_empty() {
        return None;
    }
    let count = packed[0] as usize;
    let mut offset = 1 + count;
    let segments: Vec<Vec<Position>> = (0..count)
        .map(|index| {
            let length = packed[1 + index] as usize;
            let segment = pairs(&packed[offset..offset + length]);
            offset += length;
            segment
        })
        .collect();
    (segments.len() >= 2).then_some(segments)
}

fn run(request: Request) -> Response {
    let response = execute(&request).expect("admitted request");
    let bytes = serde_json::to_vec(&response).unwrap();
    assert_eq!(decode_response(&request, &bytes).unwrap(), response);
    response
}

const LINE: [Position; 3] = [[30.0, -2.0], [30.001, -2.0], [30.001, -1.999]];

#[test]
fn line_edits_match_the_retired_network_exports_exactly() {
    let line = [[0.0, 0.0], [2.0, 2.0]];
    let target = [[5.0, 6.0], [5.0, -1.0]];
    let Response::ExtendLine(extended) = run(Request::ExtendLine(ExtendLineInput {
        line: line.to_vec(),
        target: target.to_vec(),
    })) else {
        panic!("extend reply")
    };
    assert_eq!(
        extended.line,
        Some(pairs(&ds_geo_lite::extend_line(
            &flat(&line),
            &flat(&target)
        )))
    );
    assert!(extended.line.is_some());

    let line = [[0.0, 0.0], [5.0, 5.0]];
    let cutter = [[3.0, 0.0], [3.0, 5.0]];
    let Response::TrimLine(trimmed) = run(Request::TrimLine(TrimLineInput {
        line: line.to_vec(),
        cutter: cutter.to_vec(),
        keep: [1.0, 1.0],
    })) else {
        panic!("trim reply")
    };
    assert_eq!(
        trimmed.line,
        Some(pairs(&ds_geo_lite::trim_line(
            &flat(&line),
            &flat(&cutter),
            1.0,
            1.0
        )))
    );

    let line = [[0.0, 0.0], [10.0, 0.0]];
    let Response::SplitLineAtPoint(split) = run(Request::SplitLineAtPoint(SplitLineAtPointInput {
        line: line.to_vec(),
        point: [4.0, 0.0],
    })) else {
        panic!("split reply")
    };
    assert_eq!(
        split.segments,
        unpack_segments(&ds_geo_lite::split_line_at_point(&flat(&line), 4.0, 0.0))
    );
    // The retired Network export's own regression fixture.
    assert_eq!(
        split.segments,
        Some(vec![
            vec![[0.0, 0.0], [4.0, 0.0]],
            vec![[4.0, 0.0], [10.0, 0.0]]
        ])
    );

    let cutter = [[5.0, -1.0], [5.0, 1.0]];
    let Response::SplitLineWithCutter(cut) =
        run(Request::SplitLineWithCutter(SplitLineWithCutterInput {
            line: line.to_vec(),
            cutter: cutter.to_vec(),
        }))
    else {
        panic!("cutter reply")
    };
    assert_eq!(
        cut.segments,
        unpack_segments(&ds_geo_lite::split_line_with_cutter(
            &flat(&line),
            &flat(&cutter)
        ))
    );
    assert_eq!(cut.segments.as_ref().map(Vec::len), Some(2));

    for right_side in [true, false] {
        let Response::OffsetLine(offset) = run(Request::OffsetLine(OffsetLineInput {
            line: LINE.to_vec(),
            distance_m: 25.0,
            right_side,
        })) else {
            panic!("offset reply")
        };
        assert_eq!(
            offset.line,
            Some(pairs(&ds_geo_lite::offset_line(
                &flat(&LINE),
                25.0,
                right_side
            )))
        );
    }
}

#[test]
fn absent_answers_stay_absent_rather_than_inventing_geometry() {
    let far = [[2.0, 0.0], [2.0, 1.0]];
    let short = [[0.0, 0.0], [1.0, 1.0]];
    let parallel = [[0.0, 1.0], [1.0, 2.0]];
    assert!(ds_geo_lite::extend_line(&flat(&short), &flat(&parallel)).is_empty());
    for (request, empty) in [
        (
            Request::ExtendLine(ExtendLineInput {
                line: short.to_vec(),
                target: parallel.to_vec(),
            }),
            json!({"operation":"extend_line","result":{"line":null}}),
        ),
        (
            Request::TrimLine(TrimLineInput {
                line: short.to_vec(),
                cutter: far.to_vec(),
                keep: [0.0, 0.0],
            }),
            json!({"operation":"trim_line","result":{"line":null}}),
        ),
        (
            Request::SplitLineAtPoint(SplitLineAtPointInput {
                line: short.to_vec(),
                point: [0.0, 0.0],
            }),
            json!({"operation":"split_line_at_point","result":{"segments":null}}),
        ),
        (
            Request::SplitLineWithCutter(SplitLineWithCutterInput {
                line: short.to_vec(),
                cutter: far.to_vec(),
            }),
            json!({"operation":"split_line_with_cutter","result":{"segments":null}}),
        ),
        (
            Request::OffsetLine(OffsetLineInput {
                line: short.to_vec(),
                distance_m: 0.0,
                right_side: true,
            }),
            json!({"operation":"offset_line","result":{"line":null}}),
        ),
        (
            Request::OffsetLine(OffsetLineInput {
                line: vec![[30.0, 0.0]],
                distance_m: 5.0,
                right_side: true,
            }),
            json!({"operation":"offset_line","result":{"line":null}}),
        ),
        (
            Request::BufferGeometry(BufferGeometryInput {
                geometry: json!({"type":"Point","coordinates":[0.0,0.0]}),
                distance_m: -5.0,
                quad_segments: 8,
            }),
            json!({"operation":"buffer_geometry","result":{"ring":null}}),
        ),
        (
            Request::BufferGeometry(BufferGeometryInput {
                geometry: json!({"type":"MultiPoint","coordinates":[[0.0,0.0]]}),
                distance_m: 5.0,
                quad_segments: 8,
            }),
            json!({"operation":"buffer_geometry","result":{"ring":null}}),
        ),
        (
            Request::OrientedExtentBuffered(OrientedExtentInput {
                positions: vec![],
                buffer_m: 10.0,
            }),
            json!({"operation":"oriented_extent_buffered","result":{"ring":null}}),
        ),
    ] {
        assert_eq!(serde_json::to_value(run(request)).unwrap(), empty);
    }
}

#[test]
fn buffers_dispatch_point_line_and_outer_ring_exactly_as_the_packet_kinds_did() {
    let cases = [
        (
            json!({"type":"Point","coordinates":[30.0,-2.0]}),
            0,
            vec![[30.0, -2.0]],
            8,
        ),
        (
            json!({"type":"LineString","coordinates":[[30.0,0.0],[30.01,0.0]]}),
            1,
            vec![[30.0, 0.0], [30.01, 0.0]],
            6,
        ),
        (
            json!({"type":"Polygon","coordinates":[[[0.0,0.0],[0.001,0.0],[0.001,0.001],[0.0,0.0]],[[0.0002,0.0001],[0.0003,0.0001],[0.0003,0.0002],[0.0002,0.0001]]]}),
            2,
            vec![[0.0, 0.0], [0.001, 0.0], [0.001, 0.001], [0.0, 0.0]],
            8,
        ),
    ];
    for (geometry, kind, source, quad_segments) in cases {
        let Response::BufferGeometry(buffer) = run(Request::BufferGeometry(BufferGeometryInput {
            geometry,
            distance_m: 50.0,
            quad_segments,
        })) else {
            panic!("buffer reply")
        };
        let expected = pairs(&ds_geo_lite::buffer_geometry(
            kind,
            &flat(&source),
            50.0,
            quad_segments,
        ));
        assert!(expected.len() >= 4);
        assert_eq!(buffer.ring, Some(expected));
    }
}

#[test]
fn profile_sampling_matches_the_retired_pooled_exports_exactly() {
    let Response::LineLength(length) = run(Request::LineLength(LineLengthInput {
        line: LINE.to_vec(),
    })) else {
        panic!("length reply")
    };
    let measured = ds_geo_lite::measurement::measure_frame(
        serde_json::from_value(json!({"vertices": LINE, "mode": "distance"})).unwrap(),
    );
    assert_eq!(length.length_m, measured.total_length_m);
    assert!(length.length_m > 200.0);

    let Response::PointsAlongLineWithVertices(sampled) =
        run(Request::PointsAlongLineWithVertices(PointsAlongLineInput {
            line: LINE.to_vec(),
            interval_m: 50.0,
        }))
    else {
        panic!("points reply")
    };
    let packet = ds_geo_lite::points_along_line_with_vertices(&flat(&LINE), 50.0);
    let expected: Vec<[f64; 3]> = packet.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
    let actual: Vec<[f64; 3]> = sampled
        .points
        .iter()
        .map(|p| [p.lng, p.lat, p.distance_m])
        .collect();
    assert_eq!(actual, expected);
    assert!(
        actual.iter().any(|p| p[0] == 30.001 && p[1] == -2.0),
        "the bend is an exact station"
    );

    let lines = vec![
        vec![[30.0, -2.0], [30.002, -2.0]],
        vec![[30.001, -2.001], [30.001, -2.0]],
    ];
    let Response::ClassifyProfileStations(classified) =
        run(Request::ClassifyProfileStations(ProfileStationsInput {
            lines: lines.clone(),
        }))
    else {
        panic!("stations reply")
    };
    let mut packed = vec![lines.len() as f64];
    for line in &lines {
        packed.push(line.len() as f64);
        packed.extend(flat(line));
    }
    let roles = [
        ProfileStationRole::Start,
        ProfileStationRole::End,
        ProfileStationRole::AnglePoint,
        ProfileStationRole::Tap,
    ];
    let expected: Vec<Value> = ds_geo_lite::classify_profile_stations(&packed)
        .chunks_exact(6)
        .map(|s| {
            json!({
                "lineIndex": s[0] as u32,
                "vertexIndex": (s[1] >= 0.0).then_some(s[1] as u32),
                "role": roles[s[2] as usize],
                "station_m": s[3], "lng": s[4], "lat": s[5],
            })
        })
        .collect();
    assert_eq!(
        serde_json::to_value(&classified.stations).unwrap(),
        Value::Array(expected)
    );
    // The retired export's regression: a virtual tap on the receiving line.
    assert!(classified
        .stations
        .iter()
        .any(|station| station.line_index == 0
            && station.vertex_index.is_none()
            && station.role == ProfileStationRole::Tap));

    let positions = vec![[30.0, -2.0], [30.01, -2.0], [30.01, -1.995], [30.0, -1.995]];
    let Response::OrientedExtentBuffered(extent) =
        run(Request::OrientedExtentBuffered(OrientedExtentInput {
            positions: positions.clone(),
            buffer_m: 20.0,
        }))
    else {
        panic!("extent reply")
    };
    let ring = ds_geo_lite::extent::oriented_extent_buffered(&flat(&positions), 20.0).unwrap();
    assert_eq!(extent.ring, Some(pairs(&ring)));
}

#[test]
fn malformed_requests_are_refused_by_name() {
    for bytes in [
        &br#"{"operation":"extend_line","input":{"line":[[0,0],[1,1]]}}"#[..],
        br#"{"operation":"rotate_line","input":{}}"#,
        br#"{"operation":"points_along_line_with_vertices","input":{"line":[[30,-2],[30.001,-2]],"interval_m":0}}"#,
        br#"{"operation":"buffer_geometry","input":{"geometry":{"type":"Point","coordinates":[30,-2]},"distance_m":5,"quad_segments":0}}"#,
        br#"{"operation":"buffer_geometry","input":{"geometry":{"type":"Point","coordinates":"x"},"distance_m":5,"quad_segments":8}}"#,
        br#"{"operation":"oriented_extent_buffered","input":{"positions":[[200,-2]],"buffer_m":5}}"#,
    ] {
        let refused = decode_request(bytes)
            .and_then(|request| execute(&request).map(|_| ()))
            .unwrap_err()
            .to_string();
        assert!(refused.starts_with("geometry_request_invalid:"), "{refused}");
    }
    let nan = Request::TrimLine(TrimLineInput {
        line: vec![[0.0, 0.0], [5.0, 5.0]],
        cutter: vec![[3.0, 0.0], [3.0, 5.0]],
        keep: [f64::NAN, 1.0],
    });
    assert!(execute(&nan)
        .unwrap_err()
        .to_string()
        .starts_with("geometry_request_invalid:"));
    let huge = Request::LineLength(LineLengthInput {
        line: vec![[30.0, -2.0]; 1_000_001],
    });
    assert!(execute(&huge)
        .unwrap_err()
        .to_string()
        .contains("position limit"));
}

#[test]
fn the_decoder_refuses_replies_that_do_not_answer_the_request() {
    let request = Request::SplitLineAtPoint(SplitLineAtPointInput {
        line: vec![[0.0, 0.0], [10.0, 0.0]],
        point: [4.0, 0.0],
    });
    let reply = serde_json::to_value(execute(&request).unwrap()).unwrap();
    let mut crossed = reply.clone();
    crossed["operation"] = json!("split_line_with_cutter");
    let mut single = reply.clone();
    single["result"]["segments"] = json!([[[0, 0], [4, 0]]]);
    let mut short = reply.clone();
    short["result"]["segments"][1] = json!([[4, 0]]);
    let mut shape = reply.clone();
    shape["result"] = json!("segments");
    for bad in [crossed, single, short, shape] {
        let refused = decode_response(&request, &serde_json::to_vec(&bad).unwrap()).unwrap_err();
        assert!(
            refused
                .to_string()
                .starts_with("geometry_response_invalid:"),
            "{refused}"
        );
    }

    let stations = Request::ClassifyProfileStations(ProfileStationsInput {
        lines: vec![vec![[30.0, -2.0], [30.002, -2.0]]],
    });
    let reply = serde_json::to_value(execute(&stations).unwrap()).unwrap();
    for (pointer, value) in [
        ("/result/stations/0/lineIndex", json!(3)),
        ("/result/stations/0/vertexIndex", json!(9)),
        ("/result/stations/0/role", json!("bend")),
        ("/result/stations/0/station_m", json!(-1.0)),
    ] {
        let mut bad = reply.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        let refused = decode_response(&stations, &serde_json::to_vec(&bad).unwrap()).unwrap_err();
        assert!(
            refused
                .to_string()
                .starts_with("geometry_response_invalid:"),
            "{refused}"
        );
    }

    let sampled = Request::PointsAlongLineWithVertices(PointsAlongLineInput {
        line: LINE.to_vec(),
        interval_m: 50.0,
    });
    let mut reply = serde_json::to_value(execute(&sampled).unwrap()).unwrap();
    reply["result"]["points"][1]["distance_m"] = json!(-3.0);
    assert!(decode_response(&sampled, &serde_json::to_vec(&reply).unwrap()).is_err());

    let ring = Request::BufferGeometry(BufferGeometryInput {
        geometry: json!({"type":"Point","coordinates":[30.0,-2.0]}),
        distance_m: 5.0,
        quad_segments: 8,
    });
    let mut reply = serde_json::to_value(execute(&ring).unwrap()).unwrap();
    reply["result"]["ring"] = json!([[30, -2], [30.1, -2], [30, -2]]);
    assert!(decode_response(&ring, &serde_json::to_vec(&reply).unwrap()).is_err());
}

#[test]
fn compatible_additive_fields_are_accepted_on_both_ends() {
    let request = decode_request(
        br#"{"operation":"offset_line","input":{"line":[[30,0],[30.01,0]],"distance_m":50,"right_side":true,"future_hint":1},"trace":"x"}"#,
    )
    .unwrap();
    let reply = execute(&request).unwrap();
    let mut extended = serde_json::to_value(&reply).unwrap();
    extended["result"]["future_field"] = json!({"available": true});
    extended["receipt"] = json!("additive");
    assert_eq!(
        decode_response(&request, &serde_json::to_vec(&extended).unwrap()).unwrap(),
        reply
    );
}
