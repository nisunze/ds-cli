//! A host that supplies no calculation (DS Core) refuses local calculation
//! before any workspace effect, with its documented remedy; malformed requests
//! keep their decoding refusal.
#[path = "../../ds-solar-reference-host/tests/common/mod.rs"]
mod common;

use ds_solar_contracts::{PortfolioIdentity, PortfolioRepresentativeStrategy};
use ds_solar_project::{ProjectIdentity, Workspace, control, portfolio};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn engine_free_control_refuses_run_and_portfolio_before_any_effect() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("workspace");
    let mut w = Workspace::initialize(
        &path,
        ProjectIdentity {
            project_id: "project-1".into(),
            root: "eds_project/project-1/eds_solar".into(),
        },
    )
    .unwrap();
    let mut city = common::city_input("aderm_bere");
    city.identity.project_id = "project-1".into();
    city.identity.root = "eds_project/project-1/eds_solar".into();
    let city = city.sealed();
    w.seed(std::slice::from_ref(&city), &BTreeMap::new())
        .unwrap();
    drop(w);
    let status = || control::execute(json!({"operation":"status","workspace":path})).unwrap();
    let before = status();
    let run = json!({"operation":"run","workspace":path,"cache":tmp.path().join("references"),
        "request":{"run_id":"refused","cities":[city.identity.city_id],"concurrency":1,"charts":false,"drafts":["apd"]}});
    let aggregate = json!({"operation":"portfolio_calculate","request":portfolio::Request {
        source: path.join("runs/refused"),
        source_run_id: "refused".into(),
        out: tmp.path().join("portfolio"),
        identity: PortfolioIdentity {
            project_id: "project-1".into(),
            root: "eds_project/project-1/eds_solar".into(),
            portfolio_id: "pf_1".into(),
            portfolio_name: "Two cities".into(),
            membership_revision: format!("sha256:{}", "a".repeat(64)),
        },
        cities: vec!["aderm_bere".into(), "aderm_gagal".into()],
        run_id: "aggregate".into(),
        strategy: PortfolioRepresentativeStrategy::First,
    }});
    for request in [run.clone(), aggregate] {
        let error = control::execute(request).unwrap_err().to_string();
        assert_eq!(
            error,
            "Local project calculation is available through solar.application; the native host does not link the Solar engine"
        );
    }
    assert_eq!(status(), before);
    assert_eq!(before["runs"], json!([]));
    assert!(!path.join("jobs").exists());
    assert!(!path.join("runs/refused").exists());
    assert!(!tmp.path().join("portfolio").exists());
    let mut malformed = run;
    malformed["request"]["unexpected"] = json!(true);
    let error = control::execute(malformed).unwrap_err().to_string();
    assert!(error.contains("unknown field"), "{error}");
}
