//! Owner 2026-10-23: a repeated lv_poles identity is resolved by renumbering
//! one exact, digest-pinned feature. Nothing but its member id moves.
use ds_command_kernel::{
    design::{self, Snapshot},
    design_repair::*,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn scope() -> Scope {
    Scope {
        project: "project".into(),
        principal: "designer".into(),
        lane: "stable".into(),
        audience: "audience".into(),
    }
}
fn point(id: &str, x: f64, properties: Value) -> Value {
    json!({"type":"Feature","id":id,"geometry":{"type":"Point","coordinates":[x,-1.5]},"properties":properties})
}
fn fc(features: Vec<Value>) -> Value {
    json!({"type":"FeatureCollection","features":features})
}
/// Pole "14" and pole "16" are each held twice; the later ones are renumbered.
fn snapshot(customers: Vec<Value>) -> Snapshot {
    serde_json::from_value(
        json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326",
        "layers":{"lv_poles":fc(vec![
            point("14", 30.001, json!({"struct_type":"S140","assembly_type":""})),
            point("15", 30.002, json!({"struct_type":"S190","assembly_type":""})),
            point("16", 30.003, json!({"struct_type":"S140"})),
            point("14", 30.004, json!({"struct_type":"S140","assembly_type":""})),
            point("16", 30.005, json!({"struct_type":"S255"})),
        ]),
        "customers":fc(customers)},
        "settings":{},"network_config":{"sheets":{}},"sources":[],"include_design_customers":true}),
    )
    .unwrap()
}
fn customer(properties: Value) -> Value {
    point("c1", 30.0041, properties)
}
fn digest_of(target: &Snapshot, index: usize) -> String {
    design::digest(&target.layers["lv_poles"]["features"][index]).unwrap()
}
fn renumber(target: &Snapshot, id: &str, index: usize, new_id: &str) -> RepairOperation {
    RepairOperation::Renumber {
        target_layer: "lv_poles".into(),
        target_id: id.into(),
        before_feature_digest: digest_of(target, index),
        new_id: new_id.into(),
    }
}
fn request(target: &Snapshot, operations: Vec<RepairOperation>) -> ProposalRequest {
    let rule = FieldRule {
        field_type: FieldType::String,
        nullable: false,
        required: true,
        allowed_values: vec![],
        unit: None,
    };
    ProposalRequest {
        scope: scope(),
        target: TargetFence {
            snapshot_revision: design::digest(target).unwrap(),
            server_version: None,
            layers_content_digest: None,
        },
        policy: AuditPolicy {
            scope: scope(),
            fields: BTreeMap::from([(
                "lv_poles".into(),
                BTreeMap::from([("id".into(), rule.clone()), ("assembly_type".into(), rule)]),
            )]),
            references: vec![],
            connectivity: None,
            max_details: 100,
        },
        allowlist: BTreeMap::from([(
            "lv_poles".into(),
            BTreeSet::from(["id".into(), "assembly_type".into()]),
        )]),
        operations,
    }
}
fn selection(proposal: &Proposal, ids: Vec<String>) -> ApplySelection {
    ApplySelection {
        scope: scope(),
        proposal_digest: proposal.digest.clone(),
        change_ids: ids,
        confirmed: true,
    }
}
fn all(proposal: &Proposal) -> Vec<String> {
    proposal.changes.iter().map(|c| c.id.clone()).collect()
}
fn code<T: std::fmt::Debug>(value: Result<T, RepairError>) -> String {
    value.unwrap_err().code
}

#[test]
fn renumber_gives_each_pinned_repeated_pole_a_new_id_and_nothing_else_moves() {
    let target = snapshot(vec![customer(json!({"pole_number":""}))]);
    let request = request(
        &target,
        vec![
            renumber(&target, "14", 3, "99"),
            renumber(&target, "16", 4, "100"),
        ],
    );
    let proposal = propose(None, &target, &request).unwrap();
    assert_eq!(proposal.changes.len(), 2);
    for change in &proposal.changes {
        assert_eq!(change.field, "id");
    }
    assert_eq!(
        proposal.changes[0].before,
        Cell::Value { value: json!("14") }
    );
    assert_eq!(
        proposal.changes[0].after,
        Cell::Value { value: json!("99") }
    );
    // The caller's evaluate wire spells the operation kind "renumber".
    let wire = serde_json::to_value(&request.operations[0]).unwrap();
    assert_eq!(wire["kind"], "renumber");
    let applied = apply_selected(
        None,
        &target,
        &proposal,
        &selection(&proposal, all(&proposal)),
    )
    .unwrap();
    assert_eq!(applied.changed_cells, 2);
    let mut expected = serde_json::to_value(&target).unwrap();
    expected["layers"]["lv_poles"]["features"][3]["id"] = json!("99");
    expected["layers"]["lv_poles"]["features"][4]["id"] = json!("100");
    assert_eq!(
        serde_json::to_value(&applied.snapshot).unwrap(),
        expected,
        "only the pinned member ids change"
    );
    applied
        .snapshot
        .validate_retained()
        .expect("the renumbered head has unique identities");

    // A selected subset may leave the other repeat for a later renumber.
    let one = vec![proposal.changes[0].id.clone()];
    let partial = apply_selected(None, &target, &proposal, &selection(&proposal, one)).unwrap();
    assert_eq!(
        partial.snapshot.layers["lv_poles"]["features"][3]["id"],
        "99"
    );
    assert_eq!(
        partial.snapshot.layers["lv_poles"]["features"][4]["id"],
        "16"
    );

    // After the renumber the poles are repairable cell by cell again.
    let repaired = request_after(&applied.snapshot);
    let cells = propose(None, &applied.snapshot, &repaired).unwrap();
    assert_eq!(cells.changes.len(), 1);
    assert_eq!(cells.changes[0].feature_id, "99");
}

fn request_after(target: &Snapshot) -> ProposalRequest {
    request(
        target,
        vec![RepairOperation::Assign {
            target_layer: "lv_poles".into(),
            target_ids: vec!["99".into()],
            values: BTreeMap::from([(
                "assembly_type".into(),
                Cell::Value {
                    value: json!("Terminal"),
                },
            )]),
        }],
    )
}

#[test]
fn renumber_refuses_everything_but_one_exact_repeated_member_id() {
    let target = snapshot(vec![customer(json!({"pole_number":""}))]);
    let refuse = |operations: Vec<RepairOperation>| {
        code(propose(None, &target, &request(&target, operations)))
    };

    // Without a renumber the repeated identity still refuses the whole snapshot.
    assert_eq!(
        code(propose(None, &target, &request_after(&target))),
        "repair_target_invalid"
    );
    // Only a repeated identity is renumbered.
    assert_eq!(
        refuse(vec![renumber(&target, "15", 1, "99")]),
        "repair_renumber_refused"
    );
    // The pin selects one exact feature; a moved feature refuses.
    let mut moved = renumber(&target, "14", 3, "99");
    if let RepairOperation::Renumber {
        before_feature_digest,
        ..
    } = &mut moved
    {
        *before_feature_digest = "0".repeat(64);
    }
    assert_eq!(refuse(vec![moved]), "repair_target_changed");
    // The new identity must be unused, in the layer and across the request.
    assert_eq!(
        refuse(vec![renumber(&target, "14", 3, "15")]),
        "repair_renumber_refused"
    );
    assert_eq!(
        refuse(vec![
            renumber(&target, "14", 3, "99"),
            renumber(&target, "16", 4, "99")
        ]),
        "repair_renumber_refused"
    );
    assert_eq!(
        refuse(vec![renumber(&target, "14", 3, " 99")]),
        "repair_renumber_refused"
    );
    // One renumber per repeated identity.
    assert_eq!(
        refuse(vec![
            renumber(&target, "14", 3, "99"),
            renumber(&target, "14", 0, "98")
        ]),
        "repair_change_ambiguous"
    );
    // Only lv_poles, and only with id on the reviewed allowlist and policy.
    let mut tapping = renumber(&target, "14", 3, "99");
    if let RepairOperation::Renumber { target_layer, .. } = &mut tapping {
        *target_layer = "tapping_poles".into();
    }
    assert_eq!(refuse(vec![tapping]), "repair_field_not_allowed");
    let mut narrowed = request(&target, vec![renumber(&target, "14", 3, "99")]);
    narrowed.allowlist.get_mut("lv_poles").unwrap().remove("id");
    assert_eq!(
        code(propose(None, &target, &narrowed)),
        "repair_field_not_allowed"
    );
    // An ordinary assignment can never write an identity, allowlisted or not.
    let unique = snapshot(vec![]);
    let mut unique_layers = unique.clone();
    unique_layers.layers.get_mut("lv_poles").unwrap()["features"]
        .as_array_mut()
        .unwrap()
        .truncate(3);
    let assign_id = request(
        &unique_layers,
        vec![RepairOperation::Assign {
            target_layer: "lv_poles".into(),
            target_ids: vec!["15".into()],
            values: BTreeMap::from([("id".into(), Cell::Value { value: json!("99") })]),
        }],
    );
    assert_eq!(
        code(propose(None, &unique_layers, &assign_id)),
        "repair_field_not_allowed"
    );

    // A customer reference to the repeated id cannot say which pole it means.
    let referenced = snapshot(vec![customer(json!({"connection_pole_id":"14"}))]);
    assert_eq!(
        code(propose(
            None,
            &referenced,
            &request(&referenced, vec![renumber(&referenced, "14", 3, "99")])
        )),
        "repair_renumber_refused"
    );
    // A second identity path would stay repeated after the member id moves.
    let mut aliased = snapshot(vec![]);
    aliased.layers.get_mut("lv_poles").unwrap()["features"][3]["properties"]["doc_id"] =
        json!("14");
    assert_eq!(
        code(propose(
            None,
            &aliased,
            &request(&aliased, vec![renumber(&aliased, "14", 3, "99")])
        )),
        "repair_renumber_refused"
    );
    // Byte-identical repeats cannot be told apart.
    let mut identical = snapshot(vec![]);
    let copy = identical.layers["lv_poles"]["features"][0].clone();
    identical.layers.get_mut("lv_poles").unwrap()["features"][3] = copy;
    assert_eq!(
        code(propose(
            None,
            &identical,
            &request(&identical, vec![renumber(&identical, "14", 3, "99")])
        )),
        "repair_identity_ambiguous"
    );
    // A forged after value is not the projection of the recorded request.
    let request = request(&target, vec![renumber(&target, "14", 3, "99")]);
    let mut forged = propose(None, &target, &request).unwrap();
    forged.changes[0].after = Cell::Value { value: json!("15") };
    assert_eq!(
        code(apply_selected(
            None,
            &target,
            &forged,
            &selection(&forged, all(&forged))
        )),
        "repair_proposal_changed"
    );
}
