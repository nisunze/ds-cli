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
        lane: "canary".into(),
        audience: "https://canary.example".into(),
    }
}
fn point(id: Value, x: f64, properties: Value) -> Value {
    json!({"type":"Feature","id":id,"geometry":{"type":"Point","coordinates":[x,-2.0]},"properties":properties})
}
fn fc(features: Vec<Value>) -> Value {
    json!({"type":"FeatureCollection","features":features})
}
fn snapshot() -> Snapshot {
    serde_json::from_value(json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326",
        "layers":{"tr":fc(vec![point(json!("tr1"),30.0,json!({"tr_size":0,"plan_kva":160,"drafting_status":"approved"}))]),
        "lv_poles":fc(vec![point(json!("p1"),30.001,json!({"struct_type":"S140","stay":"stay_1","drafting_status":"approved","untouched":{"source":"retained"}})),
            point(json!("p2"),30.002,json!({"struct_type":"S140","stay":null,"drafting_status":"approved"})),
            point(json!("p3"),30.003,json!({"struct_type":"S190","drafting_status":"approved"}))])},
        "settings":{"size_transformer":false},"network_config":{"sheets":{"known_columns":[]}},"sources":[],"include_design_customers":true})).unwrap()
}
fn evidence() -> EvidenceSource {
    let layers = BTreeMap::from([(
        "historical_poles".into(),
        fc(vec![
            point(
                json!("old-a"),
                30.001,
                json!({"struct_type":"S255","stay":"stay_2"}),
            ),
            point(
                json!("old-b"),
                30.002,
                json!({"struct_type":"S140","stay":"stay_1"}),
            ),
            point(
                json!("unmatched-historical"),
                30.004,
                json!({"struct_type":"S255"}),
            ),
        ]),
    )]);
    EvidenceSource {
        schema: "ds.repair.evidence/v1".into(),
        id: "historical-file".into(),
        scope: scope(),
        content_digest: design::digest(&layers).unwrap(),
        raw_file_sha256: Some("a".repeat(64)),
        revision: Some("april-captured".into()),
        crs: Some("EPSG:4326".into()),
        units: BTreeMap::new(),
        layers,
    }
}
fn text_rule() -> FieldRule {
    FieldRule {
        field_type: FieldType::String,
        nullable: true,
        required: false,
        allowed_values: vec![],
        unit: None,
    }
}
fn policy() -> AuditPolicy {
    AuditPolicy {
        scope: scope(),
        fields: BTreeMap::from([
            (
                "lv_poles".into(),
                BTreeMap::from([
                    ("struct_type".into(), text_rule()),
                    ("stay".into(), text_rule()),
                    ("drafting_status".into(), text_rule()),
                ]),
            ),
            (
                "tr".into(),
                BTreeMap::from([(
                    "tr_size".into(),
                    FieldRule {
                        field_type: FieldType::Number,
                        ..text_rule()
                    },
                )]),
            ),
        ]),
        references: vec![],
        connectivity: None,
        max_details: 100,
    }
}
fn fields() -> Vec<FieldMapping> {
    vec![
        FieldMapping {
            source: "struct_type".into(),
            target: "struct_type".into(),
            remove_if_source_missing: false,
        },
        FieldMapping {
            source: "stay".into(),
            target: "stay".into(),
            remove_if_source_missing: false,
        },
    ]
}
fn matching() -> MatchSpec {
    MatchSpec::ExactPosition {
        crs: "EPSG:4326".into(),
        dimensions: PositionDimensions::Xy,
    }
}
fn request(target: &Snapshot) -> ProposalRequest {
    ProposalRequest {
        scope: scope(),
        target: TargetFence {
            snapshot_revision: design::digest(target).unwrap(),
            server_version: None,
            layers_content_digest: None,
        },
        policy: policy(),
        allowlist: BTreeMap::from([(
            "lv_poles".into(),
            BTreeSet::from([
                "struct_type".into(),
                "stay".into(),
                "drafting_status".into(),
            ]),
        )]),
        operations: vec![RepairOperation::Restore {
            source_layer: "historical_poles".into(),
            target_layer: "lv_poles".into(),
            matching: matching(),
            fields: fields(),
        }],
    }
}
fn selection(proposal: &Proposal) -> ApplySelection {
    ApplySelection {
        scope: scope(),
        proposal_digest: proposal.digest.clone(),
        change_ids: proposal
            .changes
            .iter()
            .map(|change| change.id.clone())
            .collect(),
        confirmed: true,
    }
}
fn rehash_source(source: &mut EvidenceSource) {
    source.content_digest = design::digest(&source.layers).unwrap();
}
fn rehash_proposal(proposal: &mut Proposal) {
    let mut value = serde_json::to_value(&*proposal).unwrap();
    value.as_object_mut().unwrap().remove("digest");
    proposal.digest = design::digest(&value).unwrap();
}
fn code<T: std::fmt::Debug>(value: Result<T, RepairError>) -> String {
    value.unwrap_err().code
}

#[test]
fn retained_decode_preserves_numeric_id_missing_owner_and_all_original_facts() {
    let mut target = snapshot();
    target.layers.get_mut("tr").unwrap()["features"][0]["id"] = json!(9_007_199_254_740_993_u64);
    target.layers.get_mut("lv_poles").unwrap()["features"][1]["properties"]["transfo"] =
        json!("recorded-foreign-owner");
    target.layers.get_mut("lv_poles").unwrap()["features"][2]
        .as_object_mut()
        .unwrap()
        .remove("id");
    let original = serde_json::to_vec(&target).unwrap();
    let decoded = design::decode_retained(&original).unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), original);
    assert_eq!(
        design::digest(&decoded).unwrap(),
        design::digest(&target).unwrap()
    );
    assert!(decoded.layers["tr"]["features"][0]["properties"]
        .get("transfo")
        .is_none());
    assert!(decoded.layers["lv_poles"]["features"][2]
        .get("id")
        .is_none());
    assert_eq!(
        decoded.layers["lv_poles"]["features"][1]["properties"]["transfo"],
        "recorded-foreign-owner"
    );
}

#[test]
fn retained_decode_refuses_corrupt_geometry_duplicate_typed_ids_and_bad_source_digest() {
    let mut target = snapshot();
    target.layers.get_mut("tr").unwrap()["features"][0]["geometry"]["coordinates"] =
        json!([190, 0]);
    assert!(design::decode_retained(&serde_json::to_vec(&target).unwrap()).is_err());
    let mut target = snapshot();
    target.layers.get_mut("lv_poles").unwrap()["features"][1]["id"] = json!("p1");
    assert!(design::decode_retained(&serde_json::to_vec(&target).unwrap()).is_err());
    let mut target = snapshot();
    target.layers.get_mut("lv_poles").unwrap()["features"][0]["id"] = json!(1);
    target.layers.get_mut("lv_poles").unwrap()["features"][1]["id"] = json!("1");
    assert!(
        design::decode_retained(&serde_json::to_vec(&target).unwrap()).is_ok(),
        "typed GeoJSON IDs retain original number/string values"
    );
    for bad in [Value::Null, json!(true), json!({"id":"bad"})] {
        let mut target = snapshot();
        target.layers.get_mut("tr").unwrap()["features"][0]["id"] = bad;
        assert!(design::decode_retained(&serde_json::to_vec(&target).unwrap()).is_err());
    }
    let mut value = serde_json::to_value(snapshot()).unwrap();
    value["sources"] =
        json!([{"id":"survey","role":"customers","sha256":"wrong","collection":fc(vec![])}]);
    assert!(design::decode_retained(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn exact_position_restoration_preserves_approved_geometry_counts_inputs_and_unmatched_rows() {
    let target = snapshot();
    let source = evidence();
    let original = serde_json::to_value(&target).unwrap();
    let proposal = propose(Some(&source), &target, &self::request(&target)).unwrap();
    assert_eq!(proposal.changes.len(), 3);
    assert_eq!(
        proposal.match_counts[0],
        MatchCounts {
            source_rows: 3,
            target_rows: 3,
            matched: 2,
            unmatched_source: 1,
            unmatched_target: 1,
            ambiguous_source: 0,
            ambiguous_target: 0
        }
    );
    let applied = apply_selected(Some(&source), &target, &proposal, &selection(&proposal)).unwrap();
    assert_eq!(
        applied.snapshot.layers["lv_poles"]["features"][0]["properties"]["struct_type"],
        "S255"
    );
    assert_eq!(
        applied.snapshot.layers["lv_poles"]["features"][0]["properties"]["stay"],
        "stay_2"
    );
    assert_eq!(
        applied.snapshot.layers["lv_poles"]["features"][1]["properties"]["stay"],
        "stay_1"
    );
    assert_eq!(
        applied.snapshot.layers["lv_poles"]["features"][2],
        target.layers["lv_poles"]["features"][2]
    );
    assert_eq!(applied.snapshot.layers["tr"], target.layers["tr"]);
    assert_eq!(applied.snapshot.settings, target.settings);
    assert!(applied.preserved_outside_selection);
    assert!(!applied.persisted);
    assert_eq!(
        serde_json::to_value(&target).unwrap(),
        original,
        "pure operation cannot mutate its input"
    );
    for (before, after) in target.layers["lv_poles"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .zip(
            applied.snapshot.layers["lv_poles"]["features"]
                .as_array()
                .unwrap(),
        )
    {
        assert_eq!(before["geometry"], after["geometry"]);
        assert_eq!(before["id"], after["id"]);
        assert_eq!(
            before["properties"]["drafting_status"],
            after["properties"]["drafting_status"]
        );
        assert_eq!(
            before["properties"].get("transfo"),
            after["properties"].get("transfo")
        );
    }
}

#[test]
fn explicit_subset_apply_keeps_other_proposed_cells_byte_equal_and_retry_is_deterministic() {
    let target = snapshot();
    let source = evidence();
    let proposal = propose(Some(&source), &target, &self::request(&target)).unwrap();
    let mut selected = selection(&proposal);
    selected.change_ids = vec![proposal
        .changes
        .iter()
        .find(|change| change.feature_id == "p1" && change.field == "struct_type")
        .unwrap()
        .id
        .clone()];
    let applied = apply_selected(Some(&source), &target, &proposal, &selected).unwrap();
    assert_eq!(applied.changed_cells, 1);
    assert_eq!(
        applied.snapshot.layers["lv_poles"]["features"][0]["properties"]["stay"],
        "stay_1"
    );
    assert_eq!(
        applied.snapshot.layers["lv_poles"]["features"][1],
        target.layers["lv_poles"]["features"][1]
    );
    let repeated = apply_selected(Some(&source), &target, &proposal, &selected).unwrap();
    assert_eq!(
        serde_json::to_value(applied).unwrap(),
        serde_json::to_value(repeated).unwrap()
    );
}

#[test]
fn position_duplicates_never_choose_order_or_nearest_and_keep_ambiguity_counts() {
    let target = snapshot();
    let mut source = evidence();
    source.layers.get_mut("historical_poles").unwrap()["features"][1]["geometry"] =
        source.layers["historical_poles"]["features"][0]["geometry"].clone();
    rehash_source(&mut source);
    assert_eq!(
        code(propose(Some(&source), &target, &self::request(&target))),
        "repair_match_ambiguous"
    );
    let read = compare(
        &source,
        &target,
        &CompareRequest {
            scope: scope(),
            source_layer: "historical_poles".into(),
            target_layer: "lv_poles".into(),
            matching: matching(),
            fields: fields(),
            max_details: 0,
        },
    )
    .unwrap();
    assert_eq!(read.counts.ambiguous_source, 2);
    assert_eq!(read.counts.ambiguous_target, 1);
    assert_eq!(read.counts.matched, 0);
    let mut source = evidence();
    source.layers.get_mut("historical_poles").unwrap()["features"][0]["geometry"]["coordinates"]
        [0] = json!(30.00100001);
    rehash_source(&mut source);
    let proposal = propose(Some(&source), &target, &self::request(&target)).unwrap();
    assert!(
        proposal
            .changes
            .iter()
            .all(|change| change.feature_id != "p1"),
        "tiny nonzero separation is not the same position"
    );
}

#[test]
fn scalar_keys_support_table_rows_without_geometry_and_preserve_types() {
    let source = vec![
        json!({"properties":{"key":0,"value":"zero"}}),
        json!({"properties":{"key":false}}),
        json!({"properties":{"key":"0"}}),
        json!({"properties":{"key":null}}),
        json!({"properties":{}}),
        json!({"properties":{"key":9_007_199_254_740_993_u64}}),
    ];
    let target = vec![
        source[2].clone(),
        source[0].clone(),
        source[1].clone(),
        source[5].clone(),
    ];
    let spec = MatchSpec::Keys {
        pairs: vec![KeyPair {
            source: "key".into(),
            target: "key".into(),
        }],
    };
    let result = match_records(&source, &target, &spec, None, None).unwrap();
    assert_eq!(result.pairs, vec![[0, 1], [1, 2], [2, 0], [5, 3]]);
    assert_eq!(result.unmatched_source, vec![3, 4]);
    let duplicate = vec![target[0].clone(), target[0].clone()];
    let result = match_records(&source, &duplicate, &spec, None, None).unwrap();
    assert_eq!(result.counts.ambiguous_source, 1);
    assert_eq!(result.counts.ambiguous_target, 2);
    assert_eq!(result.counts.matched, 0);
}

#[test]
fn explicit_key_matching_works_independently_of_positions_or_feature_ids() {
    let mut target = snapshot();
    target.layers.get_mut("lv_poles").unwrap()["features"][0]["properties"]["review_key"] =
        json!("a");
    let mut source = evidence();
    source.layers.get_mut("historical_poles").unwrap()["features"][0]["properties"]
        ["external_key"] = json!("a");
    source.layers.get_mut("historical_poles").unwrap()["features"][0]["geometry"]["coordinates"]
        [0] = json!(31.0);
    rehash_source(&mut source);
    let mut request = self::request(&target);
    if let RepairOperation::Restore { matching, .. } = &mut request.operations[0] {
        *matching = MatchSpec::Keys {
            pairs: vec![KeyPair {
                source: "external_key".into(),
                target: "review_key".into(),
            }],
        };
    }
    let proposal = propose(Some(&source), &target, &request).unwrap();
    assert_eq!(proposal.match_counts[0].matched, 1);
    assert_eq!(proposal.changes.len(), 2);
    assert!(proposal
        .changes
        .iter()
        .all(|change| change.feature_id == "p1"));
    let applied = apply_selected(Some(&source), &target, &proposal, &selection(&proposal)).unwrap();
    assert_eq!(
        applied.snapshot.layers["lv_poles"]["features"][0]["geometry"],
        target.layers["lv_poles"]["features"][0]["geometry"]
    );
}

#[test]
fn missing_and_null_stay_distinct_and_missing_copy_requires_explicit_request() {
    assert_ne!(Cell::Missing, Cell::Value { value: Value::Null });
    let target = snapshot();
    let mut source = evidence();
    source.layers.get_mut("historical_poles").unwrap()["features"][0]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("stay");
    rehash_source(&mut source);
    let proposal = propose(Some(&source), &target, &self::request(&target)).unwrap();
    assert!(proposal
        .changes
        .iter()
        .all(|change| !(change.feature_id == "p1" && change.field == "stay")));
    let mut request = self::request(&target);
    if let RepairOperation::Restore { fields, .. } = &mut request.operations[0] {
        fields[1].remove_if_source_missing = true;
    }
    let proposal = propose(Some(&source), &target, &request).unwrap();
    assert!(proposal
        .changes
        .iter()
        .any(|change| change.feature_id == "p1"
            && change.field == "stay"
            && change.after == Cell::Missing));
    let applied = apply_selected(Some(&source), &target, &proposal, &selection(&proposal)).unwrap();
    assert!(
        applied.snapshot.layers["lv_poles"]["features"][0]["properties"]
            .get("stay")
            .is_none()
    );
    source.layers.get_mut("historical_poles").unwrap()["features"][0]["properties"]["stay"] =
        Value::Null;
    rehash_source(&mut source);
    let proposal = propose(Some(&source), &target, &request).unwrap();
    assert!(proposal
        .changes
        .iter()
        .any(|change| change.feature_id == "p1"
            && change.field == "stay"
            && change.after == Cell::Value { value: Value::Null }));
}

#[test]
fn approval_assignment_has_its_own_scope_including_unmatched_poles() {
    let mut target = snapshot();
    target.layers.get_mut("lv_poles").unwrap()["features"][2]["properties"]["drafting_status"] =
        json!("draft");
    let source = evidence();
    let mut request = self::request(&target);
    request.operations.push(RepairOperation::Assign {
        target_layer: "lv_poles".into(),
        target_ids: vec!["p3".into()],
        values: BTreeMap::from([(
            "drafting_status".into(),
            Cell::Value {
                value: json!("approved"),
            },
        )]),
    });
    let proposal = propose(Some(&source), &target, &request).unwrap();
    assert!(proposal
        .changes
        .iter()
        .any(|change| change.feature_id == "p3"
            && change.field == "drafting_status"
            && change.evidence.is_none()));
    let applied = apply_selected(Some(&source), &target, &proposal, &selection(&proposal)).unwrap();
    assert_eq!(
        applied.snapshot.layers["lv_poles"]["features"][2]["properties"]["drafting_status"],
        "approved"
    );
    assert_eq!(
        applied.snapshot.layers["lv_poles"]["features"][2]["properties"]["struct_type"],
        "S190"
    );
}

#[test]
fn assignment_without_external_source_is_explicit_and_cannot_demote() {
    let target = snapshot();
    let mut request = self::request(&target);
    request.operations = vec![RepairOperation::Assign {
        target_layer: "lv_poles".into(),
        target_ids: vec!["p1".into()],
        values: BTreeMap::from([(
            "struct_type".into(),
            Cell::Value {
                value: json!("S190"),
            },
        )]),
    }];
    let proposal = propose(None, &target, &request).unwrap();
    assert_eq!(proposal.source_digest, None);
    assert_eq!(
        apply_selected(None, &target, &proposal, &selection(&proposal))
            .unwrap()
            .changed_cells,
        1
    );
    request.operations = vec![RepairOperation::Assign {
        target_layer: "lv_poles".into(),
        target_ids: vec!["p1".into()],
        values: BTreeMap::from([(
            "drafting_status".into(),
            Cell::Value {
                value: json!("draft"),
            },
        )]),
    }];
    assert_eq!(
        code(propose(None, &target, &request)),
        "repair_value_invalid"
    );
}

#[test]
fn unconfirmed_empty_duplicate_or_unknown_selection_refuses_without_projection() {
    let target = snapshot();
    let source = evidence();
    let proposal = propose(Some(&source), &target, &self::request(&target)).unwrap();
    let original = design::digest(&target).unwrap();
    let mut selected = selection(&proposal);
    selected.confirmed = false;
    assert_eq!(
        code(apply_selected(Some(&source), &target, &proposal, &selected)),
        "repair_effect_unconfirmed"
    );
    selected.confirmed = true;
    selected.change_ids.clear();
    assert_eq!(
        code(apply_selected(Some(&source), &target, &proposal, &selected)),
        "repair_selection_invalid"
    );
    selected.change_ids = vec![proposal.changes[0].id.clone(); 2];
    assert_eq!(
        code(apply_selected(Some(&source), &target, &proposal, &selected)),
        "repair_selection_invalid"
    );
    selected.change_ids = vec!["unknown".into()];
    assert_eq!(
        code(apply_selected(Some(&source), &target, &proposal, &selected)),
        "repair_selection_invalid"
    );
    assert_eq!(design::digest(&target).unwrap(), original);
}

#[test]
fn changed_source_target_scope_or_proposal_refuses_even_with_recomputed_hash() {
    let target = snapshot();
    let source = evidence();
    let proposal = propose(Some(&source), &target, &self::request(&target)).unwrap();
    let selected = selection(&proposal);
    let mut stale_target = target.clone();
    stale_target
        .settings
        .insert("size_transformer".into(), json!(true));
    assert_eq!(
        code(apply_selected(
            Some(&source),
            &stale_target,
            &proposal,
            &selected
        )),
        "repair_target_changed"
    );
    let mut stale_source = source.clone();
    stale_source.layers.get_mut("historical_poles").unwrap()["features"][0]["properties"]
        ["struct_type"] = json!("S190");
    assert_eq!(
        code(apply_selected(
            Some(&stale_source),
            &target,
            &proposal,
            &selected
        )),
        "repair_source_changed"
    );
    rehash_source(&mut stale_source);
    assert_eq!(
        code(apply_selected(
            Some(&stale_source),
            &target,
            &proposal,
            &selected
        )),
        "repair_proposal_changed"
    );
    let mut metadata_changed = source.clone();
    metadata_changed.raw_file_sha256 = Some("b".repeat(64));
    assert_eq!(
        code(apply_selected(
            Some(&metadata_changed),
            &target,
            &proposal,
            &selected
        )),
        "repair_proposal_changed"
    );
    let mut wrong_scope = selected.clone();
    wrong_scope.scope.project = "other-project".into();
    assert_eq!(
        code(apply_selected(
            Some(&source),
            &target,
            &proposal,
            &wrong_scope
        )),
        "repair_scope_mismatch"
    );
    let mut forged = proposal.clone();
    forged.changes[0].after = Cell::Value {
        value: json!("invented"),
    };
    rehash_proposal(&mut forged);
    let mut forged_selection = selected.clone();
    forged_selection.proposal_digest = forged.digest.clone();
    assert_eq!(
        code(apply_selected(
            Some(&source),
            &target,
            &forged,
            &forged_selection
        )),
        "repair_proposal_changed"
    );
    let mut forged = proposal.clone();
    forged.target_after_revision = "f".repeat(64);
    rehash_proposal(&mut forged);
    let mut forged_selection = selected;
    forged_selection.proposal_digest = forged.digest.clone();
    assert_eq!(
        code(apply_selected(
            Some(&source),
            &target,
            &forged,
            &forged_selection
        )),
        "repair_proposal_changed"
    );
}

#[test]
fn strict_field_catalogue_rejects_identity_system_fields_unknown_fields_and_hidden_normalization() {
    let target = snapshot();
    for field in [
        "transfo",
        "doc_id",
        "id",
        "feature_id",
        "geometry",
        "drafting_locked_properties",
        // existing_status itself joined the catalogue (owner, 2026-10-23).
        "existing_status_detail",
        "_ds_source_file",
        "unknown_fact",
    ] {
        let mut request = self::request(&target);
        request
            .allowlist
            .get_mut("lv_poles")
            .unwrap()
            .insert(field.into());
        request
            .policy
            .fields
            .get_mut("lv_poles")
            .unwrap()
            .insert(field.into(), text_rule());
        request.operations = vec![RepairOperation::Assign {
            target_layer: "lv_poles".into(),
            target_ids: vec!["p1".into()],
            values: BTreeMap::from([(
                field.into(),
                Cell::Value {
                    value: json!("changed"),
                },
            )]),
        }];
        assert_eq!(
            code(propose(None, &target, &request)),
            "repair_field_not_allowed",
            "{field}"
        );
    }
    let mut request = self::request(&target);
    request
        .allowlist
        .get_mut("lv_poles")
        .unwrap()
        .remove("struct_type");
    assert_eq!(
        code(propose(Some(&evidence()), &target, &request)),
        "repair_field_not_allowed"
    );
}

#[test]
fn identity_alias_collisions_and_legacy_idless_targets_refuse_mutation() {
    let mut target = snapshot();
    target.layers.get_mut("lv_poles").unwrap()["features"][1]["properties"]["doc_id"] = json!("p1");
    assert_eq!(
        code(propose(Some(&evidence()), &target, &self::request(&target))),
        "repair_identity_ambiguous"
    );
    let mut target = snapshot();
    target.layers.get_mut("lv_poles").unwrap()["features"][0]
        .as_object_mut()
        .unwrap()
        .remove("id");
    assert_eq!(
        code(propose(Some(&evidence()), &target, &self::request(&target))),
        "repair_identity_required"
    );
    target.layers.get_mut("lv_poles").unwrap()["features"][0]["properties"]["doc_id"] =
        json!("source-doc-1");
    let proposal = propose(Some(&evidence()), &target, &self::request(&target)).unwrap();
    assert!(proposal
        .changes
        .iter()
        .any(|change| change.feature_id == "source-doc-1"));
    assert_eq!(
        feature_identity(&target.layers["lv_poles"]["features"][0]).unwrap(),
        FeatureIdentity {
            field: "properties.doc_id".into(),
            value: json!("source-doc-1")
        }
    );
}

#[test]
fn feature_identity_keeps_original_numeric_type_and_exact_path() {
    let mut feature = point(json!(123), 30.0, json!({"id":"123","doc_id":"document-id"}));
    assert_eq!(
        feature_identity(&feature).unwrap(),
        FeatureIdentity {
            field: "id".into(),
            value: json!(123)
        }
    );
    feature["id"] = json!("session/temporary");
    assert_eq!(
        feature_identity(&feature).unwrap(),
        FeatureIdentity {
            field: "properties.id".into(),
            value: json!("123")
        }
    );
}

#[test]
fn valid_coordinates_in_wrong_engineering_geometry_class_are_blocked_without_normalizing() {
    let mut target = snapshot();
    target.layers.insert(
        "lv_lines".into(),
        fc(vec![point(json!("line1"), 30.0, json!({}))]),
    );
    let before = serde_json::to_value(&target).unwrap();
    let report = audit(&target, &policy()).unwrap();
    assert_eq!(report.finding_counts["geometry_class_invalid"], 1);
    assert_eq!(report.blockers, 1);
    assert_eq!(serde_json::to_value(&target).unwrap(), before);
}

#[test]
fn native_catalogue_enforces_ratings_quantity_ranges_and_approval_even_if_caller_widens_policy() {
    assert!(validate_property_cell(
        "lv_poles",
        "struct_type",
        &Cell::Value {
            value: json!("é".repeat(1024))
        }
    )
    .is_ok());
    for text in ["é".repeat(1025), "A\u{0085}B".to_owned(), "A\nB".to_owned()] {
        assert_eq!(
            code(validate_property_cell(
                "lv_poles",
                "struct_type",
                &Cell::Value { value: json!(text) }
            )),
            "repair_value_invalid"
        );
    }
    for value in [
        json!(-1),
        json!("50"),
        json!(true),
        json!({"rating":50}),
        json!(9_007_199_254_740_992_u64),
    ] {
        assert_eq!(
            code(validate_property_cell(
                "tr",
                "tr_size",
                &Cell::Value { value }
            )),
            "repair_value_invalid"
        );
    }
    assert!(
        validate_property_cell("tr", "tr_size", &Cell::Value { value: json!(0) }).is_ok(),
        "zero is a saved fact, not a request to resize"
    );
    assert!(validate_property_cell(
        "customers",
        "num_houses",
        &Cell::Value { value: json!(1.5) }
    )
    .is_err());
    for cell in [
        Cell::Missing,
        Cell::Value { value: Value::Null },
        Cell::Value {
            value: json!("draft"),
        },
    ] {
        assert!(validate_property_cell("tr", "drafting_status", &cell).is_err());
    }
    let target = snapshot();
    let mut request = self::request(&target);
    request
        .allowlist
        .insert("tr".into(), BTreeSet::from(["tr_size".into()]));
    request
        .policy
        .fields
        .get_mut("tr")
        .unwrap()
        .get_mut("tr_size")
        .unwrap()
        .field_type = FieldType::Json;
    request.operations = vec![RepairOperation::Assign {
        target_layer: "tr".into(),
        target_ids: vec!["tr1".into()],
        values: BTreeMap::from([(
            "tr_size".into(),
            Cell::Value {
                value: json!("arbitrary"),
            },
        )]),
    }];
    assert_eq!(
        code(propose(None, &target, &request)),
        "repair_value_invalid"
    );
}

#[test]
fn unknown_crs_and_missing_xyz_are_not_position_evidence() {
    let target = snapshot();
    let mut source = evidence();
    source.crs = None;
    assert_eq!(
        code(propose(Some(&source), &target, &self::request(&target))),
        "repair_crs_required"
    );
    let mut source = evidence();
    source.crs = Some("EPSG:32735".into());
    assert_eq!(
        code(propose(Some(&source), &target, &self::request(&target))),
        "repair_crs_required"
    );
    let mut request = self::request(&target);
    if let RepairOperation::Restore { matching, .. } = &mut request.operations[0] {
        *matching = MatchSpec::ExactPosition {
            crs: "EPSG:4326".into(),
            dimensions: PositionDimensions::Xyz,
        };
    }
    assert_eq!(
        code(propose(Some(&evidence()), &target, &request)),
        "repair_geometry_invalid"
    );
    let mut source = evidence();
    source.layers.get_mut("historical_poles").unwrap()["features"][0]["geometry"]["coordinates"] =
        json!([30.0010000, -2]);
    rehash_source(&mut source);
    assert_eq!(
        propose(Some(&source), &target, &self::request(&target))
            .unwrap()
            .match_counts[0]
            .matched,
        2,
        "same coordinate numeric spellings match with no rounding"
    );
}

#[test]
fn units_need_explicit_same_unit_evidence_no_implicit_conversion() {
    let mut target = snapshot();
    target.layers.insert("lv_lines".into(),fc(vec![json!({"type":"Feature","id":"l1","geometry":{"type":"LineString","coordinates":[[30,-2],[30.001,-2]]},"properties":{"external":"x","length":10}})]));
    let mut source = evidence();
    source.layers.insert(
        "length_table".into(),
        fc(vec![
            json!({"type":"Feature","geometry":null,"properties":{"external":"x","length":20}}),
        ]),
    );
    rehash_source(&mut source);
    let mut request = self::request(&target);
    request.policy.fields.insert(
        "lv_lines".into(),
        BTreeMap::from([(
            "length".into(),
            FieldRule {
                field_type: FieldType::Number,
                unit: Some("m".into()),
                ..text_rule()
            },
        )]),
    );
    request
        .allowlist
        .insert("lv_lines".into(), BTreeSet::from(["length".into()]));
    request.operations = vec![RepairOperation::Restore {
        source_layer: "length_table".into(),
        target_layer: "lv_lines".into(),
        matching: MatchSpec::Keys {
            pairs: vec![KeyPair {
                source: "external".into(),
                target: "external".into(),
            }],
        },
        fields: vec![FieldMapping {
            source: "length".into(),
            target: "length".into(),
            remove_if_source_missing: false,
        }],
    }];
    assert_eq!(
        code(propose(Some(&source), &target, &request)),
        "repair_unit_mismatch"
    );
    source.units.insert(
        "length_table".into(),
        BTreeMap::from([("length".into(), "ft".into())]),
    );
    assert_eq!(
        code(propose(Some(&source), &target, &request)),
        "repair_unit_mismatch"
    );
    source
        .units
        .get_mut("length_table")
        .unwrap()
        .insert("length".into(), "m".into());
    assert_eq!(
        propose(Some(&source), &target, &request)
            .unwrap()
            .changes
            .len(),
        1
    );
}

#[test]
fn server_fences_remain_distinct_from_local_snapshot_revision() {
    let target = snapshot();
    let mut request = self::request(&target);
    request.target.server_version = Some(7);
    assert_eq!(
        code(propose(Some(&evidence()), &target, &request)),
        "repair_target_invalid"
    );
    request.target.layers_content_digest =
        Some(ds_command_kernel::report_export::jcs::layers_content_digest(&target.layers).unwrap());
    assert!(propose(Some(&evidence()), &target, &request).is_ok());
    request.target.layers_content_digest = Some("f".repeat(64));
    assert_eq!(
        code(propose(Some(&evidence()), &target, &request)),
        "repair_target_changed"
    );
}

#[test]
fn audit_reports_exact_full_counts_with_bounded_details_without_inferring_causality() {
    let mut target = snapshot();
    target.layers.get_mut("lv_poles").unwrap()["features"][0]["geometry"]["coordinates"] =
        json!([500, -2]);
    target.layers.get_mut("lv_poles").unwrap()["features"][1]["properties"]["transfo"] =
        json!("another-transformer");
    target.layers.get_mut("lv_poles").unwrap()["features"][2]
        .as_object_mut()
        .unwrap()
        .remove("id");
    let original = serde_json::to_value(&target).unwrap();
    let mut policy = policy();
    policy.max_details = 1;
    let report = audit(&target, &policy).unwrap();
    assert_eq!(report.features, 4);
    assert_eq!(report.blockers, 2);
    assert_eq!(report.candidates, 1);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.omitted_details, 2);
    assert_eq!(report.finding_counts["geometry_invalid"], 1);
    assert_eq!(report.finding_counts["owner_reference_mismatch"], 1);
    assert!(!report.mutated);
    assert_eq!(serde_json::to_value(target).unwrap(), original);
    assert!(!serde_json::to_string(&report)
        .unwrap()
        .contains("import_caused"));
}

#[test]
fn cross_transformer_colocation_is_a_candidate_not_meter_or_pole_deletion_authority() {
    let left = snapshot();
    let mut right = snapshot();
    right.transformer = "T2".into();
    let report = audit_scope(&[left, right], &policy()).unwrap();
    assert_eq!(report.transformers, 2);
    assert_eq!(report.features, 8);
    assert_eq!(report.blockers, 0);
    assert_eq!(report.finding_counts["cross_transformer_colocation"], 4);
    assert!(report
        .findings
        .iter()
        .all(|finding| finding.severity == "candidate"
            && finding.evidence["deletion_proven"] == false));
}

#[test]
fn declared_tapping_root_is_valid_and_exact_disconnection_remains_a_candidate() {
    let mut target = snapshot();
    target.layers.insert(
        "tapping_poles".into(),
        fc(vec![point(
            json!("tap-root"),
            31.0,
            json!({"existing_status":"existing"}),
        )]),
    );
    target.layers.insert("lv_lines".into(),fc(vec![json!({"type":"Feature","id":"tap-line","geometry":{"type":"LineString","coordinates":[[31,-2],[31.001,-2]]},"properties":{}}),json!({"type":"Feature","id":"review-line","geometry":{"type":"LineString","coordinates":[[32,-2],[32.001,-2]]},"properties":{}})]));
    let original = design::digest(&target).unwrap();
    let mut policy = policy();
    policy.connectivity = Some(ConnectivityPolicy {
        line_layers: vec!["lv_lines".into()],
        roots: vec![FeatureRef {
            layer: "tapping_poles".into(),
            id: "tap-root".into(),
        }],
        sources_complete: true,
    });
    let report = audit(&target, &policy).unwrap();
    assert_eq!(report.blockers, 0);
    assert_eq!(report.finding_counts["connectivity_unreached_candidate"], 1);
    assert!(report
        .findings
        .iter()
        .all(|finding| finding.feature_id.as_deref() != Some("tap-line")));
    assert_eq!(design::digest(&target).unwrap(), original);
    policy.connectivity.as_mut().unwrap().sources_complete = false;
    let report = audit(&target, &policy).unwrap();
    assert_eq!(report.finding_counts["connectivity_source_incomplete"], 1);
    assert!(!report
        .finding_counts
        .contains_key("connectivity_unreached_candidate"));
}

#[test]
fn reference_findings_do_not_delete_or_infer_unknown_external_customer() {
    let mut target = snapshot();
    target.layers.get_mut("lv_poles").unwrap()["features"][0]["properties"]["connection_pole_id"] =
        json!("survey-customer-outside-design");
    let mut policy = policy();
    policy.references = vec![ReferenceRule {
        layer: "lv_poles".into(),
        field: "connection_pole_id".into(),
        target_layers: vec!["lv_poles".into()],
        target_field: "node_id".into(),
        required: false,
    }];
    let report = audit(&target, &policy).unwrap();
    assert_eq!(report.blockers, 0);
    assert_eq!(report.finding_counts["reference_unresolved"], 1);
    assert_eq!(report.findings[0].severity, "candidate");
    assert_eq!(report.findings[0].evidence["deletion_proven"], false);
}

#[test]
fn comparison_detail_limit_cannot_hide_full_population_differences() {
    let target = snapshot();
    let source = evidence();
    let result = compare(
        &source,
        &target,
        &CompareRequest {
            scope: scope(),
            source_layer: "historical_poles".into(),
            target_layer: "lv_poles".into(),
            matching: matching(),
            fields: fields(),
            max_details: 1,
        },
    )
    .unwrap();
    assert_eq!(result.different_cells, 3);
    assert_eq!(result.differences.len(), 1);
    assert_eq!(result.omitted_details, 2);
    assert!(!result.mutated);
}

#[test]
fn dispatcher_and_generated_descriptors_share_closed_shapes_and_no_persistence_claim() {
    let target = snapshot();
    let source = evidence();
    let value = evaluate(
        "design.repair.propose",
        json!({"source":source,"target":target,"request":self::request(&target)}),
    )
    .unwrap();
    let proposal: Proposal = serde_json::from_value(value.clone()).unwrap();
    let selected = evaluate(
        "design.repair.apply-selected",
        json!({"source":source,"target":target,"proposal":value,"selection":selection(&proposal)}),
    )
    .unwrap();
    assert_eq!(selected["persisted"], false);
    assert_eq!(selected["preserved_outside_selection"], true);
    assert_eq!(
        code(evaluate(
            "propose",
            json!({"source":source,"target":target,"request":self::request(&target),"force":true})
        )),
        "repair_document_invalid"
    );
    assert_eq!(
        code(evaluate("run_script", json!({}))),
        "repair_operation_unknown"
    );
    let descriptors = describe();
    assert_eq!(descriptors["operations"].as_array().unwrap().len(), 4);
    for descriptor in descriptors["operations"].as_array().unwrap() {
        assert_eq!(descriptor["input_schema"]["type"], "object");
        assert_eq!(descriptor["input_schema"]["additionalProperties"], false);
        assert_eq!(descriptor["output_schema"]["type"], "object");
        assert!(!serde_json::to_string(descriptor)
            .unwrap()
            .contains("\"$ref\""));
        assert_eq!(descriptor["pure"], true);
    }
}

#[test]
fn pole_existence_status_is_an_admitted_text_cell_and_its_neighbours_are_not() {
    let target = snapshot();
    let mut request = self::request(&target);
    let status_rule = FieldRule {
        allowed_values: vec![json!("existing"), json!("new")],
        ..text_rule()
    };
    request
        .policy
        .fields
        .get_mut("lv_poles")
        .unwrap()
        .insert("existing_status".into(), status_rule.clone());
    request
        .allowlist
        .get_mut("lv_poles")
        .unwrap()
        .insert("existing_status".into());
    let assign = |layer: &str, field: &str, value: Value| RepairOperation::Assign {
        target_layer: layer.into(),
        target_ids: vec!["p2".into()],
        values: BTreeMap::from([(field.into(), Cell::Value { value })]),
    };
    request.operations = vec![assign("lv_poles", "existing_status", json!("existing"))];
    let proposal = propose(None, &target, &request).unwrap();
    assert_eq!(proposal.changes.len(), 1);
    assert_eq!(proposal.changes[0].before, Cell::Missing);
    let applied = apply_selected(None, &target, &proposal, &selection(&proposal)).unwrap();
    assert_eq!(applied.changed_cells, 1);
    let mut expected = serde_json::to_value(&target).unwrap();
    expected["layers"]["lv_poles"]["features"][1]["properties"]["existing_status"] =
        json!("existing");
    assert_eq!(
        serde_json::to_value(&applied.snapshot).unwrap(),
        expected,
        "only the selected existence cell changes"
    );

    // A lifecycle status is text: a number is refused even if a caller widens
    // its policy to admit one.
    assert_eq!(
        code(validate_property_cell(
            "lv_poles",
            "existing_status",
            &Cell::Value { value: json!(1) }
        )),
        "repair_value_invalid"
    );
    // Its neighbours stay outside the catalogue, whatever the caller allows.
    for (layer, field) in [
        ("lv_poles", "existing_status_detail"),
        ("tapping_poles", "existing_status"),
        ("lv_poles", "construction_status"),
    ] {
        assert_eq!(
            code(validate_property_cell(
                layer,
                field,
                &Cell::Value {
                    value: json!("existing")
                }
            )),
            "repair_field_not_allowed"
        );
        let mut widened = request.clone();
        widened
            .policy
            .fields
            .entry(layer.into())
            .or_default()
            .insert(field.into(), status_rule.clone());
        widened
            .allowlist
            .entry(layer.into())
            .or_default()
            .insert(field.into());
        widened.operations = vec![assign(layer, field, json!("existing"))];
        assert_eq!(
            code(propose(None, &target, &widened)),
            "repair_field_not_allowed",
            "{layer}.{field}"
        );
    }
}
