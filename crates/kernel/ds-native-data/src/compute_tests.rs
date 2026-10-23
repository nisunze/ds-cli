use super::*;
use std::io::{Cursor, Write};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn a_dropped_shapefile_brings_the_companions_beside_it() {
    // Feedback ad44f3a0: dropping overall_surveyed_customers.shp alone was
    // refused because its .dbf never came with it.
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "Customers.SHP",
        "customers.dbf",
        "CUSTOMERS.shx",
        "customers.prj",
        "customers.cpg",
        "customers.shp.xml",
        "customers_2.dbf",
        "customers.txt",
        "other.dbf",
    ] {
        std::fs::write(dir.path().join(name), b"x").unwrap();
    }
    std::fs::create_dir(dir.path().join("customers.qix")).unwrap();
    let found = shapefile_companions(&dir.path().join("Customers.SHP")).unwrap();
    let mut names: Vec<String> = found
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "CUSTOMERS.shx",
            "customers.cpg",
            "customers.dbf",
            "customers.prj",
            "customers.shp.xml"
        ]
    );
    assert!(shapefile_companions(&dir.path().join("customers.dbf")).is_err());
}

fn temp_source(contents: &[u8]) -> (PathBuf, PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ds-web-native-tabular-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("points.txt");
    let mut file = File::create(&path).unwrap();
    file.write_all(contents).unwrap();
    file.sync_all().unwrap();
    (root, path)
}

#[test]
fn file_location_requires_an_existing_absolute_regular_file() {
    let (root, path) = temp_source(b"{}\n");
    let expected = std::fs::canonicalize(&path).unwrap();
    assert_eq!(canonical_source_file(&path).unwrap(), expected);
    assert!(canonical_source_file(Path::new("relative.geojson"))
        .unwrap_err()
        .contains("absolute"));
    assert!(canonical_source_file(&root)
        .unwrap_err()
        .contains("not a file"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_host_pool_reaches_every_process_visible_logical_cpu() {
    let wanted = std::thread::available_parallelism().map_or(1, usize::from);
    let runtime = initialize_native_host_pool().expect("initialize native Rayon engine");

    assert_eq!(runtime.environment, ExecutionEnvironment::Native);
    assert_eq!(runtime.logical_cpus, wanted);
    assert_eq!(runtime.internal_worker_threads, wanted);

    let mut workers = rayon::broadcast(|context| {
        (
            context.index(),
            rayon::current_thread_index(),
            rayon::current_num_threads(),
        )
    });
    workers.sort_unstable_by_key(|(index, _, _)| *index);

    assert_eq!(workers.len(), wanted);
    assert_eq!(
        workers,
        (0..wanted)
            .map(|index| (index, Some(index), wanted))
            .collect::<Vec<_>>()
    );
}

#[test]
fn bounded_chunks_keep_complete_lines_and_absolute_row_numbers() {
    let (root, path) = temp_source(b"x y z\r\n30.0 -2.0 10\r\n30.1 -2.1 11\r\n30.2 -2.2 12\r\n");
    let size = std::fs::metadata(&path).unwrap().len();
    let request = NativeTabularImportRequest {
        source_path: path,
        file_name: "points.txt".to_string(),
        expected_size_bytes: Some(size),
        expected_modified_unix_ms: None,
        expected_quick_sha256: None,
        data_start: 1,
        columns: vec!["x".to_string(), "y".to_string(), "z".to_string()],
        z_column: Some("z".to_string()),
        input: SourceReadingV1 {
            sheet_key: "text".to_string(),
            separator: Some("whitespace".to_string()),
            header_row: None,
            x_column: Some("x".to_string()),
            y_column: Some("y".to_string()),
            crs: "wgs84_lonlat".to_string(),
            utm_zone: None,
            utm_southern: None,
            attributes_only: false,
            layer_name: Some("points".to_string()),
            column_renames: vec![],
            drop_columns: vec![],
        },
    };
    let mut session = open_tabular_session(&root.join("columnar"), request).unwrap();
    let mut features = Vec::new();
    while let Some(ipc) = next_tabular_chunk(&mut session, 7).unwrap() {
        features.extend(ds_columnar::ipc_to_rows(&ipc).unwrap());
    }

    assert_eq!(features.len(), 3);
    let rows = features
        .iter()
        .map(|row| row["_ds_import_row"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows, vec![2, 3, 4]);
    assert_eq!(features[2]["elevation_m"].as_str(), Some("12"));
    assert_eq!(session.valid_rows, 3);
    assert_eq!(session.invalid_rows, 0);
    assert!(session.done);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn changed_source_size_is_rejected_before_streaming() {
    let (root, path) = temp_source(b"x,y\n30,-2\n");
    let request = NativeTabularImportRequest {
        source_path: path,
        file_name: "points.txt".to_string(),
        expected_size_bytes: Some(999),
        expected_modified_unix_ms: None,
        expected_quick_sha256: None,
        data_start: 1,
        columns: vec![],
        z_column: None,
        input: SourceReadingV1 {
            sheet_key: "text".to_string(),
            separator: Some(",".to_string()),
            header_row: None,
            x_column: Some("x".to_string()),
            y_column: Some("y".to_string()),
            crs: "wgs84_lonlat".to_string(),
            utm_zone: None,
            utm_southern: None,
            attributes_only: false,
            layer_name: None,
            column_renames: vec![],
            drop_columns: vec![],
        },
    };
    let error = match open_tabular_session(&root.join("columnar"), request) {
        Ok(_) => panic!("changed source size should have been rejected"),
        Err(error) => error,
    };
    assert!(error.contains("source changed after it was selected"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn quoted_newlines_stay_inside_one_logical_csv_record() {
    let (root, path) = temp_source(b"x,y,note\n30,-2,\"hello\nworld\"\n31,-3,plain\n");
    let size = std::fs::metadata(&path).unwrap().len();
    let request = NativeTabularImportRequest {
        source_path: path,
        file_name: "points.csv".to_string(),
        expected_size_bytes: Some(size),
        expected_modified_unix_ms: None,
        expected_quick_sha256: None,
        data_start: 1,
        columns: vec!["x".to_string(), "y".to_string(), "note".to_string()],
        z_column: None,
        input: SourceReadingV1 {
            sheet_key: "csv".to_string(),
            separator: Some(",".to_string()),
            header_row: None,
            x_column: Some("x".to_string()),
            y_column: Some("y".to_string()),
            crs: "wgs84_lonlat".to_string(),
            utm_zone: None,
            utm_southern: None,
            attributes_only: false,
            layer_name: Some("points".to_string()),
            column_renames: vec![],
            drop_columns: vec![],
        },
    };
    let mut session = open_tabular_session(&root.join("columnar"), request).unwrap();
    let mut features = Vec::new();
    while let Some(ipc) = next_tabular_chunk(&mut session, 1).unwrap() {
        features.extend(ds_columnar::ipc_to_rows(&ipc).unwrap());
    }

    assert_eq!(features.len(), 2);
    assert_eq!(features[0]["note"], "hello\nworld");
    assert_eq!(
        features
            .iter()
            .map(|row| row["_ds_import_row"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![2, 3],
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn arrow_batches_never_exceed_the_shared_1024_row_contract() {
    let mut text = String::from("x y\n");
    for index in 0..1_100 {
        text.push_str(&format!("{} {}\n", 30.0 + index as f64 / 10_000.0, -2.0));
    }
    let (root, path) = temp_source(text.as_bytes());
    let size = std::fs::metadata(&path).unwrap().len();
    let request = NativeTabularImportRequest {
        source_path: path,
        file_name: "points.txt".to_string(),
        expected_size_bytes: Some(size),
        expected_modified_unix_ms: None,
        expected_quick_sha256: None,
        data_start: 1,
        columns: vec!["x".to_string(), "y".to_string()],
        z_column: None,
        input: SourceReadingV1 {
            sheet_key: "text".to_string(),
            separator: Some("whitespace".to_string()),
            header_row: None,
            x_column: Some("x".to_string()),
            y_column: Some("y".to_string()),
            crs: "wgs84_lonlat".to_string(),
            utm_zone: None,
            utm_southern: None,
            attributes_only: false,
            layer_name: Some("points".to_string()),
            column_renames: vec![],
            drop_columns: vec![],
        },
    };
    let mut session = open_tabular_session(&root.join("columnar"), request).unwrap();
    let first = next_tabular_chunk(&mut session, TABULAR_CHUNK_BYTES)
        .unwrap()
        .unwrap();
    let second = next_tabular_chunk(&mut session, TABULAR_CHUNK_BYTES)
        .unwrap()
        .unwrap();
    assert_eq!(ds_columnar::ipc_to_rows(&first).unwrap().len(), 1_024);
    assert_eq!(ds_columnar::ipc_to_rows(&second).unwrap().len(), 76);
    assert_eq!(session.valid_rows, 1_100);
    assert!(session.done);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rwanda_nix_xyz_rows_project_and_keep_elevation() {
    let (root, path) = temp_source(b"x y z\n562780 4730800 1325.726\n");
    let size = std::fs::metadata(&path).unwrap().len();
    let request = NativeTabularImportRequest {
        source_path: path,
        file_name: "rwanda_sub1_orthocut.txt".to_string(),
        expected_size_bytes: Some(size),
        expected_modified_unix_ms: None,
        expected_quick_sha256: None,
        data_start: 1,
        columns: vec!["x".to_string(), "y".to_string(), "z".to_string()],
        z_column: Some("z".to_string()),
        input: SourceReadingV1 {
            sheet_key: "text".to_string(),
            separator: Some("whitespace".to_string()),
            header_row: None,
            x_column: Some("x".to_string()),
            y_column: Some("y".to_string()),
            crs: "nix_itrf2005".to_string(),
            utm_zone: None,
            utm_southern: None,
            attributes_only: false,
            layer_name: Some("rwanda_sub1_orthocut".to_string()),
            column_renames: vec![],
            drop_columns: vec![],
        },
    };
    let mut session = open_tabular_session(&root.join("columnar"), request).unwrap();
    let ipc = next_tabular_chunk(&mut session, TABULAR_CHUNK_BYTES)
        .unwrap()
        .unwrap();
    let features = ds_columnar::ipc_to_rows(&ipc).unwrap();
    let geometries = ds_columnar::ipc_to_geometry_wkb(&ipc).unwrap();
    let (lon, lat) = ds_columnar::point_xy(geometries[0].as_deref().expect("row 0 has a geometry"))
        .expect("NIX x/y projected to a WGS84 point");
    assert!((28.0..32.0).contains(&lon), "lon {lon} is outside Rwanda");
    assert!((-4.0..1.0).contains(&lat), "lat {lat} is outside Rwanda");
    assert_eq!(features[0]["elevation_m"].as_str(), Some("1325.726"),);
    std::fs::remove_dir_all(root).unwrap();
}

/// The request the import builds for a large delimited source, from the
/// inspection the operator confirmed.
fn xyz_request(path: PathBuf, file_name: &str, size: u64) -> NativeTabularImportRequest {
    NativeTabularImportRequest {
        source_path: path,
        file_name: file_name.to_string(),
        expected_size_bytes: Some(size),
        expected_modified_unix_ms: None,
        expected_quick_sha256: None,
        data_start: 1,
        columns: vec!["x".to_string(), "y".to_string(), "z".to_string()],
        z_column: Some("z".to_string()),
        input: SourceReadingV1 {
            sheet_key: "text".to_string(),
            separator: Some("whitespace".to_string()),
            header_row: None,
            x_column: Some("x".to_string()),
            y_column: Some("y".to_string()),
            crs: "nix_itrf2005".to_string(),
            utm_zone: None,
            utm_southern: None,
            attributes_only: false,
            layer_name: Some("orthocut".to_string()),
            column_renames: vec![],
            drop_columns: vec![],
        },
    }
}

/// The `orthocut1` shape, at whatever row count a test can afford.
fn nix_xyz(rows: usize) -> Vec<u8> {
    let mut text = String::from("x y z\n");
    for row in 0..rows {
        text.push_str(&format!(
            "{} {} {}\n",
            562_780 + row,
            4_730_800 + row,
            1_325.726 + row as f64
        ));
    }
    text.into_bytes()
}

fn drain(
    session: &mut NativeTabularSession,
    target_bytes: usize,
) -> Vec<serde_json::Map<String, Value>> {
    let mut rows = Vec::new();
    loop {
        let Some(ipc) = next_tabular_chunk(session, target_bytes).unwrap() else {
            if session.done {
                break;
            }
            continue;
        };
        if !ipc.is_empty() {
            rows.extend(ds_columnar::ipc_to_rows(&ipc).unwrap());
        }
        if session.done {
            break;
        }
    }
    rows
}

/// The correction this slice exists for: the pass that materializes the
/// layer IS the conversion. There is no second read of the source, and no
/// size at which the conversion is declined — `orthocut1` is 1.15 GiB and
/// its only bound is the bounded batch, which is why it can be one.
#[test]
fn the_pass_that_materializes_the_layer_also_writes_the_artifact() {
    let (root, path) = temp_source(&nix_xyz(2_600));
    let size = std::fs::metadata(&path).unwrap().len();
    let store = root.join("columnar");

    let mut session =
        open_tabular_session(&store, xyz_request(path, "rwanda_sub1_orthocut.txt", size)).unwrap();
    // Deliberately small, so the source crosses in many bounded batches
    // exactly as a multi-gigabyte one would.
    let rows = drain(&mut session, 2_048);

    assert_eq!(rows.len(), 2_600, "every row reached the import");
    assert!(session.chunk_count > 1, "and it did so in bounded batches");
    let conversion = session
        .conversion
        .clone()
        .expect("the artifact was published");
    assert!(!conversion.reused);
    assert_eq!(conversion.feature_count, 2_600);
    assert_eq!(conversion.layer_name, "orthocut");
    assert_eq!(conversion.source_digest.len(), 64);
    assert_eq!(conversion.conversion_id.len(), 64);
    assert_eq!(
        conversion.artifact_path,
        store
            .join(&conversion.source_digest)
            .join(format!("{}.parquet", conversion.conversion_id)),
        "addressed by the converter's own identity, never a name invented here",
    );
    let written = std::fs::read(&conversion.artifact_path).unwrap();
    assert!(written.starts_with(b"PAR1") && written.ends_with(b"PAR1"));
    assert_eq!(written.len(), conversion.byte_count);
    assert!(session.conversion_error.is_none());
    std::fs::remove_dir_all(root).unwrap();
}

/// §0.8a.3: an unchanged re-import is neither parsed nor written again. The
/// second session serves the SAME rows out of the artifact, and reads no
/// byte of the original to do it.
#[test]
fn an_unchanged_re_import_replays_the_artifact_and_reads_no_source_bytes() {
    let (root, path) = temp_source(&nix_xyz(2_600));
    let size = std::fs::metadata(&path).unwrap().len();
    let store = root.join("columnar");

    let mut first = open_tabular_session(
        &store,
        xyz_request(path.clone(), "rwanda_sub1_orthocut.txt", size),
    )
    .unwrap();
    let converted = drain(&mut first, 2_048);
    let artifact = first.conversion.clone().unwrap().artifact_path;

    let mut second =
        open_tabular_session(&store, xyz_request(path, "rwanda_sub1_orthocut.txt", size)).unwrap();
    assert!(
        matches!(second.lane, SessionLane::Replay(_)),
        "the import reads the artifact, not the source",
    );
    let conversion = second
        .conversion
        .clone()
        .expect("the artifact was recognised");
    assert!(conversion.reused);
    assert_eq!(conversion.artifact_path, artifact);
    assert_eq!(conversion.feature_count, 2_600);
    assert_eq!(conversion.layer_name, "orthocut");

    let replayed = drain(&mut second, 2_048);
    assert_eq!(replayed.len(), converted.len());
    assert_eq!(replayed[0]["elevation_m"], converted[0]["elevation_m"]);
    assert_eq!(
        replayed[2_599]["_ds_import_row"],
        converted[2_599]["_ds_import_row"]
    );
    assert_eq!(second.bytes_read, 0, "not one source byte was read");
    assert_eq!(second.valid_rows, 2_600);
    std::fs::remove_dir_all(root).unwrap();
}

/// An artifact whose bytes are gone is not an artifact. Opening it is the
/// completeness check, so a truncated file converts again rather than being
/// replayed as an empty layer.
#[test]
fn a_truncated_artifact_is_converted_again_rather_than_replayed() {
    let (root, path) = temp_source(&nix_xyz(64));
    let size = std::fs::metadata(&path).unwrap().len();
    let store = root.join("columnar");

    let mut first =
        open_tabular_session(&store, xyz_request(path.clone(), "orthocut.txt", size)).unwrap();
    drain(&mut first, 2_048);
    let artifact = first.conversion.clone().unwrap().artifact_path;
    std::fs::write(&artifact, b"PAR1truncated").unwrap();

    let mut second = open_tabular_session(&store, xyz_request(path, "orthocut.txt", size)).unwrap();
    assert!(matches!(second.lane, SessionLane::Convert(_)));
    let rows = drain(&mut second, 2_048);
    assert_eq!(rows.len(), 64);
    assert!(second
        .conversion
        .is_some_and(|conversion| !conversion.reused));
    std::fs::remove_dir_all(root).unwrap();
}

/// An artifact that cannot be written costs the analysis, never the
/// operator's layer — the additive semantics §0.8a records, kept now that
/// the conversion is the import's only read rather than a second one.
#[test]
fn an_artifact_that_cannot_be_written_costs_the_artifact_not_the_layer() {
    let (root, path) = temp_source(&nix_xyz(1_500));
    let size = std::fs::metadata(&path).unwrap().len();
    // A regular file where the artifact store belongs: every path under it
    // is unwritable, deterministically, on every platform.
    let store = root.join("columnar");
    std::fs::write(&store, b"not a directory").unwrap();

    let mut session =
        open_tabular_session(&store, xyz_request(path, "orthocut.txt", size)).unwrap();
    let rows = drain(&mut session, 2_048);

    assert_eq!(rows.len(), 1_500, "the operator's layer is complete");
    assert!(session.conversion.is_none(), "no artifact is claimed");
    assert!(
        session.conversion_error.is_some(),
        "and the reason is stated rather than swallowed",
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn logical_record_cap_is_enforced_before_extending_the_buffer() {
    let mut reader = BufReader::new(Cursor::new(b"12345\n"));
    let error = read_logical_record(&mut reader, 4).unwrap_err();
    assert!(error.contains("logical record exceeds"));
}

#[test]
fn captured_local_imports_keep_request_identity_and_cancel_only_the_named_session() {
    let bytes = nix_xyz(3);
    let (root, path) = temp_source(&bytes);
    let state = NativeTabularImportState::default();
    let a = RequestContext::capture_local_files(
        "import-a".into(),
        crate::ExecutionHost::Headless,
        crate::online,
    )
    .unwrap();
    let b = RequestContext::capture_local_files(
        "import-b".into(),
        crate::ExecutionHost::Headless,
        crate::online,
    )
    .unwrap();
    let first = start_import(
        a.clone(),
        &state,
        &root.join("columnar"),
        xyz_request(path.clone(), "points.txt", bytes.len() as u64),
    )
    .unwrap();
    let second = start_import(
        b.clone(),
        &state,
        &root.join("columnar"),
        xyz_request(path.clone(), "points.txt", bytes.len() as u64),
    )
    .unwrap();
    assert_eq!(first.session_id, a.request_id());
    assert_eq!(second.session_id, b.request_id());
    assert!(start_import(
        a,
        &state,
        &root.join("columnar"),
        xyz_request(path, "points.txt", bytes.len() as u64)
    )
    .is_err());
    assert!(import_status(&state, "../import-a").is_err());
    cancel_import(&state, &first.session_id).unwrap();
    assert!(import_status(&state, &first.session_id).is_err());
    assert_eq!(
        import_status(&state, &second.session_id)
            .unwrap()
            .session_id,
        second.session_id
    );
    assert!(!next_import_chunk(&state, &second.session_id)
        .unwrap()
        .is_empty());
    cancel_import(&state, &second.session_id).unwrap();
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_caller_decoding_refuses_unknown_nested_reading_and_source_fields() {
    let base = serde_json::json!({"sourcePath":"/data/a.csv", "fileName":"a.csv", "input":{"sheetKey":"text"}});
    assert!(serde_json::from_value::<NativeTabularImportRequest>(base.clone()).is_ok());
    let mut unknown = base.clone();
    unknown["input"]["project"] = serde_json::json!("ambient");
    assert!(serde_json::from_value::<NativeTabularImportRequest>(unknown).is_err());
    let mut unknown = base;
    unknown["input"]["columnRenames"] =
        serde_json::json!([{"from":"a", "to":"b", "operation":"execute"}]);
    assert!(serde_json::from_value::<NativeTabularImportRequest>(unknown).is_err());
    assert!(serde_json::from_value::<crate::NativeSourcePrepareRequest>(
        serde_json::json!({"sourcePath":"/data/a.csv", "disposition":"linked", "project":"ambient"})
    )
    .is_err());
    let request: crate::NativeSourcePrepareRequest = serde_json::from_value(
        serde_json::json!({"sourcePath":"relative.csv", "disposition":"linked"}),
    )
    .unwrap();
    let context = RequestContext::local_files(crate::ExecutionHost::Headless, crate::online);
    assert!(
        prepare_source(&context, Path::new("/data/imports"), request)
            .unwrap_err()
            .contains("absolute")
    );
}
