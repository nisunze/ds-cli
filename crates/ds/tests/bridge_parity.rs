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
fn ds_web() -> Option<PathBuf> {
    let root = match std::env::var_os("DS_WEB_DIR") {
        Some(explicit) => PathBuf::from(explicit),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-web"),
    };
    let root = root.canonicalize().unwrap_or(root);
    root.is_dir().then_some(root)
}

fn skip(reason: &str) {
    let looked_in = match std::env::var_os("DS_WEB_DIR") {
        Some(explicit) => PathBuf::from(explicit),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-web"),
    };
    eprintln!(
        "SKIPPED: {reason}\n  This check proves ds map sends only operations the \
         paired desktop CLI bridge owns.\n  Looked in: {}\n  Set DS_WEB_DIR to \
         the ds-web checkout to run it.",
        looked_in.display()
    );
}

struct App {
    transport: String,
    frontend: String,
    project: String,
    map: String,
    map_layers: String,
    survey: String,
    design: String,
    design_collaboration: String,
    data: String,
    project_data: String,
    cli_errors: String,
    materialize: String,
    analysis: String,
    assets: String,
    dsgrid: String,
    dsgrid_contract: String,
    style_fill_pattern: String,
    style_line_type: String,
    sync_center: String,
    feedback_submit: String,
    solar_seed_client: String,
    solar_seed_pure: String,
    solar_seed_adapter: String,
    solar_portfolio_run: String,
    solar_batch_adapter: String,
    solar_portfolio_receipt: String,
    reliability_page: String,
}

fn app() -> Option<App> {
    let root = ds_web()?;
    let read = |leaf: &str| std::fs::read_to_string(root.join(leaf)).ok();
    Some(App {
        transport: read("src-tauri/src/cli_bridge.rs")?,
        frontend: read("src/lib/desktop/cli-bridge.ts")?,
        project: read("src/lib/desktop/cli-project.ts")?,
        map: read("src/lib/desktop/cli-map.ts")?,
        map_layers: read("src/lib/desktop/cli-map-layers.ts")?,
        survey: read("src/lib/desktop/cli-survey.ts")?,
        design: read("src/lib/desktop/cli-map-design.ts")?,
        design_collaboration: read("src/lib/desktop/cli-design.ts")?,
        data: read("src/lib/desktop/cli-data.ts")?,
        project_data: read("src/lib/desktop/cli-project-data.ts")?,
        cli_errors: read("src/lib/desktop/cli-errors.ts")?,
        materialize: read("src/lib/search-place/materialize.ts")?,
        analysis: read("src/lib/analysis/outliers.ts")?,
        assets: read("src/lib/desktop/cli-assets.ts")?,
        dsgrid: read("src/lib/desktop/cli-dsgrid.ts")?,
        dsgrid_contract: read("docs/dsgrid-local-model-and-project-publication-contract.md")?,
        style_fill_pattern: read("src/lib/styles/fill-pattern.ts")?,
        style_line_type: read("src/lib/styles/line-type.ts")?,
        sync_center: read("src/lib/desktop/cli-sync-center.ts")?,
        feedback_submit: read("src/lib/feedback/submit.ts")?,
        solar_seed_client: read("src/lib/api/solar-seed.ts")?,
        solar_seed_pure: read("src/lib/solar/seed.ts")?,
        solar_seed_adapter: read("src/lib/desktop/cli-solar-seed.ts")?,
        solar_batch_adapter: read("src/lib/desktop/cli-solar-portfolio-batch.ts")?,
        solar_portfolio_run: read("src/lib/solar/native-batch.ts")?,
        reliability_page: read("src/routes/sre/+page.svelte")?,
        solar_portfolio_receipt: read("src/lib/solar/native-portfolio-batches.ts")?,
    })
}

#[test]
fn every_sync_center_command_has_one_closed_operation_owner() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
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
    let Some(start) = source.find(open) else {
        return "";
    };
    let rest = &source[start + open.len()..];
    &rest[..rest.find(close).unwrap_or(rest.len())]
}

fn operation_contract<'a>(source: &'a str, operation: &str) -> &'a str {
    let single = format!("'{operation}': [");
    let double = format!("\"{operation}\": [");
    let marker = if source.contains(&single) {
        single
    } else if source.contains(&double) {
        double
    } else {
        return "";
    };
    let start = source.find(&marker).expect("marker checked above");
    let rest = &source[start + marker.len()..];
    &rest[..rest.find("],").unwrap_or(rest.len())]
}

fn has_operation_contract(source: &str, operation: &str) -> bool {
    source.contains(&format!("'{operation}': [")) || source.contains(&format!("\"{operation}\": ["))
}

fn quoted_contract_items(contract: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    let mut rest = contract;
    while let Some((start, quote)) = rest
        .char_indices()
        .find(|(_, character)| *character == '\'' || *character == '"')
    {
        let after = &rest[start + quote.len_utf8()..];
        let Some(end) = after.find(quote) else {
            break;
        };
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    let Some(root) = ds_web() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let transport = std::fs::read_to_string(root.join("src-tauri/src/cli_bridge.rs")).unwrap();
    let frontend = std::fs::read_to_string(root.join("src/lib/desktop/cli-bridge.ts")).unwrap();
    let source = std::fs::read_to_string(root.join("src/lib/printing/prepare.ts")).unwrap();
    let allowlist = between(&transport, "pub const CLI_OPERATIONS: &[&str] = &[", "];");
    for op in [
        &ds_cli_desktop::printing::TRANSFORMERS_OP,
        &ds_cli_desktop::printing::EXPORT_OP,
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
fn every_map_command_has_one_closed_operation_owner() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

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

        let owners = [&app.map, &app.map_layers, &app.survey]
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
            !contract.is_empty(),
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
                        || app.survey.contains(&format!("'{nested}'")),
                    "ds map sends `{argument}` to `{}`, but the adapter does not validate `{nested}`",
                    operation.operation
                );
            }
        }
    }
}

#[test]
fn design_open_has_one_exact_argument_and_keeps_typed_safety_refusals() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
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
        // Seeding is the one Solar family the application answers from its own
        // typed adapter rather than inside the dispatcher, because it needs no
        // run, workspace or native engine — so its keys are checked as an exact
        // set against that declared contract, not by grepping the dispatcher.
        if has_operation_contract(&app.solar_seed_adapter, operation.operation) {
            let accepted = quoted_contract_items(operation_contract(
                &app.solar_seed_adapter,
                operation.operation,
            ));
            let declared = operation
                .arguments
                .iter()
                .map(|argument| (*argument).to_string())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                accepted, declared,
                "`{}` arguments drifted between ds and the desktop seeding adapter",
                operation.operation
            );
        } else {
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

/// The parity boundary the seeding contract actually names — UI ↔ `ds` CLI.
///
/// ds-brain's `docs/contracts/solar-project-seeding.md` states there is ONE
/// parity boundary here and that MCP is not a third consumer, because
/// `ds mcp serve` transports these same registered commands. So what has to
/// agree is the governed request both clients build and the refusal vocabulary
/// both read back — not a rendering, and not a digest, which neither side ever
/// derives.
#[test]
fn solar_seeding_sends_the_same_governed_request_and_reads_the_same_refusals_as_the_card() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

    // One request builder on each side, and they must agree on every key. The
    // CLI declares all of them except `root`: the destination is the paired
    // session's own selected project, exactly as the card binds it, so a
    // project id is never an argument.
    let mut declared: BTreeSet<&str> = ds_cli_solar::seed::PREVIEW_OP
        .arguments
        .iter()
        .chain(ds_cli_solar::seed::APPLY_OP.arguments.iter())
        .copied()
        .collect();
    assert!(
        !declared.contains("root"),
        "`ds solar seed` must not carry a destination root; the application owns project identity"
    );
    declared.insert("root");
    assert_eq!(
        declared,
        ds_cli_solar::seed::SERVER_REQUEST_KEYS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        "the CLI's declared seeding keys drifted from the governed request"
    );

    let payload = between(
        &app.solar_seed_pure,
        "export function solarSeedRequestPayload(",
        "\nexport type SolarSeedDrift",
    );
    assert!(
        !payload.is_empty(),
        "ds-web no longer exposes its seeding request builder at the pinned marker; \
         refusing an empty string would make every assertion below vacuous"
    );
    for key in ds_cli_solar::seed::SERVER_REQUEST_KEYS {
        assert!(
            payload.contains(&format!("payload.{key}")) || payload.contains(&format!("{key}:")),
            "the card's seeding payload no longer carries `{key}`"
        );
    }
    // ds-brain decodes with DisallowUnknownFields and reads an ABSENT optional
    // as its default, so `""` and `[]` are different requests from omission.
    // Both clients must omit; `ds` proves its own half in a unit test.
    for guard in [
        "if (context.seedSourceRoot) payload.seed_source_root",
        "if (context.cities.length > 0) payload.cities",
        "if (seedDigest) payload.seed_digest",
    ] {
        assert!(
            payload.contains(guard),
            "the card must omit an unset seeding optional rather than send an empty value: {guard}"
        );
    }

    // Two actions, no others, on both sides of the boundary.
    for action in ds_cli_solar::seed::SERVER_ACTIONS {
        assert!(
            app.solar_seed_client.contains(&format!("'{action}'")),
            "the card no longer sends the `{action}` action"
        );
    }
    assert_eq!(
        seeding_operations().len(),
        ds_cli_solar::seed::SERVER_ACTIONS.len(),
        "`ds solar seed` must expose exactly one operation per governed action"
    );

    // The refusal vocabulary. A code the CLI maps but the card no longer names
    // means one surface renders a remedy the other cannot, which is precisely
    // the divergence a single parity boundary exists to prevent.
    for (server_code, cli_code) in ds_cli_solar::seed::SERVER_CODES {
        assert!(
            app.solar_seed_client.contains(server_code),
            "`{server_code}` is mapped by ds solar seed but the card no longer names it"
        );
        assert_eq!(
            *cli_code,
            server_code.to_ascii_lowercase(),
            "a CLI seeding refusal must keep the server's own identity, in snake_case"
        );
    }

    // The city ROOT row and `mutated` are the two wire facts a client can get
    // wrong silently: dropping the root undercounts what an operator confirms,
    // and inferring "this was safe" from the action name rather than reading
    // `mutated` is what the field exists to prevent.
    assert!(
        app.solar_seed_pure.contains(&format!(
            "DOCUMENT_KIND_ROOT = '{}'",
            ds_cli_solar::seed::DOCUMENT_KIND_ROOT
        )),
        "the card no longer parses ds-brain's city root row by kind"
    );
    // The card parses `mutated` onto the plan it renders; the VERDICT over it
    // — a preview that claims it wrote is a contract break, not a UI state —
    // is `ds_command_kernel::solar_seed`'s since slice 15, so both surfaces
    // ask one owner instead of each restating the rule.
    assert!(
        app.solar_seed_pure
            .contains("mutated: source.mutated === true"),
        "the card no longer parses the server's own `mutated` flag onto the plan"
    );
    let mutated = ds_command_kernel::solar_seed::Plan {
        mutated: true,
        ..Default::default()
    };
    assert_eq!(
        ds_command_kernel::solar_seed::drift(
            &mutated,
            &ds_command_kernel::solar_seed::Context::default()
        ),
        ds_command_kernel::solar_seed::Drift::Mutated,
        "the shared kernel no longer refuses a preview that claims it wrote"
    );
}

/// The two seeding operations `ds solar seed` sends, in declaration order.
fn seeding_operations() -> [&'static BridgeOp; 2] {
    [
        &ds_cli_solar::seed::PREVIEW_OP,
        &ds_cli_solar::seed::APPLY_OP,
    ]
}

/// The seeding door is landed, owned once, and still only a door.
///
/// ds-web shipped the seeding CARD before its CLI bridge. While that gap
/// existed the two operations sat in a `PENDING_DESKTOP_OPS` gap record which
/// the loop above skipped; the application has landed them, so that record is
/// deleted rather than kept as a standing exemption and the ordinary parity
/// checks now cover both. What this test adds are the negative controls
/// specific to a GOVERNED WRITE reached through the paired session: that the
/// destination is never an argument, that the digest is never derived on either
/// side, and that the application answers from the card's own client rather
/// than a second backend path.
#[test]
fn the_solar_seeding_door_is_landed_and_owned_by_one_typed_adapter() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    assert!(
        !allowlist.trim().is_empty(),
        "the desktop CLI operation allowlist is absent"
    );

    for operation in seeding_operations() {
        assert!(
            ds_cli_solar::paired::BRIDGE_OPS
                .iter()
                .any(|declared| declared.operation == operation.operation),
            "`{}` is sent by ds solar seed but is not declared in BRIDGE_OPS",
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
        // One owner. The dispatcher routes; it must not also grow a second
        // argument contract for the same operation.
        let owners = [
            &app.solar_seed_adapter,
            &app.frontend,
            &app.design_collaboration,
            &app.map,
        ]
        .into_iter()
        .filter(|source| has_operation_contract(source, operation.operation))
        .count();
        assert_eq!(
            owners, 1,
            "`{}` must have exactly one typed seeding adapter owner",
            operation.operation
        );
        // The destination is the paired session's selected project. A project,
        // root or ds_project argument would make a project id proof of
        // something, which is exactly what this contract refuses.
        let accepted = quoted_contract_items(operation_contract(
            &app.solar_seed_adapter,
            operation.operation,
        ));
        for forbidden in ["root", "project", "ds_project"] {
            assert!(
                !accepted.contains(forbidden),
                "the seeding adapter must not accept `{forbidden}` on `{}`",
                operation.operation
            );
        }
    }

    // Only the apply is a confirmation, so only the apply carries a digest.
    assert!(
        !quoted_contract_items(operation_contract(
            &app.solar_seed_adapter,
            ds_cli_solar::seed::PREVIEW_OP.operation
        ))
        .contains("seed_digest"),
        "a preview confirms nothing and must not accept `seed_digest`"
    );
    assert!(
        quoted_contract_items(operation_contract(
            &app.solar_seed_adapter,
            ds_cli_solar::seed::APPLY_OP.operation
        ))
        .contains("seed_digest"),
        "the apply must accept the digest of the plan being confirmed"
    );

    // The application answers from the card's own client — one governed
    // request, one refusal vocabulary, no second backend and no MCP transport.
    assert!(
        app.solar_seed_adapter.contains("$lib/api/solar-seed"),
        "the seeding adapter must reach ds-brain through the card's own client"
    );
    let adapter_code = seed_adapter_code(&app);
    for forbidden in [
        "/api/v1/",
        "firestore.googleapis.com",
        "documents:commit",
        "storage.googleapis.com",
        "invokeDesktop",
    ] {
        assert!(
            !adapter_code.contains(forbidden),
            "the seeding adapter must not compose a second path: {forbidden}"
        );
    }
    // Neither side derives the digest; both echo the plan's own.
    assert!(
        !adapter_code.contains("sha256Hex("),
        "the seeding adapter must echo `seed_digest`, never derive one"
    );

    // No third seeding operation, on either side of the boundary.
    for invented in ["solar.seed.run", "solar.seed.write", "solar.seed.delete"] {
        assert_eq!(count(allowlist, &format!("\"{invented}\"")), 0);
        assert_eq!(switch_case_count(&app.frontend, invented), 0);
    }
}

/// The seeding adapter with its prose removed.
///
/// Its docstring names the boundary it must not cross — the ds-brain path, the
/// server's two actions — so a substring search over the whole file would
/// report the explanation as a violation.
fn seed_adapter_code(app: &App) -> String {
    let mut code = String::with_capacity(app.solar_seed_adapter.len());
    let mut rest = app.solar_seed_adapter.as_str();
    while let Some(open) = rest.find("/*") {
        code.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        match after.find("*/") {
            Some(close) => rest = &after[close + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    code.push_str(rest);
    code.lines()
        .map(|line| match line.find("//") {
            // A `://` is part of a URL, not the start of a comment.
            Some(marker) if !line[..marker].ends_with(':') => &line[..marker],
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A governed portfolio publication that never queued is a fact the
/// application owns, and `ds solar run result` reports the same one.
///
/// The application publishes the aggregate from the run that sealed it, as a
/// handoff after the local commit: the receipt is written first, the intent is
/// queued after, and a queue failure is recorded on that already-succeeded
/// receipt instead of undoing it. An intent that never reached the outbox has
/// no Sync Center row, so the result receipt is the only place either surface
/// can learn it — which is why `ds` reads it there rather than deriving a
/// publication state of its own.
///
/// What has to agree is therefore the receipt field `ds` hand-copies, the bound
/// the application puts on it, the order that keeps the run successful, and the
/// "never queued" word both surfaces print. The application's own CLI
/// projection does not forward the field yet; the directional guard below
/// pins the spelling `ds` reads for when it does.
#[test]
fn a_failed_portfolio_publication_stays_a_sync_lane_fact_on_a_succeeded_receipt() {
    let Some(app) = app() else {
        return skip("ds-web checkout not found");
    };

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
        handoff.contains("enqueueSolarPortfolioPublication("),
        "the governed publication must be queued after the local commit, not before it"
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

    // The hand copy rests on one convention: this projection renames every
    // receipt field it forwards to snake_case. Prove the convention, then hold
    // the field to it if and when the projection carries it.
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
    if app.frontend.contains("publicationError") {
        assert!(
            projects_field(projection, ds_cli_solar::paired_run::PUBLICATION_ERROR_KEY),
            "the projection carries the receipt's publication failure under a key ds does \
             not read; ds reads `{}`",
            ds_cli_solar::paired_run::PUBLICATION_ERROR_KEY
        );
    }
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

#[test]
fn every_assets_command_has_one_closed_operation_owner() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

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
    for operation in ds_cli_assets::BRIDGE_OPS {
        assert!(
            seen.insert(operation.operation),
            "`{}` is declared twice by ds assets; one semantic operation has one owner",
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

        let contract = operation_contract(&app.assets, operation.operation);
        assert!(
            !contract.is_empty(),
            "`{}` has no typed Project Assets adapter argument contract",
            operation.operation
        );
        let accepted = quoted_contract_items(contract);
        for argument in operation.arguments {
            let mut parts = argument.split('.');
            let top = parts.next().expect("declared argument is non-empty");
            assert!(
                accepted.contains(top),
                "ds assets sends `{argument}` to `{}`, but its typed adapter does not accept `{top}`",
                operation.operation
            );
            for nested in parts {
                assert!(
                    app.assets.contains(&format!("'{nested}'")),
                    "ds assets sends `{argument}` to `{}`, but the adapter does not validate `{nested}`",
                    operation.operation
                );
            }
        }
    }
}

#[test]
fn assets_bounds_and_refusals_match_the_desktop_owner() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

    // A bound enforced in two places must be the SAME bound. A `--limit` this
    // CLI accepts and the application refuses is a round trip spent to learn
    // a number both sides already knew; a `--depth` this CLI accepts and the
    // kernel quietly cuts is worse, because the answer still looks complete.
    for (constant, value) in [
        ("MAX_PAGE_SIZE", ds_cli_assets::MAX_PAGE_SIZE),
        ("MAX_TREE_DEPTH", ds_cli_assets::MAX_TREE_DEPTH),
        ("MAX_QUERY_CHARS", ds_cli_assets::MAX_QUERY_CHARS as i64),
        (
            "MAX_CONTAINER_MEMBERS",
            ds_cli_assets::MAX_CONTAINER_MEMBERS as i64,
        ),
        (
            "MAX_LAYER_NAME_CHARS",
            ds_cli_assets::MAX_LAYER_NAME_CHARS as i64,
        ),
        (
            "MAX_FOLDER_SEGMENTS",
            ds_cli_assets::MAX_FOLDER_SEGMENTS as i64,
        ),
        ("MAX_SEGMENT_CHARS", ds_cli_assets::MAX_SEGMENT_CHARS as i64),
    ] {
        assert!(
            app.assets.contains(&format!("const {constant} = {value}")),
            "the desktop must bound Project Assets `{constant}` at {value}, exactly as ds assets does"
        );
    }

    // `classify`, `attach` and `folder` refuse a projected `sys:` row by name.
    // Only the application knows which rows are projections, so it is the side
    // that constructs the refusal; `ds assets` declares the code and the
    // remedy. If the marker leaves the adapter, a write to a system row comes
    // back as `desktop_refused` with nothing to do about it.
    assert!(
        app.assets
            .contains(ds_cli_assets::PROJECTED_ASSET_READ_ONLY.code),
        "no `{}` marker remains in the desktop Project Assets adapter; a write to a \
         projected row would report desktop_refused instead of its named refusal",
        ds_cli_assets::PROJECTED_ASSET_READ_ONLY.code
    );
}

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
const DSGRID_PROJECT_OPERATIONS: &[&str] =
    &["dsgrid.model.prepare_project", "dsgrid.model.publish"];

#[test]
fn every_dsgrid_model_command_has_one_closed_operation_owner_and_exact_arguments() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
         working copy and project cache"
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
    // what is left: the two operations that DO read the application's project
    // are the only ones the door admits, and neither carries a project of its
    // own — the application's selected project is the destination.
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
            .expect("the checkout was found above")
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
fn the_data_domain_sends_only_operations_the_desktop_owns() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    // What is left is local compute this application's own components serve:
    // the Rwanda DEM engine and the project's pinned boundary asset.
    assert_eq!(ds_cli_data::BRIDGE_OPS.len(), 4);
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
fn every_design_collaboration_command_has_one_closed_operation_owner_and_exact_arguments() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    let mut seen = BTreeSet::new();
    for operation in ds_cli_design::BRIDGE_OPS {
        assert!(
            seen.insert(operation.operation),
            "`{}` is declared twice by ds design; one semantic operation has one owner",
            operation.operation
        );
        assert_eq!(
            count(allowlist, &format!("\"{}\"", operation.operation)),
            1,
            "`{}` must appear exactly once in the native allowlist",
            operation.operation
        );
        assert_eq!(
            switch_case_count(&app.frontend, operation.operation),
            1,
            "`{}` must have exactly one frontend handler",
            operation.operation
        );
        // Presence, not non-emptiness: `design.known-columns.list` takes no
        // arguments and its contract is legitimately `[]`. Asserting the
        // extracted text was non-empty made a zero-argument operation
        // indistinguishable from a missing one.
        assert!(
            has_operation_contract(&app.design_collaboration, operation.operation),
            "`{}` has no typed design-collaboration adapter contract",
            operation.operation
        );
        let contract = operation_contract(&app.design_collaboration, operation.operation);
        // Exact, not a subset: an argument the adapter accepts but `ds design`
        // never sends is a key nothing validates, and one `ds design` sends
        // that the adapter rejects is a command that cannot work.
        let accepted = quoted_contract_items(contract);
        let declared: BTreeSet<String> = operation
            .arguments
            .iter()
            .map(|argument| (*argument).to_string())
            .collect();
        assert_eq!(
            accepted, declared,
            "`{}` must accept exactly the keys ds design declares",
            operation.operation
        );
    }
}

#[test]
fn design_collaboration_stays_metadata_only_and_owns_no_map_state() {
    // The roadmap requires metadata workflows to be headless. `ds design` lives
    // beside `ds work` rather than under `ds map` precisely because none of its
    // operations needs a map instance, an edit session or a design room — and
    // the adapter that serves them must not acquire one.
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    for map_owned in [
        "$lib/stores/map",
        "maplibre-gl",
        "$lib/design/edit-context",
        "mapInstance",
        "editSession",
    ] {
        assert!(
            !app.design_collaboration.contains(map_owned),
            "the design-collaboration adapter reaches map-owned state (`{map_owned}`); \
             these operations must work with no map open"
        );
    }
    // One client, shared with the dialogs, so the CLI and the UI exercise the
    // same server contract and the same refusal vocabulary.
    assert!(
        app.design_collaboration
            .contains("from '$lib/api/design-collab'"),
        "the design-collaboration adapter must reach ds-brain through the same \
         client the dialogs use, not a second one"
    );
}

#[test]
fn design_collaboration_bounds_match_the_desktop_owner() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    // `ds design` refuses an over-large --limit locally so it is refused once,
    // not twice. That is only true while both sides agree on the number.
    assert!(
        app.design_collaboration.contains(&format!(
            "const MAX_PAGE = {};",
            ds_cli_design::MAX_PAGE_SIZE
        )),
        "ds design bounds a page at {} but the desktop adapter does not",
        ds_cli_design::MAX_PAGE_SIZE
    );
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
        !ds_web()
            .expect("the checkout was found above")
            .join("src/lib/desktop/cli-sre.ts")
            .exists(),
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    let Some(root) = ds_web() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

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
    for name in fill_patterns.iter().chain(["directional"].iter()) {
        let named = [&app.style_fill_pattern, &app.style_line_type]
            .iter()
            .any(|source| {
                source.contains(&format!("'{name}'")) || source.contains(&format!("\"{name}\""))
            });
        assert!(
            named,
            "ds style cartography offers `{name}`, but the renderer does not name it"
        );
    }
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
    let Some(root) = ds_web() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let transport = std::fs::read_to_string(root.join("src-tauri/src/cli_bridge.rs")).unwrap();
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    // The application's own reporting path is untouched: a person filing from
    // the window still reaches the same endpoint.
    assert!(
        app.feedback_submit.contains("reporter_kind: 'agent'"),
        "the application's own feedback submission lost its reporter kind"
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

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

    let root = ds_web().expect("checked above");
    let create = std::fs::read_to_string(root.join("src/lib/design/create-from-selection.ts"))
        .expect("create-from-selection.ts is readable");
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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

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

    // Both spellings of where a descriptor lives. The registry directory is
    // where every live instance publishes; the legacy file is the one an older
    // `ds` is the only reader of, and dropping it would unpair those builds.
    for path in [
        ds_cli_desktop::discover::DESCRIPTOR_DIR,
        ds_cli_desktop::discover::DESCRIPTOR_FILE,
    ] {
        assert!(
            app.transport.contains(&format!("\"{path}\"")),
            "the shell no longer writes `{path}`, which discovery enumerates"
        );
    }

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
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
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
