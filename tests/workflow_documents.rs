use ds_command_kernel::{
    design,
    workflow_documents::{self, Binding, Projection, Source},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn recipe() -> Value {
    json!({"schema":"ds.vector-workflow/v1","name":"Compare facts","inputs":{"source":{"type":"document"},"target":{"type":"document"}},"steps":[{"id":"compare","tool":"design.repair.compare","request":{"source":{"$ref":"inputs.source"},"target":{"$ref":"inputs.target"},"request":{"$ref":"inputs.options"}}}],"outputs":{"findings":{"$ref":"steps.compare.outputs.report"}}})
}
#[test]
fn immutable_global_definition_pins_contracts_and_refuses_embedded_holdings() {
    let versions = BTreeMap::from([("design.repair.compare".into(), json!(1))]);
    let definition =
        workflow_documents::capture("repair", "Reviewed recipe", recipe(), versions.clone())
            .unwrap();
    assert_eq!(definition.document["steps"][0]["tool_version"], 1);
    workflow_documents::verify(&definition).unwrap();
    let mut vector_recipe = recipe();
    vector_recipe["steps"][0]["tool"] = json!("measure");
    let vector = workflow_documents::capture(
        "vector",
        "",
        vector_recipe,
        BTreeMap::from([("measure".into(), json!("ds.vector-tool/v1"))]),
    )
    .unwrap();
    assert_eq!(
        vector.document["steps"][0]["tool_version"],
        "ds.vector-tool/v1"
    );
    workflow_documents::verify(&vector).unwrap();
    let mut forged = definition.clone();
    forged.document["name"] = json!("changed");
    assert!(workflow_documents::verify(&forged).is_err());
    let mut extra = definition.clone();
    extra
        .operation_versions
        .insert("unadmitted.extra".into(), json!(1));
    assert!(workflow_documents::verify(&extra).is_err());
    let mut holding = recipe();
    holding["steps"][0]["request"]["source"] = json!({"type":"FeatureCollection","features":[]});
    assert!(workflow_documents::capture("repair", "", holding, versions.clone()).is_err());
    let mut embedded = recipe();
    embedded["inputs"]["source"]["default"] = json!({"secret":"data"});
    assert!(workflow_documents::capture("repair", "", embedded, versions.clone()).is_err());
    let mut old = recipe();
    old["steps"][0]["tool_version"] = json!(2);
    assert!(workflow_documents::capture("repair", "", old, versions).is_err());
}
#[test]
fn file_projection_keeps_exact_ids_and_refuses_changed_bytes() {
    let raw = json!({"type":"FeatureCollection","features":[{"type":"Feature","id":u64::MAX,"geometry":{"type":"Point","coordinates":[30,-2]},"properties":{"old":"A","nullable":null}}]});
    let mut binding:Binding=serde_json::from_value(json!({"source":{"kind":"file","path":"evidence.geojson","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"projection":"evidence","scope":{"project":"P1","principal":"user","lane":"canary","audience":"authority"},"id":"external","layer":"lv_poles","crs":"EPSG:4326"})).unwrap();
    let result = workflow_documents::project(
        &binding,
        raw.clone(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    )
    .unwrap();
    assert_eq!(
        result["layers"]["lv_poles"]["features"][0]["id"],
        json!(u64::MAX)
    );
    assert!(
        workflow_documents::project(&binding, raw.clone(), Some("changed"))
            .unwrap_err()
            .starts_with("workflow_source_changed")
    );
    binding.projection = Projection::Json;
    assert_eq!(
        workflow_documents::project(
            &binding,
            raw.clone(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        )
        .unwrap(),
        raw
    );
}
#[test]
fn retained_revision_source_cannot_substitute_another_transformer_or_project() {
    let snapshot = json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326","layers":{"lv_poles":{"type":"FeatureCollection","features":[]}},"settings":{},"network_config":{"sheets":{}},"include_design_customers":true,"sources":[]});
    let decoded: design::Snapshot = serde_json::from_value(snapshot.clone()).unwrap();
    let revision = design::digest(&decoded).unwrap();
    let snapshot = serde_json::to_value(&decoded).unwrap();
    let mut binding:Binding=serde_json::from_value(json!({"source":{"kind":"workspace","path":"workspace","project":"P1","transformer":"T1","revision":revision},"projection":"snapshot","scope":{"project":"P1","principal":"user","lane":"canary","audience":"authority"}})).unwrap();
    assert_eq!(
        workflow_documents::project(&binding, snapshot.clone(), None).unwrap(),
        snapshot
    );
    binding.source = Source::Workspace {
        path: "workspace".into(),
        project: "other".into(),
        transformer: "T1".into(),
        revision,
    };
    assert!(workflow_documents::project(&binding, snapshot, None).is_err());
}
