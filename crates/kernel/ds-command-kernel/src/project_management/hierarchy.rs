//! Where a task sits in the plan — the WBS label every attention row carries.
//!
//! A port of the two passes the browser runs to answer that: `siblingOrder` +
//! `canonicalGraphToTimelineGraph` (`gantt-adapter.ts:118-134`) and
//! `buildTaskHierarchy` + `projectRows` (`gantt/hierarchy.ts:121-205`), reached
//! from `projectWorkTableRows` (`views.ts:190-201`).
//!
//! Stable task ids never change; the WBS number is a DISPLAY LABEL recomputed
//! from position — never identity, because it renumbers on reorder. A malformed
//! parent chain (missing or cyclic) is repaired deterministically rather than
//! throwing: the timeline must still draw.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::design_dashboard::locale_compare;

use super::wire::{CanonicalProjectTask, TaskPlacement};

/// One node as the hierarchy pass needs it: identity, declared parent, the
/// sibling rank `siblingOrder` assigned, and which root pile it belongs to.
struct Node<'a> {
    id: &'a str,
    parent_id: Option<&'a str>,
    order: f64,
    placement: TaskPlacement,
}

/// `localeCompare(other, undefined, { numeric: true })`.
///
/// The numeric-aware sibling of `design_dashboard::locale_compare`, which is
/// the crate's existing answer to plain `localeCompare`. Numeric collation
/// reads a maximal run of digits as ONE element worth the number it spells, so
/// `item2` sorts before `item10` the way the browser orders a sort key an
/// operator typed by hand; everything else is collated character by character.
fn natural_compare(left: &str, right: &str) -> Ordering {
    let mut left_chunks = chunks(left).into_iter();
    let mut right_chunks = chunks(right).into_iter();
    loop {
        let ordering = match (left_chunks.next(), right_chunks.next()) {
            (None, None) => return Ordering::Equal,
            // A string that ran out is the shorter one, and shorter sorts first.
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(Chunk::Number(left_run)), Some(Chunk::Number(right_run))) => {
                let (left_value, right_value) = (
                    left_run.trim_start_matches('0'),
                    right_run.trim_start_matches('0'),
                );
                left_value
                    .len()
                    .cmp(&right_value.len())
                    .then_with(|| left_value.cmp(right_value))
            }
            // A number against a letter is still two characters to collate,
            // and the collation already ranks digits below letters.
            (Some(left_chunk), Some(right_chunk)) => {
                locale_compare(&left_chunk.head(), &right_chunk.head())
            }
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
}

/// One collation element: a whole digit run, or a single character.
enum Chunk<'a> {
    Number(&'a str),
    Character(char),
}

impl Chunk<'_> {
    /// The character this element collates as when it meets an element of the
    /// other kind — the first digit of a run, or the character itself.
    fn head(&self) -> String {
        match self {
            Chunk::Number(run) => run.chars().next().unwrap_or('0').to_string(),
            Chunk::Character(character) => character.to_string(),
        }
    }
}

fn chunks(text: &str) -> Vec<Chunk<'_>> {
    let mut chunks = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let rest = &text[cursor..];
        let digits = rest
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits > 0 {
            chunks.push(Chunk::Number(&rest[..digits]));
            cursor += digits;
            continue;
        }
        let character = rest.chars().next().unwrap_or('\0');
        chunks.push(Chunk::Character(character));
        cursor += character.len_utf8();
    }
    chunks
}

/// `siblingOrder` (gantt-adapter.ts:118-134): position among the tasks that
/// declare the same parent, by sort key then id. Tasks whose parent is absent
/// share the one nameless pile, exactly as `task.parentTaskId ?? ''` does.
fn sibling_order(tasks: &[CanonicalProjectTask]) -> BTreeMap<&str, usize> {
    let mut by_parent: BTreeMap<&str, Vec<&CanonicalProjectTask>> = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    for task in tasks {
        if !seen.insert(task.id.as_str()) {
            continue;
        }
        by_parent
            .entry(task.parent_task_id.as_deref().unwrap_or(""))
            .or_default()
            .push(task);
    }
    let mut order = BTreeMap::new();
    for siblings in by_parent.values_mut() {
        siblings.sort_by(|left, right| {
            natural_compare(&left.sort_key, &right.sort_key)
                .then_with(|| locale_compare(&left.id, &right.id))
        });
        for (index, task) in siblings.iter().enumerate() {
            order.insert(task.id.as_str(), index);
        }
    }
    order
}

/// `compareSiblings` (gantt/hierarchy.ts:57-59). The id tiebreak is a PLAIN
/// string comparison, not a collation — the browser writes `left.id < right.id`
/// there while it collates titles elsewhere.
fn compare_siblings(left: &Node<'_>, right: &Node<'_>) -> Ordering {
    left.order
        .total_cmp(&right.order)
        .then_with(|| left.id.cmp(right.id))
}

/// `compareRoots` (gantt/hierarchy.ts:61-65): the inbox pile sits above the
/// WBS, so unplaced work is seen before the plan it has not entered.
fn compare_roots(left: &Node<'_>, right: &Node<'_>) -> Ordering {
    let rank = |node: &Node<'_>| u8::from(node.placement != TaskPlacement::Inbox);
    rank(left)
        .cmp(&rank(right))
        .then_with(|| compare_siblings(left, right))
}

/// The display WBS of every task, as `projectWorkTableRows` joins it.
///
/// Repairs, in the browser's order: a duplicate id is DROPPED (the first
/// occurrence keeps the id), a task whose declared parent is missing or is
/// itself is re-rooted, and a cycle is cut at the member the cycle itself
/// picks — never at wherever the walk happened to enter it, so the same input
/// always yields the same repair.
pub fn wbs_by_task(tasks: &[CanonicalProjectTask]) -> BTreeMap<String, String> {
    ordered_rows(tasks)
        .into_iter()
        .map(|row| (row.id, row.wbs))
        .collect()
}

/// One task in display order: its id, the WBS label of its position and how
/// deep it sits. The same walk `projectRows` (`gantt/hierarchy.ts:191-217`)
/// makes for the Table, without the collapse state a window keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedRow {
    pub id: String,
    pub wbs: String,
    pub depth: usize,
}

/// Every task in the order the plan lists it — roots by placement then sort
/// key, children under their parent — with the WBS label each position earns.
/// `wbs_by_task` is this walk indexed by id; the list form is what a bounded
/// task listing pages over.
pub fn ordered_rows(tasks: &[CanonicalProjectTask]) -> Vec<OrderedRow> {
    hierarchy(tasks)
        .nodes
        .into_iter()
        .map(|node| OrderedRow {
            id: node.task_id,
            wbs: node.wbs,
            depth: node.depth,
        })
        .collect()
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyNode {
    pub task_id: String,
    pub parent_id: Option<String>,
    pub depth: usize,
    pub wbs: String,
    pub child_ids: Vec<String>,
    pub flat_index: usize,
    pub source_index: usize,
    pub order: f64,
}
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hierarchy {
    pub nodes: Vec<HierarchyNode>,
    pub root_ids: Vec<String>,
    pub ordered_ids: Vec<String>,
    pub broken_cycle_ids: Vec<String>,
    pub orphaned_ids: Vec<String>,
    pub duplicate_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineTask {
    id: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    order: Option<f64>,
    #[serde(default)]
    placement: TaskPlacement,
}
pub fn timeline_hierarchy(tasks: &[TimelineTask]) -> Hierarchy {
    walk(
        tasks
            .iter()
            .map(|task| Node {
                id: &task.id,
                parent_id: task.parent_id.as_deref(),
                order: task.order.unwrap_or(0.0),
                placement: task.placement,
            })
            .collect(),
    )
}
pub fn hierarchy(tasks: &[CanonicalProjectTask]) -> Hierarchy {
    let order = sibling_order(tasks);
    walk(
        tasks
            .iter()
            .map(|task| Node {
                id: &task.id,
                parent_id: task.parent_task_id.as_deref(),
                order: order.get(task.id.as_str()).copied().unwrap_or(0) as f64,
                placement: task.placement,
            })
            .collect(),
    )
}

fn walk(input: Vec<Node<'_>>) -> Hierarchy {
    let mut duplicate_ids = Vec::new();
    let mut source_indices = Vec::new();
    let mut nodes: Vec<Node<'_>> = Vec::new();
    let mut index_of: BTreeMap<&str, usize> = BTreeMap::new();
    for (source_index, node) in input.into_iter().enumerate() {
        if index_of.contains_key(node.id) {
            duplicate_ids.push(node.id.to_string());
            continue;
        }
        index_of.insert(node.id, nodes.len());
        source_indices.push(source_index);
        nodes.push(node);
    }
    let mut orphaned_ids: Vec<String> = nodes
        .iter()
        .filter(|node| {
            node.parent_id.is_some_and(|parent| {
                !parent.is_empty() && (parent == node.id || !index_of.contains_key(parent))
            })
        })
        .map(|node| node.id.to_string())
        .collect();
    orphaned_ids.sort();
    let mut broken_cycle_ids = Vec::new();

    // `resolveParents`: an absent, self-referential or unknown parent re-roots.
    let mut parent_of: Vec<Option<usize>> = nodes
        .iter()
        .map(|node| {
            node.parent_id
                .filter(|parent| !parent.is_empty() && *parent != node.id)
                .and_then(|parent| index_of.get(parent).copied())
        })
        .collect();

    // The iterative ancestor walk with a settled set: O(n) overall, no
    // recursion, and a cut point chosen from the cycle's own members.
    let mut settled = vec![false; nodes.len()];
    let mut on_path = vec![false; nodes.len()];
    for start in 0..nodes.len() {
        if settled[start] {
            continue;
        }
        let mut path: Vec<usize> = Vec::new();
        let mut cursor = Some(start);
        while let Some(current) = cursor {
            if settled[current] {
                break;
            }
            if on_path[current] {
                let entry = path.iter().position(|item| *item == current).unwrap_or(0);
                let cut = path[entry..]
                    .iter()
                    .copied()
                    .min_by(|left, right| {
                        compare_siblings(&nodes[*left], &nodes[*right])
                            .then_with(|| nodes[*left].id.cmp(nodes[*right].id))
                    })
                    .unwrap_or(current);
                parent_of[cut] = None;
                broken_cycle_ids.push(nodes[cut].id.to_string());
                break;
            }
            on_path[current] = true;
            path.push(current);
            cursor = parent_of[current];
        }
        for visited in path {
            settled[visited] = true;
            on_path[visited] = false;
        }
    }

    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (index, parent) in parent_of.iter().enumerate() {
        match parent {
            Some(parent) => children[*parent].push(index),
            None => roots.push(index),
        }
    }
    roots.sort_by(|left, right| compare_roots(&nodes[*left], &nodes[*right]));
    for siblings in children.iter_mut() {
        siblings.sort_by(|left, right| compare_siblings(&nodes[*left], &nodes[*right]));
    }

    // Root labels: the inbox pile counts as `Inbox N`, the plan as `N`. Both
    // counters run over the SORTED root list, so the inbox always numbers
    // first.
    let mut labels: BTreeMap<usize, String> = BTreeMap::new();
    let (mut inbox_index, mut wbs_index) = (0u32, 0u32);
    for root in &roots {
        if nodes[*root].placement == TaskPlacement::Inbox {
            inbox_index += 1;
            labels.insert(*root, format!("Inbox {inbox_index}"));
        } else {
            wbs_index += 1;
            labels.insert(*root, wbs_index.to_string());
        }
    }

    let mut rows: Vec<HierarchyNode> = Vec::with_capacity(nodes.len());
    let mut stack: Vec<(usize, String, usize)> = roots
        .iter()
        .rev()
        .map(|root| (*root, labels.get(root).cloned().unwrap_or_default(), 0))
        .collect();
    while let Some((index, label, depth)) = stack.pop() {
        rows.push(HierarchyNode {
            task_id: nodes[index].id.to_string(),
            parent_id: parent_of[index].map(|parent| nodes[parent].id.to_string()),
            child_ids: children[index]
                .iter()
                .map(|child| nodes[*child].id.to_string())
                .collect(),
            flat_index: rows.len(),
            source_index: source_indices[index],
            order: nodes[index].order,
            wbs: label.clone(),
            depth,
        });
        for (position, child) in children[index].iter().enumerate().rev() {
            stack.push((*child, format!("{label}.{}", position + 1), depth + 1));
        }
    }
    broken_cycle_ids.sort();
    Hierarchy {
        root_ids: roots
            .iter()
            .map(|root| nodes[*root].id.to_string())
            .collect(),
        ordered_ids: rows.iter().map(|row| row.task_id.clone()).collect(),
        nodes: rows,
        broken_cycle_ids,
        orphaned_ids,
        duplicate_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_management::wire::decode_project_graph;
    use serde_json::{json, Value};

    fn tasks(rows: Value) -> Vec<crate::project_management::wire::CanonicalProjectTask> {
        decode_project_graph(&json!({"project_id": "p", "tasks": rows}), "p").tasks
    }

    fn row(id: &str, parent: Option<&str>, sort_key: &str) -> Value {
        json!({"id": id, "data": {"title": id, "parent_task_id": parent, "sort_key": sort_key}})
    }

    #[test]
    fn the_wbs_is_a_position_label_and_never_an_identity() {
        // The same three tasks, reordered, keep their ids and swap their
        // labels. Anything that stored a WBS as a key would silently point at
        // the wrong task after one drag.
        let before = wbs_by_task(&tasks(json!([
            row("parent", None, "1"),
            row("child", Some("parent"), "1"),
            row("grandchild", Some("child"), "1"),
        ])));
        assert_eq!(before["parent"], "1");
        assert_eq!(before["child"], "1.1");
        assert_eq!(before["grandchild"], "1.1.1");

        let after = wbs_by_task(&tasks(json!([
            row("a", None, "0500"),
            row("b", None, "0100"),
        ])));
        assert_eq!((after["b"].as_str(), after["a"].as_str()), ("1", "2"));
    }

    #[test]
    fn siblings_order_by_sort_key_then_id_so_the_order_is_total() {
        // Two tasks that share a sort key must still have ONE order, or the
        // plan renumbers itself between two reads of the same graph.
        let wbs = wbs_by_task(&tasks(json!([
            row("z", None, "1"),
            row("a", None, "1"),
            row("m", None, "0"),
        ])));
        assert_eq!(wbs["m"], "1");
        assert_eq!(wbs["a"], "2");
        assert_eq!(wbs["z"], "3");
    }

    #[test]
    fn a_sort_key_an_operator_typed_orders_numerically() {
        // `localeCompare(…, { numeric: true })`: `item2` before `item10`, not
        // after it. A plain string sort puts every tenth task in the wrong
        // place, which is what an operator sees first.
        assert_eq!(natural_compare("item2", "item10"), Ordering::Less);
        assert_eq!(natural_compare("item10", "item9"), Ordering::Greater);
        assert_eq!(natural_compare("2", "10"), Ordering::Less);
        assert_eq!(natural_compare("a", "a1"), Ordering::Less);
        assert_eq!(natural_compare("survey", "survey"), Ordering::Equal);
        let wbs = wbs_by_task(&tasks(json!([
            row("ten", None, "item10"),
            row("two", None, "item2"),
        ])));
        assert_eq!((wbs["two"].as_str(), wbs["ten"].as_str()), ("1", "2"));
    }

    #[test]
    fn the_inbox_pile_numbers_first_and_separately() {
        // Unfiled work sits above the plan it has not entered, and is labelled
        // so it cannot be mistaken for a WBS position.
        let mut rows = vec![
            row("planned", None, "0100"),
            row("second-planned", None, "0200"),
            row("unfiled", None, "0300"),
            row("also-unfiled", None, "0400"),
        ];
        rows[2]["data"]["placement"] = json!("inbox");
        rows[3]["data"]["placement"] = json!("inbox");
        let wbs = wbs_by_task(&tasks(Value::Array(rows)));
        assert_eq!(wbs["unfiled"], "Inbox 1");
        assert_eq!(wbs["also-unfiled"], "Inbox 2");
        assert_eq!(wbs["planned"], "1");
        assert_eq!(wbs["second-planned"], "2");
    }

    #[test]
    fn a_broken_parent_chain_is_repaired_the_same_way_every_time() {
        // The timeline must still draw. A missing parent and a self-parent
        // re-root; a cycle is cut at the member the CYCLE picks, not at
        // whichever member the walk happened to enter — so reversing the input
        // cannot change the answer.
        let ghost = wbs_by_task(&tasks(json!([row("a", Some("ghost"), "1")])));
        assert_eq!(ghost["a"], "1");
        let itself = wbs_by_task(&tasks(json!([row("a", Some("a"), "1")])));
        assert_eq!(itself["a"], "1");

        let cyclic = vec![
            row("a", Some("c"), "1"),
            row("b", Some("a"), "1"),
            row("c", Some("b"), "1"),
        ];
        let forward = wbs_by_task(&tasks(Value::Array(cyclic.clone())));
        let mut reversed = cyclic;
        reversed.reverse();
        let backward = wbs_by_task(&tasks(Value::Array(reversed)));
        assert_eq!(forward, backward);
        assert_eq!(forward["a"], "1");
        assert_eq!(forward["b"], "1.1");
        assert_eq!(forward["c"], "1.1.1");
    }

    #[test]
    fn a_reused_id_keeps_the_first_row_and_numbers_it_once() {
        // Two rows claiming one identity cannot both hold a position. The
        // first keeps the id, exactly as the browser's hierarchy does.
        let wbs = wbs_by_task(&tasks(json!([
            row("a", None, "0100"),
            row("a", None, "0200"),
            row("b", None, "0300"),
        ])));
        assert_eq!(wbs.len(), 2);
        assert_eq!(wbs["a"], "1");
        assert_eq!(wbs["b"], "2");
    }
}
