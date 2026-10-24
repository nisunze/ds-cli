use super::*;
#[test]
fn converted_rows_are_bounded_and_geometry_keeps_properties() {
    let source = br#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"city":"A","phase":"II"},"geometry":{"type":"LineString","coordinates":[[15,8],[15.1,8.2]]}},{"type":"Feature","properties":{"city":"B","phase":"I"},"geometry":null}]}"#;
    let converted = crate::source_engine::fixture_geoparquet("routes.geojson", source);
    let path = std::env::temp_dir().join(format!(
        "ds-inspect-{}-{}.parquet",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, &converted.parquet).unwrap();
    let meta = inspect_parquet(&path, 0, 0, false).unwrap();
    assert_eq!(meta["total"], 2);
    assert_eq!(meta["returned"], 0);
    assert_eq!(
        meta["summary"]["source_digest"],
        converted.receipt.source_digest
    );
    let bounded = inspect_parquet(&path, 0, 1, true).unwrap();
    assert_eq!(bounded["truncated"], true);
    assert_eq!(bounded["rows"][0]["properties"]["phase"], "II");
    assert_eq!(
        bounded["rows"][0]["geometry"]["coordinates"],
        json!([[15., 8.], [15.1, 8.2]])
    );
    let full = inspect_parquet(&path, 0, 3, true).unwrap();
    assert_eq!(full["truncated"], false);
    assert!(full["rows"][1]["geometry"].is_null());
    std::fs::remove_file(path).unwrap();
}

// Feedback 46dec14e: a 10,748-point layer returned its first 5,000 rows
// and nothing could read the rest, so the GeoJSON had to be assembled
// outside ds. --offset pages through every row exactly once.
#[test]
fn converted_rows_page_through_a_layer_larger_than_one_request() {
    let features: Vec<String> = (0..12_000)
        .map(|index| {
            format!(
                r#"{{"type":"Feature","properties":{{"n":{index}}},"geometry":{{"type":"Point","coordinates":[{},{}]}}}}"#,
                29.0 + index as f64 * 1e-5,
                -2.0
            )
        })
        .collect();
    let source = format!(
        r#"{{"type":"FeatureCollection","features":[{}]}}"#,
        features.join(",")
    );
    let converted =
        crate::source_engine::fixture_geoparquet("customers.geojson", source.as_bytes());
    let path = std::env::temp_dir().join(format!(
        "ds-inspect-pages-{}-{}.parquet",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, &converted.parquet).unwrap();
    let mut offset = 0usize;
    let mut seen = Vec::new();
    let mut pages = Vec::new();
    loop {
        let page = inspect_parquet(&path, offset, 5000, true).unwrap();
        assert_eq!(page["total"], 12_000);
        assert_eq!(page["offset"], offset);
        for row in page["rows"].as_array().unwrap() {
            seen.push(row["properties"]["n"].as_u64().unwrap());
            assert_eq!(row["geometry"]["type"], "Point");
        }
        pages.push(page["returned"].as_u64().unwrap());
        match page["next_offset"].as_u64() {
            Some(next) => {
                assert_eq!(page["truncated"], true);
                offset = next as usize;
            }
            None => {
                assert_eq!(page["truncated"], false);
                break;
            }
        }
    }
    assert_eq!(pages, [5000, 5000, 2000]);
    assert_eq!(seen, (0..12_000).collect::<Vec<u64>>());
    let past_end = inspect_parquet(&path, 20_000, 10, false).unwrap();
    assert_eq!(past_end["returned"], 0);
    assert!(past_end["next_offset"].is_null());
    std::fs::remove_file(path).unwrap();
}

/// One WGS84 point with one text attribute, as the three files a GIS
/// tool writes: `.shp`, `.shx` and `.dbf`.
fn one_point_shapefile(x: f64, y: f64, name: &str) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let header = |words: i32| {
        let mut bytes = Vec::with_capacity(100);
        bytes.extend(9994i32.to_be_bytes());
        bytes.extend([0u8; 20]);
        bytes.extend(words.to_be_bytes());
        bytes.extend(1000i32.to_le_bytes());
        bytes.extend(1i32.to_le_bytes());
        for value in [x, y, x, y, 0.0, 0.0, 0.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes
    };
    let mut shp = header(64);
    shp.extend(1i32.to_be_bytes());
    shp.extend(10i32.to_be_bytes());
    shp.extend(1i32.to_le_bytes());
    shp.extend(x.to_le_bytes());
    shp.extend(y.to_le_bytes());
    let mut shx = header(54);
    shx.extend(50i32.to_be_bytes());
    shx.extend(10i32.to_be_bytes());
    let mut dbf = vec![0x03, 126, 10, 9];
    dbf.extend(1u32.to_le_bytes());
    dbf.extend(65u16.to_le_bytes());
    dbf.extend(11u16.to_le_bytes());
    dbf.extend([0u8; 20]);
    let mut field = [0u8; 32];
    field[..4].copy_from_slice(b"NAME");
    field[11] = b'C';
    field[16] = 10;
    dbf.extend(field);
    dbf.push(0x0D);
    dbf.push(b' ');
    dbf.extend(format!("{name:<10}").bytes());
    dbf.push(0x1A);
    (shp, shx, dbf)
}

// Feedback 46dec14e / ad44f3a0: `ds data inspect` and `ds data convert`
// refused a bare .shp whose sidecars sat beside it.
#[test]
fn a_bare_shapefile_is_read_with_the_sidecars_beside_it() {
    let folder = std::env::temp_dir().join(format!(
        "ds-inspect-shp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&folder).unwrap();
    let (shp, shx, dbf) = one_point_shapefile(29.55, -2.47, "Bushengo");
    std::fs::write(folder.join("customers.shp"), shp).unwrap();
    std::fs::write(folder.join("customers.shx"), shx).unwrap();
    std::fs::write(folder.join("Customers.DBF"), dbf).unwrap();
    let source = folder.join("customers.shp");
    let (name, bytes) = read_geometry_source(source.to_str().unwrap()).unwrap();
    assert_eq!(name, "customers.shp.zip");
    // The zipped shapefile is what the engine's archive reader takes.
    let engine = crate::source_engine::capture().unwrap();
    let reader = crate::source_engine::geometry_reader(&name);
    let layers = engine
        .source_inspect(
            &name,
            bytes.clone(),
            SourceInspectV1::Layers(reader.clone()),
            &|| false,
        )
        .unwrap()
        .layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].name, "customers");
    assert_eq!(layers[0].feature_count, 1);
    let read = engine
        .source_layers(&name, bytes, reader, &|| false)
        .unwrap();
    assert_eq!(read.layers[0].features.len(), 1);

    let missing = read_geometry_source(folder.join("absent.shp").to_str().unwrap())
        .expect_err("an absent .shp is refused");
    assert_eq!(missing.code(), "source_unreadable");
    std::fs::remove_dir_all(folder).unwrap();
}
