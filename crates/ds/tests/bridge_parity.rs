//! Parity between what `ds map` sends and what the application accepts.
//!
//! `tests/engine_parity.rs` does this for the engines `ds` spawns. The same
//! discipline is owed to the paired application, and for a sharper reason:
//! there is no schema to fetch at runtime. The bridge validates an operation's
//! arguments inside a webview and answers a failure as prose, so a misspelled
//! field is not a typed refusal a caller can branch on — it is an operation
//! that quietly does less than it looks like it did.
//!
//! `ds map` therefore declares its whole wire contract as data — operations,
//! argument keys, the bounds it enforces locally, the snapshot fields it
//! reads, the prose markers it classifies on — and this test proves every one
//! of those against the application's own source on disk.
//!
//! Three failures it is specifically built to catch:
//!
//! * an operation that is *listed* by the bridge but has no executor behind
//!   it, so calling it throws rather than working. `style.preview` and
//!   `workspace.save` are in the bridge's allow-list today and have no case in
//!   the frontend switch; a command built on either would compile, help
//!   correctly, and fail for every caller.
//! * a settings key that drifts case. The vector tools take `intervalM`, not
//!   `interval_m`, and every flag on this side is the other convention.
//! * a snapshot field that is renamed. `ds map view` reads them by index, so
//!   a rename does not fail — it reports a map with nothing on it.
//!
//! The application is a sibling repository, not a build dependency. When it
//! is absent the check says so loudly rather than passing quietly.

use std::path::PathBuf;

/// The application's source: `DS_WEB_DIR` when set, otherwise the sibling
/// repository.
///
/// The override exists because the sibling path is not always the layout —
/// a git worktree of this repository sits two directories deeper, and the
/// first run of this suite from one skipped all six checks and reported
/// green. A skip that looks like a pass is worse than no test, which is why
/// [`skip`] names the path it looked in.
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
        "SKIPPED: {reason}\n  \
         This check proves `ds map` sends operations and argument keys the \
         paired application actually accepts.\n  \
         Looked in: {}\n  \
         Set DS_WEB_DIR to the ds-web checkout to run it; CI checks it out so \
         the proof is real there.",
        looked_in.display()
    );
}

struct App {
    /// `src-tauri/src/agent_bridge.rs` — the bridge's allow-list.
    transport: String,
    /// `src/lib/desktop/agent-bridge.ts` — schemas, executors, snapshot.
    frontend: String,
    /// `src/lib/analysis/outliers.ts` — the analysis-layer catalogue.
    analysis: String,
}

fn app() -> Option<App> {
    let root = ds_web()?;
    let read = |leaf: &str| std::fs::read_to_string(root.join(leaf)).ok();
    Some(App {
        transport: read("src-tauri/src/agent_bridge.rs")?,
        frontend: read("src/lib/desktop/agent-bridge.ts")?,
        analysis: read("src/lib/analysis/outliers.ts")?,
    })
}

/// The text between `open` and the next `close` at or after it.
fn between<'a>(source: &'a str, open: &str, close: &str) -> &'a str {
    let Some(start) = source.find(open) else {
        return "";
    };
    let rest = &source[start + open.len()..];
    match rest.find(close) {
        Some(end) => &rest[..end],
        None => rest,
    }
}

/// One operation's schema region: from its `id:` declaration to the next one,
/// bounded so a region cannot swallow its neighbours and pass by accident.
fn schema_region<'a>(frontend: &'a str, operation: &str) -> &'a str {
    const MAX_REGION: usize = 1_500;
    let marker = format!("id: '{operation}'");
    let Some(start) = frontend.find(&marker) else {
        return "";
    };
    let rest = &frontend[start + marker.len()..];
    let end = rest
        .find("id: '")
        .unwrap_or(rest.len())
        .min(MAX_REGION)
        .min(rest.len());
    let mut end = end;
    while end > 0 && !rest.is_char_boundary(end) {
        end -= 1;
    }
    &rest[..end]
}

/// The `layerId`/`settings` shape the gis operations get from a spread rather
/// than from their own block, so their region has to include it.
fn gis_spread(frontend: &str) -> &str {
    between(frontend, "...GIS_OPERATION_SCHEMAS.map(", "})),")
}

#[test]
fn every_operation_ds_map_sends_is_one_the_application_implements() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

    // The bridge's allow-list, and the frontend's own record of what it has
    // actually built. Both, because they disagree: the allow-list is ahead.
    let allowed = between(&app.transport, "const LOCAL_OPERATIONS: &[&str] = &[", "];");
    let implemented = between(&app.frontend, "const implemented = new Set([", "]);");

    for op in ds_cli_map::BRIDGE_OPS {
        let operation = op.operation;
        assert!(
            allowed.contains(&format!("\"{operation}\"")),
            "`{operation}` is not in the bridge's LOCAL_OPERATIONS; it would be \
             refused as an unknown operation"
        );
        assert!(
            implemented.contains(&format!("'{operation}'")),
            "`{operation}` is listed by the bridge but the application does not \
             count it as implemented; it would report `not_implemented`"
        );
        assert!(
            app.frontend.contains(&format!("case '{operation}':")),
            "`{operation}` has no executor in the application's operation \
             switch, so every call would throw. A listed operation is not a \
             working one — `style.preview` is listed too."
        );
    }
}

#[test]
fn every_argument_key_ds_map_sends_is_one_the_operation_declares() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let spread = gis_spread(&app.frontend);

    for op in ds_cli_map::BRIDGE_OPS {
        let mut region = schema_region(&app.frontend, op.operation).to_string();
        assert!(
            !region.is_empty(),
            "no input schema found for `{}` in the application",
            op.operation
        );
        if op.operation.starts_with("gis.") {
            region.push_str(spread);
        }

        for declared in op.arguments {
            // `settings.intervalM` asserts both halves: the object the
            // application declares, and the key inside it.
            for part in declared.split('.') {
                assert!(
                    region.contains(&format!("{part}:")),
                    "`ds map` sends `{declared}` to `{}`, but the application's \
                     input schema declares no `{part}`. This is the hand copy \
                     drifting — check the spelling against the schema, not \
                     against what reads plausibly.",
                    op.operation
                );
            }
        }
    }
}

#[test]
fn the_bounds_ds_map_enforces_locally_are_the_applications_own() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };
    let root = ds_web().expect("checked above");

    // `ds map` refuses an over-large file itself so the operator hears a
    // local refusal naming the bound. A bound that drifts below the
    // application's would refuse work the application would have accepted;
    // above it, the refusal arrives from a webview instead.
    assert!(
        app.frontend.contains(&format!(
            "MAX_AGENT_FEATURES = {}",
            grouped(ds_cli_map::MAX_LAYER_FEATURES)
        )),
        "MAX_LAYER_FEATURES is {} but the application's MAX_AGENT_FEATURES is not",
        ds_cli_map::MAX_LAYER_FEATURES
    );

    let design = std::fs::read_to_string(root.join("src/lib/desktop/agent-design.ts"))
        .expect("agent-design.ts is readable");
    assert!(
        design.contains(&format!(
            "MAX_DESIGN_FEATURE_SAMPLE = {}",
            ds_cli_map::MAX_FEATURE_SAMPLE
        )),
        "MAX_FEATURE_SAMPLE is {} but the application's is not",
        ds_cli_map::MAX_FEATURE_SAMPLE
    );

    let create = std::fs::read_to_string(root.join("src/lib/design/create-from-selection.ts"))
        .expect("create-from-selection.ts is readable");
    assert!(
        create.contains(&format!(
            "MAX_CREATE_FROM_SELECTION = {}",
            grouped(ds_cli_map::MAX_CREATE_FEATURES)
        )),
        "MAX_CREATE_FEATURES is {} but the application's is not",
        ds_cli_map::MAX_CREATE_FEATURES
    );

    // The selector's id bound is written into the schema rather than a
    // constant, so it is read from where it is enforced.
    let selector = schema_region(&app.frontend, "design.features.select");
    assert!(
        selector.contains(&format!(
            "maxItems: {}",
            grouped(ds_cli_map::design::MAX_SELECTOR_IDS)
        )),
        "MAX_SELECTOR_IDS is {} but design.features.select does not bound ids there",
        ds_cli_map::design::MAX_SELECTOR_IDS
    );
}

/// TypeScript writes large literals with underscore separators.
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
fn the_analysis_id_ds_map_composes_is_the_one_the_application_resolves() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

    // The one identifier `ds` builds rather than receives, because the bridge
    // publishes no operation that lists analysis layers. If the application
    // ever changes how it keys a temporary layer, every vector-tool call
    // starts refusing with "not available for analysis" and nothing else
    // would say why.
    let prefix = ds_cli_map::ANALYSIS_SKETCH_PREFIX.trim_end_matches(':');
    assert!(
        app.analysis
            .contains(&format!("id: `{prefix}:${{layer.id}}`")),
        "`ds map` composes analysis ids as `{prefix}:<layer id>`, but \
         loadOutlierLayerOptions no longer keys temporary layers that way. \
         Every vector-tool call would refuse."
    );
}

#[test]
fn the_snapshot_fields_map_view_reads_are_still_published() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

    // Read by index, so a rename is silent: `ds map view` would report a map
    // with no layers rather than an error.
    let snapshot = between(&app.frontend, "function mapSnapshot(", "\nfunction ");
    assert!(
        !snapshot.is_empty(),
        "mapSnapshot was not found in the application"
    );

    let mut expected: Vec<&str> = vec![
        ds_cli_map::SNAPSHOT_OPEN,
        ds_cli_map::SNAPSHOT_LAYERS,
        ds_cli_map::SNAPSHOT_LAYER_ID,
    ];
    expected.extend(ds_cli_map::SNAPSHOT_VIEW_FIELDS);
    expected.extend(
        ds_cli_map::SNAPSHOT_LAYER_FIELDS
            .iter()
            .map(|(_, published)| *published),
    );

    for field in expected {
        assert!(
            snapshot.contains(&format!("{field}:")),
            "`ds map view` reads `{field}` from the map snapshot, but \
             mapSnapshot no longer publishes it. A renamed field does not \
             fail — it reports null."
        );
    }
}

#[test]
fn the_messages_ds_map_classifies_on_are_still_the_applications() {
    let Some(app) = app() else {
        skip("the ds-web sibling repository is not on disk");
        return;
    };

    // Two refusals are translated from the application's prose into typed
    // codes: a signed-out design call becomes `desktop_signed_out`, and a
    // stale room becomes the conflict `transformer_changed`. Keying on prose
    // is the least stable thing this domain does, so the markers are checked
    // — and when none match, the untranslated refusal is what a caller gets,
    // which is wrong but never misleading.
    let lowered = app.frontend.to_ascii_lowercase();

    assert!(
        ds_cli_map::SIGNED_OUT_MARKERS
            .iter()
            .any(|marker| lowered.contains(marker)),
        "none of the signed-out markers appear in the application any more, so \
         a design call with no project would report `desktop_refused` instead \
         of `desktop_signed_out`"
    );
    assert!(
        lowered.contains(ds_cli_map::design::save::CONFLICT_MARKER),
        "the save-conflict marker no longer appears in the application, so a \
         stale room would report as a failure rather than a conflict"
    );
}
