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
    survey_forms: String,
    survey_project_forms: String,
    survey_templates: String,
    design: String,
    design_collaboration: String,
    analysis: String,
    work: String,
    sre: String,
    style: String,
    style_fill_pattern: String,
    style_line_type: String,
    tile: String,
    feedback: String,
    feedback_submit: String,
    solar_seed_client: String,
    solar_seed_pure: String,
    solar_seed_adapter: String,
    solar_portfolio_run: String,
    solar_portfolio_receipt: String,
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
        survey_forms: read("src/lib/desktop/cli-survey-forms.ts")?,
        survey_project_forms: read("src/lib/desktop/cli-survey-project-forms.ts")?,
        survey_templates: read("src/lib/desktop/cli-survey-templates.ts")?,
        design: read("src/lib/desktop/cli-map-design.ts")?,
        design_collaboration: read("src/lib/desktop/cli-design.ts")?,
        analysis: read("src/lib/analysis/outliers.ts")?,
        work: read("src/lib/desktop/cli-work.ts")?,
        sre: read("src/lib/desktop/cli-sre.ts")?,
        style: read("src/lib/desktop/cli-style.ts")?,
        style_fill_pattern: read("src/lib/styles/fill-pattern.ts")?,
        style_line_type: read("src/lib/styles/line-type.ts")?,
        tile: read("src/lib/desktop/cli-tile.ts")?,
        feedback: read("src/lib/desktop/cli-feedback.ts")?,
        feedback_submit: read("src/lib/feedback/submit.ts")?,
        solar_seed_client: read("src/lib/api/solar-seed.ts")?,
        solar_seed_pure: read("src/lib/solar/seed.ts")?,
        solar_seed_adapter: read("src/lib/desktop/cli-solar-seed.ts")?,
        solar_portfolio_run: read("src/lib/solar/native-batch.ts")?,
        solar_portfolio_receipt: read("src/lib/solar/native-portfolio-batches.ts")?,
    })
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
fn every_survey_control_plane_command_has_one_api_only_owner_and_exact_arguments() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    let adapters = [
        &app.survey_forms,
        &app.survey_project_forms,
        &app.survey_templates,
    ];
    let mut seen = BTreeSet::new();
    for operation in ds_cli_survey::BRIDGE_OPS {
        assert!(
            seen.insert(operation.operation),
            "`{}` is declared twice by ds survey",
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
        let owners = adapters
            .iter()
            .filter(|source| has_operation_contract(source, operation.operation))
            .collect::<Vec<_>>();
        assert_eq!(
            owners.len(),
            1,
            "`{}` must have exactly one typed survey adapter owner",
            operation.operation
        );
        let accepted = quoted_contract_items(operation_contract(owners[0], operation.operation));
        let declared = operation
            .arguments
            .iter()
            .map(|argument| (*argument).to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            accepted, declared,
            "`{}` arguments drifted between ds and the desktop",
            operation.operation
        );
    }

    for source in adapters {
        for forbidden in [
            "$lib/stores/map",
            "mapInstance",
            "activeProject",
            "editSession",
            "indexedDB",
            "bearer token",
        ] {
            assert!(
                !source.contains(forbidden),
                "survey control-plane adapters must not depend on map or credential state: {forbidden}"
            );
        }
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
            for argument in operation.arguments {
                assert!(
                    app.frontend.contains(&format!("args.{argument}")),
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
            let declared = operation.arguments.iter().copied().collect();
            assert_eq!(
                consumed, declared,
                "solar.run.start must consume exactly the keys declared by ds; conditional portfolio inputs cannot be hidden in another handler",
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
    assert!(
        app.solar_seed_pure.contains("plan.mutated"),
        "the card no longer reads the server's own `mutated` flag"
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

#[test]
fn every_work_command_has_one_closed_operation_owner() {
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
    for operation in ds_cli_work::BRIDGE_OPS {
        assert!(
            seen.insert(operation.operation),
            "`{}` is declared twice by ds work; one semantic operation has one owner",
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

        let contract = operation_contract(&app.work, operation.operation);
        assert!(
            !contract.is_empty(),
            "`{}` has no typed Project Work adapter argument contract",
            operation.operation
        );
        for argument in operation.arguments {
            let mut parts = argument.split('.');
            let top = parts.next().expect("declared argument is non-empty");
            assert!(
                contract.contains(&format!("'{top}'")),
                "ds work sends `{argument}` to `{}`, but its typed adapter does not accept `{top}`",
                operation.operation
            );
            for nested in parts {
                assert!(
                    app.work.contains(&format!("'{nested}'")),
                    "ds work sends `{argument}` to `{}`, but the adapter does not validate `{nested}`",
                    operation.operation
                );
            }
        }
    }
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
        let contract = operation_contract(&app.design_collaboration, operation.operation);
        assert!(
            !contract.is_empty(),
            "`{}` has no typed design-collaboration adapter contract",
            operation.operation
        );
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
fn every_sre_command_has_one_closed_operation_owner_and_exact_arguments() {
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
    for operation in ds_cli_sre::BRIDGE_OPS {
        assert!(
            seen.insert(operation.operation),
            "`{}` is declared twice by ds sre",
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
        assert!(
            has_operation_contract(&app.sre, operation.operation),
            "`{}` has no typed SRE adapter argument contract",
            operation.operation
        );
        let accepted = quoted_contract_items(operation_contract(&app.sre, operation.operation));
        let declared = operation
            .arguments
            .iter()
            .map(|argument| (*argument).to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            accepted, declared,
            "`{}` arguments drifted between ds and the desktop",
            operation.operation
        );
    }
}

#[test]
fn sre_bounds_outputs_and_typed_refusals_match_the_desktop_owner() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    for (name, value) in [
        ("CLI_SRE_MAX_DAYS", ds_cli_sre::MAX_DAYS),
        ("CLI_SRE_MAX_EVENTS", ds_cli_sre::MAX_EVENTS),
        ("CLI_SRE_MAX_SCAN_EVENTS", ds_cli_sre::MAX_SCAN_EVENTS),
        (
            "CLI_SRE_MAX_EVENT_TEXT_CHARS",
            ds_cli_sre::MAX_EVENT_TEXT_CHARS as i64,
        ),
        (
            "CLI_SRE_MAX_ERROR_MESSAGE_CHARS",
            ds_cli_sre::MAX_ERROR_MESSAGE_CHARS as i64,
        ),
    ] {
        let plain = format!("export const {name} = {value};");
        let grouped = format!("export const {name} = {};", grouped(value as usize));
        assert!(
            app.sre.contains(&plain) || app.sre.contains(&grouped),
            "the desktop's {name} must match ds sre"
        );
    }

    let overview = between(
        &app.sre,
        "export function projectCliSreOverview",
        "function same",
    );
    assert!(
        !overview.is_empty(),
        "the bounded SRE overview projection is absent"
    );
    for field in [
        "generated_at",
        "fleet",
        "combined_reports",
        "services",
        "service_ops",
        "stale",
        "incidents",
        "error_catalog",
        "totals",
        "more",
    ] {
        assert!(
            projects_field(overview, field),
            "the desktop SRE owner no longer projects `{field}`"
        );
    }

    let events = between(
        &app.sre,
        "export function projectCliSreEvents",
        "export async function readCliSreOverview",
    );
    assert!(
        !events.is_empty(),
        "the bounded SRE event projection is absent"
    );
    for field in [
        "filters", "scanned", "matching", "returned", "events", "more",
    ] {
        assert!(
            projects_field(events, field),
            "the desktop SRE event projection no longer projects `{field}`"
        );
    }
    let events_read = between(&app.sre, "export async function readCliSreEvents", "\n}");
    assert!(!events_read.is_empty(), "the SRE events owner is absent");
    for field in ["generated_at", "window_days", "scan_limit"] {
        assert!(
            projects_field(events_read, field),
            "the desktop SRE event owner no longer projects `{field}`"
        );
    }

    let lowered = app.sre.to_ascii_lowercase();
    for marker in ds_cli_sre::NOT_PERMITTED_MARKERS {
        assert!(
            lowered.contains(marker),
            "the SRE permission marker `{marker}` no longer appears in the owner"
        );
    }
    assert!(
        ds_cli_sre::SRE_SIGNED_OUT_MARKERS
            .iter()
            .any(|marker| lowered.contains(marker)),
        "no SRE sign-in marker remains in the owner"
    );
    assert!(
        !app.sre.contains("activeProject") && !app.sre.contains("getActiveProject"),
        "platform-global SRE reads must not require an active project"
    );
}

#[test]
fn every_style_command_has_one_closed_operation_owner() {
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
    for operation in ds_cli_style::BRIDGE_OPS {
        assert!(
            seen.insert(operation.operation),
            "`{}` is declared twice by ds style; one semantic operation has one owner",
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
        let contract = operation_contract(&app.style, operation.operation);
        assert!(
            !contract.is_empty(),
            "`{}` has no typed style adapter argument contract",
            operation.operation
        );
        for argument in operation.arguments {
            assert!(
                contract.contains(&format!("'{argument}'"))
                    || contract.contains(&format!("\"{argument}\"")),
                "ds style sends `{argument}` to `{}`, but its typed adapter does not accept it",
                operation.operation
            );
        }
    }
    // The value bound is one number on both sides of the bridge.
    assert!(
        app.style
            .contains(&format!("const MAX_VALUES = {};", ds_cli_style::MAX_VALUES)),
        "ds style's MAX_VALUES must equal the adapter's MAX_VALUES"
    );
}

#[test]
fn style_cartography_sends_exactly_the_arguments_and_bounds_the_desktop_owns() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let operation = ds_cli_style::CARTOGRAPHY_SET.operation;

    // Ten camelCase keys hand-copied from the application's own input schema
    // is the largest such copy in this domain, and `ds style appearance set`
    // proves a subset check is not enough: a key the adapter accepts but ds
    // never sends is a property no caller can reach. Hold both directions.
    assert!(
        has_operation_contract(&app.style, operation),
        "`{operation}` has no typed style-adapter argument contract"
    );
    let accepted = quoted_contract_items(operation_contract(&app.style, operation));
    let declared = ds_cli_style::CARTOGRAPHY_SET
        .arguments
        .iter()
        .map(|argument| (*argument).to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        accepted, declared,
        "`{operation}` arguments drifted between ds and the desktop"
    );

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
        "the desktop must rasterise exactly the seamless pattern tile sizes ds offers: [{spacings}]"
    );

    // The fill-pattern vocabulary is the adapter's own — unlike the dash
    // presets, which ds-brain publishes — so every name a caller may pass
    // must appear in it. `directional` is the one line type that is a marker
    // rather than a dash, and the adapter is what knows that.
    let fill_patterns = ds_cli_style::cartography::plan::COMMAND
        .arg("fill-pattern")
        .expect("--fill-pattern is declared")
        .choices;
    for name in fill_patterns.iter().chain(["directional"].iter()) {
        let named = [&app.style, &app.style_fill_pattern, &app.style_line_type]
            .iter()
            .any(|source| {
                source.contains(&format!("'{name}'")) || source.contains(&format!("\"{name}\""))
            });
        assert!(
            named,
            "ds style cartography offers `{name}`, but the desktop adapter does not name it"
        );
    }
}

#[test]
fn every_tile_command_has_one_closed_operation_owner() {
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
    for operation in ds_cli_tile::BRIDGE_OPS {
        assert!(
            seen.insert(operation.operation),
            "`{}` is declared twice by ds tile; one semantic operation has one owner",
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
        let contract = operation_contract(&app.tile, operation.operation);
        assert!(
            !contract.is_empty(),
            "`{}` has no typed tile adapter argument contract",
            operation.operation
        );
        for argument in operation.arguments {
            assert!(
                contract.contains(&format!("'{argument}'")),
                "ds tile sends `{argument}` to `{}`, but its typed adapter does not accept it",
                operation.operation
            );
        }
    }
}

#[test]
fn every_feedback_command_has_one_closed_operation_owner() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    for operation in ds_cli_feedback::BRIDGE_OPS {
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
        let contract = operation_contract(&app.feedback, operation.operation);
        assert!(
            !contract.is_empty(),
            "`{}` has no typed feedback-adapter argument contract",
            operation.operation
        );
        for argument in operation.arguments {
            assert!(
                contract.contains(&format!("'{argument}'")),
                "ds feedback sends `{argument}` to `{}`, but the adapter does not accept it",
                operation.operation
            );
        }
    }
    assert!(
        app.feedback_submit.contains("reporter_kind: 'agent'"),
        "the desktop must pin CLI reports as agent sightings"
    );
    assert!(
        app.feedback.contains("brain('/api/v1/feedback', payload)"),
        "the CLI adapter must reuse the existing feedback endpoint"
    );
    // Closing is the `fb` tab's own triage call, so it inherits that tab's
    // platform capability gate rather than opening a second one.
    assert!(
        app.feedback.contains("updateFeedbackStatus("),
        "ds feedback close must reuse the application's governed triage call"
    );

    // Three triage conditions reach `ds feedback` as the adapter's prose and
    // leave it as codes. Each needs at least one marker still present, or the
    // command reports `desktop_refused` for something that has a name, a
    // remedy and a different next step.
    let lowered = app.feedback.to_ascii_lowercase();
    for (condition, markers) in [
        ("not found", ds_cli_feedback::NOT_FOUND_MARKERS),
        ("version conflict", ds_cli_feedback::CONFLICT_MARKERS),
        ("not permitted", ds_cli_feedback::NOT_PERMITTED_MARKERS),
    ] {
        assert!(
            markers.iter().any(|marker| lowered.contains(marker)),
            "no `{condition}` marker remains in the desktop feedback adapter; \
             `ds feedback close` would report desktop_refused instead of its \
             named refusal"
        );
    }
}

#[test]
fn work_bounds_and_refusals_match_the_desktop_owner() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

    // A page bound enforced in two places must be the SAME bound, or an
    // accepted --limit becomes a refusal from the application.
    assert!(
        app.work.contains(&format!(
            "const MAX_PAGE_SIZE = {}",
            ds_cli_work::MAX_PAGE_SIZE
        )),
        "the desktop must bound a Project Work page exactly as ds work does"
    );
    assert!(
        app.work.contains(&format!(
            "const MAX_RELATED_ROWS = {}",
            ds_cli_work::MAX_RELATED_ROWS
        )),
        "the desktop must bound Project Work detail collections exactly as ds work documents"
    );
    // The assignee bound is the engine's, published in the graph's field
    // model; ds work carries a hand copy so an over-long list is refused
    // locally, and the adapter must fall back to the same number.
    assert!(
        app.work
            .contains(&format!("maxAssignees ?? {}", ds_cli_work::MAX_ASSIGNEES)),
        "the desktop must fall back to the same assignee bound as ds work"
    );

    let lowered = app.work.to_ascii_lowercase();
    // Three conditions reach `ds work` as prose and leave it as codes. Each
    // needs at least one marker still present in the application's own
    // message, or the command reports `desktop_refused` for something that has
    // a name, a remedy and a different next step.
    for (condition, markers) in [
        ("signed out", ds_cli_work::SIGNED_OUT_MARKERS),
        ("not permitted", ds_cli_work::NOT_PERMITTED_MARKERS),
        ("revision conflict", ds_cli_work::CONFLICT_MARKERS),
    ] {
        assert!(
            markers.iter().any(|marker| lowered.contains(marker)),
            "no `{condition}` marker remains in the desktop Project Work adapter; \
             `ds work` would report desktop_refused instead of its named refusal"
        );
    }
}

#[test]
fn project_work_gets_no_messaging_door() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

    // messages-v1 is human-only. Assignment and state notifications are side
    // effects of a governed command, never something `ds` composes.
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    for forbidden in ["messaging.send", "messages.send", "work.message"] {
        assert!(
            !allowlist.contains(forbidden),
            "the desktop allowlist admits `{forbidden}`; the CLI has no messaging door"
        );
        assert!(
            !app.frontend.contains(&format!("case '{forbidden}")),
            "the frontend dispatcher routes `{forbidden}`; the CLI has no messaging door"
        );
    }
    for operation in ds_cli_work::BRIDGE_OPS {
        assert!(
            !operation.operation.contains("message"),
            "`{}` reads as a messaging operation",
            operation.operation
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
        &app.work,
    ] {
        assert!(
            !source.contains("agent_bridge") && !source.contains("agent-bridge"),
            "paired-domain CLI support must not restore a retired automation bridge"
        );
    }
}
