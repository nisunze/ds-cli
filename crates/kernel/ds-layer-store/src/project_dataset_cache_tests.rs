use super::*;
use ds_command_kernel::project_dataset_cache::{
    dataset_key, BufferPolicy, Coverage, Dataset, IsolatedRule, Quality, Scope,
};
use std::collections::BTreeMap;

fn scope() -> Scope {
    Scope {
        principal: "uid-1".into(),
        project: "project-a".into(),
    }
}

fn buildings() -> Dataset {
    Dataset {
        id: "google_open_buildings".into(),
        provider: "ds-brain:query_print_context".into(),
        quality: Quality::Source,
        parameters: BTreeMap::from([("min_area_m2".into(), json!(25))]),
        source_version: String::new(),
        crs: "EPSG:4326".into(),
    }
}

fn policy_value() -> Value {
    serde_json::to_value(BufferPolicy {
        design_buffer_m: 500.0,
        isolated_buffer_m: 1_000.0,
        isolated_rule: IsolatedRule::PointOnly,
        provisional: true,
        sheets: Vec::new(),
    })
    .unwrap()
}

fn open(root: &Path, scope: &Scope, dataset: &Dataset) -> String {
    let key = dataset_key(scope, dataset);
    execute_at(
        root,
        json!({
            "action": "open", "dataset_key": key,
            "principal": scope.principal, "project": scope.project,
            "dataset_id": dataset.id, "provider": dataset.provider,
            "quality": "source", "parameters": dataset.parameters,
            "crs": dataset.crs, "source_version": dataset.source_version,
            "buffer_policy": policy_value(),
        }),
    )
    .unwrap();
    key
}

fn feature(lon: f64, lat: f64, name: &str) -> Value {
    json!({"type":"Feature","properties":{"name":name},
        "geometry":{"type":"Polygon","coordinates":[[
            [lon, lat], [lon + 0.0001, lat], [lon + 0.0001, lat + 0.0001],
            [lon, lat + 0.0001], [lon, lat]]]}})
}

fn area(w: f64, s: f64, e: f64, n: f64) -> Value {
    json!({"type":"Polygon","coordinates":[[[w,s],[e,s],[e,n],[w,n],[w,s]]]})
}

fn acquisition_digest_at(cells: [f64; 4], version: &str) -> String {
    let mut dataset = buildings();
    dataset.source_version = version.into();
    let policy: BufferPolicy = serde_json::from_value(policy_value()).unwrap();
    ds_command_kernel::project_dataset_cache::query_digest(
        &scope(),
        &dataset,
        &policy,
        &Coverage { cells: vec![cells] },
    )
}

fn acquisition_digest(cells: [f64; 4]) -> String {
    acquisition_digest_at(cells, "v1")
}

/// One acquisition at an explicit source version, optionally as a refresh
/// of coverage the room already holds.
fn acquire_at(
    root: &Path,
    key: &str,
    cells: [f64; 4],
    features: Vec<Value>,
    version: &str,
    refresh: bool,
) -> Value {
    let digest = acquisition_digest_at(cells, version);
    let [w, s, e, n] = cells;
    let begun = execute_at(
        root,
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"area":area(w,s,e,n),"cells":{"cells":[cells]},
            "source_version":version,"refresh":refresh}),
    )
    .unwrap();
    assert_eq!(begun["accepted"], true, "{begun}");
    execute_at(
        root,
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cells]},
            "features":features,"source_version":version}),
    )
    .unwrap()
}

fn status_of(root: &Path, key: &str) -> Value {
    execute_at(
        root,
        json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":key}),
    )
    .unwrap()
}

#[test]
fn shared_vector_corridor_drives_the_exact_scoped_held_query() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    acquire(
        root.path(),
        &key,
        [29.99, -2.01, 30.02, -1.98],
        vec![
            feature(30.005, -2.00005, "inside"),
            feature(29.9996, -1.9996, "bbox-only"),
            feature(30.015, -1.985, "outside"),
        ],
        0,
    );
    let packet = ds_command_kernel::vector_ops::evaluate(
        &serde_json::to_vec(&json!({
            "schema":ds_command_kernel::vector_ops::SCHEMA,"operation":"buffer",
            "source":{"type":"LineString","coordinates":[[30.,-2.],[30.01,-2.]]},
            "radius_m":50.,"result_projection":"complete_produced"
        }))
        .unwrap(),
    )
    .unwrap();
    let buffered: Value = serde_json::from_str(&packet).unwrap();
    let corridor = buffered["result"]["features"][0]["geometry"].clone();
    ds_command_kernel::reference_cache::validate_area(&corridor).unwrap();
    let before = status_of(root.path(), &key);
    let request = json!({"action":"query","dataset_key":key,
        "principal":"uid-1","project":"project-a","area":corridor,"limit":10});
    let read = execute_at(root.path(), request.clone()).unwrap();
    assert_eq!(read["features"].as_array().unwrap().len(), 1);
    assert_eq!(read["features"][0]["properties"]["name"], "inside");
    assert_eq!(read["truncated"], false);
    assert_eq!(read["generation"], before["generation"]);
    let mut cross_project = request.clone();
    cross_project["project"] = json!("project-b");
    assert!(execute_at(root.path(), cross_project)
        .unwrap_err()
        .contains("different account or project"));
    let mut cross_principal = request;
    cross_principal["principal"] = json!("uid-2");
    assert!(execute_at(root.path(), cross_principal).is_err());
    assert_eq!(
        status_of(root.path(), &key),
        before,
        "vector processing/query does not mutate held data"
    );
}

#[test]
fn paginated_queries_are_complete_ordered_and_generation_pinned() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    acquire(
        root.path(),
        &key,
        [0., 0., 1., 1.],
        vec![
            feature(0.1, 0.1, "one"),
            feature(0.2, 0.2, "two"),
            feature(0.3, 0.3, "three"),
        ],
        0,
    );
    let mut request = json!({"action":"query","dataset_key":key,"principal":"uid-1","project":"project-a","area":area(0.,0.,1.,1.),"limit":1});
    let zero = execute_at(root.path(), request.clone()).unwrap();
    assert_eq!(zero["features"][0]["properties"]["name"], "one");
    assert_eq!(zero["next_after_id"], 0);
    request["limit"] = json!(2);
    let first = execute_at(root.path(), request.clone()).unwrap();
    assert_eq!(first["features"].as_array().unwrap().len(), 2);
    assert_eq!(first["truncated"], true);
    request["after_id"] = first["next_after_id"].clone();
    request["generation"] = first["generation"].clone();
    let second = execute_at(root.path(), request.clone()).unwrap();
    assert_eq!(second["features"].as_array().unwrap().len(), 1);
    assert_eq!(second["truncated"], false);
    assert!(second["next_after_id"].is_null());
    let names = first["features"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second["features"].as_array().unwrap())
        .map(|f| f["properties"]["name"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(names.len(), 3);
    request["generation"] = json!(0);
    assert!(execute_at(root.path(), request)
        .unwrap_err()
        .contains("between pages"));
    #[cfg(unix)]
    crate::private::assert_private_tree(root.path());
}

/// Move every recorded request back in time, so expiry can be proved
/// without a test that sleeps for half an hour.
fn age_requests(root: &Path, key: &str, by_ms: i64) {
    let connection = Connection::open(directory(root, key).unwrap().join(ROOM_FILE)).unwrap();
    connection
        .execute(
            "UPDATE queries SET requested_at = requested_at - ?1",
            params![by_ms],
        )
        .unwrap();
}

fn acquire(
    root: &Path,
    key: &str,
    cells: [f64; 4],
    features: Vec<Value>,
    generation: i64,
) -> Value {
    let digest = acquisition_digest(cells);
    let [w, s, e, n] = cells;
    let begun = execute_at(
        root,
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"area":area(w,s,e,n),"cells":{"cells":[cells]},
            "source_version":"v1","expected_generation":generation}),
    )
    .unwrap();
    assert_eq!(begun["accepted"], true, "{begun}");
    execute_at(
        root,
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cells]},"features":features,
            "source_version":"v1","expected_generation":generation}),
    )
    .unwrap()
}

#[test]
fn an_unseeded_dataset_reads_as_absent_and_creates_nothing() {
    let root = tempfile::tempdir().unwrap();
    let key = dataset_key(&scope(), &buildings());
    let status = execute_at(
        root.path(),
        json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":key}),
    )
    .unwrap();
    assert_eq!(status["exists"], false);
    assert_eq!(status["ready"], false);
    assert_eq!(status["index_state"], "absent");
    assert!(!root.path().join(ROOT).join(VERSION).join(&key).exists());
    // Querying an unseeded dataset is an explicit refusal, never an
    // empty answer that would read as "there are no buildings here".
    assert!(execute_at(
        root.path(),
        json!({"action":"query","principal":"uid-1","project":"project-a","dataset_key":key,"area":area(0.,0.,1.,1.)})
    )
    .is_err());
}

#[test]
fn opening_a_room_requires_its_canonical_dataset_identity() {
    let root = tempfile::tempdir().unwrap();
    let dataset = buildings();
    let key = dataset_key(&scope(), &dataset);
    let wrong = execute_at(
        root.path(),
        json!({
            "action": "open", "dataset_key": key,
            "principal": "uid-1", "project": "project-a",
            "dataset_id": "elevation_contours", "provider": dataset.provider,
            "quality": "source", "parameters": dataset.parameters,
            "crs": dataset.crs, "source_version": "v1",
            "buffer_policy": policy_value(),
        }),
    );
    assert!(
        wrong.unwrap_err().contains("canonical identity"),
        "a caller must not relabel a digest-addressed room"
    );
    assert!(!root
        .path()
        .join(ROOT)
        .join(VERSION)
        .join(&key)
        .join(ROOM_FILE)
        .exists());
}

#[test]
fn begin_binds_the_digest_and_area_to_the_requested_coverage() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let cell = [29.9, -2.1, 30.1, -1.9];

    let wrong_digest = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":"a".repeat(64),"area":area(cell[0],cell[1],cell[2],cell[3]),
            "cells":{"cells":[cell]},"source_version":"v1","expected_generation":0}),
    );
    assert!(wrong_digest.unwrap_err().contains("canonical identity"));

    let wrong_area = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":acquisition_digest(cell),"area":area(0.,0.,1.,1.),
            "cells":{"cells":[cell]},"source_version":"v1","expected_generation":0}),
    );
    assert!(wrong_area.unwrap_err().contains("exactly match"));

    let status = execute_at(
        root.path(),
        json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":key}),
    )
    .unwrap();
    assert_eq!(status["queries"].as_array().unwrap().len(), 0);
    assert!(status["requested"]["cells"].as_array().unwrap().is_empty());
}

#[test]
fn commit_requires_the_matching_requested_state_and_exact_cells() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let cell = [29.9, -2.1, 30.1, -1.9];
    let digest = acquisition_digest(cell);

    let unbegun = execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"cells":{"cells":[cell]},"features":[],
            "source_version":"v1","expected_generation":0}),
    );
    assert!(unbegun
        .unwrap_err()
        .contains("no matching begun acquisition"));

    let begun = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"area":area(cell[0],cell[1],cell[2],cell[3]),
            "cells":{"cells":[cell]},"source_version":"v1","expected_generation":0}),
    )
    .unwrap();
    let altered = execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[[29.8,-2.2,30.2,-1.8]]},"features":[],
            "source_version":"v1","expected_generation":0}),
    );
    assert!(altered.unwrap_err().contains("differs from the begun"));

    let completed = execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cell]},"features":[],
            "source_version":"v1","expected_generation":0}),
    )
    .unwrap();
    assert_eq!(completed["generation"], 1);

    let repeated = execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cell]},"features":[],
            "source_version":"v1","expected_generation":1}),
    );
    assert!(repeated.unwrap_err().contains("only a requested"));
}

#[test]
fn a_completed_acquisition_indexes_and_answers_by_area() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let status = acquire(
        root.path(),
        &key,
        [29.9, -2.1, 30.1, -1.9],
        vec![feature(30.0, -2.0, "inside"), feature(30.05, -2.05, "also")],
        0,
    );
    assert_eq!(status["feature_count"], 2);
    assert_eq!(status["index_state"], "ready");
    assert_eq!(status["ready"], true);
    assert_eq!(status["generation"], 1);
    assert_eq!(status["last_error"], Value::Null);

    let hit = execute_at(
        root.path(),
        json!({"action":"query","principal":"uid-1","project":"project-a","dataset_key":key,"area":area(29.99,-2.01,30.01,-1.99)}),
    )
    .unwrap();
    assert_eq!(hit["features"].as_array().unwrap().len(), 1);
    assert_eq!(hit["features"][0]["properties"]["name"], "inside");
    assert_eq!(hit["cache"], "project_room");

    let miss = execute_at(
        root.path(),
        json!({"action":"query","principal":"uid-1","project":"project-a","dataset_key":key,"area":area(10.,10.,11.,11.)}),
    );
    assert!(miss.unwrap_err().contains("completed coverage"));
}

#[test]
fn overlapping_acquisitions_never_double_count_a_building() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let shared = feature(30.0, -2.0, "shared");
    acquire(
        root.path(),
        &key,
        [29.9, -2.1, 30.1, -1.9],
        vec![shared.clone()],
        0,
    );
    let status = acquire(
        root.path(),
        &key,
        [30.0, -2.05, 30.2, -1.85],
        vec![shared, feature(30.15, -1.9, "new")],
        1,
    );
    assert_eq!(status["feature_count"], 2);
    let all = execute_at(
        root.path(),
        json!({"action":"query","principal":"uid-1","project":"project-a","dataset_key":key,"area":status["completed_geometry"]}),
    )
    .unwrap();
    assert_eq!(all["features"].as_array().unwrap().len(), 2);
}

#[test]
fn a_repeat_of_identical_work_is_refused_and_concurrency_is_fenced() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let digest = acquisition_digest([29.9, -2.1, 30.1, -1.9]);
    acquire(
        root.path(),
        &key,
        [29.9, -2.1, 30.1, -1.9],
        vec![feature(30.0, -2.0, "one")],
        0,
    );
    // The same acquisition asked for again is answered from what is held.
    let again = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"area":area(29.9,-2.1,30.1,-1.9),
            "cells":{"cells":[[29.9,-2.1,30.1,-1.9]]},"source_version":"v1"}),
    )
    .unwrap();
    assert_eq!(again["accepted"], false);
    assert_eq!(again["reason"], "already_completed");

    // A second worker starting the same NEW acquisition is told it is
    // already in flight rather than buying the same rows twice.
    let fresh = acquisition_digest([31., -2.1, 31.2, -1.9]);
    let first = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":fresh,"area":area(31.,-2.1,31.2,-1.9),
            "cells":{"cells":[[31.,-2.1,31.2,-1.9]]},"source_version":"v1"}),
    )
    .unwrap();
    assert_eq!(first["accepted"], true);
    let second = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":fresh,"area":area(31.,-2.1,31.2,-1.9),
            "cells":{"cells":[[31.,-2.1,31.2,-1.9]]},"source_version":"v1"}),
    )
    .unwrap();
    assert_eq!(second["accepted"], false);
    assert_eq!(second["reason"], "in_flight");
}

#[test]
fn waiting_claims_recheck_overlapping_completion_and_failed_owners() {
    for (other, fail, accepted) in [
        ([30.0, -2.05, 30.05, -1.95], false, false),
        ([30.0, -2.05, 30.2, -1.95], false, true),
        ([30.0, -2.05, 30.05, -1.95], true, true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let key = open(root.path(), &scope(), &buildings());
        let cell = [29.9, -2.1, 30.1, -1.9];
        let digest = acquisition_digest(cell);
        let begun = execute_at(
            root.path(),
            json!({
                "action":"begin", "dataset_key":key, "principal":"uid-1", "project":"project-a",
                "query_digest":digest, "area":area(cell[0],cell[1],cell[2],cell[3]),
                "cells":{"cells":[cell]}, "source_version":"v1"
            }),
        )
        .unwrap();
        let path = root.path().to_owned();
        let other_key = key.clone();
        let (send, receive) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let result = execute_at(
                &path,
                json!({
                    "action":"begin", "dataset_key":other_key, "principal":"uid-1", "project":"project-a",
                    "query_digest":acquisition_digest(other), "area":area(other[0],other[1],other[2],other[3]),
                    "cells":{"cells":[other]}, "source_version":"v1", "wait_for_pending":true
                }),
            );
            send.send(result).unwrap();
        });
        assert!(matches!(
            receive.recv_timeout(std::time::Duration::from_millis(80)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        execute_at(
            root.path(),
            json!({
                "action":if fail {"fail"} else {"commit"}, "dataset_key":key,
                "principal":"uid-1", "project":"project-a", "query_digest":digest,
                "ticket":begun["ticket"], "cells":{"cells":[cell]}, "features":[],
                "source_version":"v1", "message":"provider failed"
            }),
        )
        .unwrap();
        let result = receive
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .unwrap();
        waiter.join().unwrap();
        assert_eq!(result["accepted"], accepted, "{result}");
        if !accepted {
            assert_eq!(result["reason"], "already_completed");
        }
    }
}

#[test]
fn a_stale_completion_is_refused_and_held_data_survives_a_failure() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    acquire(
        root.path(),
        &key,
        [29.9, -2.1, 30.1, -1.9],
        vec![feature(30.0, -2.0, "kept")],
        0,
    );
    // A completion with no matching acquisition is refused.
    let stale = execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":"e".repeat(64),"cells":{"cells":[[0.,0.,1.,1.]]},
            "features":[],"source_version":"v1","expected_generation":0}),
    );
    assert!(stale.is_err());
    // So is one carrying the wrong account or project.
    let wrong = execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-2","project":"project-a",
            "query_digest":"e".repeat(64),"cells":{"cells":[[0.,0.,1.,1.]]},
            "features":[],"source_version":"v1"}),
    );
    assert!(wrong.is_err());

    let pending = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,
        "principal":"uid-1","project":"project-a","query_digest":acquisition_digest([0.,0.,1.,1.]),
        "area":area(0.,0.,1.,1.),"cells":{"cells":[[0.,0.,1.,1.]]},"source_version":"v1"}),
    )
    .unwrap();
    let failed = execute_at(
        root.path(),
        json!({"action":"fail","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":pending["query_digest"],"ticket":pending["ticket"],"message":"the provider was unreachable"}),
    )
    .unwrap();
    assert_eq!(failed["last_error"], "the provider was unreachable");
    assert_eq!(failed["feature_count"], 1, "a failure kept held data");
    assert_eq!(failed["completed"]["cells"].as_array().unwrap().len(), 1);
    let still = execute_at(
        root.path(),
        json!({"action":"query","principal":"uid-1","project":"project-a","dataset_key":key,"area":area(29.9,-2.1,30.1,-1.9)}),
    )
    .unwrap();
    assert_eq!(still["features"].as_array().unwrap().len(), 1);
}

#[test]
fn a_complete_empty_result_counts_as_covered() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let status = acquire(root.path(), &key, [40.0, 10.0, 40.1, 10.1], vec![], 0);
    // A complete provider query returned no rows. Preserve that receipt
    // without claiming the provider covers every real-world building.
    assert_eq!(status["feature_count"], 0);
    assert_eq!(status["index_state"], "empty");
    assert_eq!(status["ready"], true);
    let completed: Coverage = serde_json::from_value(status["completed"].clone()).unwrap();
    let wanted = Coverage {
        cells: vec![[40.0, 10.0, 40.1, 10.1]],
    };
    assert!(
        ds_command_kernel::project_dataset_cache::assess(&completed, &wanted)
            .unwrap()
            .covered
    );
}

#[test]
fn a_requested_area_that_never_completed_is_not_held() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":acquisition_digest([0.,0.,1.,1.]),"area":area(0.,0.,1.,1.),
            "cells":{"cells":[[0.,0.,1.,1.]]},"source_version":"v1"}),
    )
    .unwrap();
    let status = execute_at(
        root.path(),
        json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":key}),
    )
    .unwrap();
    assert_eq!(status["requested"]["cells"].as_array().unwrap().len(), 1);
    assert_eq!(status["completed"]["cells"].as_array().unwrap().len(), 0);
    // Something is mid-flight, so the dataset is not ready even though
    // nothing has failed.
    assert_eq!(status["pending_queries"], 1);
    assert_eq!(status["ready"], false);
}

#[test]
fn datasets_and_principals_never_share_a_room_or_a_readiness() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    acquire(
        root.path(),
        &key,
        [29.9, -2.1, 30.1, -1.9],
        vec![feature(30.0, -2.0, "building")],
        0,
    );
    let contours = Dataset {
        id: "elevation_contours".into(),
        ..buildings()
    };
    let other_key = dataset_key(&scope(), &contours);
    let other = execute_at(
        root.path(),
        json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":other_key}),
    )
    .unwrap();
    assert_eq!(other["exists"], false, "buildings spoke for contours");

    let elsewhere = Scope {
        principal: "uid-2".into(),
        ..scope()
    };
    let foreign = dataset_key(&elsewhere, &buildings());
    assert_eq!(
        execute_at(
            root.path(),
            json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":foreign})
        )
        .unwrap()["exists"],
        false
    );

    let inventory = project_inventory(root.path(), "uid-1", "project-a").unwrap();
    assert_eq!(inventory["rooms"].as_array().unwrap().len(), 1);
    assert_eq!(
        inventory["datasets"]["google_open_buildings"]["feature_count"],
        1
    );
    assert_eq!(
        project_inventory(root.path(), "uid-2", "project-a").unwrap()["rooms"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn a_new_source_version_is_freshness_and_keeps_the_same_room() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    acquire(
        root.path(),
        &key,
        [29.9, -2.1, 30.1, -1.9],
        vec![feature(30.0, -2.0, "held")],
        0,
    );
    let republished = Dataset {
        source_version: "v2".into(),
        ..buildings()
    };
    assert_eq!(dataset_key(&scope(), &republished), key);
    let reopened = open(root.path(), &scope(), &republished);
    let status = execute_at(
        root.path(),
        json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":reopened}),
    )
    .unwrap();
    assert_eq!(
        status["source_version"], "v1",
        "opening must not relabel held data"
    );
    assert_eq!(status["feature_count"], 1, "freshness reset held coverage");
    assert_eq!(status["completed"]["cells"].as_array().unwrap().len(), 1);
}

#[test]
fn a_refresh_deletes_under_the_same_rule_the_provider_selects_with() {
    // A footprint that straddles the eastern edge of the refreshed cell is
    // the case that separates the two possible rules. The buildings door
    // fetches by intersection, so this footprint belongs to this cell and a
    // refresh both removes it and is allowed to put it back. Were deletion
    // wider than acquisition, the building would leave the room and never
    // return.
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let cell = [29.9, -2.1, 30.1, -1.9];
    let crossing = json!({"type":"Feature","properties":{"name":"crossing"},
        "geometry":{"type":"Polygon","coordinates":[[
            [30.05,-2.0],[30.15,-2.0],[30.15,-1.99],[30.05,-1.99],[30.05,-2.0]]]}});
    assert_eq!(
        policy::acquisition_selection(policy::BUILDINGS_DATASET_ID, "").unwrap(),
        ds_command_kernel::reference_cache::AreaSelection::Intersects
    );
    acquire(root.path(), &key, cell, vec![crossing.clone()], 0);
    assert_eq!(status_of(root.path(), &key)["feature_count"], 1);

    // Re-acquired unchanged: still exactly one row, never a duplicate.
    let again = acquire_at(root.path(), &key, cell, vec![crossing.clone()], "v2", true);
    assert_eq!(again["feature_count"], 1);

    // Re-acquired as gone: a provider that selects by intersection would
    // have returned it if it still existed, so the room stops holding it.
    let emptied = acquire_at(root.path(), &key, cell, vec![], "v3", true);
    assert_eq!(emptied["feature_count"], 0);
}

#[test]
fn a_dataset_with_no_recorded_selection_rule_cannot_land_rows_at_all() {
    // Deleting under a guessed predicate is the operation that loses data,
    // so an unregistered dataset is refused rather than defaulted.
    let root = tempfile::tempdir().unwrap();
    let unknown = Dataset {
        id: "some_new_dataset".into(),
        ..buildings()
    };
    let key = open(root.path(), &scope(), &unknown);
    let cell = [29.9, -2.1, 30.1, -1.9];
    let digest = ds_command_kernel::project_dataset_cache::query_digest(
        &scope(),
        &Dataset {
            source_version: "v1".into(),
            ..unknown.clone()
        },
        &serde_json::from_value(policy_value()).unwrap(),
        &Coverage { cells: vec![cell] },
    );
    let begun = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"area":area(cell[0],cell[1],cell[2],cell[3]),
            "cells":{"cells":[cell]},"source_version":"v1"}),
    )
    .unwrap();
    let refused = execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cell]},
            "features":[feature(30.0,-2.0,"nowhere")],"source_version":"v1"}),
    )
    .unwrap_err();
    assert!(refused.contains("acquisition_selection"), "{refused}");
    assert_eq!(status_of(root.path(), &key)["feature_count"], 0);
}

#[test]
fn coverage_completed_under_a_replaced_version_is_reported_and_never_silently_reused() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let older = [29.9, -2.1, 30.1, -1.9];
    let newer = [30.9, -2.1, 31.1, -1.9];
    acquire(
        root.path(),
        &key,
        older,
        vec![feature(30.0, -2.0, "held")],
        0,
    );
    let first = status_of(root.path(), &key);
    assert_eq!(first["fresh"], true, "one version is not stale");
    assert_eq!(first["available_version"], "v1");

    // A second acquisition answers under a newer publication. That is how
    // the room learns the first one is now behind.
    acquire_at(
        root.path(),
        &key,
        newer,
        vec![feature(31.0, -2.0, "fresh")],
        "v2",
        false,
    );
    let stale = status_of(root.path(), &key);
    assert_eq!(stale["available_version"], "v2");
    assert_eq!(stale["fresh"], false);
    assert_eq!(stale["stale"]["cells"], json!([older]));
    assert_eq!(stale["source_versions"], json!(["v1", "v2"]));
    // Staleness on its own destroyed nothing: both holdings are intact and
    // the older area still answers reads.
    assert_eq!(stale["feature_count"], 2);
    assert_eq!(
        execute_at(
            root.path(),
            json!({"action":"query","dataset_key":key,
            "principal":"uid-1","project":"project-a",
            "area":area(older[0],older[1],older[2],older[3])})
        )
        .unwrap()["features"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // The dedicated read says the same thing and costs nothing.
    let report = execute_at(
        root.path(),
        json!({"action":"freshness","dataset_key":key,"principal":"uid-1","project":"project-a"}),
    )
    .unwrap();
    assert_eq!(report["current"], "v2");
    assert_eq!(report["stale"]["cells"], json!([older]));
    assert_eq!(report["stale_geometry"]["type"], "MultiPolygon");

    // Held coverage still answers a repeat — but the refusal now carries
    // the verdict, so no caller reuses obsolete rows without being told.
    let reused = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":acquisition_digest_at(older,"v2"),
            "area":area(older[0],older[1],older[2],older[3]),
            "cells":{"cells":[older]},"source_version":"v2"}),
    )
    .unwrap();
    assert_eq!(reused["accepted"], false);
    assert_eq!(reused["reason"], "already_completed");
    assert_eq!(reused["stale"], true);
    assert_eq!(reused["available_version"], "v2");

    // The explicit refresh is the only thing that replaces those rows.
    let refreshed = acquire_at(
        root.path(),
        &key,
        older,
        vec![feature(30.0, -2.0, "re-acquired")],
        "v2",
        true,
    );
    assert_eq!(refreshed["fresh"], true);
    assert_eq!(refreshed["stale_count"], 0);
    assert_eq!(refreshed["source_versions"], json!(["v2"]));
    assert_eq!(refreshed["source_version"], "v2");
    assert_eq!(refreshed["feature_count"], 2);
    let rows = refreshed["queries"].as_array().unwrap();
    assert!(
        rows.iter().any(|row| row["refresh"] == true),
        "a refresh is recorded as one: {rows:?}"
    );
}

#[test]
fn a_failed_refresh_of_an_exact_tile_keeps_its_completed_provenance() {
    // An explicit refresh begins at the version the room holds, so over a
    // tile it completed before, its ticket has that completion's digest and
    // reuses its record. Until a commit replaces the tile, the old
    // completion still accounts for its rows — in flight, after a failure,
    // and once the ticket expires.
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let cell = [29.9, -2.1, 30.1, -1.9];
    let [w, s, e, n] = cell;
    let digest = acquisition_digest_at(cell, "v1");
    acquire_at(
        root.path(),
        &key,
        cell,
        vec![feature(30.0, -2.0, "held")],
        "v1",
        false,
    );
    open(
        root.path(),
        &scope(),
        &Dataset {
            source_version: "v2".into(),
            ..buildings()
        },
    );
    let held = |label: &str| {
        let status = status_of(root.path(), &key);
        assert_eq!(status["source_version"], "v1", "{label}");
        assert_eq!(status["source_versions"], json!(["v1"]), "{label}");
        assert_eq!(status["fresh"], false, "{label}");
        assert_eq!(status["stale"]["cells"], json!([cell]), "{label}");
        assert_eq!(status["unattributed"]["cells"], json!([]), "{label}");
        assert_eq!(status["completed"]["cells"], json!([cell]), "{label}");
        assert_eq!(status["feature_count"], 1, "{label}");
        let report = execute_at(
            root.path(),
            json!({"action":"freshness","dataset_key":key,"principal":"uid-1","project":"project-a"}),
        )
        .unwrap();
        assert_eq!(report["versions"], json!(["v1"]), "{label}");
        assert_eq!(report["stale"]["cells"], json!([cell]), "{label}");
        let rows = execute_at(
            root.path(),
            json!({"action":"query","dataset_key":key,"principal":"uid-1","project":"project-a",
                "area":area(w,s,e,n)}),
        )
        .unwrap();
        assert_eq!(rows["features"][0]["properties"]["name"], "held", "{label}");
        assert_eq!(rows["features"].as_array().unwrap().len(), 1, "{label}");
    };
    held("completed");
    let begin = || {
        execute_at(
            root.path(),
            json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
                "query_digest":digest,"area":area(w,s,e,n),"cells":{"cells":[cell]},
                "source_version":"v1","refresh":true}),
        )
        .unwrap()
    };

    let pending = begin();
    assert_eq!(pending["accepted"], true, "{pending}");
    held("refresh in flight");
    let failed = execute_at(
        root.path(),
        json!({"action":"fail","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":pending["ticket"],"message":"the provider was unreachable"}),
    )
    .unwrap();
    assert_eq!(failed["last_error"], "the provider was unreachable");
    held("refresh failed");

    let abandoned = begin();
    assert_eq!(abandoned["accepted"], true, "{abandoned}");
    age_requests(
        root.path(),
        &key,
        ds_command_kernel::project_dataset_cache::ACQUISITION_TICKET_TTL_MS + 1,
    );
    held("refresh expired");

    // Only a successful replacement moves the tile to the new version.
    let refreshed = begin();
    assert_eq!(refreshed["accepted"], true, "{refreshed}");
    let replaced = execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":refreshed["ticket"],"cells":{"cells":[cell]},
            "features":[feature(30.0, -2.0, "re-acquired")],"source_version":"v2"}),
    )
    .unwrap();
    assert_eq!(replaced["source_version"], "v2");
    assert_eq!(replaced["source_versions"], json!(["v2"]));
    assert_eq!(replaced["fresh"], true);
    assert_eq!(replaced["feature_count"], 1);
    assert_eq!(replaced["completed"]["cells"], json!([cell]));
}

#[test]
fn an_abandoned_acquisition_expires_and_can_no_longer_settle_anything() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let cell = [0., 0., 1., 1.];
    let digest = acquisition_digest(cell);
    let pending = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"area":area(0.,0.,1.,1.),
            "cells":{"cells":[cell]},"source_version":"v1"}),
    )
    .unwrap();
    assert_eq!(status_of(root.path(), &key)["pending_queries"], 1);

    age_requests(
        root.path(),
        &key,
        ds_command_kernel::project_dataset_cache::ACQUISITION_TICKET_TTL_MS + 1,
    );
    let after = status_of(root.path(), &key);
    // The count stops spinning: nothing is pending, one attempt is gone.
    assert_eq!(after["pending_queries"], 0);
    assert_eq!(after["expired_queries"], 1);
    assert_eq!(after["queries"][0]["state"], "expired");
    assert_eq!(
        after["queries"][0]["status"], "requested",
        "expiry is a verdict on a stored request, not a rewrite of it"
    );

    // An expired ticket settles nothing, either way.
    let late = execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":pending["ticket"],"cells":{"cells":[cell]},
            "features":[],"source_version":"v1"}),
    )
    .unwrap_err();
    assert!(late.contains("expired"), "{late}");
    let blamed = execute_at(
        root.path(),
        json!({"action":"fail","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":pending["ticket"],"message":"too late"}),
    )
    .unwrap_err();
    assert!(blamed.contains("expired"), "{blamed}");
    assert_eq!(status_of(root.path(), &key)["last_error"], Value::Null);

    // And the area it was holding is free to be acquired again.
    assert_eq!(
        acquire_at(root.path(), &key, cell, vec![], "v1", false)["completed"]["cells"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn an_oversized_source_receipt_is_refused_and_costs_no_held_data() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let first = [29.9, -2.1, 30.1, -1.9];
    acquire(
        root.path(),
        &key,
        first,
        vec![feature(30.0, -2.0, "held")],
        0,
    );
    let cell = [30.9, -2.1, 31.1, -1.9];
    let digest = acquisition_digest(cell);
    let begun = execute_at(
        root.path(),
        json!({"action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"area":area(cell[0],cell[1],cell[2],cell[3]),
            "cells":{"cells":[cell]},"source_version":"v1"}),
    )
    .unwrap();
    let refused = execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cell]},
            "features":[],"source_version":"v1",
            "source_receipt":{"body":"x".repeat(
                ds_command_kernel::project_dataset_cache::MAX_SOURCE_RECEIPT_BYTES)}}),
    )
    .unwrap_err();
    assert!(refused.contains("source receipt"), "{refused}");
    // A receipt that will not fit is a refusal, not a trimmed receipt, and
    // it costs the room nothing it already held.
    let after = status_of(root.path(), &key);
    assert_eq!(after["feature_count"], 1);
    assert_eq!(after["completed"]["cells"], json!([first]));
    // A receipt that is not an object is refused for the same reason.
    assert!(execute_at(
        root.path(),
        json!({"action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
            "query_digest":digest,"ticket":begun["ticket"],"cells":{"cells":[cell]},
            "features":[],"source_version":"v1","source_receipt":"a receipt"}),
    )
    .is_err());
}

#[test]
fn an_inventory_never_reveals_another_accounts_rooms() {
    let root = tempfile::tempdir().unwrap();
    let mine = open(root.path(), &scope(), &buildings());
    acquire(
        root.path(),
        &mine,
        [29.9, -2.1, 30.1, -1.9],
        vec![feature(30.0, -2.0, "mine")],
        0,
    );
    let theirs = Scope {
        principal: "uid-2".into(),
        project: "project-z".into(),
    };
    open(root.path(), &theirs, &buildings());

    let ours = inventory(root.path(), "uid-1").unwrap();
    assert_eq!(ours["rooms"].as_array().unwrap().len(), 1);
    assert_eq!(ours["rooms"][0]["dataset_key"], mine);
    assert_eq!(ours["principal"], "uid-1");
    // The other account's room exists on this computer and is invisible —
    // not merely filtered out of the answer, but never projected into one.
    let names: Vec<String> = ours["rooms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|room| room["project"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(!names.contains(&"project-z".to_string()));
    assert_eq!(
        inventory(root.path(), "uid-2").unwrap()["rooms"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // An inventory with no principal is refused rather than answered for
    // everyone.
    assert!(inventory(root.path(), "  ").is_err());
    assert!(project_inventory(root.path(), "uid-1", " ").is_err());
}

#[test]
fn traversal_and_linked_rooms_are_refused() {
    let root = tempfile::tempdir().unwrap();
    assert!(execute_at(
        root.path(),
        json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":"../escape"})
    )
    .is_err());
    assert!(execute_at(root.path(), json!({"action":"status"})).is_err());
    assert!(execute_at(
        root.path(),
        json!({"action":"status","principal":"uid-1","project":"project-a","dataset_key":"A".repeat(64)})
    )
    .is_err());
}
#[test]
fn disjoint_acquisitions_commit_in_any_order_and_refresh_replaces_rows() {
    let root = tempfile::tempdir().unwrap();
    let key = open(root.path(), &scope(), &buildings());
    let begin = |cell: [f64; 4], refresh: bool| {
        execute_at(
            root.path(),
            json!({
        "action":"begin","dataset_key":key,"principal":"uid-1","project":"project-a",
        "query_digest":acquisition_digest(cell),"cells":{"cells":[cell]},
        "area":area(cell[0],cell[1],cell[2],cell[3]),"source_version":"v1","refresh":refresh}),
        )
        .unwrap()
    };
    let commit = |cell: [f64; 4], ticket: &Value, features: Vec<Value>| {
        execute_at(
            root.path(),
            json!({
        "action":"commit","dataset_key":key,"principal":"uid-1","project":"project-a",
        "query_digest":acquisition_digest(cell),"cells":{"cells":[cell]},
        "ticket":ticket["ticket"],"features":features,"source_version":"v1"}),
        )
    };
    let a = [29.9, -2.1, 30.1, -1.9];
    let b = [30.9, -2.1, 31.1, -1.9];
    let first = begin(a, false);
    let second = begin(b, false);
    assert_eq!(first["generation"], second["generation"]);
    commit(b, &second, vec![feature(31., -2., "retained")]).unwrap();
    assert_eq!(
        commit(a, &first, vec![feature(30., -2., "old")]).unwrap()["feature_count"],
        2
    );
    let refresh = begin(a, true);
    assert!(commit(a, &first, vec![]).unwrap_err().contains("ticket"));
    let replaced = commit(a, &refresh, vec![]).unwrap();
    assert_eq!(replaced["feature_count"], 1);
    let forbidden = execute_at(
        root.path(),
        json!({"action":"query","dataset_key":key,
        "principal":"uid-2","project":"project-a","area":area(30.9,-2.1,31.1,-1.9)}),
    );
    assert!(forbidden.unwrap_err().contains("different account"));
}

#[test]
fn applied_shared_revisions_are_recorded_per_head_and_never_create_a_room() {
    let root = tempfile::tempdir().unwrap();
    let key = dataset_key(&scope(), &buildings());
    let request = |action: &str, extra: Value| {
        let mut request = json!({"action": action, "dataset_key": key,
            "principal": "uid-1", "project": "project-a", "head": "pshared"});
        for (name, value) in extra.as_object().unwrap() {
            request[name] = value.clone();
        }
        request
    };
    let absent = execute_at(root.path(), request("revisions", json!({}))).unwrap();
    assert_eq!(absent["applied"], json!([]));
    assert!(!root
        .path()
        .join(ROOT)
        .join(VERSION)
        .join(&key)
        .join(ROOM_FILE)
        .exists());
    assert!(execute_at(
        root.path(),
        request(
            "applied",
            json!({"revision": 1, "manifest_sha256": "a".repeat(64)})
        )
    )
    .is_err());
    open(root.path(), &scope(), &buildings());
    for (revision, digest) in [(2, "b"), (1, "a")] {
        execute_at(
            root.path(),
            request(
                "applied",
                json!({"revision": revision, "manifest_sha256": digest.repeat(64)}),
            ),
        )
        .unwrap();
    }
    let read = execute_at(root.path(), request("revisions", json!({}))).unwrap();
    assert_eq!(read["applied"], json!([1, 2]));
    assert_eq!(read["manifest_sha256"], json!("b".repeat(64)));
    let mut other_head = request("revisions", json!({}));
    other_head["head"] = json!("pother");
    assert_eq!(
        execute_at(root.path(), other_head).unwrap()["applied"],
        json!([])
    );
    let mut foreign = request("revisions", json!({}));
    foreign["principal"] = json!("uid-2");
    assert!(execute_at(root.path(), foreign).is_err());
    for revision in [0, 100] {
        assert!(execute_at(
            root.path(),
            request(
                "applied",
                json!({"revision": revision, "manifest_sha256": "a".repeat(64)})
            )
        )
        .is_err());
    }
    assert!(execute_at(
        root.path(),
        request(
            "applied",
            json!({"revision": 3, "manifest_sha256": "not-a-digest"})
        )
    )
    .is_err());
}
