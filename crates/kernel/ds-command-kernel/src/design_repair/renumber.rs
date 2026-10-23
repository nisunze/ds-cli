//! Owner 2026-10-23: a saved head that repeats a feature identity gets one of
//! the repeated features a new, unused identity. The exact feature is pinned by
//! its reviewed digest, so no ordinal or coordinate chooses it; only the member
//! `id` moves. Everything else about the snapshot stays byte-equal.
use super::{error, hash, identities, layer, CellChange, ProposalRequest, RepairError};
use super::{Cell, MAX_CHANGES};
use crate::design::{self, Snapshot};
use ds_geo_lite::identity::geojson_feature_identity_candidates;
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Customer fields that name a pole by its id. A reference to a repeated id
/// cannot say which pole it means, so the renumber refuses it.
const POLE_REFERENCE_FIELDS: [&str; 2] = ["connection_pole_id", "connection_override_pole_id"];

/// The only layer whose member identity a renumber admits.
pub fn repair_identity_renumber_layer(layer: &str) -> bool {
    layer == "lv_poles"
}

fn refused(message: &str) -> RepairError {
    error("repair_renumber_refused", message)
}

/// `id` is admitted on the reviewed allowlist and policy only for a renumber.
pub(super) fn identity_rule(request: &ProposalRequest, layer: &str) -> Result<(), RepairError> {
    let admitted = repair_identity_renumber_layer(layer)
        && request
            .allowlist
            .get(layer)
            .is_some_and(|fields| fields.contains("id"))
        && request
            .policy
            .fields
            .get(layer)
            .is_some_and(|fields| fields.contains_key("id"));
    if admitted {
        Ok(())
    } else {
        Err(error(
            "repair_field_not_allowed",
            format!("{layer}.id is renumbered only on lv_poles with a reviewed id allowlist and policy."),
        ))
    }
}

fn reference_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_owned()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

pub(super) struct Renumber<'a> {
    pub target_layer: &'a str,
    pub target_id: &'a str,
    pub before_feature_digest: &'a str,
    pub new_id: &'a str,
}

pub(super) fn propose(
    changes: &mut Vec<CellChange>,
    seen: &mut BTreeSet<(String, String, String)>,
    new_ids: &mut BTreeSet<(String, String)>,
    request: &ProposalRequest,
    target: &Snapshot,
    op: Renumber<'_>,
) -> Result<(), RepairError> {
    identity_rule(request, op.target_layer)?;
    let features = layer(&target.layers, op.target_layer)?;
    let index = identities(features);
    let rows = index.get(op.target_id).ok_or_else(|| {
        error(
            "repair_identity_required",
            "The identity to renumber is absent.",
        )
    })?;
    if rows.len() < 2 {
        return Err(refused("Only a repeated identity is renumbered."));
    }
    let mut pinned = Vec::new();
    for &row in rows {
        if hash(&features[row])? == op.before_feature_digest {
            pinned.push(row);
        }
    }
    let feature = match pinned.as_slice() {
        [row] => &features[*row],
        [] => {
            return Err(error(
                "repair_target_changed",
                "No feature holding the repeated identity has the reviewed digest.",
            ))
        }
        _ => {
            return Err(error(
                "repair_identity_ambiguous",
                "Byte-identical features share the identity; a renumber cannot choose one.",
            ))
        }
    };
    let old = feature.get("id").cloned().unwrap_or(Value::Null);
    let only_member = geojson_feature_identity_candidates(feature).len() == 1
        && ["id", "doc_id", "feature_id"]
            .iter()
            .all(|alias| feature["properties"].get(alias).is_none());
    if !only_member || reference_text(&old).as_deref() != Some(op.target_id) {
        return Err(refused(
            "A renumber requires the member id to be the feature's only identity.",
        ));
    }
    let after = match &old {
        Value::String(_) => {
            design::identifier(op.new_id)
                .map_err(|_| refused("A new identity must be an exact bounded identifier."))?;
            json!(op.new_id)
        }
        _ => op
            .new_id
            .parse::<u64>()
            .ok()
            .filter(|n| *n <= 9_007_199_254_740_991)
            .map(Value::from)
            .ok_or_else(|| refused("A numeric identity is renumbered to a safe integer."))?,
    };
    if op.new_id == op.target_id
        || index.contains_key(op.new_id)
        || !new_ids.insert((op.target_layer.to_owned(), op.new_id.to_owned()))
    {
        return Err(refused("The new identity is already used in the layer."));
    }
    let mut reference_fields: Vec<(Option<&str>, &str)> =
        POLE_REFERENCE_FIELDS.iter().map(|f| (None, *f)).collect();
    for reference in &request.policy.references {
        if reference.target_field == "id"
            && reference.target_layers.iter().any(|l| l == op.target_layer)
        {
            reference_fields.push((Some(reference.layer.as_str()), reference.field.as_str()));
        }
    }
    for (name, collection) in &target.layers {
        for row in super::rows(collection)? {
            for (only, field) in &reference_fields {
                if only.is_some_and(|l| l != name) {
                    continue;
                }
                if row["properties"]
                    .get(*field)
                    .and_then(reference_text)
                    .is_some_and(|text| text == op.target_id)
                {
                    return Err(refused(
                        "A reference names the repeated identity, so the feature it means is ambiguous.",
                    ));
                }
            }
        }
    }
    if !seen.insert((
        op.target_layer.to_owned(),
        op.target_id.to_owned(),
        "id".into(),
    )) {
        return Err(error(
            "repair_change_ambiguous",
            "Two operations renumber the same repeated identity.",
        ));
    }
    if changes.len() >= MAX_CHANGES {
        return Err(error(
            "repair_bound_exceeded",
            "Proposal exceeds 5000 changed cells; narrow the explicit scope.",
        ));
    }
    let before = Cell::Value { value: old };
    let after = Cell::Value { value: after };
    let before_feature_digest = op.before_feature_digest.to_owned();
    let id = hash(&json!([
        op.target_layer,
        op.target_id,
        before_feature_digest,
        "id",
        before,
        after,
        Value::Null
    ]))?;
    changes.push(CellChange {
        id,
        layer: op.target_layer.into(),
        feature_id: op.target_id.into(),
        before_feature_digest,
        field: "id".into(),
        before,
        after,
        evidence: None,
    });
    Ok(())
}
