//! Free-card hierarchy composition. The host supplies ordered identities,
//! authored dates/positions and display metrics; no business task facts enter
//! this module. Compose never persists derived expansion displacement.
use crate::arrange::{ArrangeEdge, ArrangeInput, ArrangeNode, arrange};
use crate::curve::Point;
use crate::error::{GeometryError, finite, finite_non_negative, finite_positive};
use crate::input::{MAX_EDGES, MAX_NODES, Orientation};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FreeCardInput {
    pub id: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub day: Option<f64>,
    #[serde(default)]
    pub expanded: bool,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub position: Option<Point>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompositionMetrics {
    pub node_width: f64,
    pub node_height: f64,
    pub expanded_width: f64,
    pub expanded_height: f64,
    pub collapsed_width: f64,
    pub collapsed_height: f64,
    pub grid_step: f64,
    pub organized_left: f64,
    pub organized_top: f64,
    pub organized_row: f64,
    pub container_header: f64,
    pub container_pad: f64,
    pub child_gap_x: f64,
    pub child_gap_y: f64,
    pub day_width: f64,
    pub lane_gap: f64,
    pub column_gap: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposeInput {
    pub nodes: Vec<FreeCardInput>,
    pub edges: Vec<ArrangeEdge>,
    pub metrics: CompositionMetrics,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposedCard {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub depth: usize,
    pub container: bool,
    pub expanded: bool,
    pub collapsed: bool,
    pub child_count: usize,
    pub hidden_ids: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FreeCardBox {
    pub id: String,
    #[serde(default)]
    pub parent: Option<String>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutoArrangeInput {
    pub nodes: Vec<FreeCardInput>,
    pub edges: Vec<ArrangeEdge>,
    pub cards: Vec<FreeCardBox>,
    pub metrics: CompositionMetrics,
}

struct Hierarchy<'a> {
    nodes: &'a [FreeCardInput],
    by_id: BTreeMap<&'a str, usize>,
    parents: Vec<Option<usize>>,
    children: Vec<Vec<usize>>,
    candidates: Vec<usize>,
    earliest: Vec<Option<f64>>,
}
impl<'a> Hierarchy<'a> {
    fn new(
        nodes: &'a [FreeCardInput],
        edges: &[ArrangeEdge],
        metrics: CompositionMetrics,
    ) -> Result<Self, GeometryError> {
        validate(nodes, edges, metrics)?;
        let by_id: BTreeMap<_, _> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        let parents: Vec<_> = nodes
            .iter()
            .map(|node| node.parent.as_deref().and_then(|id| by_id.get(id).copied()))
            .collect();
        let mut children = vec![Vec::new(); nodes.len()];
        let mut candidates = Vec::new();
        for (index, parent) in parents.iter().enumerate() {
            match parent {
                Some(parent) => children[*parent].push(index),
                None => candidates.push(index),
            }
        }
        candidates.extend((0..nodes.len()).filter(|&i| parents[i].is_some()));
        let earliest = earliest_days(nodes, &parents, &children);
        Ok(Self {
            nodes,
            by_id,
            parents,
            children,
            candidates,
            earliest,
        })
    }
    fn ancestor(&self, mut at: usize, within: &BTreeSet<usize>) -> Option<usize> {
        for _ in 0..=self.nodes.len() {
            if within.contains(&at) {
                return Some(at);
            }
            at = self.parents[at]?;
        }
        None
    }
    fn lifted(&self, edges: &[ArrangeEdge], within: &BTreeSet<usize>) -> Vec<ArrangeEdge> {
        let mut seen = BTreeSet::new();
        let mut output = Vec::new();
        for edge in edges {
            let Some(from) = self
                .by_id
                .get(edge.from.as_str())
                .and_then(|&i| self.ancestor(i, within))
            else {
                continue;
            };
            let Some(to) = self
                .by_id
                .get(edge.to.as_str())
                .and_then(|&i| self.ancestor(i, within))
            else {
                continue;
            };
            if from != to && seen.insert((from, to)) {
                output.push(ArrangeEdge {
                    from: self.nodes[from].id.clone(),
                    to: self.nodes[to].id.clone(),
                });
            }
        }
        output
    }
    fn descendants(&self, at: usize) -> Vec<usize> {
        let mut queue: VecDeque<_> = self.children[at].iter().copied().collect();
        let mut seen = BTreeSet::new();
        let mut output = Vec::new();
        while let Some(next) = queue.pop_front() {
            if !seen.insert(next) {
                continue;
            }
            output.push(next);
            queue.extend(self.children[next].iter().copied());
        }
        output
    }
}

fn earliest_days(
    nodes: &[FreeCardInput],
    parents: &[Option<usize>],
    children: &[Vec<usize>],
) -> Vec<Option<f64>> {
    let mut days: Vec<_> = nodes.iter().map(|n| n.day).collect();
    let mut remaining: Vec<_> = children.iter().map(Vec::len).collect();
    let mut queue: VecDeque<_> = remaining
        .iter()
        .enumerate()
        .filter_map(|(i, &count)| if count == 0 { Some(i) } else { None })
        .collect();
    while let Some(at) = queue.pop_front() {
        if let Some(parent) = parents[at] {
            days[parent] = earlier(days[parent], days[at]);
            remaining[parent] -= 1;
            if remaining[parent] == 0 {
                queue.push_back(parent);
            }
        }
    }
    // Remaining nodes are disjoint containment cycles. Fold their already
    // accumulated descendants once, without recursion or repeated ancestor walks.
    let mut seen = vec![false; nodes.len()];
    for start in 0..nodes.len() {
        if remaining[start] == 0 || seen[start] {
            continue;
        }
        let mut members = Vec::new();
        let mut at = start;
        while !seen[at] {
            seen[at] = true;
            members.push(at);
            let Some(parent) = parents[at] else {
                break;
            };
            at = parent;
        }
        let best = members.iter().fold(None, |best, &i| earlier(best, days[i]));
        for member in members {
            days[member] = best;
        }
    }
    days
}
fn earlier(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

#[derive(Clone, Copy, Default)]
struct Size {
    width: f64,
    height: f64,
}
struct Measured {
    sizes: Vec<Size>,
    insets: Vec<BTreeMap<usize, Point>>,
}
fn leaf(node: &FreeCardInput, m: CompositionMetrics, expanded: bool) -> Size {
    if expanded && node.expanded {
        Size {
            width: m.expanded_width,
            height: m.expanded_height,
        }
    } else {
        Size {
            width: m.node_width,
            height: m.node_height,
        }
    }
}
fn measure(
    h: &Hierarchy<'_>,
    edges: &[ArrangeEdge],
    m: CompositionMetrics,
    expanded: bool,
) -> Result<Measured, GeometryError> {
    let mut sizes = vec![Size::default(); h.nodes.len()];
    let mut insets = vec![BTreeMap::new(); h.nodes.len()];
    let mut marks = vec![0u8; h.nodes.len()];
    for &root in &h.candidates {
        if marks[root] == 2 {
            continue;
        }
        marks[root] = 1;
        let mut stack = vec![(root, 0usize)];
        while let Some((at, next_child)) = stack.last_mut() {
            let at = *at;
            let collapsed = !h.children[at].is_empty() && h.nodes[at].collapsed;
            if !collapsed && *next_child < h.children[at].len() {
                let child = h.children[at][*next_child];
                *next_child += 1;
                if marks[child] == 0 {
                    marks[child] = 1;
                    stack.push((child, 0));
                }
                continue;
            }
            let size = if collapsed {
                Size {
                    width: m.collapsed_width,
                    height: m.collapsed_height,
                }
            } else if h.children[at].is_empty() {
                leaf(&h.nodes[at], m, expanded)
            } else {
                let within = h.children[at].iter().copied().collect();
                let request = ArrangeInput {
                    nodes: h.children[at]
                        .iter()
                        .map(|&child| {
                            let size = if marks[child] == 2 {
                                sizes[child]
                            } else {
                                leaf(&h.nodes[child], m, expanded)
                            };
                            ArrangeNode {
                                id: h.nodes[child].id.clone(),
                                width: size.width,
                                height: size.height,
                                minimum_flow: 0.0,
                                day: h.earliest[child],
                            }
                        })
                        .collect(),
                    edges: h.lifted(edges, &within),
                    orientation: Orientation::Horizontal,
                    gap_x: m.child_gap_x,
                    gap_y: m.child_gap_y,
                    origin: h.earliest[at],
                    day_width: m.day_width,
                };
                let laid = arrange(&request)?;
                for (index, &child) in h.children[at].iter().enumerate() {
                    insets[at].insert(child, Point::new(laid[2 + index * 3], laid[3 + index * 3]));
                }
                Size {
                    width: m.node_width.max(laid[0]) + m.container_pad * 2.0,
                    height: m.container_header + laid[1] + m.container_pad,
                }
            };
            finite_positive("composedWidth", size.width)?;
            finite_positive("composedHeight", size.height)?;
            sizes[at] = size;
            marks[at] = 2;
            stack.pop();
        }
    }
    Ok(Measured { sizes, insets })
}

/// The output order remains parent-first, with the host's supplied sibling
/// order. Unknown parents and malformed cycles remain visible fallback roots.
pub fn compose(input: &ComposeInput) -> Result<Vec<ComposedCard>, GeometryError> {
    let h = Hierarchy::new(&input.nodes, &input.edges, input.metrics)?;
    let m = input.metrics;
    let measured = measure(&h, &input.edges, m, true)?;
    let shifts = if input.nodes.iter().any(|node| node.expanded) {
        let rest = measure(&h, &input.edges, m, false)?;
        expansion_shifts(&h, &measured.sizes, &rest.sizes, m)
    } else {
        vec![0.0; input.nodes.len()]
    };
    let mut output = Vec::new();
    let mut placed = vec![false; input.nodes.len()];
    let mut root_y = m.organized_top;
    for &root in &h.candidates {
        if placed[root] {
            continue;
        }
        let position = h.nodes[root].position;
        let x = position.map_or(m.organized_left, |p| p.x);
        let y = position.map_or(root_y, |p| p.y + shifts[root]);
        let mut stack = vec![(root, x, y, 0usize)];
        while let Some((at, x, y, depth)) = stack.pop() {
            if placed[at] {
                continue;
            }
            placed[at] = true;
            let kids = &h.children[at];
            let collapsed = !kids.is_empty() && h.nodes[at].collapsed;
            let hidden = if collapsed {
                h.descendants(at)
            } else {
                Vec::new()
            };
            for &child in &hidden {
                placed[child] = true;
            }
            finite("composedX", x)?;
            finite("composedY", y)?;
            output.push(ComposedCard {
                id: h.nodes[at].id.clone(),
                x,
                y,
                width: measured.sizes[at].width,
                height: measured.sizes[at].height,
                depth,
                container: !kids.is_empty(),
                expanded: kids.is_empty() && h.nodes[at].expanded,
                collapsed,
                child_count: kids.len(),
                hidden_ids: hidden.iter().map(|&i| h.nodes[i].id.clone()).collect(),
            });
            if !collapsed {
                for &child in kids.iter().rev() {
                    let inset = measured.insets[at]
                        .get(&child)
                        .copied()
                        .unwrap_or(Point::new(0.0, 0.0));
                    stack.push((
                        child,
                        x + m.container_pad + inset.x,
                        y + m.container_header + inset.y,
                        depth + 1,
                    ));
                }
            }
        }
        if position.is_none() {
            root_y += measured.sizes[root].height + m.organized_row - m.node_height;
        }
    }
    Ok(output)
}

fn expansion_shifts(
    h: &Hierarchy<'_>,
    now: &[Size],
    rest: &[Size],
    m: CompositionMetrics,
) -> Vec<f64> {
    let mut boxes: Vec<_> = h
        .candidates
        .iter()
        .copied()
        .filter(|&i| h.nodes[i].position.is_some())
        .collect();
    boxes.sort_by(|&a, &b| {
        h.nodes[a]
            .position
            .unwrap()
            .y
            .total_cmp(&h.nodes[b].position.unwrap().y)
    });
    let mut shifts = vec![0.0; h.nodes.len()];
    if boxes.len() < 2 {
        return shifts;
    }
    let grew = |i: usize| now[i].height > rest[i].height || now[i].width > rest[i].width;
    let overlap = |i: usize, j: usize, baseline: bool, shifts: &[f64]| {
        let a = h.nodes[i].position.unwrap();
        let b = h.nodes[j].position.unwrap();
        let (size_a, size_b, dy_a, dy_b) = if baseline {
            (rest[i], rest[j], 0.0, 0.0)
        } else {
            (now[i], now[j], shifts[i], shifts[j])
        };
        a.x < b.x + size_b.width
            && b.x < a.x + size_a.width
            && a.y + dy_a < b.y + dy_b + size_b.height
            && b.y + dy_b < a.y + dy_a + size_a.height
    };
    // Preserve the existing expansion-only settlement and its pass fence.
    // Baseline overlap is authored and never grants displacement.
    for _ in 0..12 {
        let mut moved = false;
        for (i, &mover) in boxes.iter().enumerate() {
            if !grew(mover) && shifts[mover] == 0.0 {
                continue;
            }
            for (j, &other) in boxes.iter().enumerate() {
                if i == j
                    || overlap(mover, other, true, &shifts)
                    || !overlap(mover, other, false, &shifts)
                {
                    continue;
                }
                if (grew(other) && !grew(mover)) || (grew(other) == grew(mover) && j < i) {
                    continue;
                }
                let a = h.nodes[mover].position.unwrap();
                let b = h.nodes[other].position.unwrap();
                let gap = if b.y >= a.y {
                    m.grid_step.max(b.y - (a.y + rest[mover].height))
                } else {
                    m.grid_step
                };
                let target = a.y + shifts[mover] + now[mover].height + gap - b.y;
                if target > shifts[other] {
                    shifts[other] = target;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    shifts
}

/// Explicit Arrange returns root placements only. The undated parking lane
/// retains its current vertical order and never acquires an invented date.
pub fn auto_arrange(input: &AutoArrangeInput) -> Result<BTreeMap<String, Point>, GeometryError> {
    let h = Hierarchy::new(&input.nodes, &input.edges, input.metrics)?;
    let m = input.metrics;
    if input.cards.len() > MAX_NODES {
        return Err(GeometryError::InputTooLarge {
            field: "cards".into(),
            count: input.cards.len(),
            max: MAX_NODES,
        });
    }
    let mut seen = BTreeSet::new();
    let mut roots = Vec::new();
    for card in &input.cards {
        if !seen.insert(card.id.as_str()) {
            return Err(GeometryError::DuplicateNode {
                id: card.id.clone(),
            });
        }
        let &index = h
            .by_id
            .get(card.id.as_str())
            .ok_or_else(|| GeometryError::UnknownNode {
                reference: "arrange.card".into(),
                node: card.id.clone(),
            })?;
        finite("cardX", card.x)?;
        finite("cardY", card.y)?;
        finite_positive("cardWidth", card.width)?;
        finite_positive("cardHeight", card.height)?;
        if card.parent.is_none() {
            roots.push((index, card));
        }
    }
    let dated: Vec<_> = roots
        .iter()
        .copied()
        .filter(|(i, _)| h.earliest[*i].is_some())
        .collect();
    let mut undated: Vec<_> = roots
        .iter()
        .copied()
        .filter(|(i, _)| h.earliest[*i].is_none())
        .collect();
    undated.sort_by(|(_, a), (_, b)| a.y.total_cmp(&b.y));
    let width = if undated.is_empty() {
        0.0
    } else {
        undated
            .iter()
            .map(|(_, card)| card.width)
            .fold(0.0, f64::max)
            + m.column_gap
    };
    let mut output = BTreeMap::new();
    for (index, (_, card)) in undated.iter().enumerate() {
        output.insert(
            card.id.clone(),
            Point::new(
                m.organized_left - width,
                m.organized_top + index as f64 * (card.height + m.lane_gap),
            ),
        );
    }
    let root_ids = roots.iter().map(|(i, _)| *i).collect();
    let dated_ids: BTreeSet<_> = dated
        .iter()
        .map(|(i, _)| input.nodes[*i].id.as_str())
        .collect();
    let request = ArrangeInput {
        nodes: dated
            .iter()
            .map(|(i, card)| ArrangeNode {
                id: card.id.clone(),
                width: card.width,
                height: card.height,
                minimum_flow: 0.0,
                day: h.earliest[*i],
            })
            .collect(),
        edges: h
            .lifted(&input.edges, &root_ids)
            .into_iter()
            .filter(|edge| {
                dated_ids.contains(edge.from.as_str()) && dated_ids.contains(edge.to.as_str())
            })
            .collect(),
        orientation: Orientation::Horizontal,
        gap_x: m.column_gap,
        gap_y: m.lane_gap,
        origin: dated
            .iter()
            .filter_map(|(i, _)| h.earliest[*i])
            .min_by(f64::total_cmp),
        day_width: m.day_width,
    };
    let arranged = arrange(&request)?;
    for (index, (_, card)) in dated.iter().enumerate() {
        output.insert(
            card.id.clone(),
            Point::new(
                m.organized_left + arranged[2 + index * 3],
                m.organized_top + arranged[3 + index * 3],
            ),
        );
    }
    for point in output.values() {
        finite("arrangedRootX", point.x)?;
        finite("arrangedRootY", point.y)?;
    }
    Ok(output)
}

fn validate(
    nodes: &[FreeCardInput],
    edges: &[ArrangeEdge],
    m: CompositionMetrics,
) -> Result<(), GeometryError> {
    for (field, count, max) in [
        ("nodes", nodes.len(), MAX_NODES),
        ("edges", edges.len(), MAX_EDGES),
    ] {
        if count > max {
            return Err(GeometryError::InputTooLarge {
                field: field.into(),
                count,
                max,
            });
        }
    }
    for (field, value) in [
        ("nodeWidth", m.node_width),
        ("nodeHeight", m.node_height),
        ("expandedWidth", m.expanded_width),
        ("expandedHeight", m.expanded_height),
        ("collapsedWidth", m.collapsed_width),
        ("collapsedHeight", m.collapsed_height),
        ("gridStep", m.grid_step),
        ("organizedRow", m.organized_row),
    ] {
        finite_positive(field, value)?;
    }
    for (field, value) in [
        ("containerHeader", m.container_header),
        ("containerPad", m.container_pad),
        ("childGapX", m.child_gap_x),
        ("childGapY", m.child_gap_y),
        ("dayWidth", m.day_width),
        ("laneGap", m.lane_gap),
        ("columnGap", m.column_gap),
    ] {
        finite_non_negative(field, value)?;
    }
    finite("organizedLeft", m.organized_left)?;
    finite("organizedTop", m.organized_top)?;
    let mut seen = BTreeSet::new();
    for node in nodes {
        if node.id.is_empty() {
            return Err(GeometryError::EmptyIdentifier {
                field: "node.id".into(),
            });
        }
        if !seen.insert(node.id.as_str()) {
            return Err(GeometryError::DuplicateNode {
                id: node.id.clone(),
            });
        }
        if let Some(day) = node.day {
            finite("day", day)?;
        }
        if let Some(point) = node.position {
            finite("positionX", point.x)?;
            finite("positionY", point.y)?;
        }
    }
    Ok(())
}
