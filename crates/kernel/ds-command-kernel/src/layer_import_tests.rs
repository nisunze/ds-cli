use super::*;
use serde_json::json;
use std::io::Read;

fn run(request: &LayerImportRequest, parts: &[&[u8]]) -> LayerImportReply {
    let bytes = encode_request(request).expect("encodes");
    let request = decode_request(&bytes).expect("admits");
    execute(&request, parts).expect("reads")
}

fn heads(chunks: &[Vec<u8>]) -> Vec<(usize, [u8; 6], [u8; 6])> {
    chunks.iter().map(|chunk| chunk_head(chunk)).collect()
}

/// The reply crosses the worker boundary as JSON-shaped data plus chunks;
/// decode exactly what the other end receives.
fn round_trip(request: &LayerImportRequest, reply: &LayerImportReply) -> LayerImportResponse {
    let input = encode_request(request).unwrap();
    let request = decode_request(&input).unwrap();
    decode_response(
        &request,
        &serde_json::to_vec(&reply.response).unwrap(),
        &heads(&reply.chunks),
    )
    .expect("the owning decoder accepts the handler reply")
}

fn decoded_features(chunks: &[Vec<u8>]) -> Vec<Value> {
    chunks
        .iter()
        .flat_map(|chunk| ds_cleaning::arrow_ipc::ipc_to_features(chunk).expect("Arrow IPC"))
        .collect()
}

/// Success parity with the retired Network export: it ran the same reader in a
/// build whose JSON maps iterate sorted and encoded every 1024 features with
/// the ds-cleaning codec. The reference is that output: the reader's envelope
/// with sorted objects, encoded with the same codec — byte for byte.
fn assert_ipc_parity(envelope: io::GeoLayerImportEnvelope, reply: &LayerImportReply) {
    let envelope = canonical_envelope(envelope);
    let LayerImportResponse::IpcLayers(layers) = &reply.response else {
        panic!("expected layers, got {:?}", reply.response);
    };
    assert_eq!(layers.len(), envelope.layers.len());
    let mut offset = 0;
    for (layer, source) in layers.iter().zip(&envelope.layers) {
        assert_eq!(layer.name, source.name);
        assert_eq!(layer.feature_count, source.features.len());
        assert_eq!(layer.geometry_type, source.geometry_type);
        assert_eq!(
            serde_json::to_value(&layer.crs).unwrap(),
            serde_json::to_value(&source.crs).unwrap()
        );
        assert_eq!(layer.style_document, source.style_document);
        let schema: Vec<String> = source.schema.iter().map(|f| f.name.clone()).collect();
        let expected_columns = if schema.is_empty() {
            ds_cleaning::arrow_ipc::columns_for_features(&source.features)
        } else {
            schema
        };
        assert_eq!(layer.columns, expected_columns);
        let expected_chunks: Vec<usize> = source
            .features
            .chunks(LAYER_IMPORT_FEATURES_PER_CHUNK)
            .map(<[Value]>::len)
            .collect();
        assert_eq!(layer.chunk_feature_counts, expected_chunks);
        let chunks = &reply.chunks[offset..offset + expected_chunks.len()];
        offset += expected_chunks.len();
        for (index, chunk) in chunks.iter().enumerate() {
            let expected = ds_cleaning::arrow_ipc::features_to_ipc(
                &source.features[index * LAYER_IMPORT_FEATURES_PER_CHUNK
                    ..(index * LAYER_IMPORT_FEATURES_PER_CHUNK + expected_chunks[index])],
                &layer.columns,
            )
            .unwrap();
            assert_eq!(chunk, &expected, "chunk {index} of {}", layer.name);
        }
        let decoded = decoded_features(chunks);
        assert_eq!(decoded.len(), source.features.len());
        for (got, want) in decoded.iter().zip(&source.features) {
            // The upload WKB codec is deliberately 2D (elevation travels as a column).
            assert_eq!(got["geometry"], planar(&want["geometry"]));
            for column in &layer.columns {
                let want_value = &want["properties"][column];
                if !want_value.is_null() {
                    assert_eq!(
                        got["properties"][column].to_string().trim_matches('"'),
                        want_value.to_string().trim_matches('"'),
                        "{column}"
                    );
                }
            }
        }
    }
    assert_eq!(offset, reply.chunks.len());
}

fn planar(value: &Value) -> Value {
    match value {
        Value::Array(items) if items.first().is_some_and(Value::is_number) => {
            Value::Array(items.iter().take(2).cloned().collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(planar).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), planar(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn point_collection(count: usize) -> String {
    let features: Vec<Value> = (0..count)
        .map(|index| {
            json!({"type":"Feature","id":format!("p{index}"),"geometry":{"type":"Point","coordinates":[30.0 + index as f64 * 1e-5, -2.0]},"properties":{"name":format!("P{index}"),"kva":index}})
        })
        .collect();
    json!({"type":"FeatureCollection","features":features}).to_string()
}

#[test]
fn geojson_layers_match_the_reader_and_chunk_at_1024_features() {
    let text = point_collection(LAYER_IMPORT_FEATURES_PER_CHUNK + 5);
    let request = LayerImportRequest::GeojsonLayers {
        name: "poles.geojson".into(),
        crs: None,
        center: None,
    };
    let reply = run(&request, &[text.as_bytes()]);
    let envelope = io::geojson_import(io::GeoJsonImportInput {
        text: io::GeoTextImportInput {
            name: "poles.geojson".into(),
            text: text.clone(),
        },
        crs: None,
        center: None,
    })
    .unwrap();
    assert_ipc_parity(envelope, &reply);
    assert_eq!(reply.chunks.len(), 2);
    round_trip(&request, &reply);
}

#[test]
fn geojson_text_drops_a_byte_order_mark_like_file_text() {
    let text = point_collection(2);
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(text.as_bytes());
    let request = LayerImportRequest::GeojsonLayers {
        name: "bom.geojson".into(),
        crs: None,
        center: None,
    };
    let with_bom = run(&request, &[&bytes]);
    let without = run(&request, &[text.as_bytes()]);
    assert_eq!(with_bom.chunks, without.chunks);
}

#[test]
fn pasted_geojson_keeps_the_crs_choice_and_map_center() {
    let text = json!({"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30.06,-1.94]},"properties":{}}]}).to_string();
    for crs in [None, Some("auto"), Some("wgs84")] {
        let request = LayerImportRequest::GeojsonLayers {
            name: "paste.geojson".into(),
            crs: crs.map(str::to_string),
            center: Some(ImportMapCenter {
                lng: 30.06,
                lat: -1.94,
            }),
        };
        let envelope = io::geojson_import(io::GeoJsonImportInput {
            text: io::GeoTextImportInput {
                name: "paste.geojson".into(),
                text: text.clone(),
            },
            crs: crs.map(str::to_string),
            center: Some(io::GeoJsonMapCenter {
                lng: 30.06,
                lat: -1.94,
            }),
        });
        match envelope {
            Ok(envelope) => assert_ipc_parity(envelope, &run(&request, &[text.as_bytes()])),
            Err(message) => {
                let refused = execute(&request, &[text.as_bytes()]).unwrap_err();
                assert_eq!(refused, LayerImportError::Reader(message));
            }
        }
    }
}

#[test]
fn reader_refusals_keep_their_own_message() {
    let text = json!({"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[2,2],[2,0],[0,2],[0,0]]]},"properties":{}}]}).to_string();
    let request = LayerImportRequest::GeojsonLayers {
        name: "parcels.geojson".into(),
        crs: None,
        center: None,
    };
    let expected = io::geojson_import(io::GeoJsonImportInput {
        text: io::GeoTextImportInput {
            name: "parcels.geojson".into(),
            text: text.clone(),
        },
        crs: None,
        center: None,
    })
    .unwrap_err();
    let refused = execute(&request, &[text.as_bytes()]).unwrap_err();
    assert_eq!(refused.to_string(), expected);
    assert!(expected.starts_with("geometry_invalid"), "{expected}");
}

#[test]
fn transformer_backup_json_matches_the_reader() {
    let text = json!({"gdfs":{"poles":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"id":"p1"}}]},"tr":{"type":"FeatureCollection","features":[]}}}).to_string();
    let request = LayerImportRequest::TransformerBackupLayers {
        name: "backup.json".into(),
    };
    let reply = run(&request, &[text.as_bytes()]);
    let envelope = io::transformer_backup_json_to_layer(io::GeoTextImportInput {
        name: "backup.json".into(),
        text,
    })
    .unwrap();
    assert_ipc_parity(envelope, &reply);
    round_trip(&request, &reply);
}

const CITY_KML: &str = r#"
    <kml><Document>
      <Folder><name>poles</name>
        <Placemark><name>P1</name><Point><coordinates>30,-2,0</coordinates></Point></Placemark>
      </Folder>
      <Folder><name>lines</name>
        <Placemark><LineString><coordinates>30,-2,0 30.1,-2,0</coordinates></LineString></Placemark>
      </Folder>
    </Document></kml>"#;

#[test]
fn kml_layers_and_inspection_match_the_reader() {
    let input = || io::GeoTextImportInput {
        name: "city.kml".into(),
        text: CITY_KML.into(),
    };
    let layers = LayerImportRequest::KmlLayers {
        name: "city.kml".into(),
    };
    let reply = run(&layers, &[CITY_KML.as_bytes()]);
    assert_ipc_parity(io::kml_text_to_layer(input()).unwrap(), &reply);
    round_trip(&layers, &reply);

    let inspect = LayerImportRequest::KmlInspect {
        name: "city.kml".into(),
    };
    let reply = run(&inspect, &[CITY_KML.as_bytes()]);
    let LayerImportResponse::Inspection(summary) = round_trip(&inspect, &reply) else {
        panic!("inspection");
    };
    assert_eq!(
        serde_json::to_value(summary).unwrap(),
        serde_json::to_value(io::kml_text_inspect_entry(input()).unwrap()).unwrap()
    );
}

fn export_layers() -> io::GeoLayerExportInput {
    io::GeoLayerExportInput {
        layers: vec![
            io::GeoLayerInput {
                name: "poles".into(),
                geojson: json!({"type":"FeatureCollection","features":[
                    {"type":"Feature","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":"A","kva":50}},
                    {"type":"Feature","geometry":{"type":"Point","coordinates":[30.001,-2.001]},"properties":{"name":"B","kva":100}}
                ]}),
            },
            io::GeoLayerInput {
                name: "lines".into(),
                geojson: json!({"type":"FeatureCollection","features":[
                    {"type":"Feature","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.001,-2.001]]},"properties":{"phase":"LV"}}
                ]}),
            },
        ],
    }
}

#[test]
fn kmz_layers_and_inspection_match_the_reader() {
    let kmz = io::layers_to_kmz(export_layers()).unwrap();
    let layers = LayerImportRequest::KmzLayers {
        name: "network.kmz".into(),
    };
    let reply = run(&layers, &[&kmz]);
    assert_ipc_parity(io::kmz_bytes_to_layer("network.kmz", &kmz).unwrap(), &reply);
    round_trip(&layers, &reply);
    let inspect = LayerImportRequest::KmzInspect {
        name: "network.kmz".into(),
    };
    let LayerImportResponse::Inspection(summary) = run(&inspect, &[&kmz]).response else {
        panic!("inspection");
    };
    assert_eq!(
        serde_json::to_value(summary).unwrap(),
        serde_json::to_value(io::kmz_bytes_inspect_entry("network.kmz", &kmz).unwrap()).unwrap()
    );
}

#[test]
fn archives_and_shapefile_zips_match_the_reader_with_upload_provenance() {
    let zip = io::layers_to_shp_zip(export_layers()).unwrap();
    let layers = LayerImportRequest::ArchiveLayers {
        name: "network.zip".into(),
    };
    let reply = run(&layers, &[&zip]);
    assert_ipc_parity(
        io::upload_provenance::collapse(
            io::archive_bytes_to_layer("network.zip", &zip).unwrap(),
            "network.zip",
        ),
        &reply,
    );
    round_trip(&layers, &reply);

    let inspect = LayerImportRequest::ArchiveInspect {
        name: "network.zip".into(),
    };
    let LayerImportResponse::Inspection(summary) = run(&inspect, &[&zip]).response else {
        panic!("inspection");
    };
    assert_eq!(
        serde_json::to_value(summary).unwrap(),
        serde_json::to_value(io::upload_provenance::collapse_inspection(
            io::archive_bytes_inspect("network.zip", &zip).unwrap(),
            "network.zip"
        ))
        .unwrap()
    );

    let request = LayerImportRequest::ShapefileZip {
        name: "zones.zip".into(),
    };
    let reply = run(&request, &[&zip]);
    let LayerImportResponse::Envelope(envelope) = round_trip(&request, &reply) else {
        panic!("envelope");
    };
    assert_eq!(
        serde_json::to_value(envelope).unwrap(),
        serde_json::to_value(io::shp_zip_bytes_to_layer("zones.zip", &zip).unwrap()).unwrap()
    );
}

fn zip_member(zip: &[u8], suffix: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        if file.name().starts_with("poles") && file.name().ends_with(suffix) {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            return bytes;
        }
    }
    panic!("no {suffix} member");
}

#[test]
fn loose_shapefile_parts_match_the_reader_and_empty_sidecars_are_absent() {
    let zip = io::layers_to_shp_zip(export_layers()).unwrap();
    let (shp, dbf, prj) = (
        zip_member(&zip, ".shp"),
        zip_member(&zip, ".dbf"),
        zip_member(&zip, ".prj"),
    );
    let request = LayerImportRequest::Shapefile {
        name: "poles".into(),
    };
    for (dbf_part, prj_part) in [(&dbf[..], &prj[..]), (&[][..], &[][..])] {
        let reply = run(&request, &[&shp, dbf_part, prj_part]);
        let expected = io::shp_dbf_prj_bytes_to_layer(
            "poles",
            &shp,
            (!dbf_part.is_empty()).then_some(dbf_part),
            (!prj_part.is_empty()).then_some(prj_part),
        )
        .unwrap();
        let LayerImportResponse::Envelope(envelope) = round_trip(&request, &reply) else {
            panic!("envelope");
        };
        assert_eq!(
            serde_json::to_value(envelope).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    let refused = execute(&request, &[&shp]).unwrap_err();
    assert!(
        refused
            .to_string()
            .starts_with("layer_import_request_invalid: expected 3 byte part(s)"),
        "{refused}"
    );
}

#[test]
fn pls_cadd_backup_layers_and_probe_match_the_reader() {
    let backup =
        io::pls_cadd_author_humble_pole_backup(&io::PlsCaddHumbleFixtureOptions::default())
            .unwrap();
    for options in [
        io::PlsCaddImportOptions::default(),
        serde_json::from_value(json!({"includeTerrain": false})).unwrap(),
    ] {
        let request = LayerImportRequest::PlsCaddBackupLayers {
            name: "humble-pole.bak".into(),
            options,
        };
        let reply = run(&request, &[&backup]);
        assert_ipc_parity(
            io::pls_cadd_backup_to_layers_with("humble-pole.bak", &backup, options).unwrap(),
            &reply,
        );
        round_trip(&request, &reply);
    }
    let probe = |bytes: &[u8]| match run(&LayerImportRequest::PlsCaddBackupProbe, &[bytes]).response
    {
        LayerImportResponse::BackupProbe(value) => value,
        other => panic!("{other:?}"),
    };
    assert!(probe(&backup));
    assert_eq!(probe(&backup), io::pls_cadd_is_backup_container(&backup));
    assert!(!probe(b"not a backup"));
}

fn staking_table() -> Vec<u8> {
    let (e0, n0) = ds_geo_lite::projection::wgs84_to_nix_itrf2005(30.0588, -1.9441);
    let (e1, n1) = ds_geo_lite::projection::wgs84_to_nix_itrf2005(30.0599, -1.9450);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<pls_cadd>
<staking_table><structure_number>S1</structure_number><x_easting>{e0:.4}</x_easting><y_northing>{n0:.4}</y_northing><station>0.0</station></staking_table>
<staking_table><structure_number>S2</structure_number><x_easting>{e1:.4}</x_easting><y_northing>{n1:.4}</y_northing><station>150.0</station></staking_table>
</pls_cadd>"#
    )
    .into_bytes()
}

#[test]
fn pls_cadd_staking_tables_match_the_reader() {
    let xml = staking_table();
    let request = LayerImportRequest::PlsCaddXmlLayers {
        name: "line.xml".into(),
    };
    let reply = run(&request, &[&xml]);
    assert_ipc_parity(
        io::pls_cadd_xml_to_layer(io::PlsCaddXmlImportInput {
            name: "line.xml".into(),
            bytes: xml.clone(),
            input_projection: None,
        })
        .unwrap(),
        &reply,
    );
    round_trip(&request, &reply);
    let inspect = LayerImportRequest::PlsCaddXmlInspect {
        name: "line.xml".into(),
    };
    let LayerImportResponse::Inspection(summary) = run(&inspect, &[&xml]).response else {
        panic!("inspection");
    };
    assert_eq!(
        serde_json::to_value(summary).unwrap(),
        serde_json::to_value(io::pls_cadd_xml_inspect("line.xml", &xml).unwrap()).unwrap()
    );
}

const SITES_CSV: &[u8] =
    b"Site,Longitude,Latitude,kVA\nA,30.06,-1.94,50\nB,30.07,-1.95,100\nTotal,,,150\n";

fn source_input() -> io::SourceLayerInput {
    serde_json::from_value(
        json!({"sheetKey":"0","crs":"wgs84_lonlat","xColumn":"Longitude","yColumn":"Latitude"}),
    )
    .unwrap()
}

#[test]
fn tabular_sources_match_the_reader() {
    let inspect = LayerImportRequest::SourceInspect {
        name: "sites.csv".into(),
        separator: None,
    };
    let reply = run(&inspect, &[SITES_CSV]);
    let LayerImportResponse::SourceInspection(summary) = round_trip(&inspect, &reply) else {
        panic!("source inspection");
    };
    let expected =
        io::source_inspect("sites.csv", SITES_CSV, io::SourceInspectInput::default()).unwrap();
    assert_eq!(
        serde_json::to_value(&summary).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    let mut input = source_input();
    input.sheet_key = expected.sheets[0].key.clone();

    let layer = LayerImportRequest::SourceLayer {
        name: "sites.csv".into(),
        input: input.clone(),
    };
    let reply = run(&layer, &[SITES_CSV]);
    let LayerImportResponse::SourceLayer(result) = round_trip(&layer, &reply) else {
        panic!("source layer");
    };
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::to_value(io::source_to_layer("sites.csv", SITES_CSV, input.clone()).unwrap())
            .unwrap()
    );

    let layers = LayerImportRequest::SourceLayers {
        name: "sites.csv".into(),
        input: input.clone(),
    };
    let reply = run(&layers, &[SITES_CSV]);
    assert_ipc_parity(
        io::upload_provenance::collapse(
            io::source_to_layer("sites.csv", SITES_CSV, input)
                .unwrap()
                .envelope,
            "sites.csv",
        ),
        &reply,
    );
    round_trip(&layers, &reply);
}

#[test]
fn sketch_features_match_the_assembler() {
    let features = json!([
        {"id":"pole-42","type":"Feature","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":"T1","kva":50}},
        {"type":"Feature","geometry":null,"properties":{"name":"dropped"}},
        {"type":"Feature","geometry":{"type":"Point","coordinates":[30.1,-2.0]},"properties":{"nested":{"k":1}}}
    ]);
    let request = LayerImportRequest::SketchFeatures {
        columns: vec!["name".into(), "kva".into(), "nested".into()],
        id_prefix: "feat-x".into(),
    };
    let bytes = serde_json::to_vec(&features).unwrap();
    let reply = run(&request, &[&bytes]);
    let LayerImportResponse::SketchFeatures(output) = round_trip(&request, &reply) else {
        panic!("sketch features");
    };
    assert_eq!(
        output,
        io::sketch::sketch_features_from_layer(io::sketch::SketchFeaturesInput {
            features: features.as_array().unwrap().clone(),
            columns: vec!["name".into(), "kva".into(), "nested".into()],
            id_prefix: "feat-x".into(),
        })
    );
    assert_eq!(output.features.len(), 2);
    assert_eq!(output.features[1].id, "feat-x-2");
}

#[test]
fn malformed_requests_are_refused_before_any_reader_runs() {
    for bytes in [
        &b"{bad"[..],
        br#"{"operation":"invented","name":"x"}"#,
        br#"{"operation":"kmz_layers"}"#,
        br#"{"operation":"kmz_layers","name":"  "}"#,
        br#"{"operation":"geojson_layers","name":"x","center":{"lng":"east","lat":0}}"#,
        br#"{"operation":"sketch_features","columns":["a","a"],"id_prefix":"p"}"#,
    ] {
        let refused = decode_request(bytes).unwrap_err();
        assert!(
            refused
                .to_string()
                .starts_with("layer_import_request_invalid:"),
            "{refused}"
        );
    }
    let kmz = LayerImportRequest::KmzLayers {
        name: "a.kmz".into(),
    };
    assert!(matches!(
        execute(&kmz, &[b"PK", b"extra"]),
        Err(LayerImportError::InvalidRequest(_))
    ));
    let sketch = LayerImportRequest::SketchFeatures {
        columns: vec![],
        id_prefix: "p".into(),
    };
    assert!(matches!(
        execute(&sketch, &[b"{not json"]),
        Err(LayerImportError::InvalidRequest(_))
    ));
}

#[test]
fn compatible_additions_are_accepted_in_both_directions() {
    let request = decode_request(
        br#"{"operation":"kmz_layers","name":"network.kmz","future_hint":{"enabled":true}}"#,
    )
    .expect("an added request field is compatible");
    let kmz = io::layers_to_kmz(export_layers()).unwrap();
    let reply = execute(&request, &[&kmz]).unwrap();
    let mut wire = serde_json::to_value(&reply.response).unwrap();
    wire["future_reply_field"] = json!(1);
    wire["result"][0]["futureLayerField"] = json!({"x": 1});
    let decoded = decode_response(
        &request,
        &serde_json::to_vec(&wire).unwrap(),
        &heads(&reply.chunks),
    )
    .expect("additive reply fields are compatible");
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::to_value(&reply.response).unwrap()
    );
}

#[test]
fn incompatible_or_malformed_replies_are_refused_by_the_decoder() {
    let request = LayerImportRequest::GeojsonLayers {
        name: "poles.geojson".into(),
        crs: None,
        center: None,
    };
    let text = point_collection(3);
    let reply = run(&request, &[text.as_bytes()]);
    let good = serde_json::to_value(&reply.response).unwrap();
    let chunk_heads = heads(&reply.chunks);
    let refuse = |wire: Value, heads: &[(usize, [u8; 6], [u8; 6])]| {
        let error = decode_response(&request, &serde_json::to_vec(&wire).unwrap(), heads)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("layer_import_response_invalid:"),
            "{error}"
        );
    };
    let mut wrong_kind = good.clone();
    wrong_kind["kind"] = json!("inspection");
    refuse(wrong_kind, &chunk_heads);
    let mut wrong_count = good.clone();
    wrong_count["result"][0]["featureCount"] = json!(4);
    refuse(wrong_count, &chunk_heads);
    let mut oversized = good.clone();
    oversized["result"][0]["chunkFeatureCounts"] = json!([LAYER_IMPORT_FEATURES_PER_CHUNK + 1]);
    oversized["result"][0]["featureCount"] = json!(LAYER_IMPORT_FEATURES_PER_CHUNK + 1);
    refuse(oversized, &chunk_heads);
    let mut missing_field = good.clone();
    missing_field["result"][0]
        .as_object_mut()
        .unwrap()
        .remove("columns");
    refuse(missing_field, &chunk_heads);
    let mut duplicate_column = good.clone();
    duplicate_column["result"][0]["columns"] = json!(["name", "name"]);
    refuse(duplicate_column, &chunk_heads);
    refuse(good.clone(), &[]);
    refuse(good.clone(), &[chunk_heads[0], chunk_heads[0]]);
    refuse(good.clone(), &[chunk_head(b"PK\x03\x04 not arrow")]);
    refuse(good.clone(), &[chunk_head(b"")]);

    let zones = LayerImportRequest::ShapefileZip {
        name: "z.zip".into(),
    };
    let envelope = json!({"kind":"envelope","result":{"source":"z","layers":[{"name":"z","features":[{"geometry":null}],"schema":[]}]}});
    let error = decode_response(&zones, &serde_json::to_vec(&envelope).unwrap(), &[]).unwrap_err();
    assert!(
        matches!(error, LayerImportError::InvalidResponse(_)),
        "{error}"
    );
    let probe = json!({"kind":"backup_probe","result":true});
    let error = decode_response(
        &LayerImportRequest::PlsCaddBackupProbe,
        &serde_json::to_vec(&probe).unwrap(),
        &chunk_heads,
    )
    .unwrap_err();
    assert!(
        matches!(error, LayerImportError::InvalidResponse(_)),
        "{error}"
    );
}

#[test]
fn streaming_geojson_matches_the_reader_and_its_steps_validate() {
    let text = point_collection(LAYER_IMPORT_FEATURES_PER_CHUNK + 2);
    let mut import = GeoJsonStreamImport::new("big.geojson").unwrap();
    let mut reader = io::GeoJsonFeatureStream::new("big.geojson");
    let mut emitted = Vec::new();
    let mut expected = Vec::new();
    for piece in text.as_bytes().chunks(4096) {
        let (update, chunks) = import.push(piece).unwrap();
        let update = validate_stream_update(update, &heads(&chunks)).unwrap();
        assert!(!update.done || update.feature_count == LAYER_IMPORT_FEATURES_PER_CHUNK + 2);
        emitted.extend(chunks);
        expected.extend(reader.push(piece).unwrap());
    }
    let (update, chunks) = import.finish().unwrap();
    let update = validate_stream_update(update, &heads(&chunks)).unwrap();
    emitted.extend(chunks);
    expected.extend(reader.finish().unwrap());
    assert!(update.done);
    assert_eq!(update.name, "big");
    assert_eq!(update.feature_count, LAYER_IMPORT_FEATURES_PER_CHUNK + 2);
    assert_eq!(update.geometry_type.as_deref(), Some("Point"));
    assert_eq!(update.columns, reader.columns());
    assert_eq!(emitted.len(), expected.len());
    for (bytes, batch) in emitted.iter().zip(&expected) {
        assert_eq!(
            bytes,
            &ds_cleaning::arrow_ipc::features_to_ipc(&batch.features, &batch.columns).unwrap()
        );
    }
    assert_eq!(
        decoded_features(&emitted).len(),
        LAYER_IMPORT_FEATURES_PER_CHUNK + 2
    );

    let malformed = GeoJsonStreamUpdate {
        chunk_feature_counts: vec![3],
        ..update.clone()
    };
    assert!(validate_stream_update(malformed, &[]).is_err());
    let overclaimed = GeoJsonStreamUpdate {
        feature_count: 1,
        chunk_feature_counts: vec![2],
        ..update
    };
    assert!(validate_stream_update(overclaimed, &heads(&emitted[..1])).is_err());
    assert!(GeoJsonStreamImport::new(" ").is_err());
    let mut broken = GeoJsonStreamImport::new("broken.geojson").unwrap();
    broken
        .push(br#"{"type":"FeatureCollection","features":[{"#)
        .unwrap();
    assert!(matches!(broken.finish(), Err(LayerImportError::Reader(_))));
}

#[test]
fn temporary_upload_groups_match_the_shared_plan_and_validate_members() {
    let wire = json!({"mode":"layer","sources":[
        {"fileName":"one.zip","layers":[{"name":"a/roads.shp","geometryType":"Point","featureCount":1,"features":[{"properties":{"_ds_source_file":"original"}}],"columns":["id"]}]},
        {"fileName":"two.zip","layers":[{"name":"roads.shp","geometryType":"LineString","featureCount":2,"features":[],"columns":[],"chunkSources":[{"fileName":"two.zip","parsedLayer":"roads"}],"sourceLayers":[{"sourceFileName":"two.zip","sourceLayerName":"roads.shp","featureCount":2,"chunkKey":"k"}]}]}
    ]});
    let request: UploadGroupsRequest = serde_json::from_value(wire).unwrap();
    let groups = temporary_upload_groups(&request).unwrap();
    let expected = ds_cleaning::upload_groups::plan(
        serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap(),
    );
    assert_eq!(serde_json::to_value(&groups).unwrap(), expected);
    assert_eq!(groups[0].name, "roads_Point");
    assert_eq!(groups[1].name, "roads_LineString");
    assert_eq!(
        groups[1].source_layers[0].reference.extra["chunkKey"],
        json!("k")
    );
    assert_eq!(groups[1].source_layers[0].member, [1, 0, 0]);
    assert_eq!(groups[1].chunk_sources[0].parsed_layer, "roads");

    let mut forged = groups.clone();
    forged[0].members.push([9, 0]);
    assert!(matches!(
        validate_upload_groups(&request, forged),
        Err(LayerImportError::InvalidResponse(_))
    ));
    let mut shape = groups.clone();
    shape[0].geometry_type = "Circle".into();
    assert!(validate_upload_groups(&request, shape).is_err());
    let mut empty = groups;
    empty[0].members.clear();
    assert!(validate_upload_groups(&request, empty).is_err());
    assert!(
        serde_json::from_value::<UploadGroupsRequest>(json!({"mode":"merge","sources":[]}))
            .is_err()
    );
}

/// This build enables serde_json `preserve_order` (source key order); the
/// readers' default build and the retired Network export iterate keys sorted.
/// Columns, Arrow schemas and layer order follow sorted keys either way, and an
/// already-sorted control gives the same answer.
#[test]
fn columns_and_layers_follow_sorted_keys_in_every_build() {
    let unsorted = json!({"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"zeta":1,"alpha":"a","mid":true,"nested":{"b":1,"a":2}}},
        {"type":"Feature","geometry":{"type":"Point","coordinates":[30.1,-2.0]},"properties":{"beta":2.5,"zeta":3}}
    ]})
    .to_string();
    assert!(
        unsorted.contains(r#""zeta":1,"alpha""#),
        "the fixture is unsorted"
    );
    let mut control: Value = serde_json::from_str(&unsorted).unwrap();
    control.sort_all_objects();
    let control = control.to_string();
    let request = LayerImportRequest::GeojsonLayers {
        name: "order.geojson".into(),
        crs: None,
        center: None,
    };
    let read = |text: &str| {
        let reply = run(&request, &[text.as_bytes()]);
        let LayerImportResponse::IpcLayers(layers) = &reply.response else {
            panic!("layers");
        };
        let features = decoded_features(&reply.chunks);
        // Decoded rows omit null cells; their union in first-seen order is the schema.
        let mut ipc_columns: Vec<String> = Vec::new();
        for feature in &features {
            for key in feature["properties"].as_object().unwrap().keys() {
                if !ipc_columns.contains(key) {
                    ipc_columns.push(key.clone());
                }
            }
        }
        (
            layers[0].columns.clone(),
            ipc_columns,
            reply.chunks.clone(),
            features,
        )
    };
    let (columns, ipc_columns, chunks, features) = read(&unsorted);
    assert_eq!(columns, ["alpha", "mid", "nested", "zeta", "beta"]);
    assert_eq!(ipc_columns, columns, "the Arrow schema follows the columns");
    assert_eq!(features[0]["properties"]["nested"], r#"{"a":2,"b":1}"#);
    let (control_columns, _, control_chunks, _) = read(&control);
    assert_eq!(control_columns, columns);
    assert_eq!(
        control_chunks, chunks,
        "unsorted and sorted sources encode identically"
    );

    let backup = json!({"gdfs":{
        "tr":{"type":"FeatureCollection","features":[]},
        "lv_poles":{"type":"FeatureCollection","features":[]},
        "customers":{"type":"FeatureCollection","features":[]}
    }})
    .to_string();
    let reply = run(
        &LayerImportRequest::TransformerBackupLayers {
            name: "backup.json".into(),
        },
        &[backup.as_bytes()],
    );
    let LayerImportResponse::IpcLayers(layers) = reply.response else {
        panic!("layers");
    };
    let names: Vec<&str> = layers.iter().map(|layer| layer.name.as_str()).collect();
    assert_eq!(names, ["customers", "lv_poles", "tr"]);

    let sketch = run(
        &LayerImportRequest::SketchFeatures {
            columns: vec!["nested".into()],
            id_prefix: "feat".into(),
        },
        &[br#"[{"type":"Feature","geometry":{"type":"Point","coordinates":[30,-2]},"properties":{"nested":{"b":1,"a":2}}}]"#],
    );
    let LayerImportResponse::SketchFeatures(output) = sketch.response else {
        panic!("sketch");
    };
    assert_eq!(output.features[0].properties["nested"], r#"{"a":2,"b":1}"#);
}
