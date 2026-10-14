//! Stable, explicit graph arrangement shared by every graph canvas.
//! Calendar and containment adapters supply minimum flow coordinates; this
//! owner never schedules work or decides which domain edges are permitted.
use crate::error::{GeometryError, finite_non_negative, finite_positive};
use crate::input::{MAX_EDGES, MAX_NODES, Orientation};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArrangeNode {
    pub id: String,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub minimum_flow: f64,
    #[serde(default)]
    pub day: Option<f64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArrangeEdge {
    pub from: String,
    pub to: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArrangeInput {
    pub nodes: Vec<ArrangeNode>,
    pub edges: Vec<ArrangeEdge>,
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default = "gap_x")]
    pub gap_x: f64,
    #[serde(default = "gap_y")]
    pub gap_y: f64,
    #[serde(default)]
    pub origin: Option<f64>,
    #[serde(default)]
    pub day_width: f64,
}
fn gap_x() -> f64 {
    64.0
}
fn gap_y() -> f64 {
    28.0
}

/// `[width,height, x,y,rank ...]` in the supplied node order. Results depend on
/// stable identities and authored facts, never array order or viewport size.
pub fn arrange(input: &ArrangeInput) -> Result<Vec<f64>, GeometryError> {
    for (field, count, max) in [
        ("nodes", input.nodes.len(), MAX_NODES),
        ("edges", input.edges.len(), MAX_EDGES),
    ] {
        if count > max {
            return Err(GeometryError::InputTooLarge {
                field: field.into(),
                count,
                max,
            });
        }
    }
    finite_non_negative("gapX", input.gap_x)?;
    finite_non_negative("gapY", input.gap_y)?;
    finite_non_negative("dayWidth", input.day_width)?;
    if let Some(origin) = input.origin {
        crate::error::finite("origin", origin)?;
    }
    let mut ids = BTreeMap::new();
    for (index, node) in input.nodes.iter().enumerate() {
        if node.id.is_empty() {
            return Err(GeometryError::EmptyIdentifier {
                field: "node.id".into(),
            });
        }
        if ids.insert(node.id.as_str(), index).is_some() {
            return Err(GeometryError::DuplicateNode {
                id: node.id.clone(),
            });
        }
        finite_positive("width", node.width)?;
        finite_positive("height", node.height)?;
        finite_non_negative("minimumFlow", node.minimum_flow)?;
        if let Some(day) = node.day {
            crate::error::finite("day", day)?;
        }
    }
    let n = input.nodes.len();
    let mut outgoing = vec![BTreeSet::new(); n];
    for edge in &input.edges {
        let from = *ids
            .get(edge.from.as_str())
            .ok_or_else(|| GeometryError::UnknownNode {
                reference: "arrange.from".into(),
                node: edge.from.clone(),
            })?;
        let to = *ids
            .get(edge.to.as_str())
            .ok_or_else(|| GeometryError::UnknownNode {
                reference: "arrange.to".into(),
                node: edge.to.clone(),
            })?;
        if from != to {
            outgoing[from].insert((input.nodes[to].id.as_str(), to));
        }
    }
    // Iterative DFS drops only back edges for malformed display graphs. Native
    // workflow/domain validation independently refuses illegal authored cycles.
    let mut marks = vec![0u8; n];
    let mut active = vec![Vec::new(); n];
    let mut post = Vec::with_capacity(n);
    for &root in ids.values() {
        if marks[root] != 0 {
            continue;
        }
        marks[root] = 1;
        let mut stack = vec![(root, outgoing[root].iter())];
        while let Some((at, children)) = stack.last_mut() {
            if let Some((_, next)) = children.next() {
                if marks[*next] == 1 {
                    continue;
                }
                active[*at].push(*next);
                if marks[*next] == 0 {
                    marks[*next] = 1;
                    stack.push((*next, outgoing[*next].iter()));
                }
            } else {
                let at = *at;
                marks[at] = 2;
                post.push(at);
                stack.pop();
            }
        }
    }
    post.reverse();
    let flow = |i: usize| match input.orientation {
        Orientation::Horizontal => input.nodes[i].width,
        Orientation::Vertical => input.nodes[i].height,
    };
    let cross = |i: usize| match input.orientation {
        Orientation::Horizontal => input.nodes[i].height,
        Orientation::Vertical => input.nodes[i].width,
    };
    let mut predecessors = vec![Vec::new(); n];
    let mut ranks = vec![0usize; n];
    let mut xs: Vec<f64> = input
        .nodes
        .iter()
        .map(|node| match (input.origin, node.day) {
            (Some(origin), Some(day)) => node
                .minimum_flow
                .max(((day - origin) * input.day_width).max(0.0)),
            _ => node.minimum_flow,
        })
        .collect();
    for &at in &post {
        for &next in &active[at] {
            predecessors[next].push(at);
            ranks[next] = ranks[next].max(ranks[at] + 1);
            xs[next] = xs[next].max(xs[at] + flow(at) + input.gap_x);
        }
    }
    let mut coordinates: Vec<_> = (0..n)
        .flat_map(|i| [xs[i], xs[i] + flow(i) + input.gap_x])
        .map(Ordered)
        .collect();
    if coordinates.iter().any(|v| !v.0.is_finite()) {
        return Err(GeometryError::non_finite("arrangedFlow", f64::INFINITY));
    }
    coordinates.sort();
    coordinates.dedup();
    let mut occupancy = Occupancy::new(coordinates.len().saturating_sub(1));
    let mut ys = vec![0.0; n];
    let mut bands = BTreeMap::<usize, Vec<usize>>::new();
    for (index, rank) in ranks.iter().enumerate() {
        bands.entry(*rank).or_default().push(index);
    }
    for band in bands.values_mut() {
        let wanted = |at: usize, ys: &[f64]| {
            let mut centres: Vec<_> = predecessors[at]
                .iter()
                .map(|&p| ys[p] + cross(p) / 2.0)
                .collect();
            centres.sort_by(f64::total_cmp);
            if centres.is_empty() {
                return 0.0;
            }
            let mid = centres.len() / 2;
            let centre = if centres.len().is_multiple_of(2) {
                (centres[mid - 1] + centres[mid]) / 2.0
            } else {
                centres[mid]
            };
            (centre - cross(at) / 2.0).max(0.0)
        };
        band.sort_by(|&a, &b| {
            wanted(a, &ys)
                .total_cmp(&wanted(b, &ys))
                .then_with(|| xs[a].total_cmp(&xs[b]))
                .then_with(|| input.nodes[a].id.cmp(&input.nodes[b].id))
        });
        for &at in band.iter() {
            let start = coordinates.binary_search(&Ordered(xs[at])).unwrap();
            let end = coordinates
                .binary_search(&Ordered(xs[at] + flow(at) + input.gap_x))
                .unwrap();
            let height = cross(at) + input.gap_y;
            let preferred = wanted(at, &ys);
            let below = occupancy
                .free(start, end, preferred, height, false)
                .expect("downward compaction has no lower fence");
            let above = occupancy.free(start, end, preferred, height, true);
            let y = above
                .filter(|y| preferred - *y <= below - preferred)
                .unwrap_or(below);
            finite_non_negative("arrangedCross", y)?;
            ys[at] = y;
            occupancy.insert(start, end, y, y + height);
        }
    }
    let mut width: f64 = 0.0;
    let mut height: f64 = 0.0;
    let mut output = vec![0.0, 0.0];
    for at in 0..n {
        let (x, y) = match input.orientation {
            Orientation::Horizontal => (xs[at], ys[at]),
            Orientation::Vertical => (ys[at], xs[at]),
        };
        width = width.max(x + input.nodes[at].width);
        height = height.max(y + input.nodes[at].height);
        output.extend_from_slice(&[x, y, ranks[at] as f64]);
    }
    output[0] = width;
    output[1] = height;
    if output.iter().any(|value| !value.is_finite()) {
        return Err(GeometryError::non_finite("arrangement", f64::INFINITY));
    }
    Ok(output)
}

#[derive(Clone, Copy, Debug)]
struct Ordered(f64);
impl PartialEq for Ordered {
    fn eq(&self, rhs: &Self) -> bool {
        self.cmp(rhs).is_eq()
    }
}
impl Eq for Ordered {}
impl PartialOrd for Ordered {
    fn partial_cmp(&self, rhs: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(rhs))
    }
}
impl Ord for Ordered {
    fn cmp(&self, rhs: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&rhs.0)
    }
}
#[derive(Default)]
struct Intervals(BTreeMap<Ordered, f64>);
impl Intervals {
    fn insert(&mut self, mut start: f64, mut end: f64) {
        if let Some((&a, &b)) = self.0.range(..=Ordered(start)).next_back()
            && b >= start
        {
            start = a.0;
            end = end.max(b);
            self.0.remove(&a);
        }
        loop {
            let next = self
                .0
                .range(Ordered(start)..=Ordered(end))
                .next()
                .map(|(&a, &b)| (a, b));
            if let Some((a, b)) = next {
                end = end.max(b);
                self.0.remove(&a);
            } else {
                break;
            }
        }
        self.0.insert(Ordered(start), end);
    }
    fn collision(&self, start: f64, end: f64) -> Option<(f64, f64)> {
        self.0
            .range(..Ordered(end))
            .next_back()
            .and_then(|(a, &b)| if b > start { Some((a.0, b)) } else { None })
    }
}
#[derive(Default)]
struct Slot {
    own: Intervals,
    all: Intervals,
}
/// Compressed x-interval tree with merged vertical occupancy. A dense source
/// column is one merged interval, rather than a scan over every earlier card.
struct Occupancy {
    size: usize,
    slots: Vec<Slot>,
}
impl Occupancy {
    fn new(size: usize) -> Self {
        Self {
            size,
            slots: (0..size.max(1) * 4).map(|_| Slot::default()).collect(),
        }
    }
    fn insert(&mut self, left: usize, right: usize, top: f64, bottom: f64) {
        if left < right {
            self.insert_at(1, 0, self.size, left, right, top, bottom);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn insert_at(
        &mut self,
        at: usize,
        low: usize,
        high: usize,
        left: usize,
        right: usize,
        top: f64,
        bottom: f64,
    ) {
        if right <= low || left >= high {
            return;
        }
        self.slots[at].all.insert(top, bottom);
        if left <= low && high <= right {
            self.slots[at].own.insert(top, bottom);
            return;
        }
        let mid = (low + high) / 2;
        self.insert_at(at * 2, low, mid, left, right, top, bottom);
        self.insert_at(at * 2 + 1, mid, high, left, right, top, bottom);
    }
    #[allow(clippy::too_many_arguments)]
    fn collision(
        &self,
        at: usize,
        low: usize,
        high: usize,
        left: usize,
        right: usize,
        top: f64,
        bottom: f64,
    ) -> Option<(f64, f64)> {
        if right <= low || left >= high {
            return None;
        }
        if left <= low && high <= right {
            return self.slots[at].all.collision(top, bottom);
        }
        let mut hit = self.slots[at].own.collision(top, bottom);
        let mid = (low + high) / 2;
        for (a, b) in [
            self.collision(at * 2, low, mid, left, right, top, bottom),
            self.collision(at * 2 + 1, mid, high, left, right, top, bottom),
        ]
        .into_iter()
        .flatten()
        {
            hit = Some(match hit {
                Some((c, d)) => (a.min(c), b.max(d)),
                None => (a, b),
            });
        }
        hit
    }
    fn free(
        &self,
        left: usize,
        right: usize,
        preferred: f64,
        height: f64,
        up: bool,
    ) -> Option<f64> {
        let mut y = preferred;
        loop {
            match self.collision(1, 0, self.size, left, right, y, y + height) {
                None => return Some(y),
                Some((top, bottom)) => {
                    y = if up { top - height } else { bottom };
                    if y < 0.0 {
                        return None;
                    }
                }
            }
        }
    }
}
