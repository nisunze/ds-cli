use ds_canvas_graph::arrange::{ArrangeInput, arrange};
use serde_json::json;
fn run(value: serde_json::Value) -> Vec<f64> {
    arrange(&serde_json::from_value(value).unwrap()).unwrap()
}
fn node(id: &str, width: f64, height: f64) -> serde_json::Value {
    json!({"id":id,"width":width,"height":height})
}
#[test]
fn topology_calendar_and_variable_card_sizes_are_compacted_together() {
    let result = run(
        json!({"nodes":[node("a",200.0,60.0),node("b",200.0,80.0),node("next",120.0,50.0)],"edges":[{"from":"a","to":"next"},{"from":"b","to":"next"}],"gapX":20,"gapY":10}),
    );
    assert_eq!(result[2], 0.0);
    assert_eq!(result[5], 0.0);
    assert_eq!(result[8], 220.0);
    assert_eq!(result[3], 0.0);
    assert_eq!(result[6], 70.0);
    assert!(result[9] > 0.0 && result[9] < 150.0);
    let dated = run(
        json!({"nodes":[{"id":"early","width":20,"height":40,"day":1},{"id":"late","width":20,"height":40,"day":5}],"edges":[],"origin":1,"dayWidth":20,"gapX":10,"gapY":10}),
    );
    assert_eq!(dated[3], dated[6]);
    assert_eq!(dated[5], 80.0);
}
#[test]
fn array_order_and_duplicate_edges_cannot_change_layout() {
    let nodes = vec![
        node("a", 100.0, 40.0),
        node("b", 100.0, 40.0),
        node("after-b", 100.0, 40.0),
        node("after-a", 100.0, 40.0),
    ];
    let edges = json!([{"from":"a","to":"after-a"},{"from":"b","to":"after-b"}]);
    let first = run(json!({"nodes":nodes,"edges":edges}));
    let mut backwards = nodes.clone();
    backwards.reverse();
    let second = run(
        json!({"nodes":backwards,"edges":[{"from":"b","to":"after-b"},{"from":"a","to":"after-a"},{"from":"a","to":"after-a"}]}),
    );
    for i in 0..4 {
        assert_eq!(
            &first[2 + i * 3..5 + i * 3],
            &second[2 + (3 - i) * 3..5 + (3 - i) * 3]
        );
    }
    assert!(first[12] < first[9]);
}
#[test]
fn malformed_cycles_still_draw_without_recursive_depth_or_unstable_order() {
    let result = run(
        json!({"nodes":[node("a",100.0,40.0),node("b",100.0,40.0),node("c",100.0,40.0)],"edges":[{"from":"a","to":"b"},{"from":"b","to":"c"},{"from":"c","to":"a"}]}),
    );
    assert!(result.iter().all(|n| n.is_finite()));
    assert!(result[8] > result[5] && result[5] > result[2]);
}
#[test]
fn long_chains_and_dense_columns_use_bounded_iterative_work() {
    let nodes: Vec<_> = (0..10000)
        .map(|i| node(&format!("node-{i:05}"), 100.0, 40.0))
        .collect();
    let edges: Vec<_> = (1..nodes.len())
        .map(|i| json!({"from":format!("node-{:05}",i-1),"to":format!("node-{i:05}")}))
        .collect();
    let chain = run(json!({"nodes":nodes,"edges":edges}));
    assert_eq!(*chain.last().unwrap(), 9999.0);
    let column = run(json!({"nodes":nodes,"edges":[]}));
    assert_eq!(column[3], 0.0);
    assert!(column[1] > 400000.0);
}
#[test]
fn mixed_footprints_never_overlap_and_vertical_orientation_swaps_axes() {
    let nodes:Vec<_>=(0..50).map(|i|json!({"id":format!("n{i:02}"),"width":20+(i%7)*13,"height":30+(i%3)*11,"minimumFlow":(i%5)*20})).collect();
    let result = run(json!({"nodes":nodes,"edges":[],"gapX":10,"gapY":10}));
    for i in 0..nodes.len() {
        for j in i + 1..nodes.len() {
            let (x, y) = (result[2 + i * 3], result[3 + i * 3]);
            let (a, b) = (result[2 + j * 3], result[3 + j * 3]);
            let w = nodes[i]["width"].as_f64().unwrap();
            let h = nodes[i]["height"].as_f64().unwrap();
            let u = nodes[j]["width"].as_f64().unwrap();
            let v = nodes[j]["height"].as_f64().unwrap();
            assert!(
                x + w + 10.0 <= a || a + u + 10.0 <= x || y + h + 10.0 <= b || b + v + 10.0 <= y,
                "overlapping nodes {i} {j}"
            );
        }
    }
    let vertical = run(
        json!({"nodes":[node("a",100.0,60.0),node("b",100.0,60.0)],"edges":[{"from":"a","to":"b"}],"orientation":"vertical","gapX":20}),
    );
    assert_eq!(vertical[2], vertical[5]);
    assert_eq!(vertical[6], 80.0);
}
#[test]
fn malformed_arrange_requests_are_refused() {
    for value in [
        json!({"nodes":[node("a",0.0,40.0)],"edges":[]}),
        json!({"nodes":[node("a",100.0,40.0),node("a",100.0,40.0)],"edges":[]}),
        json!({"nodes":[node("a",100.0,40.0)],"edges":[{"from":"a","to":"missing"}]}),
        json!({"nodes":[],"edges":[],"gapX":-1}),
    ] {
        let input: ArrangeInput = serde_json::from_value(value).unwrap();
        assert!(arrange(&input).is_err());
    }
    assert!(
        serde_json::from_value::<ArrangeInput>(json!({"nodes":[],"edges":[],"typo":1})).is_err()
    );
    assert_eq!(run(json!({"nodes":[],"edges":[]})), vec![0.0, 0.0]);
}

#[test]
fn four_times_more_dense_cards_stays_below_quadratic_work() {
    use std::time::{Duration, Instant};
    let input = |count: usize| {
        serde_json::from_value::<ArrangeInput>(json!({"nodes":(0..count).map(|i|node(&format!("n{i:05}"),100.0,40.0)).collect::<Vec<_>>(),"edges":[]})).unwrap()
    };
    let small = input(2000);
    let large = input(8000);
    let timed = |request: &ArrangeInput| {
        let start = Instant::now();
        let result = arrange(request).unwrap();
        assert_eq!(result.len(), 2 + request.nodes.len() * 3);
        start.elapsed()
    };
    let small_time = (0..3).map(|_| timed(&small)).min().unwrap();
    let large_time = (0..3).map(|_| timed(&large)).min().unwrap();
    eprintln!("native dense arrange: 2000={small_time:?},8000={large_time:?}");
    assert!(
        large_time < small_time * 12 + Duration::from_millis(10),
        "dense arrangement grew quadratically: {small_time:?} -> {large_time:?}"
    );
}

#[test]
fn expanded_card_metrics_keep_authored_adjacent_cells_clear() {
    let input:ds_canvas_graph::GraphInput=serde_json::from_value(json!({"nodes":[{"id":"a","column":0,"row":0},{"id":"b","column":0,"row":1},{"id":"c","column":1,"row":0}],"metrics":{"nodeWidth":286,"nodeHeight":124}})).unwrap();
    let scene = ds_canvas_graph::build_scene(&input).unwrap();
    let a = scene.nodes[0].rect;
    let b = scene.nodes[1].rect;
    let c = scene.nodes[2].rect;
    assert!(a.max_y() < b.y);
    assert!(a.max_x() < c.x);
    assert_eq!(a.width, 286.0);
    assert_eq!(a.height, 124.0);
}

#[test]
fn explicit_cell_arrangement_is_stable_atomic_and_retains_hit_identities() {
    use ds_canvas_graph::GeometrySession;
    let input=serde_json::from_value(json!({"nodes":[{"id":"a","column":4,"row":3},{"id":"b","column":2,"row":2}],"edges":[{"id":"a-b","from":{"node":"a"},"to":{"node":"b"}}],"metrics":{"nodeWidth":286,"nodeHeight":124}})).unwrap();
    let mut session = GeometrySession::open(input).unwrap();
    let before = session.scene_index();
    assert!(session.arrange_cells().unwrap());
    assert_eq!(session.revision(), 2);
    assert_eq!(session.scene_index().nodes, before.nodes);
    assert_eq!(session.scene_index().edges, before.edges);
    assert_eq!(session.input().nodes[0].column, 0);
    assert_eq!(session.input().nodes[1].column, 1);
    let rect = session.scene().nodes[1].rect;
    let hit = session
        .hit_test_world(rect.center().x, rect.center().y)
        .unwrap();
    assert!(matches!(hit, ds_canvas_graph::HitTarget::Node { .. }));
    assert!(!session.arrange_cells().unwrap());
    assert_eq!(session.revision(), 2);
    let mut fenced = session.input().clone();
    fenced.metrics.max_column = 0;
    fenced.nodes[1].column = 0;
    let mut fenced = GeometrySession::open(fenced).unwrap();
    let before = fenced.scene_buffer();
    assert!(fenced.arrange_cells().is_err());
    assert_eq!(fenced.scene_buffer(), before);
    assert_eq!(fenced.revision(), 1);
}
