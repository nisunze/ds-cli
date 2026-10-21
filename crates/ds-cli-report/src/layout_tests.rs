use super::*;

fn listed() -> Value {
    json!({"setups": [
        {"id":"mv-a3-plan-profile-template","name":"A3 LANDSCAPE · MV PLAN AND PROFILE · TEMPLATE","revision":"r1","layout":null,
         "paper":{"orientation":"landscape","size":"A3"},"classification":{"purpose":"mv_model","role":"plan_profile"}},
        {"id":"nyaruguru-district-mv-a3-landscape-core-v0","name":"A3 LANDSCAPE · NYARUGURU MV DISTRICT MAP · v0","revision":"r2","layout":null,
         "paper":{"orientation":"landscape","size":"A3"},"classification":{"purpose":"mv_model","role":"network"}},
        {"id":"project-overview-a3","name":"Project overview · A3","revision":"r3","layout":null,
         "paper":{"orientation":"landscape","size":"A3"},"classification":{"purpose":"project","role":"overview"}},
        {"id":"print-legacy","name":"A0-Landscape HuYE","revision":"r4","layout":null,"paper":null}
    ]})
}

#[test]
fn the_catalogue_groups_front_matter_mv_and_overview_sheets_from_one_read() {
    let pin = "a".repeat(64);
    let governed = json!({"scope":"project","project_id":"template_project","schema":"ds.printing-standard-catalog/v1","documents":[
        {"kind":"standard","id":"mv-cover-a3","template_id":"mv-booklet-a3-v1","role":"cover","paper":"A3","orientation":"landscape","revision_id":pin,"bound":false}
    ]});
    let out = catalogue(&listed(), "template_project", Ok(governed)).unwrap();
    assert_eq!(out["project_id"], "template_project");
    assert_eq!(out["governed"], json!({"read": true}));
    let labels: Vec<&str> = out["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        labels,
        [
            "Front matter",
            "MV Sheets",
            "Overview Sheets",
            "Unclassified"
        ]
    );
    let front = &out["groups"][0]["roles"][0]["templates"][0];
    assert_eq!(front["location"]["kind"], "governed_standard");
    assert_eq!(front["subject"], "layout");
    let mv = &out["groups"][1]["roles"][0];
    assert_eq!(mv["role"], "plan_profile");
    assert_eq!(mv["templates"].as_array().unwrap().len(), 1);
    assert_eq!(mv["templates"][0]["id"], "mv-a3-plan-profile-template");
    assert_eq!(
        out["groups"][2]["roles"][0]["templates"][0]["subject"],
        "extent"
    );
    let unclassified = out["groups"][3]["roles"][0]["templates"]
        .as_array()
        .unwrap();
    assert_eq!(unclassified.len(), 2);
    let district = unclassified
        .iter()
        .find(|t| t["id"] == "nyaruguru-district-mv-a3-landscape-core-v0")
        .unwrap();
    assert_eq!(
        district["mismatch"]["code"],
        "print_classification_mv_requires_plan_profile"
    );
}

#[test]
fn a_failed_governed_read_keeps_the_layouts_and_names_the_refusal() {
    let refused = Failure::failed(
        "print_standard_route_unavailable",
        "this deployment does not serve the governed printing read",
    );
    let out = catalogue(&listed(), "template_project", Err(refused)).unwrap();
    assert_eq!(out["governed"]["read"], false);
    assert_eq!(out["governed"]["code"], "print_standard_route_unavailable");
    assert_eq!(out["groups"][0]["label"], "MV Sheets");
    assert!(catalogue(&json!({}), "template_project", Ok(Value::Null)).is_err());
}

#[test]
fn save_receipt_names_one_heading_change_without_echoing_embedded_assets() {
    let previous = json!({
        "id":"gisagara-a0",
        "assets":{"logo": format!("data:image/png;base64,{}", "A".repeat(400_000))},
        "elements":[{"id":"pole-schedule","table":{"headings":["Pole", "Angle Â°"]}}]
    });
    let mut updated = previous.clone();
    updated["elements"][0]["table"]["headings"][1] = json!("Angle (deg)");
    let saved = json!({
        "id":"gisagara-a0", "name":"A0", "revision":"b".repeat(64),
        "updated_at":"2026-09-28T11:46:48Z", "layout":updated
    });
    let receipt = compact_save_receipt(&saved, Some(&previous), "gisagara-a0").unwrap();
    assert_eq!(receipt["revision"], "b".repeat(64));
    assert_eq!(
        receipt["changed_paths"],
        json!(["layout.elements[0].table.headings[1]"])
    );
    assert_eq!(receipt["verification"], "committed_backend_result");
    let encoded = serde_json::to_string(&receipt).unwrap();
    assert!(
        encoded.len() < 1_000 && !encoded.contains("base64"),
        "{encoded}"
    );
    assert_eq!(
        compact_save_receipt(&saved, None, "gisagara-a0").unwrap()["changed_paths"],
        json!(["layout"])
    );
    assert!(SAVE.args.iter().any(|arg| arg.name == "full"));
    assert_eq!(saved["layout"]["assets"], previous["assets"]);
}

/// Fill one documented transaction's placeholders with real values.
///
/// A placeholder is any string in angle brackets; what it stands for is
/// decided by the key it sits under, so the test never re-states the
/// shape the schema is publishing.
fn filled(key: &str, template: &Value) -> Value {
    match template {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), filled(name, value)))
                .collect(),
        ),
        // A documented choice list is filled with its first choice.
        Value::String(text) if text.contains('|') => json!(text.split('|').next()),
        Value::String(text) if text.starts_with('<') => match key {
            "layout" => {
                json!(ds_command_kernel::printing::default_layout())
            }
            "revision" => json!("a".repeat(64)),
            // An update pins an exact revision; a save may name none,
            // which is the fact that makes it a create.
            "expected_revision" if text.contains("empty") => json!(""),
            "expected_revision" => json!("a".repeat(64)),
            "id" => json!("sample_layout"),
            "name" => json!("Sample"),
            "scope" => json!("global"),
            _ => template.clone(),
        },
        other => other.clone(),
    }
}

fn documented_schema() -> Value {
    let context = Context {
        confirmed: false,
        output: ds_cli_contract::Output {
            format: ds_cli_contract::Format::Json,
            pretty: false,
            color: false,
        },
    };
    schema(&Inputs::default(), &context).expect("the schema is local and cannot fail")
}

fn transaction(name: &str) -> ds_cli_auth::PrintingRequest {
    let schema = documented_schema();
    let documented = filled(name, &schema["transactions"][name]);
    serde_json::from_value(documented)
        .unwrap_or_else(|error| panic!("documented `{name}` does not parse: {error}"))
}

/// Discovery and acceptance were two different contracts: `schema`
/// documented `create`, `update` and `copy`, `save` accepted only `save`,
/// and an agent that read the schema was refused by every command it
/// could reach. Every documented transaction must now parse into a
/// request the command named for it actually accepts.
#[test]
fn every_documented_transaction_parses_into_the_request_its_command_accepts() {
    assert!(matches!(
        transaction("create"),
        ds_cli_auth::PrintingRequest::Create { .. }
    ));
    assert!(matches!(
        transaction("update"),
        ds_cli_auth::PrintingRequest::Update { .. }
    ));
    assert!(matches!(
        transaction("save"),
        ds_cli_auth::PrintingRequest::Save { .. }
    ));
    assert!(matches!(
        transaction("copy"),
        ds_cli_auth::PrintingRequest::Copy { .. }
    ));
    // Delete is documented as the two flags it takes, not as a body.
    let schema = documented_schema();
    let delete = schema["transactions"]["delete"]
        .as_str()
        .expect("delete is documented as its flags");
    assert!(delete.contains("--id") && delete.contains("--expected-revision"));
}

/// `layout save` publishes whichever of the three publishing shapes the
/// caller derived from the schema. Create-versus-update stays a decision
/// taken from the revision, never from the word.
#[test]
fn save_publishes_every_schema_derived_publishing_shape_and_names_what_it_refuses() {
    for name in ["save", "create", "update"] {
        let (layout, revision) = publication(transaction(name))
            .unwrap_or_else(|error| panic!("save refused documented `{name}`: {error:?}"));
        assert_eq!(layout, ds_command_kernel::printing::default_layout());
        // A create carries no revision; the other two carry the one the
        // schema documents for them.
        let expected = if name == "update" { 64 } else { 0 };
        assert_eq!(revision.len(), expected, "{name}");
    }
    for other in [
        transaction("copy"),
        ds_cli_auth::PrintingRequest::List {},
        ds_cli_auth::PrintingRequest::Delete {
            id: "sample_layout".into(),
            expected_revision: "a".repeat(64),
        },
    ] {
        let refusal = publication(other).expect_err("a different transaction is refused");
        let message = format!("{refusal:?}");
        for action in ["save", "create", "update"] {
            assert!(message.contains(action), "{message}");
        }
    }
}
