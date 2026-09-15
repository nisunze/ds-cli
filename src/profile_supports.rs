//! Profile support selection and stringing policy shared by native and browser hosts.
//!
//! Hosts project a profile scene into supports, attachment points, and conductor
//! spans. This module owns ordering, range parsing, attachment compatibility,
//! tension boundaries, ambiguity, replacement ownership, and stale-section
//! detection. It performs no IO and carries no browser scene types.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const SCHEMA: &str = "ds.profile-supports/v1";
pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SUPPORTS: usize = 100_000;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachmentPoint {
    pub attachment_point_id: String,
    pub set_label: String,
    pub slot: i64,
    #[serde(default)]
    pub dead_end: Option<bool>,
    #[serde(default)]
    pub connected_section_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupportInfo {
    pub entity_id: String,
    pub alignment_id: String,
    pub station_m: f64,
    pub number: Option<String>,
    pub type_name: String,
    pub attachment_points: Vec<AttachmentPoint>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneStructure {
    pub entity_id: String,
    pub station_m: f64,
    pub engineering_name: String,
    pub attachment_points: Vec<AttachmentPoint>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneBand {
    pub alignment_id: String,
    pub structures: Vec<SceneStructure>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConductorSpan {
    pub section_id: String,
    #[serde(default)]
    pub section_set_member_ids: Vec<String>,
    pub from_structure_id: String,
    pub to_structure_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StringingRunAmbiguity {
    pub kind: AmbiguityKind,
    pub key: String,
    pub support_id: String,
    pub position: usize,
    pub candidate_labels: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AmbiguityKind {
    SupportSet,
    AnchorSeed,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StringingRun {
    pub key: String,
    pub group_key: String,
    pub support_ids: Vec<String>,
    pub labels: Vec<String>,
    pub uniform: bool,
    pub display_label: String,
    pub slots: Vec<i64>,
    pub support_slots: Vec<Vec<i64>>,
    pub support_label_options: Vec<Vec<String>>,
    pub ambiguities: Vec<StringingRunAmbiguity>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SupportNumbersResolution {
    pub entity_ids: Vec<String>,
    pub unmatched: Vec<String>,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSupport {
    pub structure_id: String,
    pub attachment_point_id: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ResolvedSectionSupports {
    pub supports: Vec<ResolvedSupport>,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OccupyingSections {
    pub by_set: BTreeMap<String, Vec<String>>,
    pub stale: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Request {
    Index {
        bands: Vec<SceneBand>,
        #[serde(default)]
        structure_numbers: HashMap<String, String>,
    },
    ResolveRange {
        index: Vec<SupportInfo>,
        from_id: String,
        to_id: String,
    },
    ResolveNumbers {
        index: Vec<SupportInfo>,
        text: String,
    },
    CommonLabels {
        index: Vec<SupportInfo>,
        entity_ids: Vec<String>,
    },
    CommonSlots {
        index: Vec<SupportInfo>,
        entity_ids: Vec<String>,
        set_label: String,
    },
    CompleteSlots {
        index: Vec<SupportInfo>,
        entity_ids: Vec<String>,
        set_label: String,
    },
    ResolveAttachments {
        index: Vec<SupportInfo>,
        entity_ids: Vec<String>,
        set_label: Option<String>,
        slot: Option<i64>,
    },
    ResolveStringing {
        index: Vec<SupportInfo>,
        support_ids: Vec<String>,
        #[serde(default)]
        choices: HashMap<String, String>,
        #[serde(default)]
        attachment_set_picks: HashMap<String, String>,
        group_attachment_set_picks: Option<HashMap<String, HashMap<String, String>>>,
    },
    ReplacingSections {
        spans: Vec<ConductorSpan>,
        index: Vec<SupportInfo>,
        runs: Vec<StringingRun>,
    },
    OccupyingSections {
        spans: Vec<ConductorSpan>,
        index: Vec<SupportInfo>,
        support_ids: Vec<String>,
    },
}

fn validate_index(index: &[SupportInfo]) -> Result<(), String> {
    if index.len() > MAX_SUPPORTS {
        return Err(format!("profile support count exceeds {MAX_SUPPORTS}"));
    }
    if index.iter().any(|support| !support.station_m.is_finite()) {
        return Err("profile support stations must be finite".into());
    }
    Ok(())
}

pub fn profile_support_index(
    bands: Vec<SceneBand>,
    structure_numbers: &HashMap<String, String>,
) -> Result<Vec<SupportInfo>, String> {
    let count: usize = bands.iter().map(|band| band.structures.len()).sum();
    if count > MAX_SUPPORTS {
        return Err(format!("profile support count exceeds {MAX_SUPPORTS}"));
    }
    let mut index = Vec::with_capacity(count);
    for band in bands {
        for structure in band.structures {
            if !structure.station_m.is_finite() {
                return Err("profile support stations must be finite".into());
            }
            index.push(SupportInfo {
                number: structure_numbers.get(&structure.entity_id).cloned(),
                entity_id: structure.entity_id,
                alignment_id: band.alignment_id.clone(),
                station_m: structure.station_m,
                type_name: structure.engineering_name,
                attachment_points: structure.attachment_points,
            });
        }
    }
    index.sort_by(|left, right| {
        left.alignment_id
            .cmp(&right.alignment_id)
            .then(left.station_m.total_cmp(&right.station_m))
    });
    Ok(index)
}

pub fn resolve_support_range(
    index: &[SupportInfo],
    from_id: &str,
    to_id: &str,
) -> Option<Vec<String>> {
    let from = index.iter().find(|support| support.entity_id == from_id)?;
    let to = index.iter().find(|support| support.entity_id == to_id)?;
    if from.alignment_id != to.alignment_id {
        return None;
    }
    let low = from.station_m.min(to.station_m);
    let high = from.station_m.max(to.station_m);
    let mut between = index
        .iter()
        .filter(|support| {
            support.alignment_id == from.alignment_id
                && support.station_m >= low
                && support.station_m <= high
        })
        .collect::<Vec<_>>();
    between.sort_by(|left, right| left.station_m.total_cmp(&right.station_m));
    let mut ids = between
        .into_iter()
        .map(|support| support.entity_id.clone())
        .collect::<Vec<_>>();
    if from.station_m > to.station_m {
        ids.reverse();
    }
    Some(ids)
}

fn range_parts(token: &str) -> Option<(&str, &str)> {
    if let Some(position) = token.find("..") {
        return Some((&token[..position], &token[position + 2..]));
    }
    for (position, character) in token.char_indices() {
        if character == '-' || character == '–' {
            return Some((
                &token[..position],
                &token[position + character.len_utf8()..],
            ));
        }
    }
    None
}

pub fn resolve_support_numbers(index: &[SupportInfo], text: &str) -> SupportNumbersResolution {
    let by_number = index
        .iter()
        .filter_map(|support| {
            support
                .number
                .as_ref()
                .map(|number| (number.as_str(), support))
        })
        .collect::<HashMap<_, _>>();
    let normalized = regex::Regex::new(r"\s*(?:-|–|\.\.)\s*")
        .expect("static regex")
        .replace_all(text, |caps: &regex::Captures<'_>| {
            let matched = caps.get(0).map_or("", |value| value.as_str());
            if matched.contains("..") {
                ".."
            } else if matched.contains('–') {
                "–"
            } else {
                "-"
            }
        });
    let mut answer = SupportNumbersResolution {
        entity_ids: vec![],
        unmatched: vec![],
        issues: vec![],
    };
    for token in normalized
        .split(|character: char| character == ',' || character == ';' || character.is_whitespace())
        .filter(|item| !item.is_empty())
    {
        if let Some((left, right)) = range_parts(token).filter(|_| !by_number.contains_key(token)) {
            let (Some(from), Some(to)) = (by_number.get(left.trim()), by_number.get(right.trim()))
            else {
                answer.unmatched.push(token.into());
                continue;
            };
            let Some(expanded) = resolve_support_range(index, &from.entity_id, &to.entity_id)
            else {
                answer
                    .issues
                    .push(format!("{token}: structures are on different alignments."));
                continue;
            };
            for id in expanded {
                if !answer.entity_ids.contains(&id) {
                    answer.entity_ids.push(id);
                }
            }
        } else if let Some(support) = by_number.get(token) {
            if !answer.entity_ids.contains(&support.entity_id) {
                answer.entity_ids.push(support.entity_id.clone());
            }
        } else {
            answer.unmatched.push(token.into());
        }
    }
    answer
}

fn selected<'a>(index: &'a [SupportInfo], ids: &[String]) -> Vec<&'a SupportInfo> {
    ids.iter()
        .filter_map(|id| index.iter().find(|support| support.entity_id == *id))
        .collect()
}

pub fn common_attachment_set_labels(index: &[SupportInfo], ids: &[String]) -> Vec<String> {
    let chosen = selected(index, ids);
    let Some(first) = chosen.first() else {
        return vec![];
    };
    let mut labels = first
        .attachment_points
        .iter()
        .map(|point| point.set_label.clone())
        .collect::<BTreeSet<_>>();
    for support in &chosen[1..] {
        let own = support
            .attachment_points
            .iter()
            .map(|point| point.set_label.as_str())
            .collect::<HashSet<_>>();
        labels.retain(|label| own.contains(label.as_str()));
    }
    labels.into_iter().collect()
}

pub fn common_set_slots(index: &[SupportInfo], ids: &[String], set_label: &str) -> Vec<i64> {
    let chosen = selected(index, ids);
    let Some(first) = chosen.first() else {
        return vec![];
    };
    let mut slots = first
        .attachment_points
        .iter()
        .filter(|point| point.set_label == set_label)
        .map(|point| point.slot)
        .collect::<BTreeSet<_>>();
    for support in &chosen[1..] {
        let own = support
            .attachment_points
            .iter()
            .filter(|point| point.set_label == set_label)
            .map(|point| point.slot)
            .collect::<HashSet<_>>();
        slots.retain(|slot| own.contains(slot));
    }
    slots.into_iter().collect()
}

pub fn complete_attachment_set_slots(
    index: &[SupportInfo],
    ids: &[String],
    set_label: &str,
) -> Vec<i64> {
    let chosen = selected(index, ids);
    if chosen.is_empty() || chosen.len() != ids.len() {
        return vec![];
    }
    let inventories = chosen
        .iter()
        .map(|support| {
            let mut slots = support
                .attachment_points
                .iter()
                .filter(|point| point.set_label == set_label)
                .map(|point| point.slot)
                .collect::<Vec<_>>();
            slots.sort();
            slots
        })
        .collect::<Vec<_>>();
    let expected = &inventories[0];
    if expected.iter().collect::<HashSet<_>>().len() != expected.len() {
        return vec![];
    }
    if inventories.iter().all(|slots| slots == expected) {
        expected.clone()
    } else {
        vec![]
    }
}

pub fn resolve_section_supports(
    index: &[SupportInfo],
    ids: &[String],
    set_label: Option<&str>,
    slot: Option<i64>,
) -> ResolvedSectionSupports {
    let mut answer = ResolvedSectionSupports {
        supports: vec![],
        issues: vec![],
    };
    for id in ids {
        let Some(support) = index.iter().find(|support| support.entity_id == *id) else {
            answer
                .issues
                .push(format!("{id} is no longer on the profile."));
            continue;
        };
        let label = support.number.as_deref().unwrap_or(&support.type_name);
        let mut candidates = support
            .attachment_points
            .iter()
            .filter(|point| {
                set_label.is_none_or(|set| point.set_label == set)
                    && slot.is_none_or(|wanted| point.slot == wanted)
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            let set_issue = set_label.map_or(String::new(), |set| format!(" in set “{set}”"));
            let slot_issue = slot.map_or(String::new(), |slot| format!(" slot {slot}"));
            answer.issues.push(format!(
                "{label}: no attachment point{set_issue}{slot_issue}."
            ));
            continue;
        }
        if set_label.is_none() && candidates.len() > 1 {
            answer.issues.push(format!(
                "{label}: {} attachment points — choose a set.",
                candidates.len()
            ));
            continue;
        }
        candidates.sort_by_key(|point| point.slot);
        answer.supports.push(ResolvedSupport {
            structure_id: id.clone(),
            attachment_point_id: candidates[0].attachment_point_id.clone(),
        });
    }
    answer
}

#[derive(Clone, Copy, PartialEq)]
enum Termination {
    Strain,
    Suspension,
    Unknown,
    Mixed,
}

fn run_termination(support: &SupportInfo, label: &str, slots: &[i64]) -> Termination {
    let mut strain = false;
    let mut suspension = false;
    for point in support
        .attachment_points
        .iter()
        .filter(|point| point.set_label == label && slots.contains(&point.slot))
    {
        match point.dead_end {
            Some(true) => strain = true,
            Some(false) => suspension = true,
            None => {}
        }
    }
    match (strain, suspension) {
        (true, true) => Termination::Mixed,
        (true, false) => Termination::Strain,
        (false, true) => Termination::Suspension,
        _ => Termination::Unknown,
    }
}

fn candidate_admissible(
    picked: &[&SupportInfo],
    position: usize,
    label: &str,
    slots: &[i64],
) -> bool {
    let support = picked[position];
    let points = support
        .attachment_points
        .iter()
        .filter(|point| point.set_label == label && slots.contains(&point.slot))
        .collect::<Vec<_>>();
    if points.len() != slots.len() {
        return false;
    }
    let termination = run_termination(support, label, slots);
    let endpoint = position == 0 || position + 1 == picked.len();
    if endpoint {
        termination != Termination::Suspension && termination != Termination::Mixed
    } else {
        termination != Termination::Mixed
    }
}

fn valid_tension_boundaries(
    picked: &[&SupportInfo],
    labels: &[String],
    slots: &[Vec<i64>],
) -> bool {
    picked.iter().enumerate().all(|(position, support)| {
        let own_slots = slots.get(position).map(Vec::as_slice).unwrap_or(&[]);
        let points = support
            .attachment_points
            .iter()
            .filter(|point| point.set_label == labels[position] && own_slots.contains(&point.slot))
            .collect::<Vec<_>>();
        if points.len() != own_slots.len() {
            return false;
        }
        let endpoint = position == 0 || position + 1 == picked.len();
        points.into_iter().all(|point| {
            if endpoint {
                point.dead_end != Some(false)
            } else {
                point.dead_end != Some(true)
            }
        })
    })
}

fn json_string(value: impl Serialize) -> String {
    serde_json::to_string(&value).expect("serializable key")
}

pub fn resolve_stringing_runs(
    index: &[SupportInfo],
    support_ids: &[String],
    choices: &HashMap<String, String>,
    attachment_set_picks: &HashMap<String, String>,
    group_attachment_set_picks: Option<&HashMap<String, HashMap<String, String>>>,
) -> Vec<StringingRun> {
    let picked = selected(index, support_ids);
    if picked.len() < 2 || picked.len() != support_ids.len() {
        return vec![];
    }
    let support_sets = picked
        .iter()
        .map(|support| {
            let mut sets = BTreeMap::<String, BTreeSet<i64>>::new();
            for point in &support.attachment_points {
                sets.entry(point.set_label.clone())
                    .or_default()
                    .insert(point.slot);
            }
            sets.into_iter()
                .map(|(label, slots)| (label, slots.into_iter().collect::<Vec<_>>()))
                .collect::<BTreeMap<_, _>>()
        })
        .collect::<Vec<_>>();
    let legacy = if group_attachment_set_picks.is_none() {
        attachment_set_picks
    } else {
        &HashMap::new()
    };
    let anchor_pick = legacy.get(&support_ids[0]);
    let seeds = support_sets[0]
        .iter()
        .filter(|(label, slots)| {
            anchor_pick.is_none_or(|pick| pick == *label)
                && candidate_admissible(&picked, 0, label, slots)
        })
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    let mut runs = BTreeMap::<String, StringingRun>::new();
    for seed in &seeds {
        let inventory = support_sets[0].get(seed).expect("seed inventory").clone();
        let root_key = format!("{}\u{2}{seed}", support_ids.join("\u{1}"));
        let group_key = format!(
            "attachment-group:{}",
            json_string((support_ids, seed, &inventory))
        );
        let group_picks = group_attachment_set_picks.and_then(|groups| groups.get(&group_key));
        let mut labels = vec![];
        let mut support_slots = vec![];
        let mut support_label_options = vec![];
        let mut ambiguities = vec![];
        let mut linked = true;
        for (position, sets) in support_sets.iter().enumerate() {
            let matches = sets
                .iter()
                .filter(|(label, slots)| {
                    **slots == inventory
                        && candidate_admissible(&picked, position, label, slots)
                        && (position != 0 || *label == seed)
                })
                .map(|(label, slots)| (label.clone(), slots.clone()))
                .collect::<Vec<_>>();
            support_label_options.push(matches.iter().map(|(label, _)| label.clone()).collect());
            let canvas_pick = attachment_set_picks.get(&support_ids[position]);
            let owner_count = canvas_pick.map_or(0, |pick| {
                seeds
                    .iter()
                    .filter(|candidate_seed| {
                        if position == 0 {
                            *candidate_seed == pick
                        } else {
                            let candidate_inventory = support_sets[0]
                                .get(*candidate_seed)
                                .expect("seed inventory");
                            sets.get(pick).is_some_and(|candidate_slots| {
                                candidate_slots == candidate_inventory
                                    && candidate_admissible(
                                        &picked,
                                        position,
                                        pick,
                                        candidate_slots,
                                    )
                            })
                        }
                    })
                    .count()
            });
            let canvas_choice = canvas_pick.filter(|pick| {
                group_attachment_set_picks.is_some()
                    && owner_count == 1
                    && matches.iter().any(|(label, _)| label == *pick)
            });
            let explicit = group_picks
                .and_then(|picks| picks.get(&support_ids[position]))
                .or(canvas_choice)
                .or_else(|| legacy.get(&support_ids[position]));
            if let Some(explicit) = explicit {
                if let Some((_, slots)) = matches.iter().find(|(label, _)| label == explicit) {
                    labels.push(explicit.clone());
                    support_slots.push(slots.clone());
                    continue;
                }
                linked = false;
                break;
            }
            if let Some(own) = sets.get(seed).filter(|slots| {
                **slots == inventory && candidate_admissible(&picked, position, seed, slots)
            }) {
                labels.push(seed.clone());
                support_slots.push(own.clone());
                continue;
            }
            if matches.len() == 1 {
                labels.push(matches[0].0.clone());
                support_slots.push(matches[0].1.clone());
                continue;
            }
            if matches.len() > 1 {
                let key = format!("{root_key}\u{3}{position}");
                if let Some(chosen) = choices
                    .get(&key)
                    .and_then(|choice| matches.iter().find(|(label, _)| label == choice))
                {
                    labels.push(chosen.0.clone());
                    support_slots.push(chosen.1.clone());
                    continue;
                }
                labels.push(String::new());
                support_slots.push(vec![]);
                ambiguities.push(StringingRunAmbiguity {
                    kind: AmbiguityKind::SupportSet,
                    key,
                    support_id: support_ids[position].clone(),
                    position,
                    candidate_labels: matches.iter().map(|(label, _)| label.clone()).collect(),
                });
                continue;
            }
            linked = false;
            break;
        }
        if !linked {
            continue;
        }
        if !ambiguities.is_empty() {
            let key = format!("{root_key}\u{4}unresolved");
            runs.insert(
                key.clone(),
                StringingRun {
                    key,
                    group_key,
                    support_ids: support_ids.to_vec(),
                    labels,
                    uniform: false,
                    display_label: format!("{seed} ⇄ ?"),
                    slots: inventory,
                    support_slots,
                    support_label_options,
                    ambiguities,
                },
            );
            continue;
        }
        let first = run_termination(picked[0], &labels[0], &support_slots[0]);
        let last_pos = picked.len() - 1;
        let last = run_termination(
            picked[last_pos],
            &labels[last_pos],
            &support_slots[last_pos],
        );
        if matches!(first, Termination::Suspension | Termination::Mixed)
            || matches!(last, Termination::Suspension | Termination::Mixed)
        {
            continue;
        }
        let mut boundaries = vec![0];
        for position in 1..last_pos {
            if run_termination(
                picked[position],
                &labels[position],
                &support_slots[position],
            ) == Termination::Strain
            {
                boundaries.push(position);
            }
        }
        boundaries.push(last_pos);
        for pair in boundaries.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            if to <= from
                || !valid_tension_boundaries(
                    &picked[from..=to],
                    &labels[from..=to],
                    &support_slots[from..=to],
                )
            {
                continue;
            }
            let key = format!("{root_key}\u{4}{from}:{to}");
            if runs.contains_key(&key) {
                continue;
            }
            let segment_labels = labels[from..=to].to_vec();
            let uniform = segment_labels
                .iter()
                .all(|label| label == &segment_labels[0]);
            let mut distinct = vec![];
            for label in &segment_labels {
                if !distinct.contains(label) {
                    distinct.push(label.clone());
                }
            }
            runs.insert(
                key.clone(),
                StringingRun {
                    key,
                    group_key: group_key.clone(),
                    support_ids: support_ids[from..=to].to_vec(),
                    labels: segment_labels.clone(),
                    uniform,
                    display_label: if uniform {
                        segment_labels[0].clone()
                    } else {
                        distinct.join(" ⇄ ")
                    },
                    slots: support_slots[from].clone(),
                    support_slots: support_slots[from..=to].to_vec(),
                    support_label_options: support_label_options[from..=to].to_vec(),
                    ambiguities: vec![],
                },
            );
        }
    }

    let unresolved = runs
        .values()
        .filter(|run| !run.ambiguities.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    let mut physical = BTreeMap::<String, StringingRun>::new();
    for run in runs.values().filter(|run| run.ambiguities.is_empty()) {
        let signature = json_string((
            &run.support_ids,
            &run.labels,
            &run.slots,
            &run.support_slots,
        ));
        physical.entry(signature.clone()).or_insert_with(|| {
            let mut run = run.clone();
            run.key = format!("physical-segment:{signature}");
            run
        });
    }
    #[derive(Clone)]
    struct Claim {
        key: String,
        support_id: String,
        set_label: String,
        neighbor_id: String,
        owners: BTreeSet<String>,
    }
    let mut claims = BTreeMap::<String, Claim>::new();
    for (segment_key, run) in &physical {
        for position in 0..run.support_ids.len() - 1 {
            for (support_id, set_label, neighbor_id) in [
                (
                    &run.support_ids[position],
                    &run.labels[position],
                    &run.support_ids[position + 1],
                ),
                (
                    &run.support_ids[position + 1],
                    &run.labels[position + 1],
                    &run.support_ids[position],
                ),
            ] {
                let key = json_string((support_id, set_label, neighbor_id));
                claims
                    .entry(key.clone())
                    .or_insert_with(|| Claim {
                        key,
                        support_id: support_id.clone(),
                        set_label: set_label.clone(),
                        neighbor_id: neighbor_id.clone(),
                        owners: BTreeSet::new(),
                    })
                    .owners
                    .insert(segment_key.clone());
            }
        }
    }
    let positions = support_ids
        .iter()
        .enumerate()
        .map(|(position, id)| (id.clone(), position))
        .collect::<HashMap<_, _>>();
    let mut ordered_claims = claims.into_values().collect::<Vec<_>>();
    ordered_claims.sort_by(|left, right| {
        let lp = positions
            .get(&left.support_id)
            .copied()
            .unwrap_or(usize::MAX)
            .min(
                positions
                    .get(&left.neighbor_id)
                    .copied()
                    .unwrap_or(usize::MAX),
            );
        let rp = positions
            .get(&right.support_id)
            .copied()
            .unwrap_or(usize::MAX)
            .min(
                positions
                    .get(&right.neighbor_id)
                    .copied()
                    .unwrap_or(usize::MAX),
            );
        lp.cmp(&rp).then(left.key.cmp(&right.key))
    });
    let mut active = physical.keys().cloned().collect::<BTreeSet<_>>();
    let mut shared = vec![];
    loop {
        let Some((claim, owners)) = ordered_claims.iter().find_map(|claim| {
            let owners = claim
                .owners
                .iter()
                .filter(|owner| active.contains(*owner))
                .cloned()
                .collect::<Vec<_>>();
            (owners.len() > 1).then_some((claim, owners))
        }) else {
            break;
        };
        let owner_runs = owners
            .iter()
            .filter_map(|owner| physical.get(owner))
            .collect::<Vec<_>>();
        let common = support_ids
            .iter()
            .filter(|id| owner_runs.iter().all(|run| run.support_ids.contains(id)))
            .collect::<Vec<_>>();
        let neighbor_position = positions.get(&claim.neighbor_id).copied().unwrap_or(0);
        let mut sorted_common = common;
        sorted_common.sort_by_key(|id| {
            positions
                .get(*id)
                .copied()
                .unwrap_or(0)
                .abs_diff(neighbor_position)
        });
        let choice_support = sorted_common
            .into_iter()
            .find(|id| {
                owner_runs
                    .iter()
                    .map(|run| {
                        run.labels[run
                            .support_ids
                            .iter()
                            .position(|own| own == *id)
                            .expect("common support")]
                        .as_str()
                    })
                    .collect::<HashSet<_>>()
                    .len()
                    > 1
            })
            .cloned()
            .unwrap_or_else(|| claim.neighbor_id.clone());
        let label_for = |run: &StringingRun, id: &str| {
            run.support_ids
                .iter()
                .position(|own| own == id)
                .and_then(|position| run.labels.get(position))
                .cloned()
                .unwrap_or_default()
        };
        let candidates = owner_runs
            .iter()
            .map(|run| label_for(run, &choice_support))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let sorted_owners = owners.iter().cloned().collect::<Vec<_>>();
        let key = format!(
            "shared-segment:{}",
            json_string((
                &claim.support_id,
                &claim.set_label,
                &claim.neighbor_id,
                &choice_support,
                &candidates,
                &sorted_owners
            ))
        );
        if let Some(chosen) = choices
            .get(&key)
            .filter(|chosen| candidates.contains(chosen))
        {
            let retained = owners
                .iter()
                .filter(|owner| {
                    physical
                        .get(*owner)
                        .is_some_and(|run| label_for(run, &choice_support) == **chosen)
                })
                .cloned()
                .collect::<Vec<_>>();
            if !retained.is_empty() && retained.len() < owners.len() {
                for owner in owners.iter().filter(|owner| !retained.contains(owner)) {
                    active.remove(owner);
                }
                continue;
            }
        }
        for owner in &owners {
            active.remove(owner);
        }
        let first = positions.get(&choice_support).copied().unwrap_or(0);
        let last = positions.get(&claim.support_id).copied().unwrap_or(first);
        let local_ids = support_ids[first.min(last)..=first.max(last)].to_vec();
        let representative = owner_runs[0];
        let local_labels = local_ids
            .iter()
            .map(|id| {
                if id == &choice_support {
                    return String::new();
                }
                let values = owner_runs
                    .iter()
                    .map(|run| label_for(run, id))
                    .collect::<Vec<_>>();
                if values.iter().all(|value| value == &values[0]) {
                    values[0].clone()
                } else {
                    String::new()
                }
            })
            .collect::<Vec<_>>();
        let local_slots = local_ids
            .iter()
            .enumerate()
            .map(|(position, id)| {
                if local_labels[position].is_empty() {
                    vec![]
                } else {
                    representative
                        .support_ids
                        .iter()
                        .position(|own| own == id)
                        .map(|source| representative.support_slots[source].clone())
                        .unwrap_or_default()
                }
            })
            .collect::<Vec<_>>();
        let label_options = local_ids
            .iter()
            .enumerate()
            .map(|(position, id)| {
                if id == &choice_support {
                    candidates.clone()
                } else if let Some(source) =
                    representative.support_ids.iter().position(|own| own == id)
                {
                    representative.support_label_options[source].clone()
                } else if !local_labels[position].is_empty() {
                    vec![local_labels[position].clone()]
                } else {
                    vec![]
                }
            })
            .collect();
        shared.push(StringingRun {
            key: format!("{key}:unresolved"),
            group_key: format!("attachment-group-conflict:{key}"),
            support_ids: local_ids.clone(),
            labels: local_labels,
            uniform: false,
            display_label: format!("{} ⇄ {}", candidates.join(" / "), claim.set_label),
            slots: representative.slots.clone(),
            support_slots: local_slots,
            support_label_options: label_options,
            ambiguities: vec![StringingRunAmbiguity {
                kind: AmbiguityKind::AnchorSeed,
                key,
                support_id: choice_support.clone(),
                position: local_ids
                    .iter()
                    .position(|id| id == &choice_support)
                    .unwrap_or(0),
                candidate_labels: candidates,
            }],
        });
    }
    let mut answer = physical
        .into_iter()
        .filter(|(key, _)| active.contains(key))
        .map(|(_, run)| run)
        .chain(unresolved)
        .chain(shared)
        .collect::<Vec<_>>();
    answer.sort_by(|left, right| {
        (!left.ambiguities.is_empty())
            .cmp(&(!right.ambiguities.is_empty()))
            .then(right.uniform.cmp(&left.uniform))
            .then(left.display_label.cmp(&right.display_label))
    });
    answer
}

pub fn sections_replacing_stringing_runs(
    spans: &[ConductorSpan],
    index: &[SupportInfo],
    runs: &[StringingRun],
) -> Vec<String> {
    if runs.is_empty() {
        return vec![];
    }
    let mut members = HashMap::<String, BTreeSet<String>>::new();
    let mut structures = HashMap::<String, BTreeSet<String>>::new();
    for span in spans {
        let group = std::iter::once(&span.section_id)
            .chain(span.section_set_member_ids.iter())
            .cloned()
            .collect::<BTreeSet<_>>();
        for member in &group {
            members
                .entry(member.clone())
                .or_default()
                .extend(group.iter().cloned());
            let crossed = structures.entry(member.clone()).or_default();
            crossed.insert(span.from_structure_id.clone());
            crossed.insert(span.to_structure_id.clone());
        }
    }
    let mut replacements = BTreeSet::new();
    for run in runs.iter().filter(|run| run.ambiguities.is_empty()) {
        let selected = run.support_ids.iter().cloned().collect::<BTreeSet<_>>();
        let mut candidates: Option<BTreeSet<String>> = None;
        for position in 0..run.support_ids.len() {
            let Some(support) = index
                .iter()
                .find(|support| support.entity_id == run.support_ids[position])
            else {
                candidates = Some(BTreeSet::new());
                break;
            };
            let local = support
                .attachment_points
                .iter()
                .filter(|point| {
                    point.set_label == run.labels[position]
                        && run
                            .support_slots
                            .get(position)
                            .is_some_and(|slots| slots.contains(&point.slot))
                })
                .flat_map(|point| point.connected_section_ids.iter().cloned())
                .collect::<BTreeSet<_>>();
            candidates = Some(match candidates {
                None => local,
                Some(prior) => prior.intersection(&local).cloned().collect(),
            });
        }
        for candidate in candidates.unwrap_or_default() {
            if structures.get(&candidate).is_some_and(|crossed| {
                crossed.len() == selected.len() && crossed.iter().all(|id| selected.contains(id))
            }) {
                replacements.extend(
                    members
                        .get(&candidate)
                        .cloned()
                        .unwrap_or_else(|| BTreeSet::from([candidate])),
                );
            }
        }
    }
    replacements.into_iter().collect()
}

pub fn sections_occupying_supports(
    spans: &[ConductorSpan],
    index: &[SupportInfo],
    support_ids: &[String],
) -> OccupyingSections {
    let picked = support_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut sets = BTreeMap::<String, BTreeSet<String>>::new();
    let mut reachable = HashSet::new();
    for support in index
        .iter()
        .filter(|support| picked.contains(support.entity_id.as_str()))
    {
        for point in &support.attachment_points {
            for section in &point.connected_section_ids {
                sets.entry(point.set_label.clone())
                    .or_default()
                    .insert(section.clone());
                reachable.insert(section.clone());
            }
        }
    }
    let mut stale = BTreeSet::new();
    for span in spans.iter().filter(|span| {
        picked.contains(span.from_structure_id.as_str())
            || picked.contains(span.to_structure_id.as_str())
    }) {
        for section in std::iter::once(&span.section_id).chain(span.section_set_member_ids.iter()) {
            if !reachable.contains(section) {
                stale.insert(section.clone());
            }
        }
    }
    OccupyingSections {
        by_set: sets
            .into_iter()
            .map(|(label, ids)| (label, ids.into_iter().collect()))
            .collect(),
        stale: stale.into_iter().collect(),
    }
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "profile supports request exceeds {MAX_INPUT_BYTES} bytes"
        ));
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid profile supports request: {error}"))?;
    let value = match request {
        Request::Index {
            bands,
            structure_numbers,
        } => serde_json::to_value(profile_support_index(bands, &structure_numbers)?),
        Request::ResolveRange {
            index,
            from_id,
            to_id,
        } => {
            validate_index(&index)?;
            serde_json::to_value(resolve_support_range(&index, &from_id, &to_id))
        }
        Request::ResolveNumbers { index, text } => {
            validate_index(&index)?;
            serde_json::to_value(resolve_support_numbers(&index, &text))
        }
        Request::CommonLabels { index, entity_ids } => {
            validate_index(&index)?;
            serde_json::to_value(common_attachment_set_labels(&index, &entity_ids))
        }
        Request::CommonSlots {
            index,
            entity_ids,
            set_label,
        } => {
            validate_index(&index)?;
            serde_json::to_value(common_set_slots(&index, &entity_ids, &set_label))
        }
        Request::CompleteSlots {
            index,
            entity_ids,
            set_label,
        } => {
            validate_index(&index)?;
            serde_json::to_value(complete_attachment_set_slots(
                &index,
                &entity_ids,
                &set_label,
            ))
        }
        Request::ResolveAttachments {
            index,
            entity_ids,
            set_label,
            slot,
        } => {
            validate_index(&index)?;
            serde_json::to_value(resolve_section_supports(
                &index,
                &entity_ids,
                set_label.as_deref(),
                slot,
            ))
        }
        Request::ResolveStringing {
            index,
            support_ids,
            choices,
            attachment_set_picks,
            group_attachment_set_picks,
        } => {
            validate_index(&index)?;
            serde_json::to_value(resolve_stringing_runs(
                &index,
                &support_ids,
                &choices,
                &attachment_set_picks,
                group_attachment_set_picks.as_ref(),
            ))
        }
        Request::ReplacingSections { spans, index, runs } => {
            validate_index(&index)?;
            serde_json::to_value(sections_replacing_stringing_runs(&spans, &index, &runs))
        }
        Request::OccupyingSections {
            spans,
            index,
            support_ids,
        } => {
            validate_index(&index)?;
            serde_json::to_value(sections_occupying_supports(&spans, &index, &support_ids))
        }
    }
    .map_err(|error| format!("profile supports response failed: {error}"))?;
    serde_json::to_string(&json!({ "schema": SCHEMA, "value": value }))
        .map_err(|error| format!("profile supports response failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(id: &str, set: &str, slot: i64, dead_end: Option<bool>) -> AttachmentPoint {
        AttachmentPoint {
            attachment_point_id: id.into(),
            set_label: set.into(),
            slot,
            dead_end,
            connected_section_ids: vec![],
        }
    }
    fn support(id: &str, station: f64, points: Vec<AttachmentPoint>) -> SupportInfo {
        SupportInfo {
            entity_id: id.into(),
            alignment_id: "line".into(),
            station_m: station,
            number: None,
            type_name: format!("type-{id}"),
            attachment_points: points,
        }
    }

    #[test]
    fn ranges_numbers_and_complete_inventories_are_owned_here() {
        let mut index = vec![
            support(
                "a",
                0.0,
                vec![
                    point("a0", "phases", 0, None),
                    point("a1", "phases", 1, None),
                ],
            ),
            support(
                "b",
                100.0,
                vec![
                    point("b0", "phases", 0, None),
                    point("b1", "phases", 1, None),
                ],
            ),
        ];
        index[0].number = Some("405".into());
        index[1].number = Some("406".into());
        assert_eq!(
            resolve_support_range(&index, "b", "a"),
            Some(vec!["b".into(), "a".into()])
        );
        assert_eq!(
            resolve_support_numbers(&index, "405 - 406, 999").entity_ids,
            vec!["a", "b"]
        );
        assert_eq!(
            complete_attachment_set_slots(&index, &["a".into(), "b".into()], "phases"),
            vec![0, 1]
        );
    }

    #[test]
    fn stringing_links_renamed_sets_and_requires_explicit_ambiguity_resolution() {
        let index = vec![
            support("a", 0.0, vec![point("a", "OPGW", 0, None)]),
            support(
                "b",
                100.0,
                vec![point("bx", "x", 0, None), point("by", "y", 0, None)],
            ),
        ];
        let ids = vec!["a".into(), "b".into()];
        let unresolved =
            resolve_stringing_runs(&index, &ids, &HashMap::new(), &HashMap::new(), None);
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].labels, vec!["OPGW", ""]);
        assert_eq!(
            unresolved[0].ambiguities[0].candidate_labels,
            vec!["x", "y"]
        );
        let choices = HashMap::from([(unresolved[0].ambiguities[0].key.clone(), "y".into())]);
        assert_eq!(
            resolve_stringing_runs(&index, &ids, &choices, &HashMap::new(), None)[0].labels,
            vec!["OPGW", "y"]
        );
    }

    #[test]
    fn strain_splits_runs_and_suspension_cannot_end_one() {
        let mut index = vec![
            support("a", 0.0, vec![point("a", "p", 0, Some(true))]),
            support("b", 100.0, vec![point("b", "p", 0, Some(false))]),
            support("c", 200.0, vec![point("c", "p", 0, Some(true))]),
        ];
        let ids = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(
            resolve_stringing_runs(&index, &ids, &HashMap::new(), &HashMap::new(), None).len(),
            1
        );
        assert!(
            resolve_stringing_runs(&index, &ids[..2], &HashMap::new(), &HashMap::new(), None)
                .is_empty()
        );
        index[1].attachment_points[0].dead_end = Some(true);
        assert_eq!(
            resolve_stringing_runs(&index, &ids, &HashMap::new(), &HashMap::new(), None).len(),
            2
        );
    }

    #[test]
    fn stale_sections_are_distinct_from_current_attachment_ownership() {
        let mut a = support("a", 0.0, vec![point("a", "OPGW", 0, None)]);
        a.attachment_points[0]
            .connected_section_ids
            .push("live".into());
        let spans = vec![
            ConductorSpan {
                section_id: "live".into(),
                section_set_member_ids: vec![],
                from_structure_id: "a".into(),
                to_structure_id: "b".into(),
            },
            ConductorSpan {
                section_id: "stale".into(),
                section_set_member_ids: vec![],
                from_structure_id: "a".into(),
                to_structure_id: "b".into(),
            },
        ];
        let answer = sections_occupying_supports(&spans, &[a], &["a".into()]);
        assert_eq!(answer.by_set["OPGW"], vec!["live"]);
        assert_eq!(answer.stale, vec!["stale"]);
    }

    #[test]
    fn request_door_is_schema_strict_and_byte_bounded() {
        let unknown = br#"{"operation":"common_labels","index":[],"entityIds":[],"surprise":true}"#;
        assert!(evaluate(unknown)
            .expect_err("unknown input must be refused")
            .contains("unknown field"));
        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1])
            .expect_err("oversized input must be refused")
            .contains("request exceeds"));
    }
}
