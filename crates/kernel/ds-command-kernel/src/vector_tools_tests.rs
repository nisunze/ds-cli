//! Vector tools: the runner's regressions (moved with it from ds-network),
//! byte parity with the former Network exports, and the typed exchange.
use super::*;

/// The engine regressions, unchanged apart from their module paths.
mod engine {
    use super::super::{admission, collisions, descriptor::describe, runner, Refusal};
    use ds_geo::layer::{Feature, Layer};
    use serde_json::{json, Value};

    fn prepare(id: &str, request: Value) -> Result<Value, Refusal> {
        admission::prepare(id, request)
    }
    /// Strip import payloads at the boundary, before any computation.
    fn control_request(mut request: Value) -> Value {
        for name in ["source", "against"] {
            if let Some(o) = request.get_mut(name).and_then(Value::as_object_mut) {
                o.remove("geojson");
                if !o.contains_key("layer_id") && !o.contains_key("file") {
                    o.insert("layer_id".into(), json!(name));
                }
            }
        }
        request
    }
    fn run(id: &str, request: Value, options: runner::RunOptions) -> Result<Value, Refusal> {
        let request = prepare(id, request)?;
        let source =
            std::sync::Arc::new(Layer::import_geojson(&request["source"]["geojson"]).unwrap());
        let against = request
            .get("against")
            .map(|v| std::sync::Arc::new(Layer::import_geojson(&v["geojson"]).unwrap()));
        let output = runner::run(id, control_request(request), source, against, options)?;
        let mut result = output.metadata;
        result["result"] = output
            .layer
            .map(|l| l.export_geojson())
            .unwrap_or(Value::Null);
        Ok(result)
    }
    fn source() -> Value {
        json!({"source":{"geojson":{"type":"LineString","coordinates":[[30.,-2.],[30.001,-2.]]}}})
    }
    #[test]
    fn import_admission_distinguishes_absent_and_malformed_geometry_and_against_limits() {
        let layer = Layer::import_geojson(&json!({"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.001,-2.]]}},
            {"type":"Feature","geometry":null},
            {"type":"Feature","geometry":{"type":"LineString"}},
            {"type":"Feature","geometry":{"type":"LineString","coordinates":"bad"}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[30.,-2.]}},
            {"type":"Feature","geometry":{"type":"Unknown","coordinates":[]}}
        ]})).unwrap();
        let (rows, skipped, _) = runner::planned(
            &layer,
            ds_geo_lite::vector_types::VectorOperation::Sample,
            1,
            None,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            skipped.counts(),
            json!({"no_geometry":2,"bad_coordinates":1,"wrong_kind":2})
        );
        assert_eq!(
            skipped.named(10),
            json!([
                {"index":1,"reason":"no_geometry"},
                {"index":2,"reason":"no_geometry"},
                {"index":3,"reason":"bad_coordinates"},
                {"index":4,"reason":"wrong_kind"},
                {"index":5,"reason":"wrong_kind"}
            ])
        );
        let repeated = Layer {
            features: vec![layer.features[0].clone(); 3],
        };
        let (_, _, more) = runner::planned(
            &repeated,
            ds_geo_lite::vector_types::VectorOperation::Intersect,
            1,
            Some("against"),
        )
        .unwrap();
        assert_eq!(
            more.unwrap(),
            "2 feature(s) from --against were not compared; raise --limit (max 20000) to include them"
        );
    }
    #[test]
    fn catalogue_and_collision_reader_have_unique_complete_descriptors_and_valid_examples() {
        let all = describe(None).unwrap();
        let rows = all.as_array().unwrap();
        assert_eq!(rows.len(), 95);
        let ids = rows
            .iter()
            .map(|d| d["id"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), 95);
        assert_eq!(
            rows.iter().filter(|d| d["status"] == "available").count(),
            7
        );
        for d in rows {
            assert!(
                d["summary"].as_str().unwrap().chars().count() < 70,
                "{}",
                d["id"]
            );
            assert!(d["input_schema"]["properties"]["parameters"].is_object());
            assert!(d["output_schema"]["x-output-kind"].is_string());
            prepare(
                d["id"].as_str().unwrap(),
                d["examples"][0]["request"].clone(),
            )
            .unwrap();
        }
    }
    #[test]
    fn defaults_ranges_unknown_fields_and_roadmap_are_owned_here() {
        let request = prepare("sample", source()).unwrap();
        assert_eq!(request["parameters"]["interval_m"], 100.);
        assert_eq!(request["parameters"]["include_ends"], false);
        let mut bad = source();
        bad["parameters"] = json!({"interval_m":0});
        assert_eq!(
            prepare("sample", bad).unwrap_err().code,
            "vector_distance_out_of_range"
        );
        let mut bad = source();
        bad["parameters"] = json!({"intervalM":100});
        assert!(prepare("sample", bad).is_err());
        let mut fractional_seed = source();
        fractional_seed["parameters"] = json!({"seed":1.5});
        assert_eq!(
            prepare("random-points-area", fractional_seed)
                .unwrap_err()
                .code,
            "vector_request_invalid",
            "Fractional seeds must not silently truncate to another reproducible run"
        );
        assert_eq!(
            run("centroids", source(), runner::RunOptions::default())
                .unwrap_err()
                .code,
            "vector_tool_roadmap"
        );
        assert_eq!(
            describe(Some("missing")).unwrap_err().code,
            "vector_tool_unknown"
        );
    }
    #[test]
    fn preview_matches_execution_and_measure_preserves_properties() {
        for id in [
            "buffer",
            "sample",
            "measure",
            "outliers",
            "random-points-area",
            "intersect",
        ] {
            let descriptor = describe(Some(id)).unwrap();
            let request = descriptor["examples"][0]["request"].clone();
            let preview = run(id, request.clone(), runner::RunOptions { dry_run: true }).unwrap();
            let actual = run(id, request, runner::RunOptions::default()).unwrap();
            assert_eq!(preview["preview"], actual["preview"], "{id}");
            assert_eq!(preview["result"], Value::Null);
            assert!(preview["preview"]["sample"].as_array().unwrap().len() <= 5);
        }
        let mut input = source();
        input["source"]["geojson"] = json!({"type":"Feature","id":"original","properties":{"owner":"Alice"},"geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.001,-2.]]}});
        let answer = run("measure", input, runner::RunOptions::default()).unwrap();
        assert_eq!(
            answer["result"]["features"][0]["properties"]["owner"],
            "Alice"
        );
        assert!(
            answer["result"]["features"][0]["properties"]["length_m"]
                .as_f64()
                .unwrap()
                > 100.
        );
    }
    #[test]
    fn terrain_is_explicit_input_and_collision_previews_preserve_full_counts() {
        let mut request = source();
        request["parameters"] = json!({"sample_elevation":true});
        assert_eq!(
            run(
                "random-points-area",
                request.clone(),
                runner::RunOptions::default()
            )
            .unwrap_err()
            .code,
            "vector_external_data_required"
        );
        request["parameters"]["sample_elevation"] = json!(false);
        let base = run(
            "random-points-area",
            request.clone(),
            runner::RunOptions::default(),
        )
        .unwrap();
        let count = base["result"]["features"].as_array().unwrap().len();
        assert!(count > 0);
        request["parameters"]["sample_elevation"] = json!(true);
        request["parameters"]["elevations_m"] = json!(vec![Value::Null; count]);
        let enriched = run("random-points-area", request, runner::RunOptions::default()).unwrap();
        assert_eq!(enriched["ui_stats"]["elevationGaps"], count);
        assert!(enriched["fields_added"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "elevation_m"));
        let input = json!({"source":{"geojson":{"type":"FeatureCollection","features":(0..12).map(|i|json!({"type":"Feature","id":i,"properties":{},"geometry":null})).collect::<Vec<_>>()}}});
        let mut bounded = input.clone();
        bounded["output"] = json!({"limit":1});
        let answer = run("collisions", bounded, runner::RunOptions::default()).unwrap();
        assert_eq!(answer["report"]["total"], 12);
        assert_eq!(answer["report"]["more"], true);
        assert_eq!(answer["report"]["regions"].as_array().unwrap().len(), 1);
        let preview = run("collisions", input, runner::RunOptions { dry_run: true }).unwrap();
        assert_eq!(preview["report"]["more"], true);
        assert_eq!(preview["report"]["total"], 12);
        assert_eq!(preview["report"]["regions"].as_array().unwrap().len(), 5);
    }
    #[test]
    fn measure_counts_admitted_rows_and_random_preview_retains_only_its_sample() {
        let input = json!({"source":{"geojson":{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"owner":"kept"},"geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.001,-2.]]}},{"type":"Feature","properties":{},"geometry":null}]}}});
        let result = run("measure", input, runner::RunOptions::default()).unwrap();
        assert_eq!(result["processed"], 1);
        assert_eq!(result["source_features"], 2);
        assert!(!result["preview"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("replaced")));
        let input = json!({"source":{"geojson":{"type":"Polygon","coordinates":[[[30.,-2.],[30.01,-2.],[30.01,-1.99],[30.,-1.99],[30.,-2.]]]}},"parameters":{"min_spacing_m":30.,"max_spacing_m":50.},"output":{"limit":10}});
        let actual = run(
            "random-points-area",
            input.clone(),
            runner::RunOptions::default(),
        )
        .unwrap();
        let preview = run(
            "random-points-area",
            input,
            runner::RunOptions { dry_run: true },
        )
        .unwrap();
        assert!(actual["produced"].as_u64().unwrap() > 10);
        assert_eq!(actual["result"]["features"].as_array().unwrap().len(), 10);
        assert_eq!(actual["preview"], preview["preview"]);
        assert_eq!(actual["ui_stats"], preview["ui_stats"]);
    }
    /// An eligible feature whose operation yields no geometry used to vanish:
    /// Gisagara M3 buffer reported processed 121, produced 115, `skipped: {}`.
    /// Every source feature is now processed or skipped by name.
    #[test]
    fn a_feature_the_operation_yields_nothing_for_is_skipped_by_name() {
        let lines = json!({"type":"FeatureCollection","features":[
            {"type":"Feature","id":"long","properties":{},"geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.01,-2.]]}},
            {"type":"Feature","id":"short","properties":{},"geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.0001,-2.]]}},
            {"type":"Feature","id":"pole","properties":{},"geometry":{"type":"Point","coordinates":[30.,-2.]}}
        ]});
        let sampled = run(
            "sample",
            json!({"source":{"geojson":lines},"parameters":{"interval_m":100}}),
            runner::RunOptions::default(),
        )
        .unwrap();
        assert_eq!(sampled["source_features"], 3);
        assert_eq!(sampled["processed"], 1);
        assert!(sampled["produced"].as_u64().unwrap() > 0);
        assert_eq!(sampled["skipped"], json!({"no_station":1,"wrong_kind":1}));
        assert_eq!(
            sampled["skipped_features"],
            json!([
                {"index":1,"id":"short","reason":"no_station"},
                {"index":2,"id":"pole","reason":"wrong_kind"}
            ])
        );
        assert!(sampled["preview"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("no_station")));
        let area = json!({"type":"FeatureCollection","features":[
            {"type":"Feature","id":"field","properties":{},"geometry":{"type":"Polygon","coordinates":[[[30.,-2.],[30.01,-2.],[30.01,-1.99],[30.,-1.99],[30.,-2.]]]}},
            {"type":"Feature","id":"feeder","properties":{},"geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.01,-2.]]}}
        ]});
        let random = run(
            "random-points-area",
            json!({"source":{"geojson":area},"parameters":{"buffer_distance_m":0,"min_spacing_m":50,"max_spacing_m":100}}),
            runner::RunOptions::default(),
        )
        .unwrap();
        assert_eq!(random["source_features"], 2);
        assert_eq!(random["processed"], 1);
        assert_eq!(random["skipped"], json!({"no_buffer":1}));
        assert_eq!(
            random["skipped_features"],
            json!([{"index":1,"id":"feeder","reason":"no_buffer"}])
        );
        assert_eq!(random["ui_stats"]["skippedNoBuffer"], 1);
        // A crossing-free source line is a complete intersect answer, not a
        // dropped feature.
        let crossed = run(
            "intersect",
            json!({"source":{"geojson":{"type":"FeatureCollection","features":[
                {"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.001,-2.]]}},
                {"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[30.,-1.9],[30.001,-1.9]]}}
            ]}},"against":{"geojson":{"type":"LineString","coordinates":[[30.0005,-2.001],[30.0005,-1.999]]}}}),
            runner::RunOptions::default(),
        )
        .unwrap();
        assert_eq!(crossed["processed"], 2);
        assert_eq!(crossed["produced"], 1);
        assert_eq!(crossed["skipped"], json!({}));
    }
    #[test]
    fn random_seed_reproduces_and_holes_remain_empty() {
        let input = json!({"source":{"geojson":{"type":"Polygon","coordinates":[[[30.,-2.],[30.01,-2.],[30.01,-1.99],[30.,-1.99],[30.,-2.]],[[30.002,-1.998],[30.008,-1.998],[30.008,-1.992],[30.002,-1.992],[30.002,-1.998]]]}},"parameters":{"min_spacing_m":50,"max_spacing_m":100,"seed":42}});
        let a = run(
            "random-points-area",
            input.clone(),
            runner::RunOptions::default(),
        )
        .unwrap();
        let b = run("random-points-area", input, runner::RunOptions::default()).unwrap();
        assert_eq!(a, b);
        assert!(a["produced"].as_u64().unwrap() > 0);
        for f in a["result"]["features"].as_array().unwrap() {
            let x = f["geometry"]["coordinates"][0].as_f64().unwrap();
            let y = f["geometry"]["coordinates"][1].as_f64().unwrap();
            assert!(!(x > 30.002 && x < 30.008 && y > -1.998 && y < -1.992));
        }
    }
    #[test]
    fn zero_never_and_held_are_distinct_and_ranking_uses_reporter_values() {
        use collisions::summarize;
        assert_eq!(summarize(&[], false, false, None)["phase"], "idle");
        assert_eq!(summarize(&[], true, false, None)["phase"], "never");
        assert_eq!(summarize(&[], true, true, None)["phase"], "clear");
        let low = json!({"id":"low","properties":{"transformers":"A, B","coverage_pct":10.,"computed_at":"2026-10-02T12:00:00Z"}});
        let high = json!({"id":"high","properties":{"transfo_a":"B","transfo_b":"C","coverage_pct":80.,"can_consolidate":true}});
        let summary = summarize(
            &[
                Feature::import_geojson(&low),
                Feature::import_geojson(&high),
            ],
            true,
            true,
            Some(false),
        );
        assert_eq!(summary["phase"], "held");
        assert_eq!(summary["transformerCount"], 3);
        assert_eq!(summary["regions"][0]["id"], "high");
        assert_eq!(summary["consolidatableCount"], 1);
        assert_eq!(summary["computedAtMs"], 1790942400000i64);
    }
}

/// The Kernel operations answer exactly what the Network exports answered.
mod network_parity {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha(bytes: impl AsRef<[u8]>) -> String {
        Sha256::digest(bytes.as_ref())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../tests/fixtures/vector_tools_network_parity.json"
        ))
        .unwrap()
    }
    fn tool(name: &str) -> VectorToolId {
        serde_json::from_value(json!(name)).unwrap()
    }
    fn digest_of(case: &Value, key: &str) -> Option<String> {
        case[key].as_str().map(String::from)
    }

    #[test]
    fn catalogue_and_phase_copy_are_byte_identical() {
        let fixture = fixture();
        let catalogue = catalogue().unwrap();
        assert_eq!(
            catalogue.tools.len() as u64,
            fixture["catalogue_count"].as_u64().unwrap()
        );
        assert_eq!(
            sha(serde_json::to_string(&catalogue.tools).unwrap()),
            fixture["catalogue_sha256"].as_str().unwrap()
        );
        let runnable: std::collections::BTreeSet<_> = catalogue
            .tools
            .iter()
            .filter(|tool| tool.status == VectorToolStatus::Available)
            .map(|tool| tool.command.clone())
            .collect();
        assert_eq!(
            runnable,
            VectorToolId::ALL
                .map(|tool| tool.command().to_owned())
                .into_iter()
                .collect(),
            "the closed tool set is exactly the catalogue's available tools"
        );
        for (phase, expected) in fixture["phase_copy"].as_object().unwrap() {
            assert_eq!(super::super::collisions::phase_copy(phase), *expected);
        }
    }

    #[test]
    fn several_faults_refuse_with_the_former_first_fault() {
        for case in fixture()["prepare_refusals"].as_array().unwrap() {
            let tool = tool(case["tool"].as_str().unwrap());
            let document = case["text"].as_str().unwrap().to_owned();
            let refusal = prepare(&VectorToolPrepareInput { document, tool }).unwrap_err();
            assert_eq!(
                refusal.to_string(),
                case["refusal"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    /// Key order is not data: deliberately unsorted documents answer exactly
    /// like their sorted controls, as the former exports did.
    #[test]
    fn unsorted_documents_match_their_sorted_controls() {
        let fixture = fixture();
        let cases = fixture["cases"].as_array().unwrap();
        let named = |name: &str| cases.iter().find(|case| case["name"] == name).unwrap();
        for name in ["random-unsorted", "measure-unsorted", "sample-unsorted"] {
            let (unsorted, control) = (named(name), named(&format!("{name}-control")));
            assert_ne!(
                serde_json::to_string(&unsorted["document"]).unwrap(),
                serde_json::to_string(&control["document"]).unwrap(),
                "{name}: the control differs only in key order"
            );
            for key in [
                "prepared_request_sha256",
                "source_ipc_sha256",
                "receipt_sha256",
                "dry_receipt_sha256",
                "result_ipc_sha256",
                "export_geojson_sha256",
                "sketch_sha256",
            ] {
                assert_eq!(unsorted[key], control[key], "{name}: {key}");
            }
        }
    }

    #[test]
    fn every_case_prepares_runs_exports_and_renders_identically() {
        let fixture = fixture();
        let examples = catalogue().unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let tool = tool(case["tool"].as_str().unwrap());
            let document = if case["document"].is_null() {
                json!(
                    examples
                        .tools
                        .iter()
                        .find(|descriptor| descriptor.command == tool.command())
                        .unwrap()
                        .examples[0]
                        .request
                )
            } else {
                case["document"].clone()
            };
            let prepared = prepare(&VectorToolPrepareInput {
                document: serde_json::to_string(&document).unwrap(),
                tool,
            })
            .unwrap();
            assert_eq!(
                sha(serde_json::to_string(&prepared.request).unwrap()),
                case["prepared_request_sha256"].as_str().unwrap(),
                "{name}: prepared request {}",
                serde_json::to_string(&prepared.request).unwrap()
            );
            assert_eq!(
                prepared.source_ipc.as_ref().map(|b| sha(&b.0)),
                digest_of(case, "source_ipc_sha256"),
                "{name}"
            );
            assert_eq!(
                prepared.against_ipc.as_ref().map(|b| sha(&b.0)),
                digest_of(case, "against_ipc_sha256"),
                "{name}"
            );
            let source: Vec<Bytes> = prepared.source_ipc.iter().cloned().collect();
            let against: Vec<Bytes> = prepared.against_ipc.iter().cloned().collect();
            for dry_run in [false, true] {
                let operation = VectorToolOperation::Run(Box::new(VectorToolRunInput {
                    against: against.clone(),
                    dry_run,
                    request: prepared.request.clone(),
                    source: source.clone(),
                    tool,
                }));
                let VectorToolResponse::Run(run) = execute(&operation).unwrap() else {
                    panic!("{name}: run answered another operation");
                };
                let receipt = serde_json::to_string(&run.receipt).unwrap();
                let key = if dry_run {
                    "dry_receipt_sha256"
                } else {
                    "receipt_sha256"
                };
                assert_eq!(
                    sha(&receipt),
                    case[key].as_str().unwrap(),
                    "{name} dry={dry_run}: {receipt}"
                );
                if dry_run {
                    assert!(run.result_ipc.is_none(), "{name}");
                    continue;
                }
                assert_eq!(
                    run.result_ipc.as_ref().map(|b| sha(&b.0)),
                    digest_of(case, "result_ipc_sha256"),
                    "{name}"
                );
                let Some(result) = run.result_ipc else {
                    continue;
                };
                let layer = VectorLayerIpc {
                    layer: result.clone(),
                };
                let Ok(VectorToolResponse::ExportGeojson(file)) =
                    execute(&VectorToolOperation::ExportGeojson(layer.clone()))
                else {
                    panic!("{name}: export");
                };
                assert_eq!(
                    Some(sha(&file.bytes.0)),
                    digest_of(case, "export_geojson_sha256"),
                    "{name}"
                );
                let columns: Vec<String> = run
                    .receipt
                    .output_layer
                    .fields
                    .iter()
                    .map(|field| field.name.clone())
                    .collect();
                let Ok(VectorToolResponse::SketchFeatures(sketch)) =
                    execute(&VectorToolOperation::SketchFeatures(VectorSketchInput {
                        columns,
                        id_prefix: run.receipt.tool_id.clone(),
                        layer: result.clone(),
                    }))
                else {
                    panic!("{name}: sketch");
                };
                assert_eq!(
                    Some(sha(serde_json::to_string(&sketch).unwrap())),
                    digest_of(case, "sketch_sha256"),
                    "{name}"
                );
                let Ok(VectorToolResponse::PointCoordinates(points)) =
                    execute(&VectorToolOperation::PointCoordinates(layer))
                else {
                    panic!("{name}: points");
                };
                let raw: Vec<u8> = points
                    .xy
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect();
                assert_eq!(Some(sha(raw)), digest_of(case, "point_xy_sha256"), "{name}");
                if let Some(source) = &prepared.source_ipc {
                    let Ok(VectorToolResponse::MergeLayers(merged)) =
                        execute(&VectorToolOperation::MergeLayers(VectorLayerChunks {
                            chunks: vec![source.clone(), result],
                        }))
                    else {
                        panic!("{name}: merge");
                    };
                    assert_eq!(
                        Some(sha(&merged.layer.0)),
                        digest_of(case, "merge_source_and_result_sha256"),
                        "{name}"
                    );
                }
            }
            if let Some(source) = prepared.source_ipc {
                let Ok(VectorToolResponse::ExportGeojson(file)) =
                    execute(&VectorToolOperation::ExportGeojson(VectorLayerIpc {
                        layer: source,
                    }))
                else {
                    panic!("{name}: source export");
                };
                assert_eq!(
                    Some(sha(&file.bytes.0)),
                    digest_of(case, "source_export_geojson_sha256"),
                    "{name}"
                );
            }
        }
    }
}

/// The typed exchange: closed operations, named refusals, strict decoding.
mod exchange {
    use super::*;

    fn line_document() -> String {
        r#"{"source":{"name":"Lines","geojson":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line","properties":{"owner":"kept"},"geometry":{"type":"LineString","coordinates":[[30,-2],[30.001,-2]]}}]}}}"#.to_owned()
    }
    fn prepared(tool: VectorToolId) -> VectorToolPrepared {
        prepare(&VectorToolPrepareInput {
            document: line_document(),
            tool,
        })
        .unwrap()
    }
    fn run_operation(tool: VectorToolId, dry_run: bool) -> VectorToolOperation {
        let prepared = prepared(tool);
        VectorToolOperation::Run(Box::new(VectorToolRunInput {
            against: vec![],
            dry_run,
            request: prepared.request,
            source: prepared.source_ipc.into_iter().collect(),
            tool,
        }))
    }
    /// The browser wire: every value crosses as JSON-compatible data.
    fn over_the_wire(operation: &VectorToolOperation) -> (VectorToolOperation, Value) {
        let operation: VectorToolOperation =
            serde_json::from_value(serde_json::to_value(operation).unwrap()).unwrap();
        let response = serde_json::to_value(execute(&operation).unwrap()).unwrap();
        (operation, response)
    }
    fn decode(
        operation: &VectorToolOperation,
        response: Value,
    ) -> Result<VectorToolResponse, Refusal> {
        let response: VectorToolResponse =
            serde_json::from_value(response).map_err(response_invalid)?;
        validate_response(operation, &response)?;
        Ok(response)
    }

    #[test]
    fn a_wire_round_trip_runs_measure_and_keeps_feature_identity() {
        let (operation, response) = over_the_wire(&run_operation(VectorToolId::Measure, false));
        let VectorToolResponse::Run(run) = decode(&operation, response).unwrap() else {
            panic!("run answered another operation");
        };
        assert_eq!(run.receipt.operation, VectorToolId::Measure);
        assert_eq!(run.receipt.tool_id, "geometry::Add geometry attributes");
        assert_eq!(run.receipt.result, ());
        let layer = ds_geo::layer::ipc::decode(&run.result_ipc.unwrap().0).unwrap();
        let feature = layer.features[0].export_geojson();
        assert_eq!(feature["id"], "line");
        assert_eq!(feature["properties"]["owner"], "kept");
        assert!(feature["properties"]["length_m"].as_f64().unwrap() > 100.);
    }

    #[test]
    fn descriptor_ids_select_the_same_closed_tool_and_roadmap_tools_cannot_be_named() {
        for tool in VectorToolId::ALL {
            let descriptor = catalogue()
                .unwrap()
                .tools
                .into_iter()
                .find(|descriptor| descriptor.command == tool.command())
                .unwrap();
            assert_eq!(
                serde_json::from_value::<VectorToolId>(json!(descriptor.id)).unwrap(),
                tool
            );
            assert_eq!(json!(tool), json!(descriptor.command));
        }
        for roadmap in ["centroids", "overlay::Union", "", "BUFFER"] {
            assert!(
                serde_json::from_value::<VectorToolId>(json!(roadmap)).is_err(),
                "{roadmap}"
            );
        }
        for tool in VectorToolId::ALL {
            assert_eq!(tool_id(tool.command()).unwrap(), tool);
        }
        assert_eq!(
            tool_id("geoprocessing::Buffer").unwrap(),
            VectorToolId::Buffer
        );
        assert_eq!(
            tool_id("centroids").unwrap_err().code,
            "vector_tool_roadmap"
        );
        assert_eq!(
            tool_id("overlay::Union").unwrap_err().code,
            "vector_tool_roadmap"
        );
        assert_eq!(tool_id("missing").unwrap_err().code, "vector_tool_unknown");
        let mut operation =
            serde_json::to_value(run_operation(VectorToolId::Measure, false)).unwrap();
        operation["input"]["tool"] = json!("centroids");
        assert!(serde_json::from_value::<VectorToolOperation>(operation).is_err());
    }

    #[test]
    fn malformed_requests_are_refused_by_name_without_partial_results() {
        let refused = |document: &str| {
            prepare(&VectorToolPrepareInput {
                document: document.to_owned(),
                tool: VectorToolId::Sample,
            })
            .unwrap_err()
            .code
        };
        assert_eq!(refused("{"), "vector_request_invalid");
        assert_eq!(
            refused(r#"{"source":{"file":"a.arrow"},"parameters":{"intervalM":1}}"#),
            "vector_request_invalid"
        );
        assert_eq!(
            refused(r#"{"source":{"file":"a.arrow"},"parameters":{"interval_m":0}}"#),
            "vector_distance_out_of_range"
        );
        assert_eq!(refused(r#"{"source":{}}"#), "vector_request_invalid");
        let oversized = " ".repeat(ds_geo::layer::ipc::MAX_BYTES + 1);
        assert_eq!(refused(&oversized), "vector_data_bound_exceeded");

        let VectorToolOperation::Run(mut input) = run_operation(VectorToolId::Measure, false)
        else {
            unreachable!()
        };
        input.request.source.geojson = Some(json!({"type":"FeatureCollection","features":[]}));
        assert_eq!(run(&input).unwrap_err().code, "vector_request_invalid");
        input.request.source.geojson = None;
        input.source = vec![Bytes(vec![1, 2, 3])];
        assert_eq!(run(&input).unwrap_err().code, "vector_document_malformed");
        input.source = vec![];
        assert_eq!(run(&input).unwrap_err().code, "vector_document_empty");
        assert_eq!(
            execute(&VectorToolOperation::ExportGeojson(VectorLayerIpc {
                layer: Bytes(b"not ipc".to_vec())
            }))
            .unwrap_err()
            .code,
            "vector_document_malformed"
        );
        let refusal = run(&VectorToolRunInput {
            source: vec![Bytes(vec![0; 4])],
            ..*input
        })
        .unwrap_err();
        let shown: Value = serde_json::from_str(&refusal.to_string()).unwrap();
        assert_eq!(shown["code"], "vector_document_malformed");
        assert!(shown["remedy"].is_string());
        let unknown_field =
            br#"{"operation":"prepare","input":{"tool":"sample","document":"{}","future":true}}"#;
        let operation: VectorToolOperation = serde_json::from_slice(unknown_field).unwrap();
        assert!(
            matches!(operation, VectorToolOperation::Prepare(_)),
            "additive input fields are accepted"
        );
        let bad_request = br#"{"operation":"run","input":{"tool":"measure","request":{"source":{"file":"a"},"extra":1},"source":[]}}"#;
        assert!(
            serde_json::from_slice::<VectorToolOperation>(bad_request).is_err(),
            "the request keeps its closed schema"
        );
    }

    #[test]
    fn decoders_reject_crossed_incompatible_and_inconsistent_replies() {
        let (operation, response) = over_the_wire(&run_operation(VectorToolId::Measure, false));
        let refused = |change: &dyn Fn(&mut Value)| {
            let mut changed = response.clone();
            change(&mut changed);
            decode(&operation, changed).unwrap_err().code
        };
        assert_eq!(
            refused(&|r| r["operation"] = json!("prepare")),
            "vector_response_invalid"
        );
        assert_eq!(
            refused(&|r| r["result"]["receipt"]["operation"] = json!("buffer")),
            "vector_response_invalid"
        );
        assert_eq!(
            refused(&|r| r["result"]["receipt"]["operation"] = json!("centroids")),
            "vector_response_invalid"
        );
        assert_eq!(
            refused(&|r| r["result"]["receipt"]["dry_run"] = json!(true)),
            "vector_response_invalid"
        );
        assert_eq!(
            refused(&|r| r["result"]["receipt"]["processed"] = json!("one")),
            "vector_response_invalid"
        );
        assert_eq!(
            refused(&|r| r["result"]["receipt"]["produced"] = json!(99)),
            "vector_response_invalid"
        );
        assert_eq!(
            refused(
                &|r| r["result"]["receipt"]["output_layer"]["geometry_type"] = json!("Unknown")
            ),
            "vector_response_invalid"
        );
        assert_eq!(
            refused(&|r| r["result"]["receipt"]["output_layer"]["render_mode"] = json!("tiles")),
            "vector_response_invalid"
        );
        assert_eq!(
            refused(&|r| {
                r["result"]["receipt"]
                    .as_object_mut()
                    .unwrap()
                    .remove("preview");
            }),
            "vector_response_invalid"
        );
        assert_eq!(
            refused(&|r| {
                r["result"].as_object_mut().unwrap().remove("result_ipc");
            }),
            "vector_response_invalid"
        );
        assert_eq!(
            refused(&|r| r["result"]["result_ipc"] = json!([1, 2, 3])),
            "vector_response_invalid"
        );

        let (dry, dry_response) = over_the_wire(&run_operation(VectorToolId::Measure, true));
        let mut with_layer = dry_response.clone();
        with_layer["result"]["result_ipc"] = response["result"]["result_ipc"].clone();
        assert_eq!(
            decode(&dry, with_layer).unwrap_err().code,
            "vector_response_invalid"
        );

        let (outliers, outliers_response) =
            over_the_wire(&run_operation(VectorToolId::Outliers, false));
        let mut broken = outliers_response.clone();
        broken["result"]["receipt"]["report"] = json!({"layer_name":"x"});
        assert_eq!(
            decode(&outliers, broken).unwrap_err().code,
            "vector_response_invalid"
        );

        let layer = response["result"]["result_ipc"].clone();
        let sketch: VectorToolOperation = serde_json::from_value(json!({"operation":"sketch_features","input":{"columns":["owner"],"id_prefix":"p","layer":layer}})).unwrap();
        let (sketch, sketch_response) = over_the_wire(&sketch);
        let mut crossed = sketch_response.clone();
        crossed["result"]["features"][0]["properties"] = json!({"other":"x"});
        assert_eq!(
            decode(&sketch, crossed).unwrap_err().code,
            "vector_response_invalid"
        );
        let points: VectorToolOperation = serde_json::from_value(
            json!({"operation":"point_coordinates","input":{"layer":layer}}),
        )
        .unwrap();
        assert_eq!(
            decode(
                &points,
                json!({"operation":"point_coordinates","result":{"xy":[30.0]}})
            )
            .unwrap_err()
            .code,
            "vector_response_invalid"
        );
        let export: VectorToolOperation =
            serde_json::from_value(json!({"operation":"export_geojson","input":{"layer":layer}}))
                .unwrap();
        assert_eq!(
            decode(
                &export,
                json!({"operation":"export_geojson","result":{"bytes":[60]}})
            )
            .unwrap_err()
            .code,
            "vector_response_invalid"
        );
        let merge: VectorToolOperation =
            serde_json::from_value(json!({"operation":"merge_layers","input":{"chunks":[layer]}}))
                .unwrap();
        assert_eq!(
            decode(
                &merge,
                json!({"operation":"merge_layers","result":{"layer":[1,2]}})
            )
            .unwrap_err()
            .code,
            "vector_response_invalid"
        );
        let prepare_operation = VectorToolOperation::Prepare(VectorToolPrepareInput {
            document: line_document(),
            tool: VectorToolId::Sample,
        });
        let (prepare_operation, prepared) = over_the_wire(&prepare_operation);
        let mut inline = prepared.clone();
        inline["result"]["request"]["source"]["geojson"] =
            json!({"type":"FeatureCollection","features":[]});
        assert_eq!(
            decode(&prepare_operation, inline).unwrap_err().code,
            "vector_response_invalid"
        );
        let mut unnamed = prepared;
        unnamed["result"]["request"]["source"]
            .as_object_mut()
            .unwrap()
            .remove("file");
        assert_eq!(
            decode(&prepare_operation, unnamed).unwrap_err().code,
            "vector_response_invalid"
        );
    }

    #[test]
    fn decoders_accept_compatible_additive_reply_fields() {
        let (operation, response) = over_the_wire(&run_operation(VectorToolId::Sample, false));
        let expected = decode(&operation, response.clone()).unwrap();
        let mut extended = response;
        extended["result"]["receipt"]["future_total"] = json!(3);
        extended["result"]["receipt"]["output_layer"]["future_hint"] = json!({"style":"dots"});
        extended["result"]["receipt"]["preview"]["future"] = json!([]);
        extended["result"]["future_digest"] = json!("abc");
        assert_eq!(decode(&operation, extended).unwrap(), expected);
    }
}
