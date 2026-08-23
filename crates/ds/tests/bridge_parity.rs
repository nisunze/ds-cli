//! Parity between `ds map` and the desktop's closed CLI bridge.
//!
//! Map commands do not reach an open automation or assistant surface. Each
//! one names a typed operation which must occur exactly once in the native
//! allowlist, once in the frontend dispatcher, and once in the map adapter's
//! input contract. That exact-one rule prevents two CLI commands from quietly
//! becoming aliases for the same mutation.

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
    map: String,
    design: String,
    analysis: String,
}

fn app() -> Option<App> {
    let root = ds_web()?;
    let read = |leaf: &str| std::fs::read_to_string(root.join(leaf)).ok();
    Some(App {
        transport: read("src-tauri/src/cli_bridge.rs")?,
        frontend: read("src/lib/desktop/cli-bridge.ts")?,
        map: read("src/lib/desktop/cli-map.ts")?,
        design: read("src/lib/desktop/cli-map-design.ts")?,
        analysis: read("src/lib/analysis/outliers.ts")?,
    })
}

fn count(source: &str, needle: &str) -> usize {
    source.match_indices(needle).count()
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
            count(&app.frontend, &format!("case '{}':", operation.operation)),
            1,
            "`{}` must have exactly one frontend handler",
            operation.operation
        );

        let contract = operation_contract(&app.map, operation.operation);
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
                    app.map.contains(&format!("'{nested}'")),
                    "ds map sends `{argument}` to `{}`, but the adapter does not validate `{nested}`",
                    operation.operation
                );
            }
        }
    }
}

#[test]
fn solar_workflow_gaps_have_one_closed_desktop_owner() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let allowlist = between(
        &app.transport,
        "pub const CLI_OPERATIONS: &[&str] = &[",
        "];",
    );
    for operation in [
        "solar.results.read",
        "solar.sync.status",
        "solar.portfolio.list",
        "solar.final.import",
    ] {
        assert_eq!(
            count(allowlist, &format!("\"{operation}\"")),
            1,
            "{operation} must appear exactly once in the native allowlist"
        );
        assert_eq!(
            count(&app.frontend, &format!("case '{operation}':")),
            1,
            "{operation} must have exactly one frontend executor"
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

    for source in [&app.transport, &app.frontend, &app.map, &app.design] {
        assert!(
            !source.contains("agent_bridge") && !source.contains("agent-bridge"),
            "map CLI support must not restore a retired automation bridge"
        );
    }
}
