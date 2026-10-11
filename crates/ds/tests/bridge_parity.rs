//! Parity between the paired-application domains and the desktop's closed CLI
//! bridge.
//!
//! `ds map` and `ds work` do not reach an open automation or assistant
//! surface. Each command names a typed operation which must occur exactly once
//! in the native allowlist, once in the frontend dispatcher, and once in that
//! domain's adapter input contract. That exact-one rule prevents two CLI
//! commands from quietly becoming aliases for the same mutation.

use std::{collections::BTreeSet, path::PathBuf};

use ds_cli_desktop::ops::BridgeOp;

/// The sibling desktop source. It is intentionally a source-level parity
/// check: the desktop is not a Rust build dependency, but a missing operation
/// must fail CI rather than be discovered by an operator after deployment.
fn ds_web() -> PathBuf {
    let root = match std::env::var_os("DS_WEB_DIR") {
        Some(explicit) => PathBuf::from(explicit),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-web"),
    };
    assert!(
        root.is_dir(),
        "bridge parity requires the ds-web checkout at {}; set DS_WEB_DIR to the current checkout",
        root.display()
    );
    root.canonicalize().expect("canonicalize ds-web checkout")
}

/// Load only the inputs a check inspects. A removed unrelated adapter must not
/// mask that check's result; a missing required input always fails by path.
struct Source {
    path: PathBuf,
    text: std::sync::OnceLock<String>,
}

impl std::ops::Deref for Source {
    type Target = str;

    fn deref(&self) -> &str {
        self.text.get_or_init(|| {
            let source = std::fs::read_to_string(&self.path).unwrap_or_else(|error| {
                panic!(
                    "bridge parity cannot read required source {}: {error}",
                    self.path.display()
                )
            });
            assert!(
                !source.trim().is_empty(),
                "bridge parity source is empty: {}",
                self.path.display()
            );
            if self.path.ends_with("src/lib/desktop/cli-bridge.ts") {
                assert!(
                    source.contains("export async function executeCliOperation(")
                        && source.contains("switch (operation) {"),
                    "bridge parity dispatcher pattern is absent in {}",
                    self.path.display()
                );
            }
            source
        })
    }
}

fn source(root: &std::path::Path, leaf: &str) -> Source {
    Source {
        path: root.join(leaf),
        text: std::sync::OnceLock::new(),
    }
}

struct App {
    transport: Source,
    frontend: Source,
    project: Source,
    map: Source,
    map_working_set: Source,
    map_layers: Source,
    map_profile: Source,
    map_profile_model: Source,
    map_profile_results: Source,
    map_profile_calculation: Source,
    map_profile_edit: Source,
    map_profile_filter: Source,
    map_grid_lasso: Source,
    survey: Source,
    design: Source,
    data: Source,
    project_data: Source,
    cli_errors: Source,
    materialize: Source,
    analysis: Source,
    dsgrid: Source,
    dsgrid_contract: Source,
    style_fill_pattern: Source,
    style_kernel: Source,
    style_renderer: Source,
    sync_center: Source,
    feedback_submit: Source,
    solar_portfolio_run: Source,
    solar_batch_adapter: Source,
    solar_portfolio_receipt: Source,
    reliability_page: Source,
}

fn app() -> App {
    let root = ds_web();
    let read = |leaf: &str| source(&root, leaf);
    App {
        transport: read("src-tauri/src/cli_bridge.rs"),
        frontend: read("src/lib/desktop/cli-bridge.ts"),
        project: read("src/lib/desktop/cli-project.ts"),
        map: read("src/lib/desktop/cli-map.ts"),
        map_working_set: read("src/lib/desktop/cli-map-working-set.ts"),
        map_layers: read("src/lib/desktop/cli-map-layers.ts"),
        map_profile: read("src/lib/desktop/cli-map-profile.ts"),
        map_profile_model: read("src/lib/desktop/cli-profile-model.ts"),
        map_profile_results: read("src/lib/desktop/cli-profile-results.ts"),
        map_profile_calculation: read("src/lib/desktop/cli-profile-calculation.ts"),
        map_profile_edit: read("src/lib/desktop/cli-profile-edit.ts"),
        map_profile_filter: read("src/lib/desktop/cli-map-profile-filter.ts"),
        map_grid_lasso: read("src/lib/grid/lasso-request.ts"),
        survey: read("src/lib/desktop/cli-survey.ts"),
        design: read("src/lib/desktop/cli-map-design.ts"),
        data: read("src/lib/desktop/cli-data.ts"),
        project_data: read("src/lib/desktop/cli-project-data.ts"),
        cli_errors: read("src/lib/desktop/cli-errors.ts"),
        materialize: read("src/lib/search-place/materialize.ts"),
        analysis: read("src/lib/analysis/outliers.ts"),
        dsgrid: read("src/lib/desktop/cli-dsgrid.ts"),
        dsgrid_contract: read("docs/dsgrid-local-model-and-project-publication-contract.md"),
        style_fill_pattern: read("src/lib/styles/fill-pattern.ts"),
        style_kernel: read("src/lib/styles/kernel.ts"),
        style_renderer: read("src/lib/api/styles.ts"),
        sync_center: read("src/lib/desktop/cli-sync-center.ts"),
        feedback_submit: read("src/lib/feedback/submit.ts"),
        solar_batch_adapter: read("src/lib/desktop/cli-solar-portfolio-batch.ts"),
        solar_portfolio_run: read("src/lib/solar/native-batch.ts"),
        reliability_page: read("src/routes/sre/+page.svelte"),
        solar_portfolio_receipt: read("src/lib/solar/native-portfolio-batches.ts"),
    }
}

#[test]
fn every_sync_center_command_has_one_closed_operation_owner() {
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    assert!(
        !ds_cli_desktop::sync::BRIDGE_OPS.is_empty(),
        "ds_cli_desktop::sync::BRIDGE_OPS must declare operations for this parity check"
    );
    for operation in ds_cli_desktop::sync::BRIDGE_OPS {
        assert_eq!(
            count(allowlist, &format!("\"{}\"", operation.operation)),
            1,
            "`{}` must appear exactly once in the desktop allowlist",
            operation.operation
        );
        assert_eq!(
            switch_case_count(&app.frontend, operation.operation),
            1,
            "`{}` must have exactly one frontend handler",
            operation.operation
        );
        let contract = operation_contract(&app.sync_center, operation.operation);
        assert!(
            !contract.is_empty(),
            "`{}` has no typed Sync Center adapter argument contract",
            operation.operation
        );
        for argument in operation.arguments {
            assert!(
                contract.contains(&format!("'{argument}'")),
                "desktop sync sends `{argument}` to `{}`, but the adapter does not accept it",
                operation.operation
            );
        }
    }
}

fn count(source: &str, needle: &str) -> usize {
    source.match_indices(needle).count()
}

/// Count an exact TypeScript switch case independent of formatter quote style.
///
/// Both spellings retain the closing quote and colon, so `solar.run` cannot
/// accidentally match `solar.run.start`.
fn switch_case_count(source: &str, operation: &str) -> usize {
    count(source, &format!("case '{operation}':"))
        + count(source, &format!("case \"{operation}\":"))
}

#[test]
fn switch_case_matcher_accepts_both_quotes_without_prefix_matches() {
    let source = "case 'solar.run':\ncase \"solar.run\":\ncase 'solar.run.start':";
    assert_eq!(switch_case_count(source, "solar.run"), 2);
    assert_eq!(switch_case_count(source, "solar.run.start"), 1);
    assert_eq!(switch_case_count(source, "solar"), 0);
}

fn between<'a>(source: &'a str, open: &str, close: &str) -> &'a str {
    let start = source
        .find(open)
        .unwrap_or_else(|| panic!("bridge parity required opening pattern {open:?} is absent"));
    let rest = &source[start + open.len()..];
    let end = rest.find(close).unwrap_or_else(|| {
        panic!("bridge parity required closing pattern {close:?} after {open:?} is absent")
    });
    let slice = &rest[..end];
    assert!(
        !slice.trim().is_empty(),
        "bridge parity required section {open:?} is empty"
    );
    slice
}

#[test]
fn missing_or_empty_sections_fail_instead_of_passing_negative_checks() {
    for source in ["unrelated", "BEGIN unclosed", "BEGIN END", "BEGIN   END"] {
        let failure = std::panic::catch_unwind(|| between(source, "BEGIN", "END"));
        assert!(
            failure.is_err(),
            "missing or empty section passed: {source:?}"
        );
    }
    assert_eq!(between("BEGIN value END", "BEGIN", "END"), " value ");
}

#[test]
fn missing_or_unclosed_operation_contracts_fail_even_for_zero_arguments() {
    for source in [
        "'other': [],",
        "'operation': [",
        "'operation': ['argument'",
        "'operation': ['argument', 'other': [] ,",
    ] {
        assert!(std::panic::catch_unwind(|| operation_contract(source, "operation")).is_err());
    }
    assert!(quoted_contract_items(operation_contract("'operation': [],", "operation")).is_empty());
    assert!(std::panic::catch_unwind(|| quoted_contract_items("'unclosed")).is_err());
}

#[test]
fn missing_empty_or_unrecognizable_sources_fail_by_path() {
    let root = tempfile::tempdir().expect("parity source fixture");
    let leaf = "src/lib/desktop/cli-bridge.ts";
    let path = root.path().join(leaf);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    for contents in [None, Some(""), Some("// dispatcher removed")] {
        if let Some(contents) = contents {
            std::fs::write(&path, contents).unwrap();
        }
        let input = source(root.path(), leaf);
        let failure =
            std::panic::catch_unwind(|| input.len()).expect_err("missing evidence must fail");
        let message = failure.downcast_ref::<String>().expect("path diagnostic");
        assert!(message.contains(path.to_string_lossy().as_ref()));
    }
    // Loading a valid required input never reads an unrelated missing input.
    std::fs::write(root.path().join("required.ts"), "required source").unwrap();
    let required = source(root.path(), "required.ts");
    let _unrelated = source(root.path(), "absent.ts");
    assert_eq!(&*required, "required source");
}

fn operation_contract<'a>(source: &'a str, operation: &str) -> &'a str {
    if operation == "map.grid.lasso" && source.contains("CLI_MAP_GRID_LASSO_OPERATION_CONTRACT") {
        let marker = "export const GRID_LASSO_ARGUMENTS = [";
        let start = source.find(marker).expect("lasso argument list is present") + marker.len();
        let rest = &source[start..];
        return &rest[..rest.find("] as const").expect("lasso argument list closes")];
    }
    let single = format!("'{operation}': [");
    let double = format!("\"{operation}\": [");
    let marker = if source.contains(&single) {
        single
    } else if source.contains(&double) {
        double
    } else {
        panic!("bridge parity typed argument contract for `{operation}` is absent");
    };
    let start = source.find(&marker).expect("marker checked above");
    let rest = &source[start + marker.len()..];
    let end = rest.find(']').unwrap_or_else(|| {
        panic!("bridge parity typed argument contract for `{operation}` has no closing pattern")
    });
    assert!(
        rest[end..].starts_with("],") && !rest[..end].contains('['),
        "bridge parity typed argument contract for `{operation}` is not a closed argument array"
    );
    &rest[..end]
}

fn has_operation_contract(source: &str, operation: &str) -> bool {
    source.contains(&format!("'{operation}': ["))
        || source.contains(&format!("\"{operation}\": ["))
        || (operation == "map.grid.lasso"
            && source.contains("CLI_MAP_GRID_LASSO_OPERATION_CONTRACT"))
}

fn quoted_contract_items(contract: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    let mut rest = contract;
    while let Some((start, quote)) = rest
        .char_indices()
        .find(|(_, character)| *character == '\'' || *character == '"')
    {
        let after = &rest[start + quote.len_utf8()..];
        let end = after
            .find(quote)
            .expect("bridge parity argument contract has an unclosed quote");
        values.insert(after[..end].to_string());
        rest = &after[end + quote.len_utf8()..];
    }
    values
}

fn dotted_arguments(source: &str) -> BTreeSet<&str> {
    source
        .split("args.")
        .skip(1)
        .filter_map(|tail| {
            let end = tail
                .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
                .unwrap_or(tail.len());
            (end > 0).then_some(&tail[..end])
        })
        .collect()
}

/// True only for an object field in the projection currently under test.
/// Searching the whole adapter made `more`, `stale`, and `events` match
/// unrelated identifiers such as `furthermore` or `staleness` in comments or
/// helpers. Projection fields are rendered one per line in the owner; keep the
/// assertion tied to that return-object slice and its exact field spelling.
fn projects_field(slice: &str, field: &str) -> bool {
    slice.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with(&format!("{field}:")) || trimmed == format!("{field},")
    })
}

#[test]
fn every_project_context_command_has_one_closed_operation_owner() {
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    assert!(
        !allowlist.trim().is_empty(),
        "ds-web no longer exposed the CLI_OPERATIONS allowlist at the pinned marker; \
         refusing an empty string would make this negative messaging-door check vacuous"
    );
    assert!(
        !ds_cli_desktop::project::BRIDGE_OPS.is_empty(),
        "ds_cli_desktop::project::BRIDGE_OPS must declare operations for this parity check"
    );
    for operation in ds_cli_desktop::project::BRIDGE_OPS {
        assert_eq!(
            count(allowlist, &format!("\"{}\"", operation.operation)),
            1,
            "`{}` must appear exactly once in the desktop allowlist",
            operation.operation
        );
        assert_eq!(
            switch_case_count(&app.frontend, operation.operation),
            1,
            "`{}` must have exactly one frontend handler",
            operation.operation
        );
        let contract = operation_contract(&app.project, operation.operation);
        assert!(
            !contract.is_empty(),
            "`{}` has no typed project-adapter argument contract",
            operation.operation
        );
        for argument in operation.arguments {
            assert!(
                contract.contains(&format!("'{argument}'")),
                "desktop project sends `{argument}` to `{}`, but the adapter does not accept it",
                operation.operation
            );
        }
    }
}

#[test]
fn printing_operations_have_one_closed_application_owner() {
    let root = ds_web();
    let transport = source(&root, "src-tauri/src/cli_bridge.rs");
    let frontend = source(&root, "src/lib/desktop/cli-bridge.ts");
    let source = source(&root, "src/lib/printing/prepare.ts");
    let allowlist = between(&transport, "pub const CLI_OPERATIONS: &[&str] = &[", "];");
    for op in [
        &ds_cli_desktop::printing::TRANSFORMERS_OP,
        &ds_cli_desktop::printing::EXPORT_OP,
        &ds_cli_desktop::printing::PREVIEW_OP,
        &ds_cli_desktop::printing::SETTINGS_OP,
        &ds_cli_desktop::printing::PREPARE_OP,
        &ds_cli_desktop::printing::SEED_CONTEXT_OP,
        &ds_cli_desktop::custom_print::AREA_OP,
        &ds_cli_desktop::custom_print::EXPORT_OP,
        &ds_cli_desktop::custom_print::LIST_OP,
        &ds_cli_desktop::custom_print::ATTACH_OP,
    ] {
        assert_eq!(
            count(allowlist, &format!("\"{}\"", op.operation)),
            1,
            "{} must appear exactly once in the native allowlist",
            op.operation
        );
        assert_eq!(
            switch_case_count(&frontend, op.operation),
            1,
            "{} must have exactly one frontend handler",
            op.operation
        );
        let accepted = quoted_contract_items(operation_contract(&source, op.operation));
        let declared = op
            .arguments
            .iter()
            .map(|argument| (*argument).to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            accepted, declared,
            "{} arguments drifted between ds and the desktop",
            op.operation
        );
    }
}

#[test]
fn printing_artifact_operations_have_one_closed_application_owner() {
    let root = ds_web();
    let transport = source(&root, "src-tauri/src/cli_bridge.rs");
    let frontend = source(&root, "src/lib/desktop/cli-bridge.ts");
    let owner = source(&root, "src/lib/desktop/cli-reporter-artifact.ts");
    let allowlist = between(&transport, "pub const CLI_OPERATIONS: &[&str] = &[", "];");
    for operation in ds_cli_desktop::artifact::BRIDGE_OPS {
        assert_eq!(count(allowlist, &format!("\"{}\"", operation.operation)), 1);
        assert_eq!(switch_case_count(&frontend, operation.operation), 1);
        let accepted = quoted_contract_items(operation_contract(&owner, operation.operation));
        let declared = operation
            .arguments
            .iter()
            .map(|key| (*key).to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            accepted, declared,
            "{} artifact arguments drifted",
            operation.operation
        );
    }
}

#[test]
fn every_map_command_has_one_closed_operation_owner() {
    let app = app();

    let mut seen = BTreeSet::new();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    assert!(
        !allowlist.is_empty(),
        "the desktop CLI operation allowlist is absent"
    );
    assert!(
        !ds_cli_map::BRIDGE_OPS.is_empty(),
        "ds_cli_map::BRIDGE_OPS must declare operations for this parity check"
    );
    for operation in ds_cli_map::BRIDGE_OPS {
        assert!(
            seen.insert(operation.operation),
            "`{}` is declared twice by ds map; one semantic operation has one owner",
            operation.operation
        );

        assert_eq!(
            count(allowlist, &format!("\"{}\"", operation.operation)),
            1,
            "`{}` must appear exactly once in the desktop allowlist",
            operation.operation
        );
        assert_eq!(
            switch_case_count(&app.frontend, operation.operation),
            1,
            "`{}` must have exactly one frontend handler",
            operation.operation
        );

        if operation.operation == ds_cli_map::SURVEY_WORKING_AREA_DOWNLOAD.operation {
            assert_survey_download_arguments(&app, operation);
        } else {
            let owners = [
                &app.map,
                &app.map_layers,
                &app.map_profile,
                &app.map_profile_model,
                &app.map_profile_results,
                &app.map_profile_calculation,
                &app.map_profile_edit,
                &app.map_profile_filter,
                &app.map_grid_lasso,
                &app.survey,
            ]
            .into_iter()
            .filter(|source| has_operation_contract(source, operation.operation))
            .collect::<Vec<_>>();
            assert_eq!(
                owners.len(),
                1,
                "`{}` must have exactly one typed map adapter owner",
                operation.operation
            );
            let contract = operation_contract(owners[0], operation.operation);
            assert!(
                operation.arguments.is_empty() || !contract.is_empty(),
                "`{}` has no typed map-adapter argument contract",
                operation.operation
            );
            for argument in operation.arguments {
                let mut parts = argument.split('.');
                let top = parts.next().expect("declared argument is non-empty");
                assert!(
                    contract.contains(&format!("'{top}'")),
                    "ds map sends `{argument}` to `{}`, but its typed adapter does not accept `{top}`",
                    operation.operation
                );
                for nested in parts {
                    assert!(
                        app.map.contains(&format!("'{nested}'"))
                            || app.map_layers.contains(&format!("'{nested}'"))
                            || app.map_profile.contains(&format!("'{nested}'"))
                            || app.survey.contains(&format!("'{nested}'")),
                        "ds map sends `{argument}` to `{}`, but the adapter does not validate `{nested}`",
                        operation.operation
                    );
                }
            }
        }
    }
}

/// Survey admission moved into the shared kernel; the web forwards the same
/// closed arguments rather than maintaining a second TypeScript key list.
fn assert_survey_download_arguments(app: &App, operation: &BridgeOp) {
    assert_eq!(operation.arguments, &["entireProject"]);
    assert!(
        app.survey.contains(&format!(
            "const operation = '{}' as const;",
            operation.operation
        )),
        "the survey adapter must name the declared download operation"
    );
    assert!(app.survey.contains("surveyEvaluate({ operation: 'bridge', mode: 'download_admit', command: operation, args, uid, project: projectId })"),
        "the survey adapter must forward arguments and captured authority to kernel admission");
    assert!(
        app.frontend
            .contains("return downloadCliWorkingAreaSurvey(args);"),
        "the download executor must forward the bridge arguments to the survey adapter"
    );
    assert!(
        app.style_kernel.contains("loaded.surveyEvaluate(bytes)"),
        "the web survey binding must execute the shared kernel"
    );
    let admit = |args| {
        ds_command_kernel::survey::survey_evaluate(
            &serde_json::to_vec(&serde_json::json!({
                "operation": "bridge", "mode": "download_admit", "command": operation.operation,
                "args": args, "uid": "parity-user", "project": "parity-project",
            }))
            .unwrap(),
        )
    };
    assert_eq!(
        admit(serde_json::json!({"entireProject": true}))
            .expect("the kernel accepts the CLI's complete request"),
        serde_json::Value::Null
    );
    for args in [
        serde_json::json!({}),
        serde_json::json!({"entireProject": false}),
    ] {
        let error = admit(args).expect_err("unscoped survey download must fail");
        assert!(
            error.contains("entireProject must be true"),
            "unexpected admission refusal: {error}"
        );
    }
    let error = admit(serde_json::json!({"entireProject": true, "extra": true}))
        .expect_err("the kernel must reject undeclared argument keys");
    assert!(
        error.contains("does not accept extra"),
        "unexpected argument refusal: {error}"
    );
}

#[test]
fn design_open_has_one_exact_argument_and_keeps_typed_safety_refusals() {
    let app = app();
    let operation = ds_cli_map::DESIGN_OPEN.operation;
    let accepted = quoted_contract_items(operation_contract(&app.map, operation));
    let declared = ds_cli_map::DESIGN_OPEN
        .arguments
        .iter()
        .map(|argument| (*argument).to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        accepted, declared,
        "design context entry must accept exactly one transformer name"
    );

    let lowered = app.map.to_ascii_lowercase();
    for (marker, _) in ds_cli_map::design::open::REFUSAL_MARKERS {
        assert!(
            lowered.contains(marker),
            "the desktop design-open owner no longer emits `{marker}`"
        );
    }
    for field in [
        "contextType",
        "previousContext",
        "contextChanged",
        "editorReady",
        "mapReady",
    ] {
        assert!(
            app.map.contains(field),
            "the desktop design-open receipt no longer publishes `{field}`"
        );
    }
}

#[test]
fn design_version_history_has_closed_operations_and_exact_arguments() {
    let app = app();
    for (operation, expected) in [
        (
            &ds_cli_map::DESIGN_VERSION_PLAY,
            ["transformer", "version"].as_slice(),
        ),
        (
            &ds_cli_map::DESIGN_VERSION_COMPARE,
            ["transformer", "from", "to"].as_slice(),
        ),
    ] {
        assert_eq!(operation.arguments, expected, "{}", operation.operation);
        let accepted = quoted_contract_items(operation_contract(&app.map, operation.operation));
        let declared = operation
            .arguments
            .iter()
            .map(|argument| (*argument).to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(accepted, declared, "{}", operation.operation);
    }
}

#[test]
fn every_survey_control_plane_command_has_one_api_only_owner_and_exact_arguments() {
    let app = app();

    // The survey control plane no longer has a paired door. Survey semantics
    // moved into the kernel and the desktop deleted its three typed control
    // plane adapters (`cli-survey-forms.ts`, `cli-survey-project-forms.ts`,
    // `cli-survey-templates.ts`) on 2026-09-05; `ds survey` declares no bridge
    // operation at all. This suite read those three files until 2026-09-09, so
    // one absent file skipped every check in it — a skipped parity suite being
    // exactly what this file says is worse than no parity suite.
    //
    // What is left to hold is the door itself, and it is now held one level
    // lower than an empty list can hold it. `ds-cli-survey` does not depend on
    // `ds-cli-desktop` at all, so the crate cannot name a bridge operation,
    // declare one, or send one — the symbols do not exist in it. That is
    // `lens_core_boundary.rs`'s inventory, which refuses a crate that reaches
    // for the bridge again, and it is why the empty `BRIDGE_OPS` const this
    // assertion used to read was deleted rather than kept as a marker.
    //
    // The moment `ds survey` needs the window again, that suite fails first
    // and the command has to be classified before it can compile: either it
    // gains the headless owner this control plane already has, or it becomes a
    // `map.*` command in the lens crate with a typed adapter owner and the
    // argument-contract assertions that used to live here.

    // The three survey operations that do cross the bridge belong to `ds map
    // survey`, and their owner is the map's own adapter. They are checked with
    // the rest of that family; what must not happen is a control-plane
    // operation reappearing in the desktop allowlist with nothing on this side
    // declaring it.
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    for retired in [
        "survey.form.create",
        "survey.form.publish",
        "survey.template.create",
        "survey.project_form.attach",
    ] {
        assert!(
            !allowlist.contains(retired),
            "the desktop allowlist admits `{retired}`, but no ds command declares it"
        );
    }
}

#[test]
fn every_solar_command_has_one_closed_operation_owner_and_exact_arguments() {
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    assert!(
        !ds_cli_solar::paired::BRIDGE_OPS.is_empty(),
        "ds_cli_solar::paired::BRIDGE_OPS must declare operations for this parity check"
    );
    for operation in ds_cli_solar::paired::BRIDGE_OPS {
        assert_eq!(
            count(allowlist, &format!("\"{}\"", operation.operation)),
            1,
            "{} must appear exactly once in the native allowlist",
            operation.operation,
        );
        assert_eq!(
            switch_case_count(&app.frontend, operation.operation),
            1,
            "{} must have exactly one frontend executor",
            operation.operation,
        );
        {
            let adapter = if operation.operation.starts_with("solar.portfolio.batch.") {
                &app.solar_batch_adapter
            } else {
                &app.frontend
            };
            for argument in operation.arguments {
                assert!(
                    adapter.contains(&format!("args.{argument}")),
                    "ds solar sends `{argument}` to `{}`, but the paired adapter does not read that exact key",
                    operation.operation,
                );
            }
        }
        if operation.operation == "solar.run.start" {
            let start = between(
                &app.frontend,
                "async function start(",
                "\nasync function portfoliosForProject",
            );
            assert!(!start.is_empty(), "the Solar start adapter is absent");
            let consumed = dotted_arguments(start);
            let declared = operation.arguments.iter().copied().collect::<BTreeSet<_>>();
            assert!(
                declared.is_subset(&consumed),
                "solar.run.start must consume every key declared by ds: missing {:?}",
                declared.difference(&consumed).collect::<Vec<_>>(),
            );
            let legacy = BTreeSet::from([
                "currency",
                "project_years",
                "discount_rate",
                "representative_city",
                "language",
                "report_intents",
            ]);
            assert_eq!(
                consumed
                    .difference(&declared)
                    .copied()
                    .collect::<BTreeSet<_>>(),
                legacy,
                "solar.run.start may name only the retired assertion keys for an explicit refusal beyond the v4 CLI contract",
            );
        }
    }
}

/// A governed portfolio publication that never queued is a fact the
/// application owns, and `ds solar run result` reports the same one.
///
/// The native completion retains the aggregate in the shell's one sync store,
/// and the run records its verdict after the local commit: the receipt is
/// written first, and a retention failure is recorded on that already-succeeded
/// receipt instead of undoing it. A retention that never happened has no Sync
/// Center row, so the result receipt is the only place either surface can
/// learn it — which is why `ds` reads it there rather than deriving a
/// publication state of its own.
///
/// What has to agree is therefore the receipt field `ds` hand-copies, the bound
/// the application puts on it, the order that keeps the run successful, and the
/// "never queued" word both surfaces print. The application's own CLI
/// projection forwards the field; the guard below pins the spelling `ds`
/// reads and requires the projection to retain it.
#[test]
fn a_failed_portfolio_publication_stays_a_sync_lane_fact_on_a_succeeded_receipt() {
    let app = app();

    assert!(
        app.solar_portfolio_receipt
            .contains("publicationError?: string;"),
        "the portfolio receipt must still own an optional publication failure; \
         `ds solar run result` reports that field and nothing it derives itself"
    );
    assert!(
        app.solar_portfolio_receipt
            .contains(r#"if (receipt.status === "succeeded") return receipt.error === undefined;"#),
        "a succeeded portfolio receipt must stay valid while carrying a publication \
         failure; if the application starts rejecting one, `ds` must stop reporting \
         a success alongside it"
    );

    // From the local commit to the value returned: everything the run does
    // about publication happens here, after success is durable.
    let handoff = between(
        &app.solar_portfolio_run,
        "await putNativePortfolioBatchReceipt(receipt);",
        "return receipt;",
    );
    assert!(
        handoff.contains("report.publication_error"),
        "the native completion's retention verdict must be read after the local commit, \
         not before it"
    );
    assert!(
        handoff.contains("receipt.publicationError ="),
        "a publication that could not be queued must be recorded on the committed receipt"
    );
    assert!(
        !handoff.contains("receipt.status ="),
        "a failed publication must never relabel the calculation; `ds` reports the \
         same receipt as succeeded"
    );
    assert!(
        handoff.contains(&format!(
            ".slice(0, {})",
            ds_cli_solar::paired_run::PUBLICATION_ERROR_CHARS
        )),
        "ds bounds the reported reason at {} characters because the application does; \
         a moved bound makes ds refuse a reply the application considers valid",
        ds_cli_solar::paired_run::PUBLICATION_ERROR_CHARS
    );

    assert!(
        app.frontend.contains(&format!(
            "\"{}\"",
            ds_cli_solar::paired_run::PUBLICATION_NOT_QUEUED
        )),
        "`{}` is the application's own word for an intent that never queued; ds prints \
         the same one rather than inventing a second vocabulary",
        ds_cli_solar::paired_run::PUBLICATION_NOT_QUEUED
    );

    // The projection renames receipt fields to snake_case, including the
    // publication failure. Every field the client reads must stay present.
    let projection = between(&app.frontend, "portfolio: {", "};");
    for (owner, wire) in [
        ("portfolio.sourceRunId", "source_run_id"),
        ("portfolio.inputDigest", "input_digest"),
    ] {
        assert!(
            projects_field(projection, wire) && projection.contains(owner),
            "the portfolio projection no longer renames `{owner}` to `{wire}`; the key \
             ds reads is derived from that convention"
        );
    }
    assert!(
        projects_field(projection, ds_cli_solar::paired_run::PUBLICATION_ERROR_KEY)
            && projection.contains("portfolio.publicationError"),
        "the portfolio projection must forward the receipt's publication failure as `{}`",
        ds_cli_solar::paired_run::PUBLICATION_ERROR_KEY
    );
}

// ---------------------------------------------------------------------------
// Project Assets
// ---------------------------------------------------------------------------
//
// The assets family reaches the same closed door as Project Work. Its nine
// operations and their exact argument keys are pinned by the campaign
// contract on both sides, so a key that drifts here is a key the application
// silently ignores: `invoke` refuses an undeclared key before it is sent, but
// nothing refuses a declared key the adapter never reads.

// ---------------------------------------------------------------------------
// DS Grid local model lifecycle and project publication
// ---------------------------------------------------------------------------
//
// This family is the one where the *vocabulary* is the boundary, so the checks
// below are unusually specific. A reverted `dsgrid model create/import/convert`
// family conflated acquisition, local activation and project publication under
// one word; ds-web's own contract document names them apart, and everything
// here proves `ds` still says the same three things it does.

/// The four operations that must remain reachable without any project, and the
/// project operations that must not be among them.
/// The four model-management operations that left this door on 2026-09-18.
/// A working copy is a fact about a machine, so `ds dsgrid model list|
/// create-local|import-external|set-active` answer from the CLI's own
/// catalogue and the desktop carries nothing for them.
const DSGRID_RETIRED_OPERATIONS: &[&str] = &[
    "dsgrid.model.list",
    "dsgrid.model.create",
    "dsgrid.model.import",
    "dsgrid.model.set_active",
];
/// `dsgrid.model.prepare_project` left this door on 2026-09-20: readiness
/// is a fact about this machine's catalogue against the project's governed
/// heads, and a missing head is downloaded through `ds dsgrid project
/// download`'s door (contract dsgrid-authority/01, decision 19).
/// `dsgrid.profile.open` (2026-09-20) is the second: it asks the window to
/// open this machine's working copy by path and occupy Profile with it. It
/// reads no project — the copy is a fact about the machine — so it is the
/// one project-independent operation the adapter admits.
const DSGRID_PROJECT_OPERATIONS: &[&str] = &["dsgrid.model.publish", "dsgrid.profile.open"];

#[test]
fn every_dsgrid_model_command_has_one_closed_operation_owner_and_exact_arguments() {
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    assert!(
        !allowlist.trim().is_empty(),
        "the desktop CLI operation allowlist is absent"
    );

    let mut seen = BTreeSet::new();
    assert!(
        !ds_cli_dsgrid::model::BRIDGE_OPS.is_empty(),
        "ds_cli_dsgrid::model::BRIDGE_OPS must declare operations for this parity check"
    );
    for operation in ds_cli_dsgrid::model::BRIDGE_OPS {
        assert!(
            seen.insert(operation.operation),
            "`{}` is declared twice by ds dsgrid; one semantic operation has one owner",
            operation.operation
        );
        assert_eq!(
            count(allowlist, &format!("\"{}\"", operation.operation)),
            1,
            "`{}` must appear exactly once in the desktop allowlist",
            operation.operation
        );
        assert_eq!(
            switch_case_count(&app.frontend, operation.operation),
            1,
            "`{}` must have exactly one frontend handler",
            operation.operation
        );
        // Exact set, not containment: an argument the adapter accepts but `ds`
        // never sends is a door nothing here has reviewed, and an argument
        // `ds` sends that the adapter rejects is a runtime failure with a
        // compile-time cause.
        let accepted = quoted_contract_items(operation_contract(&app.dsgrid, operation.operation));
        let declared: BTreeSet<String> = operation
            .arguments
            .iter()
            .map(|argument| (*argument).to_string())
            .collect();
        assert_eq!(
            accepted, declared,
            "`{}` arguments drifted between ds and the desktop",
            operation.operation
        );
    }
    assert_eq!(
        seen.len(),
        DSGRID_PROJECT_OPERATIONS.len(),
        "the family sends exactly the two operations that are about the application's own \
         window: publish, and profile open"
    );
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    for retired in DSGRID_RETIRED_OPERATIONS {
        assert_eq!(
            count(allowlist, &format!("\"{retired}\"")),
            0,
            "{retired} is still admitted as a CLI bridge operation, but `ds dsgrid model` \
             no longer sends it"
        );
    }
}

#[test]
fn the_dsgrid_project_operations_still_name_the_applications_own_project() {
    // ds-web used to publish a project-independent operation list on each side
    // because the four local operations had to work in a projectless session.
    // They no longer cross a wire, so that list is empty and this suite holds
    // what is left: the one operation that DOES read the application's project
    // is the only one the door admits, and it carries no project of its own —
    // the application's selected project is the destination.
    let app = app();
    for operation in DSGRID_PROJECT_OPERATIONS {
        assert_eq!(
            switch_case_count(&app.frontend, operation),
            1,
            "`{operation}` must have exactly one frontend handler"
        );
        assert!(
            !quoted_contract_items(operation_contract(&app.dsgrid, operation))
                .iter()
                .any(|argument| argument.contains("project_id")),
            "`{operation}` names a project of its own"
        );
    }
}

#[test]
fn dsgrid_publication_refuses_rename_coupling_exactly_as_the_desktop_does() {
    let app = app();
    // Publishing a revision must not quietly become a metadata edit. ds-web
    // refuses `name` against an existing project model; `ds` refuses it
    // earlier, by name, so the round trip is never spent.
    assert!(
        app.dsgrid
            .contains("name is only accepted when publishing a new project model"),
        "the desktop owner no longer refuses rename coupling on publication"
    );
    assert!(
        app.dsgrid_contract.contains("quietly become a rename"),
        "the owning contract no longer states the rename boundary"
    );
    let refusals: BTreeSet<&str> = ds_cli_dsgrid::model::publish_version::COMMAND
        .refusals
        .iter()
        .map(|refusal| refusal.code)
        .collect();
    for code in [
        "project_model_rename_unsupported",
        "project_model_not_found",
        "publish_head_conflict",
        "new_project_model_incomplete",
        "ambiguous_publish_source",
        "confirmation_required",
    ] {
        assert!(
            refusals.contains(code),
            "`ds dsgrid publish-version` no longer documents `{code}`"
        );
    }
}

#[test]
fn dsgrid_bounds_and_typed_refusal_markers_match_the_desktop_owner() {
    let app = app();
    // The list bound this used to hold in step belonged to `dsgrid.model.list`,
    // which no longer crosses this door: `ds dsgrid model list` pages its own
    // catalogue, so the two sides have no shared bound left to drift.
    let kinds = between(
        &app.dsgrid,
        "MODEL_KINDS: readonly GridModelKind[] = [",
        "]",
    );
    assert!(!kinds.trim().is_empty(), "the model-kind list is absent");
    assert_eq!(
        quoted_contract_items(kinds),
        ds_cli_dsgrid::model::MODEL_KINDS
            .iter()
            .map(|kind| (*kind).to_string())
            .collect::<BTreeSet<_>>(),
        "`--kind` choices drifted from the project catalogue's own kinds"
    );

    // Every prose marker `ds` keys a named refusal on must still be prose the
    // application actually emits. An unmatched marker is not a silent
    // mistranslation here — the untranslated `desktop_refused` survives — but
    // it is a remedy a caller stops being given.
    let lowered = app.dsgrid.to_ascii_lowercase();
    for marker in ds_cli_dsgrid::model::LOCAL_MODEL_MISSING_MARKERS
        .iter()
        .chain(ds_cli_dsgrid::model::EAGER_READ_MARKERS)
        .chain(ds_cli_dsgrid::model::PROJECT_MODEL_MISSING_MARKERS)
        .chain(ds_cli_dsgrid::model::HEAD_MOVED_MARKERS)
    {
        assert!(
            lowered.contains(marker),
            "the desktop DS Grid owner no longer emits `{marker}`"
        );
    }
}

#[test]
fn the_dsgrid_bridge_admits_no_conversion_verb_no_revision_activation_and_no_bytes() {
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    assert!(!allowlist.trim().is_empty());
    // The reverted family's mistake, in operation names, on both sides.
    for forbidden in [
        "dsgrid.model.convert",
        "dsgrid.convert",
        "dsgrid.model.activate",
        "dsgrid.project.activate",
        "dsgrid.revision.activate",
        "dsgrid.model.register",
    ] {
        assert!(
            !allowlist.contains(&format!("\"{forbidden}\"")),
            "`{forbidden}` is admitted by the desktop allowlist"
        );
        assert_eq!(switch_case_count(&app.frontend, forbidden), 0);
        assert!(
            ds_cli_dsgrid::model::BRIDGE_OPS
                .iter()
                .all(|op| op.operation != forbidden),
            "`{forbidden}` is sent by ds"
        );
    }
    // PLS-CADD workspaces and `.bak` backups stay at the exchange boundary, and
    // both sides say so rather than accepting one quietly.
    assert!(
        app.dsgrid.contains(
            "PLS-CADD workspaces and .bak backups convert through the DS Grid exchange boundary"
        ),
        "the desktop owner no longer routes conversion sources to the exchange boundary"
    );

    // Bytes never travel. The declared argument contract is the exact place a
    // content field would have to appear to become transportable.
    let contract_block = between(
        &app.dsgrid,
        "export const CLI_DSGRID_OPERATION_CONTRACT",
        "export const CLI_DSGRID_OPERATION_NAMES",
    );
    assert!(!contract_block.trim().is_empty());
    for forbidden in ["bytes", "content", "base64", "blob"] {
        assert!(
            !contract_block.contains(forbidden),
            "the DS Grid operation contract admits a `{forbidden}` field"
        );
    }
    for op in ds_cli_dsgrid::model::BRIDGE_OPS {
        for argument in op.arguments {
            assert!(
                !matches!(*argument, "bytes" | "content" | "base64" | "blob" | "data"),
                "`{}` sends a content-carrying argument `{argument}`",
                op.operation
            );
        }
    }

    // Publication is project state only, and the receipt fields `ds` renders
    // are what keep "published" from reading as "now current". The two that
    // belonged to the retired local family — `became_active: false` on an
    // acquisition and `status: 'unchanged'` on an idempotent open — are now
    // facts about the CLI's own catalogue, asserted where that store lives.
    for field in ["active_model_changed:", "binding_recorded"] {
        assert!(
            app.dsgrid.contains(field),
            "the desktop DS Grid receipt no longer publishes `{field}`"
        );
    }
    assert!(
        app.dsgrid_contract
            .contains("There is **no durable exclusive project revision activation"),
        "the owning contract no longer denies a durable project revision activation authority"
    );
}

#[test]
fn the_admin_hierarchy_no_longer_travels_through_the_window() {
    // A country's boundaries are national reference data: no project, no
    // selection, nothing to render. They nevertheless reached the gateway
    // through the paired application, so `ds data admin-bounds list|read`
    // refused on any machine without a window — including the server where an
    // agent reads a hierarchy. Since 2026-09-18 both reads call the same
    // `/api/v1/admin/rwanda` through `ds-client-core::admin_bounds`.
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    assert!(
        !allowlist.is_empty(),
        "the desktop CLI operation allowlist is absent"
    );
    for retired in ["data.admin_bounds.list", "data.admin_bounds.read"] {
        assert_eq!(
            count(allowlist, &format!("\"{retired}\"")),
            0,
            "{retired} is still admitted as a CLI bridge operation, but `ds` no longer sends it"
        );
        assert_eq!(switch_case_count(&app.frontend, retired), 0);
        assert!(
            !has_operation_contract(&app.data, retired),
            "{retired} still has a typed desktop adapter contract"
        );
    }
    assert!(
        !ds_web()
            .join("src/lib/search-place/cli-boundary.ts")
            .exists(),
        "the CLI boundary adapter outlived its last caller"
    );
    // Search place itself is untouched: the application still materialises a
    // boundary as a local sketch layer when an operator asks it to. What went
    // is the CLI's way of driving that gesture from outside the window.
    assert!(app.materialize.contains("materializeAdminBoundaries"));
}

#[test]
fn the_combined_report_archive_no_longer_travels_through_the_window() {
    // Owner ruling 2026-09-26: a Compounded Report archive is requested, and
    // grouped, only through the headless `ds report project compounded`. The
    // paired `ds map design batch report` and the Desktop operation it sent,
    // `design.report.export_batch`, were deleted on both sides, so neither
    // can return as a second, window-bound way to author one.
    let retired = "design.report.export_batch";
    assert!(
        !ds_cli_map::BRIDGE_OPS
            .iter()
            .any(|operation| operation.operation == retired),
        "`ds map` declares {retired} again; Compounded Report archives are `ds report project compounded` only"
    );
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    assert!(
        !allowlist.is_empty(),
        "the desktop CLI operation allowlist is absent"
    );
    assert_eq!(
        count(allowlist, &format!("\"{retired}\"")),
        0,
        "{retired} is still admitted as a CLI bridge operation, but `ds` no longer sends it"
    );
    assert_eq!(switch_case_count(&app.frontend, retired), 0);
    assert!(
        !has_operation_contract(&app.map, retired),
        "{retired} still has a typed desktop adapter contract"
    );
}

#[test]
fn the_data_domain_sends_only_operations_the_desktop_owns() {
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    // What is left is local compute this application's own components serve:
    // the Rwanda DEM engine and the project's pinned boundary asset.
    assert_eq!(ds_cli_data::BRIDGE_OPS.len(), 4);
    assert!(
        !ds_cli_data::BRIDGE_OPS.is_empty(),
        "ds_cli_data::BRIDGE_OPS must declare operations for this parity check"
    );
    for operation in ds_cli_data::BRIDGE_OPS {
        assert_eq!(
            count(allowlist, &format!("\"{}\"", operation.operation)),
            1,
            "the native allowlist must own the data operation exactly once"
        );
        assert_eq!(switch_case_count(&app.frontend, operation.operation), 1);
        // The data family has two typed adapters — local source work in
        // `cli-data.ts`, the project dataset rooms in `cli-project-data.ts` —
        // and exactly one of them owns each operation. Reading only the first
        // reported `data.project_cache.*` as having no contract at all, which
        // is the opposite of what is true.
        let owners = [&app.data, &app.project_data]
            .into_iter()
            .filter(|source| has_operation_contract(source, operation.operation))
            .collect::<Vec<_>>();
        assert_eq!(
            owners.len(),
            1,
            "`{}` must have exactly one typed data adapter owner",
            operation.operation
        );
        let accepted = quoted_contract_items(operation_contract(owners[0], operation.operation));
        let declared: BTreeSet<String> = operation
            .arguments
            .iter()
            .map(|argument| (*argument).to_string())
            .collect();
        assert_eq!(
            accepted, declared,
            "`{}` arguments drifted between ds and the desktop",
            operation.operation
        );
    }
    assert!(!app.data.contains("mapInstance"));
    assert!(!app.data.contains("maplibre-gl"));

    // Typed frontend errors must remain objects across the shell and the CLI;
    // otherwise every authority failure silently regresses to desktop_refused.
    assert!(app.frontend.contains("cliCompletionError(error)"));
    assert!(app.cli_errors.contains("CliStructuredRefusal"));
    assert!(app.transport.contains("StructuredInvocationError"));
    assert!(app.transport.contains("auth_context_mismatch"));
}

#[test]
fn platform_reliability_no_longer_travels_through_the_window() {
    // `ds sre overview` and `ds sre events` were `paired_availability`, so on a
    // machine with no window they refused with `desktop_not_paired` — and a
    // server with no window is exactly where an operator asks how the platform
    // is. Both were pure server reads: the desktop adapter called
    // `brainGet('/api/v1/sre/overview')` and the tabular `query_table` stream,
    // held no state of its own, and never touched an active project.
    //
    // Since 2026-09-18 there is one route, through `ds-client-core::sre`. The
    // desktop's two operations are retired, its adapter is deleted, and the
    // crate no longer depends on `ds-cli-desktop` (`lens_core_boundary.rs`
    // holds that).
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    for retired in ["sre.overview", "sre.events"] {
        assert_eq!(
            count(allowlist, &format!("\"{retired}\"")),
            0,
            "{retired} is still admitted as a CLI bridge operation, but `ds sre` \
             no longer sends it"
        );
        assert_eq!(
            switch_case_count(&app.frontend, retired),
            0,
            "{retired} still has a frontend handler"
        );
    }
    assert!(
        !ds_web().join("src/lib/desktop/cli-sre.ts").exists(),
        "the paired SRE adapter outlived its last caller"
    );
    // The Reliability page itself is untouched: a person at a window still
    // reads the same two owner answers on /sre.
    assert!(app.reliability_page.contains("fetchSreOverview"));
}

#[test]
fn the_governed_style_documents_no_longer_travel_through_the_window() {
    // A style document is governed shared state behind ds-brain: a project, a
    // ref, a publication. `ds style` reached it two ways — the restored native
    // user, which was the declared default, and, under `--host desktop`, the
    // paired application calling `get_style_catalog` and `update_style` for
    // the same project with the same kernel planner in between. Two routes to
    // one document, and the windowed one silently ignored `--transformer`,
    // so the same command answered `observed: null` on one host and the
    // canonical field types on the other.
    //
    // Since 2026-09-18 there is one route. `ds-cli-style` does not depend on
    // `ds-cli-desktop` (`lens_core_boundary.rs` holds that), the desktop's
    // nine operations are retired, and the adapter is deleted.
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    assert!(
        !allowlist.is_empty(),
        "the desktop CLI operation allowlist is absent"
    );
    for retired in [
        "style.list",
        "style.read",
        "style.seed.create",
        "style.appearance.set",
        "style.label.set",
        "style.print.create",
        "style.dimension.set",
        "style.dimension.clear",
        "style.cartography.set",
    ] {
        assert_eq!(
            count(allowlist, &format!("\"{retired}\"")),
            0,
            "{retired} is still admitted as a CLI bridge operation, but `ds style` \
             no longer sends it"
        );
        assert_eq!(
            switch_case_count(&app.frontend, retired),
            0,
            "{retired} still has a frontend handler"
        );
    }
    let root = ds_web();
    assert!(
        !root.join("src/lib/desktop/cli-style.ts").exists(),
        "the style adapter outlived its last caller"
    );
}

#[test]
fn style_cartography_offers_only_the_vocabulary_the_renderer_paints() {
    // The bridge is gone, but this parity is not about a bridge. `ds style
    // cartography` publishes a name — a fill pattern, a line type, a tile
    // size — into a governed document that the map then has to paint. A name
    // ds offers and the renderer does not know is a published style nothing
    // draws, and neither the kernel nor the gateway would notice.
    let app = app();

    // MapLibre repeats a pattern image by tiling it, so a tile size that is
    // not a power of two seams at every edge. `ds` refuses the others at the
    // door; that refusal is only correct while it is the same list the
    // application rasterises to.
    let spacings = ds_cli_style::PATTERN_SPACINGS
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    assert!(
        app.style_fill_pattern
            .contains(&format!("const FILL_PATTERN_SPACINGS = [{spacings}]")),
        "the renderer must rasterise exactly the seamless pattern tile sizes ds offers: [{spacings}]"
    );

    // The fill-pattern vocabulary is the renderer's own — unlike the dash
    // presets, which ds-brain publishes — so every name a caller may pass
    // must appear in it. `directional` is the one line type that is a marker
    // rather than a dash, and the renderer is what knows that.
    let fill_patterns = ds_cli_style::cartography::plan::COMMAND
        .arg("fill-pattern")
        .expect("--fill-pattern is declared")
        .choices;
    assert!(
        !fill_patterns.is_empty(),
        "ds style must declare fill-pattern choices"
    );
    let renderer_patterns = quoted_contract_items(between(
        &app.style_fill_pattern,
        "export const FILL_PATTERN_KINDS = [",
        "] as const",
    ));
    assert_eq!(
        renderer_patterns,
        fill_patterns
            .iter()
            .filter(|name| **name != "solid")
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "CLI hatch choices must equal the renderer's pattern vocabulary"
    );
    assert!(
        fill_patterns.contains(&"solid"),
        "CLI must expose solid to clear hatching"
    );
    let cleared = ds_command_kernel::style_authoring::apply_pattern(
        serde_json::json!({"type": "fill", "metadata": {"fill_pattern": "dots"}}),
        None,
    )
    .expect("the shared kernel clears hatching for solid");
    assert!(cleared["metadata"].get("fill_pattern").is_none());
    // Line authoring moved to the shared kernel. Prove the web binding and the
    // actual native transform rather than pinning the deleted TS implementation.
    assert!(
        app.style_kernel.contains("loaded.transformStyle("),
        "the web must bind line authoring to the shared kernel transform"
    );
    let styled = ds_command_kernel::style_authoring::apply_line_type(
        serde_json::json!({"type": "line", "paint": {}, "metadata": {}}),
        "directional",
        &serde_json::Map::new(),
        &serde_json::Map::new(),
    )
    .expect("the shared kernel authors directional lines");
    assert_eq!(styled["metadata"]["line_marker"], "arrow");
    let marker_renderer = between(
        &app.style_renderer,
        "export function directionalMarkerStyle(",
        "export function",
    );
    assert!(
        marker_renderer.contains("style.metadata?.line_marker !== 'arrow'")
            && marker_renderer.contains("clone(recipe)")
            && marker_renderer.contains("style_renderer_recipe_required"),
        "the renderer must consume the kernel's arrow marker through the governed companion recipe"
    );
}

#[test]
fn the_global_reference_publications_no_longer_travel_through_the_window() {
    // A reference publication belongs to the product: one governed catalog for
    // the whole system, gated by `global_tiles.manage` at the gateway. It
    // reached that gateway through the paired application anyway, so
    // `ds tile global …` refused on any machine without a window — including
    // the server where a publication is most naturally driven.
    //
    // Since 2026-09-18 the four actions travel on the same `/api/v1/tiles`
    // route through `ds-client-core::global_tiles`, and `ds-cli-tile` does not
    // depend on `ds-cli-desktop` at all (`lens_core_boundary.rs` holds that).
    let root = ds_web();
    let transport = source(&root, "src-tauri/src/cli_bridge.rs");
    let allowlist = between(&transport, "pub const CLI_OPERATIONS: &[&str] = &[", "];");
    assert!(
        !allowlist.is_empty(),
        "the desktop CLI operation allowlist is absent"
    );
    for retired in [
        "tile.global.catalog",
        "tile.global.list",
        "tile.global.generate",
        "tile.global.status",
    ] {
        assert_eq!(
            count(allowlist, &format!("\"{retired}\"")),
            0,
            "{retired} is still admitted as a CLI bridge operation, but `ds tile global` \
             no longer sends it"
        );
    }
    assert!(
        !root.join("src/lib/desktop/cli-global-tiles.ts").exists(),
        "the global tile adapter outlived its last caller"
    );
}

#[test]
fn the_shared_backlog_no_longer_travels_through_the_window() {
    // `ds feedback` reached the shared backlog two ways: the native user when
    // `--target server` was given, and the paired desktop otherwise. Two
    // routes to one backlog meant two sets of refusals for the same
    // conditions — the native owner returned `feedback_not_found` typed, while
    // the desktop returned one `desktop_refused` whose prose this suite had to
    // keep in parity with a marker list.
    //
    // Since 2026-09-18 there is one route. The crate does not depend on
    // `ds-cli-desktop` (`lens_core_boundary.rs` holds that), the desktop's
    // three operations are retired, and the adapter is deleted.
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    for retired in ["feedback.submit", "feedback.list", "feedback.close"] {
        assert_eq!(
            count(allowlist, &format!("\"{retired}\"")),
            0,
            "{retired} is still admitted as a CLI bridge operation, but `ds feedback` \
             no longer sends it"
        );
    }
    // The window retains human feedback; agent submissions belong to the
    // native client and must not be restored as a window-owned path.
    assert!(
        app.feedback_submit.contains("reporter_kind: 'human'")
            && app
                .feedback_submit
                .contains("export async function submitHumanFeedback("),
        "the application's human feedback submission lost its typed reporter kind or executor"
    );
}

#[test]
fn the_global_catalog_no_longer_travels_through_the_window() {
    // `ds library global …` governs a GLOBAL catalog: a library release
    // belongs to the product, not to a project, and never to a window. It
    // reached the gateway through the paired desktop anyway, which forwarded
    // the same bodies to the same path — so the commands declared `Available`
    // and then refused `not_paired` on any machine without a desktop.
    //
    // Since 2026-09-18 they call `ds-client-core`'s closed `grid_catalog`
    // owner directly, so there is no CLI bridge operation left to hold in
    // parity with a desktop adapter. What replaces this suite's loop is one
    // level lower: `ds-cli-library` does not depend on `ds-cli-desktop`, which
    // `lens_core_boundary.rs` pins, so the crate cannot name a bridge
    // operation at all.
    //
    // The desktop's own catalogue adapter stays: the Library screen uses it.
    // Only the CLI's door is gone.
    let app = app();
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    for retired in [
        "catalog.library.list",
        "catalog.library.read",
        "catalog.library.releases",
        "catalog.example.list",
        "catalog.example.revisions",
        "catalog.artifact.upload",
        "catalog.library.publish",
        "catalog.example.publish",
        "catalog.library.publish-prepared",
        "catalog.example.publish-prepared",
        "catalog.library.lifecycle",
        "catalog.example.lifecycle",
        "catalog.fork-example",
    ] {
        assert_eq!(
            count(allowlist, &format!("\"{retired}\"")),
            0,
            "{retired} is still admitted as a CLI bridge operation; `ds library global` \
             no longer sends it, so the desktop must not keep a door open for it"
        );
    }
}

#[test]
fn map_bounds_and_session_projection_match_the_desktop_owner() {
    let app = app();

    assert!(
        app.map.contains(&format!(
            "const MAX_LAYER_FEATURES = {}",
            grouped(ds_cli_map::MAX_LAYER_FEATURES)
        )),
        "the desktop must enforce the same temporary-layer bound as ds map"
    );
    assert!(
        app.map.contains(&format!(
            "const MAX_SELECTOR_IDS = {}",
            grouped(ds_cli_map::design::MAX_SELECTOR_IDS)
        )),
        "the desktop must enforce the same selector-id bound as ds map"
    );
    assert!(
        app.design.contains(&format!(
            "MAX_DESIGN_FEATURE_SAMPLE = {}",
            ds_cli_map::MAX_FEATURE_SAMPLE
        )),
        "the desktop must enforce the same design sample bound as ds map"
    );

    let root = ds_web();
    let create = source(&root, "src/lib/design/create-from-selection.ts");
    assert!(
        create.contains(&format!(
            "MAX_CREATE_FROM_SELECTION = {}",
            grouped(ds_cli_map::MAX_CREATE_FEATURES)
        )),
        "the desktop must enforce the same create bound as ds map"
    );

    for field in [
        ds_cli_map::SNAPSHOT_OPEN,
        ds_cli_map::SNAPSHOT_LAYERS,
        ds_cli_map::SNAPSHOT_LAYER_ID,
        "cliOwned",
        "center",
        "zoom",
        "bbox",
    ] {
        assert!(
            app.map.contains(field),
            "ds map view reads `{field}`, but the CLI map session projection no longer publishes it"
        );
    }
    assert!(
        app.transport.contains("MAX_MAP_LAYERS"),
        "the native bridge must bound the map session projection before returning it"
    );
}

#[test]
fn map_working_set_stays_a_closed_projection_of_the_desktop_owner() {
    let app = app();

    assert!(
        app.map.contains("return applyCliWorkingSet(args"),
        "the map bridge must delegate to the curated Working-set adapter"
    );
    for semantic in [
        "loadPinnedTransformers",
        "getPinnedTransformersForProject",
        "resolvedSelectionLiveIds",
        "removePinnedTransformers",
        "clearAllPins",
        "showWorkingSet",
    ] {
        assert!(
            app.map_working_set.contains(semantic),
            "the Working-set bridge no longer reuses Desktop-owned `{semantic}`"
        );
    }
    for mode in ["read", "unpin", "clear", "load"] {
        assert!(
            app.map_working_set.contains(&format!("'{mode}'")),
            "the Desktop bridge lost the `{mode}` Working-set intent"
        );
    }
    for forbidden in [
        "persistentGet",
        "persistentSet",
        "querySelector",
        "dispatchEvent",
        "invokeDesktop",
    ] {
        assert!(
            !app.map_working_set.contains(forbidden),
            "the curated Working-set adapter grew forbidden `{forbidden}` state/control access"
        );
    }
}

/// TypeScript writes large numeric literals with underscore separators.
fn grouped(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push('_');
        }
        out.push(digit);
    }
    out
}

#[test]
fn analysis_ids_and_typed_refusals_stay_owned_by_the_desktop() {
    let app = app();

    let prefix = ds_cli_map::ANALYSIS_SKETCH_PREFIX.trim_end_matches(':');
    assert!(
        app.analysis
            .contains(&format!("id: `{prefix}:${{layer.id}}`")),
        "ds map composes analysis ids as `{prefix}:<layer id>`, but the desktop no longer resolves them"
    );

    let lowered = app.map.to_ascii_lowercase();
    assert!(
        ds_cli_map::SIGNED_OUT_MARKERS
            .iter()
            .any(|marker| lowered.contains(marker)),
        "no signed-out marker remains in the desktop map adapter"
    );
    assert!(
        lowered.contains(ds_cli_map::design::save::CONFLICT_MARKER),
        "the desktop map adapter no longer carries the save-conflict marker"
    );
}

#[test]
fn retired_automation_bridge_is_not_a_map_fallback() {
    let app = app();

    for source in [
        &app.transport,
        &app.frontend,
        &app.map,
        &app.survey,
        &app.design,
    ] {
        assert!(
            !source.contains("agent_bridge") && !source.contains("agent-bridge"),
            "paired-domain CLI support must not restore a retired automation bridge"
        );
    }
}

// ---------------------------------------------------------------------------
// The instance registry: one shape, and both hosts spell it the same way
// ---------------------------------------------------------------------------

/// The text of one Rust item in the shell's source, from its opening line to
/// the matching brace. Reading the whole file for a field name would find it in
/// any struct; a bound to one item is what makes the pin mean something.
fn item<'a>(source: &'a str, opening: &str) -> Option<&'a str> {
    let start = source.find(opening)?;
    let mut depth = 0usize;
    for (offset, character) in source[start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[start..start + offset + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Every field `ds` reads out of a published descriptor, and every field it
/// reads out of the authenticated handshake, is a field the shell writes —
/// spelled identically.
///
/// This is the pin the rest of instance routing rests on. A descriptor is read
/// before anything is probed and a session is read before anything is sent, so
/// a renamed field is not a compile error on either side: it is a machine that
/// silently stops pairing, or worse, one that pairs and reports the wrong
/// project. Neither side can rename one alone.
#[test]
fn the_descriptor_and_session_this_client_reads_are_the_shells_own() {
    let app = app();

    let descriptor = item(&app.transport, "struct Descriptor<'a> {")
        .expect("the shell publishes a descriptor struct");
    for field in [
        // The four fields version 1 always had. `ds` sends the token to the
        // url and nothing else, so these three are load-bearing on every call.
        "version",
        "url",
        "token",
        "pid",
        // Added since, all optional, so an older `ds` still reads the file and
        // a newer one derives what an older file does not say.
        "instance_id",
        "lane",
        "build",
        "started_at_ms",
    ] {
        assert!(
            descriptor.contains(&format!("{field}:")),
            "the shell's descriptor no longer publishes `{field}`, which \
             `ds_cli_desktop::discover` reads:\n{descriptor}"
        );
    }
    // A profile is deliberately absent: the reader knows which install
    // directory it read the file from, and a profile spelled in the file could
    // only disagree with that. `admit_descriptor` takes the read-under profile.
    assert!(
        !descriptor.contains("profile:"),
        "a descriptor that names its own profile changes what admission means; \
         the kernel takes the directory the file was read from"
    );

    // The registry directory is where every live instance publishes; the shell
    // writes nothing else (the per-profile legacy file is retired).
    let path = ds_cli_desktop::discover::DESCRIPTOR_DIR;
    assert!(
        app.transport.contains(&format!("\"{path}\"")),
        "the shell no longer writes `{path}`, which discovery enumerates"
    );

    let session = item(&app.transport, "struct SessionView {").expect("a session view");
    let window = item(&app.transport, "struct WindowView {").expect("a window view");
    let published = item(&app.transport, "fn published_session(").expect("the published session");
    for (field, source, what) in [
        (
            "session_revision",
            session,
            "the liveness proof and the fence",
        ),
        ("uid", session, "the account a candidate is compared on"),
        ("lane", session, "the lane a candidate is compared on"),
        ("credential_audience_sha256", session, "the audience"),
        ("project", session, "which instance may serve project work"),
        (
            "windows",
            session,
            "the projects an instance holds in its views",
        ),
        ("label", window, "which view a caller may pin"),
        ("generation", window, "this view's context generation"),
        ("instance_id", published, "the instance naming itself"),
        ("build", published, "safe metadata `ds desktop list` shows"),
        (
            "started_at_ms",
            published,
            "safe metadata `ds desktop list` shows",
        ),
    ] {
        assert!(
            source.contains(field),
            "the shell's session no longer publishes `{field}` ({what}), which \
             `ds_cli_desktop::discover` reads"
        );
    }

    // And the client reads exactly that shape. The names above are a source
    // scan; this is the same names put through the reader, so a rename on
    // *this* side fails here too rather than quietly producing an instance
    // that can serve nobody.
    let descriptor = ds_cli_desktop::discover::Descriptor {
        url: "http://127.0.0.1:41234".to_owned(),
        token: "0123456789abcdef0123456789abcdef".to_owned(),
        pid: 4711,
        instance_id: "11111111111111111111111111111111".to_owned(),
        identity: ds_command_kernel::desktop_instance::Identity::Minted,
        profile: Some("canary".to_owned()),
        path: PathBuf::from("cli-bridge.d/one.json"),
        window: None,
    };
    let handshake = ds_cli_desktop::discover::handshake_of(
        &descriptor,
        &serde_json::json!({
            "session_revision": 7,
            "map_revision": 3,
            "connected": true,
            "signed_in": true,
            "uid": "uid-a",
            "lane": "canary",
            "credential_audience_sha256": "c".repeat(64),
            "project": "project-a",
            "instance_id": "11111111111111111111111111111111",
            "build": "2026.9.12+1",
            "started_at_ms": 1_757_000_000_000u64,
            "windows": [
                {"label": "main", "project": "project-a", "generation": 2},
                {"label": "workspace-2", "project": "project-b", "generation": 1},
            ],
        }),
    );
    let ds_cli_desktop::discover::Handshake::Session(candidate) = handshake else {
        panic!("the shell's own session shape must read as a routable candidate");
    };
    assert_eq!(candidate.uid, "uid-a");
    assert_eq!(candidate.lane, "canary");
    assert_eq!(candidate.project.as_deref(), Some("project-a"));
    assert_eq!(candidate.session_revision, 7);
    assert_eq!(candidate.build.as_deref(), Some("2026.9.12+1"));
    // A project open only in a second window still makes this instance the one
    // that can serve that work, which is why the roster is read at all.
    assert!(candidate.opens("project-b"));
}

/// The identity fence stays the five fields the shell verifies.
///
/// It is tempting to add the instance id to it now that one exists. The
/// instance is already fenced by transport — the operation is posted to that
/// instance's own loopback origin with that instance's own pairing token — and
/// the shell's fence struct denies unknown fields, so a sixth field would make
/// every call to an older desktop fail at the door.
#[test]
fn the_invocation_fence_is_still_the_five_fields_the_shell_verifies() {
    let app = app();
    let fence = item(&app.transport, "struct IdentityFence {").expect("the shell's fence");
    let declared: Vec<&str> = fence
        .lines()
        .filter_map(|line| line.trim().strip_suffix(','))
        .filter_map(|line| line.split(':').next())
        .filter(|name| !name.is_empty() && !name.starts_with('#') && !name.starts_with("//"))
        .collect();
    assert_eq!(
        declared,
        vec![
            "uid",
            "lane",
            "credential_audience_sha256",
            "project",
            "session_revision",
            "context_generation"
        ],
        "the shell's identity fence changed shape; `ds_cli_desktop::bridge::IdentityFence` \
         sends exactly these fields and the shell denies unknown ones"
    );
    // The one added field is optional on both sides, which is the only way it
    // could be added at all: the shell denies unknown fields, so a required
    // field would refuse every call from a `ds` that predates it, and a `ds`
    // that always sent one would refuse against a desktop that predates it.
    assert!(
        fence.contains("skip_serializing_if") && fence.contains("default"),
        "`context_generation` must stay optional in both directions:\n{fence}"
    );
    assert!(
        !fence.contains("instance_id"),
        "the instance is fenced by transport — its own loopback origin and its \
         own pairing token — not by a sixth fence field"
    );
    // Which window owns the session is the shell's rule, and `ds` reads the
    // generation of that window and no other.
    assert!(
        app.transport.contains(&format!(
            "OWNER_WINDOW_LABEL: &str = \"{}\"",
            ds_cli_desktop::discover::OWNER_WINDOW_LABEL
        )),
        "the shell's owner window is no longer `{}`, so the generation `ds` \
         sends would be another view's",
        ds_cli_desktop::discover::OWNER_WINDOW_LABEL
    );
}
