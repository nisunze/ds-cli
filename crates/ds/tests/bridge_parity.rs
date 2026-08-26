//! Parity between the paired-application domains and the desktop's closed CLI
//! bridge.
//!
//! `ds map` and `ds work` do not reach an open automation or assistant
//! surface. Each command names a typed operation which must occur exactly once
//! in the native allowlist, once in the frontend dispatcher, and once in that
//! domain's adapter input contract. That exact-one rule prevents two CLI
//! commands from quietly becoming aliases for the same mutation.

use std::{collections::BTreeSet, path::PathBuf};

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
    survey: String,
    design: String,
    analysis: String,
    work: String,
    style: String,
    tile: String,
    feedback: String,
    feedback_submit: String,
}

fn app() -> Option<App> {
    let root = ds_web()?;
    let read = |leaf: &str| std::fs::read_to_string(root.join(leaf)).ok();
    Some(App {
        transport: read("src-tauri/src/cli_bridge.rs")?,
        frontend: read("src/lib/desktop/cli-bridge.ts")?,
        project: read("src/lib/desktop/cli-project.ts")?,
        map: read("src/lib/desktop/cli-map.ts")?,
        survey: read("src/lib/desktop/cli-survey.ts")?,
        design: read("src/lib/desktop/cli-map-design.ts")?,
        analysis: read("src/lib/analysis/outliers.ts")?,
        work: read("src/lib/desktop/cli-work.ts")?,
        style: read("src/lib/desktop/cli-style.ts")?,
        tile: read("src/lib/desktop/cli-tile.ts")?,
        feedback: read("src/lib/desktop/cli-feedback.ts")?,
        feedback_submit: read("src/lib/feedback/submit.ts")?,
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
    let marker = format!("'{operation}': [");
    let Some(start) = source.find(&marker) else {
        return "";
    };
    let rest = &source[start + marker.len()..];
    &rest[..rest.find("],").unwrap_or(rest.len())]
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

        let map_contract = operation_contract(&app.map, operation.operation);
        let contract = if map_contract.is_empty() {
            operation_contract(&app.survey, operation.operation)
        } else {
            map_contract
        };
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
                        || app.survey.contains(&format!("'{nested}'")),
                    "ds map sends `{argument}` to `{}`, but the adapter does not validate `{nested}`",
                    operation.operation
                );
            }
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
        for argument in operation.arguments {
            assert!(
                app.frontend.contains(&format!("args.{argument}")),
                "ds solar sends `{argument}` to `{}`, but the paired adapter does not read that exact key",
                operation.operation,
            );
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
                contract.contains(&format!("'{argument}'")),
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
