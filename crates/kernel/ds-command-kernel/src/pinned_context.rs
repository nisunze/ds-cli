//! Pinned context — one owner for what a read-only pinned working set costs.
//!
//! A pinned transformer is READ-ONLY CONTEXT: dumb GeoJSON an operator glances
//! at while working on something else. It is not editable, not a selection
//! target and carries no session. Everything below follows from that.
//!
//! Three decisions live here, and nowhere else:
//!
//! 1. **What to fetch** (`plan`). A room the machine already holds is reused.
//!    A room that is missing one layer asks for THAT LAYER, never for the whole
//!    room again. A held copy the server has moved past is refetched whole,
//!    because a moved revision invalidates every layer of it. Default scope
//!    is every saved class, proved by the existing status inventory or a
//!    whole-room read. An anchor alone is never completeness.
//!
//! 2. **What one layer IS** (`merge`). Every pinned room of the same design
//!    class folds into ONE FeatureCollection per (source, class), so fifty
//!    pinned transformers across six classes are six map sources, not three
//!    hundred. Each merged feature keeps the transformer it came from in
//!    `pinned_transformer_name`, so a popup, an attribute table row and a
//!    visibility choice can all still name it.
//!
//! 3. **Which attributes survive** (`merge`). A preview needs the fields its
//!    style document actually reads — nothing else. The keep-set is COMPUTED
//!    from the resolved style documents (every `["get", f]` / `["has", f]`
//!    anywhere in them, aliases included), never listed in code, because the
//!    documents are backend-authored and arrive at runtime. Everything else —
//!    the nested `*_properties` bags, the topology helpers, the survey columns
//!    — is dropped before the payload is handed to a renderer or a store.
//!
//! Pure: no clock, no I/O, no host state. The same request always has the same
//! answer, which is why the CLI can answer it on a Server with no browser.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "ds.pinned-context/v1";
/// A plan is a handful of names and layer keys; a merge carries real rooms.
pub const MAX_PLAN_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_MERGE_BYTES: usize = 192 * 1024 * 1024;

/// The batch cap ds-brain-bulk's `get_transformers_data` was green-lit for.
pub const DEFAULT_BATCH: usize = 500;
/// The transformer's own structural anchor. A genuinely saved LV room carries
/// it; the historic customers-only partial cache did not.
pub const ANCHOR_LAYER: &str = "tr";
/// The read projection a context fetch asks for: ds-brain drops non-scalar
/// properties server side, so the nested bags never cross the wire at all.
pub const CONTEXT_PROJECTION: &str = "context";
/// The one source every merged pinned layer belongs to. It is the SAME
/// constant the prepared-local-layer catalogue names a pinned row's source
/// with, because a merged pinned layer is a prepared local layer: two spellings
/// of one source would split the panel group in half.
pub use crate::local_layers::PINNED_SOURCE_NAME;
/// The property that names a merged feature's transformer. It ends in
/// `transformer_name`, which is how the popup already promotes it.
pub const PINNED_TRANSFORMER_PROPERTY: &str = "pinned_transformer_name";
/// The suffix a design class's PINNED style variant is published under.
/// `transformer-pinning.ts` mints `<designRef>_pinned`; the variant is read in
/// preference to the plain document wherever it exists.
pub const PINNED_STYLE_SUFFIX: &str = "_pinned";

/// Layer spellings that mean the anchor. `layer-aliases.ts` maps these to
/// `tr`; the completeness gate must read them the same way or a room spelled
/// `transformers` is refetched forever.
const ANCHOR_ALIASES: [&str; 4] = ["tr", "transformer", "transformers", "transformerpoint"];

// ── Request ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Plan(PlanRequest),
    Merge(MergeRequest),
    Styles(StylesRequest),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub schema: String,
    /// The pin set, as the operator pinned it.
    #[serde(default)]
    pub pins: Vec<String>,
    /// The transformer being edited. It is never pinned context as well, so it
    /// is dropped from the plan rather than fetched twice.
    #[serde(default)]
    pub focus: Option<String>,
    /// Refresh: ask for every pinned room again, cache or no cache.
    #[serde(default)]
    pub force: bool,
    /// Explicitly discard captured local edits; ordinary refresh never does.
    #[serde(default)]
    pub discard_local_edits: bool,
    /// Explicit class scope. Empty means ALL saved classes: use the head's
    /// inventory, or require a proven whole-room read when it is unavailable.
    #[serde(default)]
    pub require: Vec<String>,
    #[serde(default)]
    pub held: Vec<HeldRoom>,
    #[serde(default)]
    pub heads: Vec<Head>,
    #[serde(default)]
    pub batch: Option<usize>,
}

/// What the machine already holds for one transformer. `layers` is the layer
/// key → feature count inventory, not the payload: planning never needs the
/// geometry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldRoom {
    pub name: String,
    #[serde(default)]
    pub layers: BTreeMap<String, i64>,
    /// The server revision this copy was taken at, when it is known.
    #[serde(default)]
    pub version: Option<i64>,
    /// True when this copy came from a WHOLE-ROOM fetch. A complete copy that
    /// still lacks a required layer does not have one on the server either, so
    /// asking again would loop forever.
    #[serde(default)]
    pub complete: bool,
    /// Unsaved work is authoritative even when a refresh was requested.
    #[serde(default)]
    pub dirty: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Head {
    pub name: String,
    #[serde(default)]
    pub version: Option<i64>,
    /// Saved class counts from the existing status snapshot. None means that
    /// inventory is unknown; an empty inventory is a known empty room.
    #[serde(default)]
    pub layers: Option<BTreeMap<String, i64>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeRequest {
    pub schema: String,
    /// The rooms to fold, in the order they should paint.
    #[serde(default)]
    pub rooms: Vec<Room>,
    /// Pinned transformers the operator has hidden. Visibility of ONE pinned
    /// transformer is an input to the merge, not a MapLibre filter: the row it
    /// contributes simply is not in the payload.
    #[serde(default)]
    pub hidden: Vec<String>,
    /// Fields to keep on top of whatever the style documents read.
    #[serde(default)]
    pub keep: Vec<String>,
    /// The resolved style document per design class, as the renderer will use
    /// it. Every field name reachable inside one is kept.
    #[serde(default)]
    pub styles: BTreeMap<String, Value>,
    /// Current governed nature assignments for this read-only view. None leaves
    /// saved values untouched; Some clears values on unassigned transformers.
    #[serde(default)]
    pub transformer_natures: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub name: String,
    #[serde(default)]
    pub layers: BTreeMap<String, Value>,
}

/// Resolve each design class to the style document a PINNED row of it is
/// painted with, out of the project's published style catalogue.
///
/// A browser resolves this from the `MapState` it already holds. A Server holds
/// no map, so it asks here with the catalogue ds-brain published — and gets the
/// same answer, because the rule is the same rule and it lives in one place.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylesRequest {
    pub schema: String,
    /// The published style catalogue: `styles` keyed by style ref.
    pub catalog: Value,
    #[serde(default)]
    pub classes: Vec<String>,
    /// Fields to keep whatever the documents read.
    #[serde(default)]
    pub keep: Vec<String>,
}

// ── Answer ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, PartialEq)]
pub struct Plan {
    pub schema: &'static str,
    pub op: &'static str,
    /// Rooms that are painted from what the machine holds. No bytes.
    pub reuse: Vec<Reuse>,
    /// Rooms that need bytes, and exactly which.
    pub fetch: Vec<Fetch>,
    /// The batched calls the executor performs, one per identical layer
    /// request. `layers` empty means the whole room.
    pub chunks: Vec<Chunk>,
    /// The read projection every context fetch asks for.
    pub projection: &'static str,
    pub counts: PlanCounts,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Reuse {
    pub name: String,
    pub reason: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Fetch {
    pub name: String,
    /// The layer keys to ask for. Empty means the whole room.
    pub layers: Vec<String>,
    pub whole_room: bool,
    pub reason: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Chunk {
    pub layers: Vec<String>,
    pub names: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PlanCounts {
    pub pinned: usize,
    pub reused: usize,
    pub rooms_fetched: usize,
    pub layers_fetched: usize,
    pub requests: usize,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Merged {
    pub schema: &'static str,
    pub op: &'static str,
    pub source: String,
    pub layers: Vec<MergedLayer>,
    pub totals: MergeTotals,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MergedLayer {
    pub class_name: String,
    pub source_name: String,
    /// The transformers this one FeatureCollection was folded from, in order.
    pub transformers: Vec<String>,
    pub geometry_type: &'static str,
    pub feature_count: usize,
    /// The property keys the features carry — the keep-set, as computed.
    pub fields: Vec<String>,
    pub features: Vec<MergedFeature>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MergedFeature {
    pub id: String,
    pub geometry: Value,
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Styles {
    pub schema: &'static str,
    pub op: &'static str,
    pub classes: Vec<ClassStyle>,
    /// The keep-set per class, ready to hand straight back as `merge.styles`
    /// would compute it — so a caller never re-derives fields itself.
    pub keep: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ClassStyle {
    pub class_name: String,
    /// The style ref the document came from, or null when the catalogue
    /// publishes none for this class.
    pub style_ref: Option<String>,
    /// True when the ref is the class's `_pinned` variant rather than the
    /// design document it falls back to.
    pub pinned_variant: bool,
    /// Every property name the resolved document reads.
    pub fields: Vec<String>,
    pub document: Value,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MergeTotals {
    pub rooms: usize,
    pub hidden: usize,
    /// (transformer, class) pairs — one map source and one layer each, before.
    pub sources_before: usize,
    pub layers_before: usize,
    /// (source, class) pairs — one map source and one layer each, after.
    pub sources_after: usize,
    pub layers_after: usize,
    pub features: usize,
    /// Distinct property keys the rooms carried, and how many survive.
    pub properties_before: usize,
    pub properties_after: usize,
    pub properties_dropped: usize,
}

// ── Entry ───────────────────────────────────────────────────────────────

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_MERGE_BYTES {
        return Err("Pinned context request is too large to answer.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Pinned context request is malformed: {error}"))?;
    match request {
        Request::Plan(plan) => {
            if input.len() > MAX_PLAN_BYTES {
                return Err(
                    "A pinned context plan carries an inventory, not payloads; send layer counts."
                        .into(),
                );
            }
            let answer = self::plan(plan)?;
            serde_json::to_string(&answer).map_err(|error| error.to_string())
        }
        Request::Merge(merge) => {
            let answer = self::merge(merge)?;
            serde_json::to_string(&answer).map_err(|error| error.to_string())
        }
        Request::Styles(styles) => {
            if input.len() > MAX_PLAN_BYTES {
                return Err(
                    "A pinned context style request carries a catalogue, not payloads.".into(),
                );
            }
            let answer = self::styles(styles)?;
            serde_json::to_string(&answer).map_err(|error| error.to_string())
        }
    }
}

fn check_schema(schema: &str) -> Result<(), String> {
    if schema != SCHEMA {
        return Err(format!(
            "Unsupported pinned context schema; this kernel answers {SCHEMA}."
        ));
    }
    Ok(())
}

// ── 1. What to fetch ────────────────────────────────────────────────────

/// Shared room freshness for pinning, acquisition and Project Control. Older
/// observations cannot invalidate a newer held snapshot. This is metadata-only;
/// hosts still compare-and-swap the captured room before publishing fetched bytes.
pub fn room_is_fresh(room: &HeldRoom, head: Option<&Head>) -> bool {
    if room.dirty {
        return true;
    }
    if let (Some(remote), Some(local)) = (head.and_then(|h| h.version), room.version) {
        if remote > local {
            return false;
        }
        if remote < local {
            return room.complete;
        }
    }
    if let Some(counts) = head.and_then(|h| h.layers.as_ref()) {
        let held: BTreeMap<String, i64> = room
            .layers
            .iter()
            .map(|(name, count)| (layer_key(name), *count))
            .collect();
        let saved: BTreeMap<String, i64> = counts
            .iter()
            .map(|(name, count)| (layer_key(name), *count))
            .filter(|(_, count)| *count > 0)
            .collect();
        return held
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .collect::<BTreeMap<_, _>>()
            == saved;
    }
    room.complete
}

pub fn plan(request: PlanRequest) -> Result<Plan, String> {
    check_schema(&request.schema)?;
    let batch = request
        .batch
        .unwrap_or(DEFAULT_BATCH)
        .clamp(1, DEFAULT_BATCH);
    let focus = request.focus.as_deref().map(canonical).unwrap_or_default();

    let mut required: Vec<String> = request
        .require
        .iter()
        .map(|value| canonical(value))
        .filter(|value| !value.is_empty())
        .collect();
    required.sort();
    required.dedup();

    let held: BTreeMap<String, &HeldRoom> = request
        .held
        .iter()
        .map(|room| (canonical(&room.name), room))
        .collect();
    let mut heads: BTreeMap<String, &Head> = BTreeMap::new();
    for head in &request.heads {
        let key = canonical(&head.name);
        let replace = heads.get(&key).is_none_or(|old| {
            head.version.unwrap_or(0) > old.version.unwrap_or(0)
                || (head.version == old.version && head.layers.is_some())
        });
        if replace {
            heads.insert(key, head);
        }
    }

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut names: Vec<String> = Vec::new();
    for pin in &request.pins {
        let key = canonical(pin);
        if key.is_empty() || key == focus || !seen.insert(key) {
            continue;
        }
        names.push(pin.trim().to_owned());
    }
    names.sort_by_key(|name| canonical(name));

    let mut reuse: Vec<Reuse> = Vec::new();
    let mut fetch: Vec<Fetch> = Vec::new();
    for name in &names {
        let key = canonical(name);
        let room = held.get(&key).copied();
        let head = heads.get(&key).copied();

        if request.discard_local_edits {
            fetch.push(whole(name, "discard_local_edits"));
            continue;
        }
        if room.is_some_and(|room| room.dirty) {
            reuse.push(Reuse {
                name: name.clone(),
                reason: "local_edits",
            });
            continue;
        }
        if request.force {
            fetch.push(whole(name, "forced"));
            continue;
        }
        let Some(room) = room else {
            fetch.push(whole(name, "not_held"));
            continue;
        };
        if !room.complete && room.layers.values().all(|count| *count <= 0) {
            fetch.push(whole(name, "empty_room"));
            continue;
        }
        // A moved revision invalidates every layer of the held copy, so this is
        // the one case that is still a whole room. A copy whose own revision is
        // unknown is NOT stale on that basis: absence of evidence is not
        // evidence, and refetching on it is what made a locally-written room
        // download itself on every display.
        if let (Some(server), Some(local)) = (head.and_then(|head| head.version), room.version) {
            if server > 0 && local > 0 && server > local {
                fetch.push(whole(name, "head_moved"));
                continue;
            }
        }
        if required.is_empty() {
            if room_is_fresh(room, head) {
                reuse.push(Reuse {
                    name: name.clone(),
                    reason: "held",
                });
                continue;
            }
            if let Some(inventory) = head.and_then(|head| head.layers.as_ref()) {
                // A transformer point proves neither poles nor custom design
                // classes. The saved inventory also repairs historical caches
                // that were incorrectly marked complete after a partial read.
                let held_counts: BTreeMap<String, i64> = room
                    .layers
                    .iter()
                    .map(|(name, count)| (layer_key(name), *count))
                    .collect();
                let missing: Vec<String> = inventory
                    .iter()
                    .filter(|(name, count)| {
                        **count > 0
                            && held_counts.get(&layer_key(name)).copied().unwrap_or(0) < **count
                    })
                    .map(|(name, _)| name.clone())
                    .collect();
                if missing.is_empty() {
                    fetch.push(whole(name, "class_inventory_moved"));
                } else {
                    fetch.push(Fetch {
                        name: name.clone(),
                        layers: missing,
                        whole_room: false,
                        reason: "missing_layers",
                    });
                }
            } else if room.complete {
                reuse.push(Reuse {
                    name: name.clone(),
                    reason: "held",
                });
            } else {
                // No inventory can prove which classes a legacy partial copy
                // omitted. Read it whole once; the executor records that proof.
                fetch.push(whole(name, "incomplete_room"));
            }
            continue;
        }
        let missing = missing_layers(room, &required);
        if missing.is_empty() {
            reuse.push(Reuse {
                name: name.clone(),
                reason: "held",
            });
        } else if room.complete {
            // The whole room was read and these layers were not in it. They do
            // not exist on the server; asking again would download the room
            // forever, which is the loop this owner exists to end.
            reuse.push(Reuse {
                name: name.clone(),
                reason: "absent_on_server",
            });
        } else {
            fetch.push(Fetch {
                name: name.clone(),
                layers: missing,
                whole_room: false,
                reason: "missing_layers",
            });
        }
    }

    let mut grouped: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    for item in &fetch {
        grouped
            .entry(item.layers.clone())
            .or_default()
            .push(item.name.clone());
    }
    let mut chunks: Vec<Chunk> = Vec::new();
    for (layers, names) in grouped {
        for window in names.chunks(batch) {
            chunks.push(Chunk {
                layers: layers.clone(),
                names: window.to_vec(),
            });
        }
    }

    let rooms_fetched = fetch.iter().filter(|item| item.whole_room).count();
    let layers_fetched: usize = fetch.iter().map(|item| item.layers.len()).sum();
    Ok(Plan {
        schema: SCHEMA,
        op: "plan",
        counts: PlanCounts {
            pinned: names.len(),
            reused: reuse.len(),
            rooms_fetched,
            layers_fetched,
            requests: chunks.len(),
        },
        reuse,
        fetch,
        chunks,
        projection: CONTEXT_PROJECTION,
    })
}

fn whole(name: &str, reason: &'static str) -> Fetch {
    Fetch {
        name: name.to_owned(),
        layers: Vec::new(),
        whole_room: true,
        reason,
    }
}

/// The required classes this held room cannot paint, alias-resolved. A layer
/// present but empty is missing: an empty `tr` never marked a room complete.
fn missing_layers(room: &HeldRoom, required: &[String]) -> Vec<String> {
    let held: BTreeSet<String> = room
        .layers
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(name, _)| layer_key(name))
        .collect();
    required
        .iter()
        .filter(|wanted| !held.contains(&layer_key(wanted)))
        .cloned()
        .collect()
}

/// `canonicalName` in `$lib/api/design`: trimmed, lowercased, everything that
/// is not `[a-z0-9_]` becomes `_`.
fn canonical(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The identity of a design class: its canonical name, with the anchor's
/// spellings folded together so a room whose marker is spelled `transformers`
/// is the same room as one spelling it `tr`.
///
/// Folding compares the COMPACTED form (letters and digits only, so
/// `transformer_point` meets `transformerpoint` in the alias list) but the
/// answer is the canonical name, underscores and all: this key names the merged
/// layer, and `lv_lines` must stay `lv_lines` or nothing downstream — the
/// design layer catalogue, the style ref, the humanised label — can match it.
fn layer_key(value: &str) -> String {
    let canonical = canonical(value);
    let compact: String = canonical
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if ANCHOR_ALIASES.contains(&compact.as_str()) {
        return ANCHOR_LAYER.to_owned();
    }
    canonical
}

// ── The keep-set's source: which document paints a pinned class ─────────

/// The style document a pinned row of each named class is painted with, and
/// therefore the fields that row must keep.
///
/// A pinned layer deliberately has no style ref of its own (`sketchStyleRef`
/// returns null for `sourceKind: 'pinned'`), so the document is always the
/// design class's: its `_pinned` variant where the project publishes one, the
/// plain design document otherwise. Matching is on the ref's LAST segment,
/// canonicalised, because the target half (`master/`, `ds_grid/`, …) is the
/// publisher's, not the class's.
pub fn styles(request: StylesRequest) -> Result<Styles, String> {
    check_schema(&request.schema)?;
    let published = request
        .catalog
        .get("styles")
        .and_then(Value::as_object)
        .ok_or("The style catalogue publishes no styles.")?;

    // ref → its last segment, canonicalised, and whether it is a pinned variant.
    let mut plain: BTreeMap<String, &str> = BTreeMap::new();
    let mut variant: BTreeMap<String, &str> = BTreeMap::new();
    for reference in published.keys() {
        let leaf = reference.rsplit('/').next().unwrap_or(reference);
        let key = canonical(leaf);
        match key.strip_suffix(PINNED_STYLE_SUFFIX) {
            Some(class) if !class.is_empty() => {
                variant.insert(layer_key(class), reference.as_str());
            }
            _ => {
                plain.insert(layer_key(&key), reference.as_str());
            }
        }
    }

    let base: BTreeSet<String> = request
        .keep
        .iter()
        .map(|field| field.trim().to_owned())
        .filter(|field| !field.is_empty())
        .collect();

    let mut classes: Vec<ClassStyle> = Vec::new();
    let mut keep: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for raw in &request.classes {
        let class_name = layer_key(raw);
        if class_name.is_empty() || !seen.insert(class_name.clone()) {
            continue;
        }
        let pinned_variant = variant.contains_key(&class_name);
        let reference = variant
            .get(&class_name)
            .or_else(|| plain.get(&class_name))
            .copied();
        let document = reference
            .and_then(|reference| published.get(reference))
            .cloned()
            .unwrap_or(Value::Null);
        let mut fields = base.clone();
        collect_fields(&document, &mut fields);
        let fields: Vec<String> = fields.into_iter().collect();
        let mut kept = fields.clone();
        kept.push(PINNED_TRANSFORMER_PROPERTY.to_owned());
        kept.sort();
        kept.dedup();
        keep.insert(class_name.clone(), kept);
        classes.push(ClassStyle {
            class_name,
            style_ref: reference.map(str::to_owned),
            pinned_variant,
            fields,
            document,
        });
    }

    Ok(Styles {
        schema: SCHEMA,
        op: "styles",
        classes,
        keep,
    })
}

// ── 2 + 3. One layer per (source, class), carrying only what is read ────

pub fn merge(request: MergeRequest) -> Result<Merged, String> {
    check_schema(&request.schema)?;
    let source = request
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(PINNED_SOURCE_NAME)
        .to_owned();

    let natures = request.transformer_natures.as_ref().map(|values| {
        values
            .iter()
            .map(|(name, value)| (canonical(name), value))
            .collect::<BTreeMap<_, _>>()
    });
    let hidden: BTreeSet<String> = request.hidden.iter().map(|name| canonical(name)).collect();

    // The keep-set, per class: what that class's style document reads, plus
    // whatever the caller named, plus the transformer identity every merged
    // feature must carry.
    let mut keep_by_class: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let base: BTreeSet<String> = request
        .keep
        .iter()
        .map(|field| field.trim().to_owned())
        .filter(|field| !field.is_empty())
        .collect();
    for (class_name, document) in &request.styles {
        let mut fields = base.clone();
        collect_fields(document, &mut fields);
        keep_by_class.insert(layer_key(class_name), fields);
    }

    let mut order: Vec<String> = Vec::new();
    let mut by_class: BTreeMap<String, MergedLayer> = BTreeMap::new();
    let mut ids: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut pairs = 0usize;
    let mut seen_before: BTreeSet<String> = BTreeSet::new();
    let mut seen_after: BTreeSet<String> = BTreeSet::new();
    let mut hidden_count = 0usize;

    for room in &request.rooms {
        if hidden.contains(&canonical(&room.name)) {
            hidden_count += 1;
            continue;
        }
        for (raw_class, collection) in &room.layers {
            let features = collection
                .get("features")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if features.is_empty() {
                continue;
            }
            let class_name = layer_key(raw_class);
            if class_name.is_empty() {
                return Err("A pinned room carries a layer with no name.".into());
            }
            pairs += 1;
            let keep = keep_by_class
                .get(&class_name)
                .cloned()
                .unwrap_or_else(|| base.clone());
            if !by_class.contains_key(&class_name) {
                order.push(class_name.clone());
                by_class.insert(
                    class_name.clone(),
                    MergedLayer {
                        class_name: class_name.clone(),
                        source_name: source.clone(),
                        transformers: Vec::new(),
                        geometry_type: "Point",
                        feature_count: 0,
                        fields: Vec::new(),
                        features: Vec::new(),
                    },
                );
                ids.insert(class_name.clone(), BTreeSet::new());
            }
            let slug = slug(&room.name);
            let taken = ids.get_mut(&class_name).expect("ids entry");
            let layer = by_class.get_mut(&class_name).expect("layer entry");
            if !layer.transformers.iter().any(|name| name == &room.name) {
                layer.transformers.push(room.name.clone());
            }
            for (index, feature) in features.iter().enumerate() {
                let Some(geometry) = feature.get("geometry") else {
                    continue;
                };
                if geometry.is_null() {
                    continue;
                }
                let properties = feature.get("properties").and_then(Value::as_object);
                for key in properties.into_iter().flat_map(Map::keys) {
                    seen_before.insert(key.clone());
                }
                let mut kept: BTreeMap<String, String> = BTreeMap::new();
                for field in &keep {
                    if field == PINNED_TRANSFORMER_PROPERTY {
                        continue;
                    }
                    if class_name == ANCHOR_LAYER && field == "transformer_nature" {
                        if let Some(natures) = &natures {
                            if let Some(value) = natures.get(&canonical(&room.name)) {
                                kept.insert(field.clone(), (*value).clone());
                                seen_after.insert(field.clone());
                            }
                            continue;
                        }
                    }
                    if let Some(value) = properties.and_then(|bag| bag.get(field)) {
                        kept.insert(field.clone(), scalar(value));
                        seen_after.insert(field.clone());
                    }
                }
                // Identity is a property now, because the layer is no longer
                // one transformer's: a popup, a table row and a hide choice
                // all read it.
                kept.insert(
                    PINNED_TRANSFORMER_PROPERTY.to_owned(),
                    room.name.trim().to_owned(),
                );
                seen_after.insert(PINNED_TRANSFORMER_PROPERTY.to_owned());

                let base_id = feature
                    .get("id")
                    .map(scalar)
                    .filter(|value| !value.is_empty())
                    .or_else(|| {
                        properties
                            .and_then(|bag| bag.get("id").or_else(|| bag.get("doc_id")))
                            .map(scalar)
                            .filter(|value| !value.is_empty())
                    })
                    .unwrap_or_else(|| index.to_string());
                // Two rooms can hold the same doc id. Namespacing by the
                // transformer is what makes a merged selection unambiguous.
                let mut id = format!("{slug}:{base_id}");
                if taken.contains(&id) {
                    id = format!("{id}-{index}");
                }
                taken.insert(id.clone());
                layer.geometry_type = geometry_type(geometry, layer.geometry_type);
                layer.features.push(MergedFeature {
                    id,
                    geometry: geometry.clone(),
                    properties: kept,
                });
            }
            layer.feature_count = layer.features.len();
        }
    }

    let mut layers: Vec<MergedLayer> = Vec::new();
    for class_name in order {
        let mut layer = by_class.remove(&class_name).expect("ordered layer");
        if layer.features.is_empty() {
            continue;
        }
        let mut fields: BTreeSet<String> = BTreeSet::new();
        for feature in &layer.features {
            for key in feature.properties.keys() {
                fields.insert(key.clone());
            }
        }
        layer.fields = fields.into_iter().collect();
        layers.push(layer);
    }

    let features: usize = layers.iter().map(|layer| layer.feature_count).sum();
    let properties_before = seen_before.len();
    let properties_after = seen_after.len();
    Ok(Merged {
        schema: SCHEMA,
        op: "merge",
        source,
        totals: MergeTotals {
            rooms: request.rooms.len(),
            hidden: hidden_count,
            sources_before: pairs,
            layers_before: pairs,
            sources_after: layers.len(),
            layers_after: layers.len(),
            features,
            properties_before,
            properties_after,
            properties_dropped: properties_before.saturating_sub(
                seen_before
                    .iter()
                    .filter(|key| seen_after.contains(*key))
                    .count(),
            ),
        },
        layers,
    })
}

/// Every property name a style document reads (the print contract owns the walk).
pub use ds_report_contracts::style_document::referenced_fields;
use ds_report_contracts::style_document::collect_fields;

/// `stringifyProperty` in `pinning/materialize.ts`: a scalar as written, a
/// structure as its JSON. The renderer reads strings.
fn scalar(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// The class's geometry, read from the first feature that names one. A room
/// never mixes roles, so the first answer is the answer.
fn geometry_type(geometry: &Value, current: &'static str) -> &'static str {
    match geometry.get("type").and_then(Value::as_str).unwrap_or("") {
        "LineString" | "MultiLineString" => "LineString",
        "Polygon" | "MultiPolygon" => "Polygon",
        "Point" | "MultiPoint" => "Point",
        _ => current,
    }
}

/// A readable, stable half of a merged feature id.
fn slug(value: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in value.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "pinned".to_owned()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan_request(value: Value) -> PlanRequest {
        serde_json::from_value(value).expect("plan request")
    }

    fn merge_request(value: Value) -> MergeRequest {
        serde_json::from_value(value).expect("merge request")
    }

    fn point(id: &str, properties: Value) -> Value {
        json!({
            "type": "Feature",
            "id": id,
            "geometry": {"type": "Point", "coordinates": [30.0, -1.9]},
            "properties": properties,
        })
    }

    #[test]
    fn governed_nature_is_projected_for_the_view_without_mutating_saved_rooms() {
        let rooms = json!([{ "name":"tx", "layers":{"tr":{"features":[point("p", json!({"transformer_nature":"old"}))]}} }]);
        let run = |natures: Value| {
            merge(merge_request(json!({"schema":SCHEMA,"rooms":rooms,
            "keep":["transformer_nature"],"transformer_natures":natures})))
            .unwrap()
        };
        assert_eq!(
            run(json!({"tx":"upgrade"})).layers[0].features[0].properties["transformer_nature"],
            "upgrade"
        );
        assert!(!run(json!({})).layers[0].features[0]
            .properties
            .contains_key("transformer_nature"));
        assert_eq!(
            run(Value::Null).layers[0].features[0].properties["transformer_nature"],
            "old"
        );
        assert_eq!(
            rooms[0]["layers"]["tr"]["features"][0]["properties"]["transformer_nature"],
            "old"
        );
    }

    // ── 1. what to fetch ────────────────────────────────────────────────

    #[test]
    fn changed_inventory_and_out_of_order_observations_do_not_bless_stale_rooms() {
        let request = plan_request(serde_json::json!({
            "schema":SCHEMA,"pins":["t"], "held":[{"name":"t","version":7,"complete":true,"layers":{"lv_poles":4,"removed":2}}],
            "heads":[{"name":"t","version":8,"layers":{"lv_poles":2}},{"name":"t","version":7,"layers":{"lv_poles":4,"removed":2}}]
        }));
        assert_eq!(plan(request).unwrap().fetch[0].reason, "head_moved");
        let request = plan_request(serde_json::json!({
            "schema":SCHEMA,"pins":["t"], "held":[{"name":"t","version":7,"complete":true,"layers":{"lv_poles":4,"removed":2}}],
            "heads":[{"name":"t","version":7,"layers":{"lv_poles":2}}]
        }));
        assert_eq!(
            plan(request).unwrap().fetch[0].reason,
            "class_inventory_moved"
        );
        let request = plan_request(
            serde_json::json!({"schema":SCHEMA,"pins":["t"],"discard_local_edits":true,"held":[{"name":"t","dirty":true}]}),
        );
        assert_eq!(
            plan(request).unwrap().fetch[0].reason,
            "discard_local_edits"
        );
    }

    #[test]
    fn shared_freshness_preserves_dirty_empty_and_newer_rooms() {
        let decode = |value| serde_json::from_value::<HeldRoom>(value).unwrap();
        let head: Head = serde_json::from_value(
            serde_json::json!({"name":"t", "version":7, "layers":{"lv_poles":999}}),
        )
        .unwrap();
        let newer = decode(
            serde_json::json!({"name":"t", "version":8, "complete":true, "layers":{"lv_poles":4}}),
        );
        assert!(room_is_fresh(&newer, Some(&head)));
        let dirty = decode(serde_json::json!({"name":"t", "dirty":true}));
        assert!(room_is_fresh(&dirty, Some(&head)));
        let empty = decode(serde_json::json!({"name":"t", "complete":true}));
        assert!(room_is_fresh(&empty, None));
        let request: PlanRequest = serde_json::from_value(serde_json::json!({
            "schema":SCHEMA,"pins":["t"],"force":true,"held":[{"name":"t","dirty":true}]
        }))
        .unwrap();
        let plan = plan(request).unwrap();
        assert!(plan.fetch.is_empty());
        assert_eq!(plan.reuse[0].reason, "local_edits");
    }

    #[test]
    fn a_room_missing_one_layer_asks_for_that_layer_not_the_room() {
        let answer = plan(plan_request(json!({
            "schema": SCHEMA,
            "pins": ["T1"],
            "require": ["tr", "customers"],
            "held": [{"name": "T1", "layers": {"customers": 40}}],
        })))
        .expect("plan");
        assert_eq!(
            answer.fetch,
            vec![Fetch {
                name: "T1".into(),
                layers: vec!["tr".into()],
                whole_room: false,
                reason: "missing_layers",
            }]
        );
        // The historic "pinning loads only customers" partial cache is still
        // not displayable — it just no longer costs the whole room.
        assert!(answer.reuse.is_empty());
        assert_eq!(answer.counts.rooms_fetched, 0);
        assert_eq!(answer.counts.layers_fetched, 1);
    }

    #[test]
    fn a_moved_head_still_forces_a_whole_room_fetch() {
        let answer = plan(plan_request(json!({
            "schema": SCHEMA,
            "pins": ["T1", "T2"],
            "held": [
                {"name": "T1", "layers": {"tr": 1}, "version": 6},
                {"name": "T2", "layers": {"tr": 1}, "version": 7, "complete": true},
            ],
            "heads": [{"name": "T1", "version": 7}, {"name": "T2", "version": 7}],
        })))
        .expect("plan");
        assert_eq!(
            answer.fetch,
            vec![Fetch {
                name: "T1".into(),
                layers: vec![],
                whole_room: true,
                reason: "head_moved",
            }]
        );
        assert_eq!(
            answer.reuse,
            vec![Reuse {
                name: "T2".into(),
                reason: "held"
            }]
        );
    }

    #[test]
    fn a_held_copy_with_no_known_revision_is_not_stale_on_that_basis() {
        // This is the tier-3 room that downloaded itself on every display.
        let answer = plan(plan_request(json!({
            "schema": SCHEMA,
            "pins": ["T1"],
            "held": [{"name": "T1", "layers": {"tr": 1}, "complete": true}],
            "heads": [{"name": "T1", "version": 7}],
        })))
        .expect("plan");
        assert!(answer.fetch.is_empty());
        assert_eq!(answer.reuse[0].reason, "held");
    }

    #[test]
    fn an_anchor_spelled_differently_is_still_an_anchor() {
        let answer = plan(plan_request(json!({
            "schema": SCHEMA,
            "pins": ["T1"],
            "require": ["tr"],
            "held": [{"name": "T1", "layers": {"Transformers": 1, "customers": 3}}],
        })))
        .expect("plan");
        assert!(answer.fetch.is_empty(), "{:?}", answer.fetch);
    }

    #[test]
    fn a_room_read_whole_without_the_anchor_is_never_asked_for_again() {
        let answer = plan(plan_request(json!({
            "schema": SCHEMA,
            "pins": ["T1"],
            "require": ["tr"],
            "held": [{"name": "T1", "layers": {"customers": 4}, "complete": true}],
        })))
        .expect("plan");
        assert!(answer.fetch.is_empty());
        assert_eq!(answer.reuse[0].reason, "absent_on_server");
    }

    #[test]
    fn all_classes_repairs_missing_poles_and_custom_classes_despite_legacy_complete_flag() {
        let answer = plan(plan_request(json!({
            "schema": SCHEMA,
            "pins": ["partial", "unchanged"],
            "held": [
                {"name": "partial", "version": 7, "complete": true, "layers": {"tr": 1, "lv_lines": 20}},
                {"name": "unchanged", "version": 7, "layers": {"tr": 1, "lv_poles": 25}}
            ],
            "heads": [
                {"name": "partial", "version": 7, "layers": {"tr": 1, "lv_lines": 20, "lv_poles": 25, "custom_notes": 2, "empty": 0}},
                {"name": "unchanged", "version": 7, "layers": {"tr": 1, "lv_poles": 25}}
            ]
        }))).expect("plan");
        assert_eq!(
            answer.fetch,
            vec![Fetch {
                name: "partial".into(),
                layers: vec!["custom_notes".into(), "lv_poles".into()],
                whole_room: false,
                reason: "missing_layers"
            }]
        );
        assert_eq!(answer.reuse[0].name, "unchanged");
        assert_eq!(answer.counts.requests, 1);
        assert_eq!(answer.counts.rooms_fetched, 0);
    }

    #[test]
    fn all_classes_without_inventory_reads_a_legacy_partial_room_whole_once() {
        for complete in [false, true] {
            let answer = plan(plan_request(json!({
                "schema": SCHEMA, "pins": ["T1"],
                "held": [{"name": "T1", "layers": {"tr": 1}, "complete": complete}],
                "heads": [{"name": "T1", "version": 7}]
            })))
            .expect("plan");
            if complete {
                assert!(answer.fetch.is_empty());
            } else {
                assert_eq!(answer.fetch[0], whole("T1", "incomplete_room"));
            }
        }
    }

    #[test]
    fn a_class_with_only_some_of_its_saved_features_is_not_complete() {
        let answer = plan(plan_request(json!({
            "schema": SCHEMA, "pins": ["T1"],
            "held": [{"name": "T1", "layers": {"tr": 1, "lv_poles": 4}}],
            "heads": [{"name": "T1", "layers": {"tr": 1, "lv_poles": 25}}]
        })))
        .expect("plan");
        assert_eq!(answer.fetch[0].layers, vec!["lv_poles"]);
    }

    #[test]
    fn force_asks_for_every_room_and_the_focus_is_never_context() {
        let answer = plan(plan_request(json!({
            "schema": SCHEMA,
            "pins": ["T1", "T2"],
            "focus": "T2",
            "force": true,
            "held": [{"name": "T1", "layers": {"tr": 1}}],
        })))
        .expect("plan");
        assert_eq!(answer.counts.pinned, 1);
        assert_eq!(answer.fetch.len(), 1);
        assert!(answer.fetch[0].whole_room);
        assert_eq!(answer.fetch[0].reason, "forced");
    }

    #[test]
    fn identical_layer_requests_batch_together_under_the_cap() {
        let pins: Vec<String> = (0..1_100).map(|i| format!("T{i}")).collect();
        let answer = plan(plan_request(json!({
            "schema": SCHEMA,
            "pins": pins,
            "batch": 500,
        })))
        .expect("plan");
        assert_eq!(answer.counts.pinned, 1_100);
        assert_eq!(answer.chunks.len(), 3);
        assert_eq!(answer.counts.requests, 3);
        assert!(answer.chunks.iter().all(|chunk| chunk.layers.is_empty()));
        assert_eq!(answer.projection, "context");
    }

    #[test]
    fn a_plan_refuses_a_foreign_schema() {
        let error = plan(plan_request(json!({"schema": "ds.other/v1", "pins": []}))).unwrap_err();
        assert!(error.contains("ds.pinned-context/v1"), "{error}");
    }

    // ── 2. one layer per (source, class) ────────────────────────────────

    #[test]
    fn fifty_rooms_of_six_classes_become_six_layers() {
        let rooms: Vec<Value> = (0..50)
            .map(|i| {
                let layers: Map<String, Value> = ["tr", "customers", "lv_poles", "lv_lines", "spans", "service_cables"]
                    .iter()
                    .map(|class| {
                        (
                            (*class).to_owned(),
                            json!({"type": "FeatureCollection", "features": [point(&format!("f{i}"), json!({"name": "x"}))]}),
                        )
                    })
                    .collect();
                json!({"name": format!("T{i}"), "layers": layers})
            })
            .collect();
        let answer =
            merge(merge_request(json!({"schema": SCHEMA, "rooms": rooms}))).expect("merge");
        assert_eq!(answer.totals.sources_before, 300);
        assert_eq!(answer.totals.layers_before, 300);
        assert_eq!(answer.totals.sources_after, 6);
        assert_eq!(answer.totals.layers_after, 6);
        assert_eq!(answer.totals.features, 300);
        // Every merged feature can still name its transformer.
        let customers = answer
            .layers
            .iter()
            .find(|layer| layer.class_name == "customers")
            .expect("customers");
        assert_eq!(customers.transformers.len(), 50);
        assert_eq!(
            customers.features[7].properties[PINNED_TRANSFORMER_PROPERTY],
            "T7"
        );
        assert_eq!(customers.source_name, PINNED_SOURCE_NAME);
    }

    #[test]
    fn two_rooms_sharing_a_document_id_keep_separate_features() {
        let answer = merge(merge_request(json!({
            "schema": SCHEMA,
            "rooms": [
                {"name": "T 1", "layers": {"customers": {"type": "FeatureCollection", "features": [point("doc-9", json!({}))]}}},
                {"name": "T 2", "layers": {"customers": {"type": "FeatureCollection", "features": [point("doc-9", json!({}))]}}},
            ],
        })))
        .expect("merge");
        let ids: Vec<&str> = answer.layers[0]
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .collect();
        assert_eq!(ids, vec!["t-1:doc-9", "t-2:doc-9"]);
    }

    #[test]
    fn a_hidden_transformer_contributes_nothing() {
        let answer = merge(merge_request(json!({
            "schema": SCHEMA,
            "hidden": ["T2"],
            "rooms": [
                {"name": "T1", "layers": {"customers": {"type": "FeatureCollection", "features": [point("a", json!({}))]}}},
                {"name": "T2", "layers": {"customers": {"type": "FeatureCollection", "features": [point("b", json!({}))]}}},
            ],
        })))
        .expect("merge");
        assert_eq!(answer.totals.features, 1);
        assert_eq!(answer.totals.hidden, 1);
        assert_eq!(answer.layers[0].transformers, vec!["T1".to_owned()]);
    }

    #[test]
    fn a_line_room_reports_its_geometry() {
        let answer = merge(merge_request(json!({
            "schema": SCHEMA,
            "rooms": [{"name": "T1", "layers": {"lv_lines": {"type": "FeatureCollection", "features": [
                {"type": "Feature", "geometry": {"type": "LineString", "coordinates": [[30.0, -1.9], [30.1, -1.8]]}, "properties": {}}
            ]}}}],
        })))
        .expect("merge");
        assert_eq!(answer.layers[0].geometry_type, "LineString");
    }

    // ── 3. only the attributes the style reads ──────────────────────────

    #[test]
    fn a_pinned_feature_keeps_only_what_the_style_document_reads() {
        let mut bag = Map::new();
        for key in [
            "network_properties",
            "computed_properties",
            "desirable_properties",
            "surveyed_properties",
            "geohash",
            "meter_number",
            "phone",
            "speed",
            "precedent",
            "antecedent",
        ] {
            bag.insert(key.to_owned(), json!("junk"));
        }
        bag.insert("existing_status".into(), json!("existing"));
        bag.insert("struct_type".into(), json!("wood"));
        bag.insert("name".into(), json!("C-1"));
        let before = bag.len();
        assert_eq!(before, 13);

        let style = json!({
            "type": "circle",
            "paint": {
                "circle-color": ["match", ["coalesce", ["get", "existing_status"], ["get", "status"]], "existing", "#0f0", "#f00"],
                "circle-stroke-color": ["case", ["has", "struct_type"], "#fff", "#000"],
            },
            "label": {"layout": {"text-field": ["concat", ["get", "name"], " kVA"]}},
        });
        let answer = merge(merge_request(json!({
            "schema": SCHEMA,
            "styles": {"customers": style},
            "rooms": [{"name": "T1", "layers": {"customers": {"type": "FeatureCollection", "features": [
                {"type": "Feature", "id": "c1", "geometry": {"type": "Point", "coordinates": [30.0, -1.9]}, "properties": bag}
            ]}}}],
        })))
        .expect("merge");

        let feature = &answer.layers[0].features[0];
        // name, existing_status, struct_type — and the transformer identity.
        // `status` is read by the coalesce but this feature does not carry it.
        assert_eq!(
            feature.properties.keys().cloned().collect::<Vec<_>>(),
            vec![
                "existing_status".to_owned(),
                "name".to_owned(),
                PINNED_TRANSFORMER_PROPERTY.to_owned(),
                "struct_type".to_owned(),
            ]
        );
        assert_eq!(answer.totals.properties_before, 13);
        assert_eq!(answer.totals.properties_after, 4);
        assert_eq!(answer.totals.properties_dropped, 10);
    }

    #[test]
    fn a_class_with_no_style_document_keeps_only_what_the_caller_named() {
        let answer = merge(merge_request(json!({
            "schema": SCHEMA,
            "keep": ["name"],
            "rooms": [{"name": "T1", "layers": {"spans": {"type": "FeatureCollection", "features": [
                point("s1", json!({"name": "S", "network_properties": {"a": 1}}))
            ]}}}],
        })))
        .expect("merge");
        assert_eq!(
            answer.layers[0].fields,
            vec!["name".to_owned(), PINNED_TRANSFORMER_PROPERTY.to_owned()]
        );
    }

    #[test]
    fn the_identity_property_is_the_kernels_even_when_the_data_carries_one() {
        let answer = merge(merge_request(json!({
            "schema": SCHEMA,
            "keep": [PINNED_TRANSFORMER_PROPERTY],
            "rooms": [{"name": "T1", "layers": {"customers": {"type": "FeatureCollection", "features": [
                point("c1", json!({PINNED_TRANSFORMER_PROPERTY: "WRONG"}))
            ]}}}],
        })))
        .expect("merge");
        assert_eq!(
            answer.layers[0].features[0].properties[PINNED_TRANSFORMER_PROPERTY],
            "T1"
        );
    }

    #[test]
    fn a_structure_value_survives_as_its_json_when_the_style_reads_it() {
        let answer = merge(merge_request(json!({
            "schema": SCHEMA,
            "keep": ["tags"],
            "rooms": [{"name": "T1", "layers": {"customers": {"type": "FeatureCollection", "features": [
                point("c1", json!({"tags": ["a", "b"]}))
            ]}}}],
        })))
        .expect("merge");
        assert_eq!(
            answer.layers[0].features[0].properties["tags"],
            "[\"a\",\"b\"]"
        );
    }

    #[test]
    fn the_wire_entry_answers_both_operations_and_refuses_anything_else() {
        let plan = evaluate(
            json!({"schema": SCHEMA, "op": "plan", "pins": ["T1"]})
                .to_string()
                .as_bytes(),
        )
        .expect("plan");
        assert!(plan.contains("\"op\":\"plan\""));
        let merged = evaluate(
            json!({"schema": SCHEMA, "op": "merge", "rooms": []})
                .to_string()
                .as_bytes(),
        )
        .expect("merge");
        assert!(merged.contains("\"op\":\"merge\""));
        let styles = evaluate(
            json!({"schema": SCHEMA, "op": "styles", "catalog": {"styles": {}}, "classes": []})
                .to_string()
                .as_bytes(),
        )
        .expect("styles");
        assert!(styles.contains("\"op\":\"styles\""));
        let error = evaluate(
            json!({"schema": SCHEMA, "op": "paint"})
                .to_string()
                .as_bytes(),
        )
        .unwrap_err();
        assert!(error.contains("malformed"), "{error}");
    }

    fn catalog() -> Value {
        json!({"styles": {
            "master/customers": {
                "type": "circle",
                "paint": {"circle-color": ["match", ["get", "existing_status"], "new", "#f00", "#888"]},
            },
            "master/customers_pinned": {
                "type": "circle",
                "paint": {"circle-color": "#64748b"},
                "label": {"layout": {"text-field": ["coalesce", ["get", "name"], ["get", "customer_name"]]}},
            },
            "master/lv_lines": {
                "type": "line",
                "paint": {"line-color": ["match", ["get", "cable_size"], "50", "#0f0", "#333"]},
            },
        }})
    }

    /// The merged layer's class name is what the rest of the system matches on
    /// — the design layer catalogue's key, the style ref's leaf, the humanised
    /// label. Compacting it to fold anchor aliases turned `lv_lines` into
    /// `lvlines`, which matched nothing anywhere.
    #[test]
    fn a_merged_class_keeps_its_canonical_name_underscores_and_all() {
        let merged = merge(merge_request(json!({
            "schema": SCHEMA,
            "rooms": [{"name": "T1", "layers": {
                "lv_lines": {"features": [{
                    "geometry": {"type": "LineString", "coordinates": [[0, 0], [1, 1]]},
                    "properties": {},
                }]},
                "unconnected_customers": {"features": [point("c1", json!({}))]},
                "TRANSFORMERS": {"features": [point("t1", json!({}))]},
            }}],
        })))
        .expect("merge");

        let names: Vec<&str> = merged
            .layers
            .iter()
            .map(|layer| layer.class_name.as_str())
            .collect();
        // Underscores survive; only the ANCHOR's spellings are folded.
        assert!(names.contains(&"lv_lines"), "{names:?}");
        assert!(names.contains(&"unconnected_customers"), "{names:?}");
        assert!(names.contains(&"tr"), "{names:?}");
    }

    /// The variant is the point: a project that published a quiet grey pinned
    /// document must not be painted with the loud design one.
    #[test]
    fn a_pinned_class_is_resolved_to_its_pinned_variant_when_one_is_published() {
        let answer = styles(
            serde_json::from_value(json!({
                "schema": SCHEMA, "catalog": catalog(),
                "classes": ["customers", "lv_lines", "spans"],
            }))
            .expect("request"),
        )
        .expect("styles");

        let customers = &answer.classes[0];
        assert_eq!(
            customers.style_ref.as_deref(),
            Some("master/customers_pinned")
        );
        assert!(customers.pinned_variant);
        // The variant reads name/customer_name; `existing_status` belongs to the
        // DESIGN document and must not leak into a pinned keep-set.
        assert_eq!(customers.fields, vec!["customer_name", "name"]);

        let lines = &answer.classes[1];
        assert_eq!(lines.style_ref.as_deref(), Some("master/lv_lines"));
        assert!(!lines.pinned_variant);
        assert_eq!(lines.fields, vec!["cable_size"]);

        // A class the catalogue never published keeps nothing but its identity.
        let spans = &answer.classes[2];
        assert_eq!(spans.style_ref, None);
        assert!(spans.fields.is_empty());
        assert_eq!(
            answer.keep["spans"],
            vec![PINNED_TRANSFORMER_PROPERTY.to_owned()]
        );
    }

    /// The resolved keep-set is the merge's input, not a second opinion: what
    /// `styles` says to keep is exactly what `merge` then keeps.
    #[test]
    fn the_resolved_keep_set_is_what_the_merge_keeps() {
        let resolved = styles(
            serde_json::from_value(json!({
                "schema": SCHEMA, "catalog": catalog(), "classes": ["customers"],
            }))
            .expect("request"),
        )
        .expect("styles");
        let document = resolved.classes[0].document.clone();

        let merged = merge(merge_request(json!({
            "schema": SCHEMA,
            "styles": {"customers": document},
            "rooms": [{"name": "T1", "layers": {"customers": {"features": [
                point("c1", json!({
                    "name": "Ada", "customer_name": "Ada L", "existing_status": "new",
                    "network_properties": {"a": 1}, "geohash": "xyz",
                })),
            ]}}}],
        })))
        .expect("merge");

        assert_eq!(merged.layers[0].fields, resolved.keep["customers"]);
        assert_eq!(
            merged.layers[0].fields,
            vec!["customer_name", "name", PINNED_TRANSFORMER_PROPERTY]
        );
    }
}
