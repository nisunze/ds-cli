use ds_canvas_graph::arrange::ArrangeEdge;
use ds_canvas_graph::composition::{
    AutoArrangeInput, ComposeInput, ComposedCard, CompositionMetrics, FreeCardBox, FreeCardInput,
    auto_arrange, compose,
};
use ds_canvas_graph::curve::Point;
use ds_canvas_graph::error::GeometryError;

fn metrics() -> CompositionMetrics {
    CompositionMetrics {
        node_width: 192.0,
        node_height: 68.0,
        expanded_width: 286.0,
        expanded_height: 188.0,
        collapsed_width: 228.0,
        collapsed_height: 76.0,
        grid_step: 24.0,
        organized_left: 32.0,
        organized_top: 36.0,
        organized_row: 88.0,
        container_header: 40.0,
        container_pad: 14.0,
        child_gap_x: 40.0,
        child_gap_y: 26.0,
        day_width: 7.0,
        lane_gap: 44.0,
        column_gap: 64.0,
    }
}
fn node(id: &str, parent: Option<&str>) -> FreeCardInput {
    FreeCardInput {
        id: id.into(),
        parent: parent.map(str::to_owned),
        day: None,
        expanded: false,
        collapsed: false,
        position: None,
    }
}
fn request(nodes: Vec<FreeCardInput>) -> ComposeInput {
    ComposeInput {
        nodes,
        edges: vec![],
        metrics: metrics(),
    }
}
fn at<'a>(cards: &'a [ComposedCard], id: &str) -> &'a ComposedCard {
    cards.iter().find(|card| card.id == id).unwrap()
}
fn edge(from: &str, to: &str) -> ArrangeEdge {
    ArrangeEdge {
        from: from.into(),
        to: to.into(),
    }
}
fn auto(input: &ComposeInput, cards: &[ComposedCard]) -> AutoArrangeInput {
    AutoArrangeInput {
        nodes: input.nodes.clone(),
        edges: input.edges.clone(),
        metrics: input.metrics,
        cards: cards
            .iter()
            .map(|card| FreeCardBox {
                id: card.id.clone(),
                parent: input
                    .nodes
                    .iter()
                    .find(|n| n.id == card.id)
                    .unwrap()
                    .parent
                    .clone(),
                x: card.x,
                y: card.y,
                width: card.width,
                height: card.height,
            })
            .collect(),
    }
}
fn overlaps(a: &ComposedCard, b: &ComposedCard) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

#[test]
fn ordered_roots_and_children_keep_exact_container_metrics() {
    let input = request(vec![
        node("z-parent", None),
        node("z-child", Some("z-parent")),
        node("a-child", Some("z-parent")),
        node("a-root", None),
    ]);
    let cards = compose(&input).unwrap();
    assert_eq!(
        cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["z-parent", "z-child", "a-child", "a-root"]
    );
    assert_eq!(
        (cards[0].x, cards[0].y, cards[0].width, cards[0].height),
        (32.0, 36.0, 220.0, 216.0)
    );
    assert_eq!(
        (at(&cards, "a-child").x, at(&cards, "a-child").y),
        (46.0, 76.0)
    );
    assert_eq!(
        (at(&cards, "z-child").x, at(&cards, "z-child").y),
        (46.0, 170.0)
    );
    assert_eq!(at(&cards, "a-root").y, 272.0);
    assert!(cards[0].container);
    assert_eq!(cards[0].child_count, 2);
}

#[test]
fn dates_and_lifted_descendant_dependencies_flow_through_nested_containers() {
    let mut input = request(vec![
        node("root", None),
        node("A", Some("root")),
        node("a", Some("A")),
        node("B", Some("root")),
        node("b", Some("B")),
    ]);
    input.nodes[0].day = Some(200.0);
    input.nodes[2].day = Some(0.0);
    input.nodes[4].day = Some(40.0);
    input.edges = vec![edge("a", "b")];
    let cards = compose(&input).unwrap();
    assert_eq!(at(&cards, "A").x, 46.0);
    assert_eq!(at(&cards, "B").x, 326.0);
    assert_eq!(at(&cards, "A").y, at(&cards, "B").y);
    let root = at(&cards, "root");
    let inner = at(&cards, "b");
    assert!(inner.x + inner.width <= root.x + root.width);
    assert!(inner.y + inner.height <= root.y + root.height);
    assert_eq!(inner.depth, 2);
}

#[test]
fn collapsed_group_reports_breadth_first_hidden_identities() {
    let mut input = request(vec![
        node("group", None),
        node("a", Some("group")),
        node("aa", Some("a")),
        node("b", Some("group")),
        node("outside", None),
    ]);
    input.nodes[0].collapsed = true;
    let cards = compose(&input).unwrap();
    assert_eq!(
        cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["group", "outside"]
    );
    let group = &cards[0];
    assert_eq!((group.width, group.height), (228.0, 76.0));
    assert!(group.container && group.collapsed && !group.expanded);
    assert_eq!(group.child_count, 2);
    assert_eq!(group.hidden_ids, ["a", "b", "aa"]);
}

#[test]
fn child_authored_position_is_ignored_and_root_authored_position_is_preserved() {
    let mut input = request(vec![node("root", None), node("child", Some("root"))]);
    input.nodes[0].position = Some(Point::new(-400.0, 220.0));
    input.nodes[1].position = Some(Point::new(9000.0, 9000.0));
    let cards = compose(&input).unwrap();
    assert_eq!((cards[0].x, cards[0].y), (-400.0, 220.0));
    assert_eq!((cards[1].x, cards[1].y), (-386.0, 260.0));
}

#[test]
fn expansion_displacement_cascades_and_collapsing_restores_authored_positions() {
    let mut input = request(vec![
        node("a", None),
        node("b", None),
        node("c", None),
        node("side", None),
    ]);
    for (node, point) in
        input
            .nodes
            .iter_mut()
            .zip([(24.0, 24.0), (24.0, 120.0), (24.0, 216.0), (480.0, 120.0)])
    {
        node.position = Some(Point::new(point.0, point.1));
    }
    input.nodes[0].expanded = true;
    let cards = compose(&input).unwrap();
    assert_eq!((at(&cards, "b").y, at(&cards, "c").y), (240.0, 336.0));
    assert_eq!(at(&cards, "side").y, 120.0);
    assert!(!overlaps(at(&cards, "a"), at(&cards, "b")));
    assert!(!overlaps(at(&cards, "b"), at(&cards, "c")));
    input.nodes[0].expanded = false;
    let collapsed = compose(&input).unwrap();
    assert_eq!(
        (at(&collapsed, "b").y, at(&collapsed, "c").y),
        (120.0, 216.0)
    );
    assert_eq!(input.nodes[1].position, Some(Point::new(24.0, 120.0)));
}

#[test]
fn width_growth_moves_new_collision_but_keeps_authored_overlaps() {
    let mut input = request(vec![node("a", None), node("b", None), node("side", None)]);
    input.nodes[0].position = Some(Point::new(24.0, 24.0));
    input.nodes[0].expanded = true;
    input.nodes[1].position = Some(Point::new(24.0, 40.0));
    input.nodes[2].position = Some(Point::new(224.0, 24.0));
    let cards = compose(&input).unwrap();
    assert_eq!(at(&cards, "b").y, 40.0);
    assert!(!overlaps(at(&cards, "a"), at(&cards, "side")));
}

#[test]
fn malformed_cycles_orphans_and_unknown_edges_remain_drawable() {
    let mut input = request(vec![
        node("a", Some("b")),
        node("b", Some("a")),
        node("orphan", Some("missing")),
    ]);
    input.nodes[0].day = Some(2.0);
    input.edges = vec![edge("unknown", "a"), edge("a", "b"), edge("b", "a")];
    let cards = compose(&input).unwrap();
    assert_eq!(cards.len(), 3);
    assert_eq!(cards[0].id, "orphan");
    assert_eq!(
        cards
            .iter()
            .map(|card| &card.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    let arranged = auto_arrange(&auto(&input, &cards)).unwrap();
    assert!(arranged.is_empty()); // Raw orphan parent and cycle parents are authored nonroots.
}

#[test]
fn deep_hierarchy_composes_without_using_call_stack() {
    let input = request(
        (0..1200)
            .map(|i| node(&i.to_string(), None))
            .enumerate()
            .map(|(i, mut n)| {
                n.parent = (i > 0).then(|| (i - 1).to_string());
                n
            })
            .collect(),
    );
    let cards = compose(&input).unwrap();
    assert_eq!(cards.len(), 1200);
    assert_eq!(cards.last().unwrap().depth, 1199);
    let outer = &cards[0];
    let inner = cards.last().unwrap();
    assert!(inner.x + inner.width <= outer.x + outer.width);
    assert!(inner.y + inner.height <= outer.y + outer.height);
}

#[test]
fn arrange_parks_undated_roots_by_current_y_and_keeps_calendar_scale() {
    let mut input = request(vec![
        node("late", None),
        node("early", None),
        node("draft-b", None),
        node("draft-a", None),
    ]);
    input.nodes[0].day = Some(100.0);
    input.nodes[1].day = Some(0.0);
    input.nodes[2].position = Some(Point::new(2.0, 50.0));
    input.nodes[3].position = Some(Point::new(2.0, -50.0));
    let cards = compose(&input).unwrap();
    let placed = auto_arrange(&auto(&input, &cards)).unwrap();
    assert_eq!(placed["early"], Point::new(32.0, 36.0));
    assert_eq!(placed["late"], Point::new(732.0, 36.0));
    assert_eq!(placed["draft-a"], Point::new(-224.0, 36.0));
    assert_eq!(placed["draft-b"], Point::new(-224.0, 148.0));
}

#[test]
fn arrange_uses_earliest_descendants_and_lifts_dependencies_to_roots() {
    let mut input = request(vec![
        node("A", None),
        node("a", Some("A")),
        node("B", None),
        node("b", Some("B")),
    ]);
    input.nodes[0].day = Some(200.0);
    input.nodes[1].day = Some(0.0);
    input.nodes[3].day = Some(0.0);
    input.edges = vec![edge("a", "b")];
    let cards = compose(&input).unwrap();
    let placed = auto_arrange(&auto(&input, &cards)).unwrap();
    assert_eq!(placed.len(), 2);
    assert_eq!(placed["A"], Point::new(32.0, 36.0));
    assert_eq!(placed["B"], Point::new(316.0, 36.0));
}

#[test]
fn native_refusals_cover_identity_metrics_coordinates_and_unknown_visible_cards() {
    let mut input = request(vec![node("a", None), node("a", None)]);
    assert!(matches!(
        compose(&input),
        Err(GeometryError::DuplicateNode { .. })
    ));
    input.nodes.pop();
    input.nodes[0].id.clear();
    assert!(matches!(
        compose(&input),
        Err(GeometryError::EmptyIdentifier { .. })
    ));
    input.nodes[0].id = "a".into();
    input.nodes[0].day = Some(f64::NAN);
    assert!(matches!(
        compose(&input),
        Err(GeometryError::NonFiniteInput { .. })
    ));
    input.nodes[0].day = None;
    input.nodes[0].position = Some(Point::new(f64::INFINITY, 0.0));
    assert!(matches!(
        compose(&input),
        Err(GeometryError::NonFiniteInput { .. })
    ));
    input.nodes[0].position = None;
    input.metrics.container_pad = -1.0;
    assert!(matches!(
        compose(&input),
        Err(GeometryError::InvalidMetric { .. })
    ));
    input.metrics = metrics();
    let cards = compose(&input).unwrap();
    let mut arrange = auto(&input, &cards);
    arrange.cards[0].id = "missing".into();
    assert!(matches!(
        auto_arrange(&arrange),
        Err(GeometryError::UnknownNode { .. })
    ));
    arrange.cards[0].id = "a".into();
    arrange.cards[0].width = 0.0;
    assert!(matches!(
        auto_arrange(&arrange),
        Err(GeometryError::InvalidMetric { .. })
    ));
    arrange.cards[0].width = 192.0;
    arrange.cards.push(arrange.cards[0].clone());
    assert!(matches!(
        auto_arrange(&arrange),
        Err(GeometryError::DuplicateNode { .. })
    ));
}

#[test]
fn compose_contract_refuses_unknown_fields_and_has_no_domain_facts() {
    let input = request(vec![node("a", None)]);
    let mut json = serde_json::to_value(&input).unwrap();
    json["nodes"][0]["title"] = serde_json::json!("domain fact");
    assert!(serde_json::from_value::<ComposeInput>(json).is_err());
    let output = serde_json::to_value(compose(&input).unwrap()).unwrap();
    assert!(output[0].get("hiddenIds").is_some());
    assert!(output[0].get("childCount").is_some());
    assert!(output[0].get("title").is_none());
}

#[test]
fn empty_self_contained_and_oversized_inputs_have_bounded_results() {
    let empty = request(vec![]);
    assert!(compose(&empty).unwrap().is_empty());
    assert!(auto_arrange(&auto(&empty, &[])).unwrap().is_empty());
    let input = request(vec![node("self", Some("self"))]);
    assert_eq!(compose(&input).unwrap().len(), 1);
    let oversized = request(vec![node("a", None); ds_canvas_graph::input::MAX_NODES + 1]);
    assert!(matches!(
        compose(&oversized),
        Err(GeometryError::InputTooLarge { .. })
    ));
    let mut oversized_edges = request(vec![]);
    oversized_edges.edges = vec![edge("a", "b"); ds_canvas_graph::input::MAX_EDGES + 1];
    assert!(matches!(
        compose(&oversized_edges),
        Err(GeometryError::InputTooLarge { .. })
    ));
}
