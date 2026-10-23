use super::*;

#[test]
fn linked_local_controls_prepare_reference_without_legacy_executable() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("project");
    let cache = directory.path().join("cache");
    let request_file = directory.path().join("reference-request.json");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../ds-solar/fixtures/city/aderm_bere.snapshot.json");
    assert_eq!(availability(), Availability::Available);
    invoke(json!({"operation":"initialize","workspace":workspace,
        "identity":{"project_id":"p","root":"eds_project/p/eds_solar"}}))
    .unwrap();
    invoke(
        json!({"operation":"city_create","workspace":workspace,"city":"aderm_bere",
        "display_name":"Bere","input":fixture}),
    )
    .unwrap();
    let planned = invoke(
        json!({"operation":"reference_plan_file","workspace":workspace,
        "project":"p","city":"aderm_bere","cache":cache,"out":request_file}),
    )
    .unwrap();
    assert_eq!(planned["ready"], false);
    assert!(request_file.is_file());
    assert!(
        !cache.exists(),
        "planning does not create/acquire reference data"
    );
    let wrong_project = invoke(
        json!({"operation":"reference_import_files","workspace":workspace,
        "project":"other","city":"aderm_bere","cache":cache,
        "expected":planned["input_digest"],"bundle":directory.path().join("missing")}),
    )
    .unwrap_err();
    assert_eq!(wrong_project.code(), "engine_refused");
    assert!(wrong_project.message().contains("another project"));
    assert!(
        !cache.exists(),
        "project refusal precedes bundle reads/cache writes"
    );
    let bundle = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../ds-solar/fixtures/reference/aderm_bere");
    let import_request = json!({"operation":"reference_import_files","workspace":workspace,
        "project":"p","city":"aderm_bere","cache":cache,
        "expected":planned["input_digest"],"bundle":bundle});
    let imported = invoke(import_request.clone()).unwrap();
    assert_eq!(imported["ready"], true);
    let reused = invoke(import_request).unwrap();
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["acquired"], false);
    assert_eq!(
        reused["prepared_input_digest"],
        imported["prepared_input_digest"]
    );
    let rejected = directory.path().join("must-not-exist");
    assert!(
        invoke(json!({"operation":"initialize","workspace":rejected,
        "identity":{"project_id":"p","root":"eds_project/p/eds_solar"},"unknown":true}))
        .is_err()
    );
    assert!(
        !rejected.exists(),
        "closed request admission precedes owner effects"
    );
}

#[test]
fn run_declares_its_required_city_selection() {
    let city = RUN.args.iter().find(|arg| arg.name == "city").unwrap();
    assert!(city.required);
    assert!(city.summary.contains("Required explicit city selection"));
}
use ds_cli_contract::spec::Command;
use ds_cli_contract::{ExitClass, Format, Output, parse};

fn inputs(command: &'static Command, tokens: &[String]) -> Inputs {
    parse(command, tokens).expect("declared tokens parse")
}

fn context() -> Context {
    Context {
        confirmed: true,
        output: Output::resolve(Format::Json, false, true),
    }
}

fn workspace() -> Vec<String> {
    vec![
        "--workspace".to_owned(),
        "/nonexistent/ds-solar-project".to_owned(),
    ]
}

fn repeat(tokens: &mut Vec<String>, flag: &str, count: usize) {
    for index in 0..count {
        tokens.push(flag.to_owned());
        tokens.push(format!("city-{index:03}"));
    }
}

// These call the handlers directly on purpose. Dispatch applies the
// owner is now linked directly. Reaching these bounds here proves each
// refusal arrives before local owner effects.
#[test]
fn seed_refuses_an_empty_input_set_before_reaching_the_owner() {
    let error = seed(&inputs(&SEED, &workspace()), &context())
        .expect_err("a seed with no --input is refused");
    assert_eq!(error.class(), ExitClass::InvalidInput);
    assert_eq!(error.code(), "solar_project_inputs");
    assert!(
        error.remedy_text().is_some_and(|remedy| remedy.len() > 10),
        "the refusal must say how to get out of it"
    );
}

#[test]
fn seed_refuses_more_inputs_than_one_atomic_import_carries() {
    let mut tokens = workspace();
    repeat(&mut tokens, "--input", MAX_PROJECT_SEED_INPUTS + 1);
    let error = seed(&inputs(&SEED, &tokens), &context())
        .expect_err("65 inputs exceed the owner's atomic seed");
    assert_eq!(error.class(), ExitClass::InvalidInput);
    assert_eq!(error.code(), "solar_project_inputs");
    assert_eq!(
        error.detail_value(),
        Some(&json!({ "given": MAX_PROJECT_SEED_INPUTS + 1, "max": MAX_PROJECT_SEED_INPUTS }))
    );
}

#[test]
fn run_refuses_a_concurrency_outside_the_range_its_help_states() {
    let mut tokens = workspace();
    tokens.extend([
        "--cache".to_owned(),
        "/nonexistent/cache".to_owned(),
        "--run-id".to_owned(),
        "r1".to_owned(),
        "--concurrency".to_owned(),
        "0".to_owned(),
        "--city".to_owned(),
        "city-000".to_owned(),
    ]);
    let error =
        run(&inputs(&RUN, &tokens), &context()).expect_err("0 is not a concurrency of 1..32");
    assert_eq!(error.class(), ExitClass::InvalidInput);
    assert_eq!(error.code(), "solar_project_concurrency");
}

// Both branches of `solar_project_concurrency` are the same code to a
// caller, so both have to hand back the same way out. The non-integer
// branch predates the range check and carried none.
#[test]
fn every_concurrency_refusal_carries_the_remedy_its_help_declares() {
    for given in ["0", "33", "abc"] {
        let mut tokens = workspace();
        tokens.extend([
            "--cache".to_owned(),
            "/nonexistent/cache".to_owned(),
            "--run-id".to_owned(),
            "r1".to_owned(),
            "--city".to_owned(),
            "city-000".to_owned(),
            "--concurrency".to_owned(),
            given.to_owned(),
        ]);
        let error = run(&inputs(&RUN, &tokens), &context())
            .err()
            .unwrap_or_else(|| panic!("`--concurrency {given}` is not a concurrency"));
        assert_eq!(
            error.class(),
            ExitClass::InvalidInput,
            "--concurrency {given}"
        );
        assert_eq!(
            error.code(),
            "solar_project_concurrency",
            "--concurrency {given}"
        );
        assert!(
            error.remedy_text().is_some_and(|remedy| remedy.len() > 10),
            "`--concurrency {given}` refuses with no way out"
        );
    }
}

#[test]
fn run_refuses_a_run_that_selects_no_city_at_all() {
    let mut tokens = workspace();
    tokens.extend([
        "--cache".to_owned(),
        "/nonexistent/cache".to_owned(),
        "--run-id".to_owned(),
        "r1".to_owned(),
    ]);
    let error = parse(&RUN, &tokens)
        .expect_err("a run without an explicit city is refused by its declared contract");
    assert_eq!(error.class(), ExitClass::InvalidInput);
    assert!(error.message().contains("--city"));
}

#[test]
fn run_refuses_more_selected_cities_than_the_owner_accepts() {
    let mut tokens = workspace();
    tokens.extend([
        "--cache".to_owned(),
        "/nonexistent/cache".to_owned(),
        "--run-id".to_owned(),
        "r1".to_owned(),
    ]);
    repeat(&mut tokens, "--city", MAX_PROJECT_RUN_CITIES + 1);
    let error = run(&inputs(&RUN, &tokens), &context())
        .expect_err("65 cities exceed the owner's selection");
    assert_eq!(error.class(), ExitClass::InvalidInput);
    assert_eq!(error.code(), "solar_project_cities");
}

/// One local workspace holding the governed Bere fixture with its sealed
/// reference imported: calculation-ready without a network or sign-in.
fn ready_workspace(directory: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let workspace = directory.join("project");
    let cache = directory.join("cache");
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures");
    invoke(json!({"operation":"initialize","workspace":workspace,
        "identity":{"project_id":"p","root":"eds_project/p/eds_solar"}}))
    .unwrap();
    invoke(
        json!({"operation":"city_create","workspace":workspace,"city":"aderm_bere",
        "display_name":"Bere","input":fixtures.join("city/aderm_bere.snapshot.json")}),
    )
    .unwrap();
    let planned = invoke(
        json!({"operation":"reference_plan_file","workspace":workspace,
        "project":"p","city":"aderm_bere","cache":cache,"out":directory.join("request.json")}),
    )
    .unwrap();
    invoke(
        json!({"operation":"reference_import_files","workspace":workspace,
        "project":"p","city":"aderm_bere","cache":cache,
        "expected":planned["input_digest"],"bundle":fixtures.join("reference/aderm_bere")}),
    )
    .unwrap();
    (workspace, cache)
}

fn run_tokens(workspace: &std::path::Path, cache: &std::path::Path, run_id: &str) -> Vec<String> {
    [
        "--workspace",
        workspace.to_str().unwrap(),
        "--cache",
        cache.to_str().unwrap(),
        "--run-id",
        run_id,
        "--city",
        "aderm_bere",
        "--concurrency",
        "1",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn run_refuses_a_missing_addon_before_capturing_a_run() {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, cache) = ready_workspace(directory.path());
    let error = run_with(
        &inputs(&RUN, &run_tokens(&workspace, &cache, "no-addon")),
        || {
            Err(Failure::unavailable(
                "solar_engine_missing",
                "no Solar addon for this lane",
            ))
        },
    )
    .expect_err("a missing addon refuses");
    assert_eq!(error.code(), "solar_engine_missing");
    let status = invoke(json!({"operation":"status","workspace":workspace})).unwrap();
    assert_eq!(
        status["runs"],
        json!([]),
        "no run is captured without an engine"
    );
    assert!(!workspace.join("runs/no-addon").exists());
    assert!(
        RUN.refusals
            .iter()
            .any(|refusal| refusal.code == "solar_engine_missing")
    );
}

#[cfg(unix)]
#[test]
#[ignore = "requires explicitly qualified retained Solar executable"]
fn run_calculates_the_captured_city_through_the_selected_addon() {
    let binary = std::path::PathBuf::from(std::env::var_os("DS_MODULE_TEST_SOLAR_BIN").unwrap());
    let directory = tempfile::tempdir().unwrap();
    let (workspace, cache) = ready_workspace(directory.path());
    let tokens = run_tokens(&workspace, &cache, "addon-run");
    let receipt = run_with(&inputs(&RUN, &tokens), || {
        crate::SOLAR_ENGINE.capture_pinned(&binary)
    })
    .expect("the selected addon calculates the captured run");
    assert_eq!(receipt["source_notices"], json!([]));
    invoke(json!({"operation":"result","workspace":workspace,"run_id":"addon-run"}))
        .expect("the committed local result verifies");
    let run = workspace.join("runs/addon-run");
    let batch: ds_solar_contracts::SolarArtifactBatch =
        serde_json::from_slice(&std::fs::read(run.join("batch.json")).unwrap()).unwrap();
    ds_solar_project::files::verify(&run, &batch).unwrap();
    assert_eq!(batch.run.run_id, "addon-run");
    assert!(
        batch
            .artifacts
            .iter()
            .any(|a| a.name.ends_with("apd-draft-fr.md"))
    );
}
