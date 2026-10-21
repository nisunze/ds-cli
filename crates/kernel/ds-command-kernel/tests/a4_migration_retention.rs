use ds_command_kernel::{
    style_governance::{admit, Command, Request},
    style_plan::style_digest,
};
use serde_json::{json, Value};

fn fixtures() -> [Value; 2] {
    [
        serde_json::from_str(include_str!("fixtures/a4-migration-go-plan.json")).unwrap(),
        serde_json::from_str(include_str!("fixtures/a4-migration-go-fitting-plan.json")).unwrap(),
    ]
}

fn rehash(response: &mut Value) {
    response["data"]["plan_sha256"] = json!("");
    response["data"]["plan_sha256"] = json!(style_digest(&response["data"]));
}

#[test]
fn both_go_baselines_admit_exact_retention_and_fenced_apply_receipts() {
    for response in fixtures() {
        let request = Request {
            project: "project_a".into(),
            command: Command::PlanA4Migration,
        };
        let plan = admit(&request, &response).expect("approved Go baseline must be admitted");
        assert_eq!(plan["updates"], 1);
        assert_eq!(plan["retains"], 1);
        let apply = Request {
            project: request.project,
            command: Command::ApplyA4Migration {
                expected_definition_sha256: plan["definition_sha256"].as_str().unwrap().into(),
                expected_plan_sha256: plan["plan_sha256"].as_str().unwrap().into(),
            },
        };
        let mut receipt = json!({"success":true,"data":{"schema":"ds.print-a4-migration-result/v1","applied":true,"updated":true,"preserved":false,"retained":true,"plan":plan}});
        admit(&apply, &receipt).expect("exact retention receipt must be admitted");
        receipt["data"]["retained"] = json!(false);
        assert!(
            admit(&apply, &receipt).is_err(),
            "retention cannot be omitted"
        );
        receipt["data"]["retained"] = json!(true);
        receipt["data"]["plan"]["plan_sha256"] = json!("0".repeat(64));
        assert!(
            admit(&apply, &receipt).is_err(),
            "apply cannot drift from its reviewed fence"
        );
    }
}

#[test]
fn rehashed_plans_cannot_mix_baselines_or_redirect_retention() {
    let sources = fixtures();
    let request = Request {
        project: "project_a".into(),
        command: Command::PlanA4Migration,
    };
    for (i, response) in sources.iter().enumerate() {
        for field in [
            "existing_sha256",
            "previous_content_sha256",
            "previous_html_sha256",
            "retention_path",
        ] {
            let mut bad = response.clone();
            bad["data"][field] = sources[1 - i]["data"][field].clone();
            rehash(&mut bad);
            assert!(admit(&request, &bad).is_err(), "mixed {field} was admitted");
        }
        let mut unknown = response.clone();
        unknown["data"]["existing_sha256"] = json!("f".repeat(64));
        unknown["data"]["retention_path"] = json!(format!(
            "eds_project/project_a/printing_a4_documents/voltage-drop-a4-v1/revisions/{}",
            "f".repeat(64)
        ));
        rehash(&mut unknown);
        assert!(
            admit(&request, &unknown).is_err(),
            "unreviewed baseline was admitted"
        );
        let mut redirected = response.clone();
        redirected["data"]["retention_path"] = json!(response["data"]["retention_path"]
            .as_str()
            .unwrap()
            .replace("project_a", "project_b"));
        rehash(&mut redirected);
        assert!(
            admit(&request, &redirected).is_err(),
            "another project's retention was admitted"
        );
        let mut conflict = response.clone();
        conflict["data"]["existing_state"] = json!("conflict");
        conflict["data"]["apply_allowed"] = json!(false);
        conflict["data"]["updates"] = json!(0);
        conflict["data"]["retains"] = json!(0);
        for field in [
            "retention_path",
            "previous_content_sha256",
            "previous_html_sha256",
        ] {
            conflict["data"][field] = json!("");
        }
        rehash(&mut conflict);
        assert!(
            admit(&request, &conflict).is_err(),
            "a reviewed baseline cannot be relabelled as authored drift"
        );
    }
}
