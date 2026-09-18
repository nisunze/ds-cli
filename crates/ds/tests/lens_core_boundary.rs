//! The lens/core crate fence, pinned.
//!
//! `ds-command-kernel/docs/contracts/ds-lens-core-boundary.md` §4 L0a: the
//! desktop is the Server plus a window, so every command is either **core**
//! (answers identically on both) or **lens** (needs the window). The window is
//! reachable through exactly one crate — `ds-cli-desktop`, which owns the
//! descriptor, the pairing secret, `paired_availability` and every `desktop_*`
//! refusal. A core crate that cannot name that crate cannot open a window,
//! cannot invent a `desktop_*` refusal, and cannot grow a second host-shaped
//! code path, because the symbol does not exist.
//!
//! Today it does exist nearly everywhere: fifteen crates depend on the bridge
//! and ~35 non-test files reach for it. None of that is a bug on its own —
//! each call site is a command that has not been given its headless form yet
//! (the host-transparency backlog). What was missing is a floor: nothing
//! stopped a sixteenth crate from joining, and nothing recorded the size of
//! the problem.
//!
//! So this suite pins the inventory rather than the prose, the same way
//! `process_boundary.rs` pins process construction. The numbers may only fall.
//! A new crate reaching for the bridge is red here and has to be classified:
//! either it is a lens crate, or its command needs the core implementation the
//! contract asks for.
//!
//! The finish line is two dependents — `ds-cli-map` (the window's own crate)
//! and `ds` (the one binary, which is Server + window). At that point L0a
//! holds by construction and this suite is the proof, not the reminder.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The only crate that may open a paired window.
const BRIDGE: &str = "ds-cli-desktop";

/// Which layer a bridge dependent belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    /// The window's own crate. Depending on the bridge is its purpose.
    Lens,
    /// The one binary. The desktop is Server + window, so it links both.
    Host,
    /// A core domain crate that still holds desktop-bound commands. Every
    /// entry here is host-transparency work that has not landed; the entry
    /// leaves when its last command gains a headless form or moves to the lens
    /// crate, and the `ds-cli-desktop` dependency line goes with it.
    CorePending,
}

/// Every crate that may name the bridge today, its layer, and how many of its
/// non-test source files reach for it.
///
/// Measured 2026-09-18 at `52b1292`. `files` is a ceiling, never a target: a
/// crate may drop to zero and leave, but it may not climb. `ds-cli-survey`
/// left on the same day: its control plane had already moved to the kernel and
/// all that remained was an empty `BRIDGE_OPS` const holding the dependency
/// open. `ds-cli-library` left the same day too, by the other route: its
/// thirteen governed global-catalog operations were given the headless owner
/// they always should have had, and `ds-cli-feedback` followed by collapsing
/// its two routes to the native one it already had.
const INVENTORY: &[(&str, Layer, usize)] = &[
    ("ds", Layer::Host, 1),
    ("ds-cli-map", Layer::Lens, 4),
    // ── core, pending a headless form ───────────────────────────────────────
    // Project documents: preview, read, classify and ingest still ask the
    // paired application for bytes.
    ("ds-cli-assets", Layer::CorePending, 10),
    // Device pairing discovery reads the running application's descriptor.
    ("ds-cli-auth", Layer::CorePending, 1),
    // GIS uploads and their tiling status.
    ("ds-cli-data", Layer::CorePending, 4),
    // Immutable attachments on design objects are paired-only (audit item 5).
    ("ds-cli-design", Layer::CorePending, 1),
    // Model preparation reads the application's active project cache.
    ("ds-cli-dsgrid", Layer::CorePending, 1),
    // Solar runs read the workspace the application holds open.
    ("ds-cli-solar", Layer::CorePending, 2),
    // Diagnostics inspect the live application.
    ("ds-cli-sre", Layer::CorePending, 1),
    // Styling saves round-trip through the open map's layer state.
    ("ds-cli-style", Layer::CorePending, 2),
    // Tile regeneration is requested through the application.
    ("ds-cli-tile", Layer::CorePending, 2),
    // Project Work tasks read the window's selection.
    ("ds-cli-work", Layer::CorePending, 1),
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("ds-cli workspace root")
}

/// Every crate directory in the workspace.
fn crate_dirs() -> Vec<(String, PathBuf)> {
    let crates = workspace_root().join("crates");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("crates directory") {
        let path = entry.expect("crate entry").path();
        if path.join("Cargo.toml").is_file() {
            let name = path
                .file_name()
                .expect("crate directory name")
                .to_string_lossy()
                .into_owned();
            found.push((name, path));
        }
    }
    found.sort();
    found
}

/// Does this manifest declare a dependency on the bridge?
///
/// Deliberately blind to which table it sits in: a core crate that reaches the
/// window from its own tests is entangled in exactly the same way.
fn declares_bridge(manifest: &Path) -> bool {
    let text = std::fs::read_to_string(manifest).expect("crate manifest");
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .any(|line| line.starts_with(BRIDGE))
}

/// Non-test source files that name the bridge.
fn bridge_call_sites(crate_dir: &Path) -> usize {
    fn walk(dir: &Path, hits: &mut usize) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, hits);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                if text.contains("ds_cli_desktop") {
                    *hits += 1;
                }
            }
        }
    }
    let mut hits = 0;
    walk(&crate_dir.join("src"), &mut hits);
    hits
}

#[test]
fn only_the_frozen_inventory_may_open_a_paired_window() {
    let declared: BTreeSet<String> = crate_dirs()
        .into_iter()
        .filter(|(name, dir)| name != BRIDGE && declares_bridge(&dir.join("Cargo.toml")))
        .map(|(name, _)| name)
        .collect();
    let frozen: BTreeSet<String> = INVENTORY
        .iter()
        .map(|(name, _, _)| (*name).to_string())
        .collect();

    let arrivals: Vec<&String> = declared.difference(&frozen).collect();
    assert!(
        arrivals.is_empty(),
        "these crates newly depend on {BRIDGE}, which only a lens crate may do: {arrivals:?}\n\
         Give the command its headless core form, or move it to ds-cli-map as a map.* \
         command (ds-lens-core-boundary.md §4 L0a)."
    );

    let departures: Vec<&String> = frozen.difference(&declared).collect();
    assert!(
        departures.is_empty(),
        "these crates no longer depend on {BRIDGE} — remove them from INVENTORY so the \
         fence records the ground gained: {departures:?}"
    );
}

#[test]
fn core_crates_never_grow_a_new_bridge_call_site() {
    let measured: BTreeMap<String, usize> = crate_dirs()
        .into_iter()
        .map(|(name, dir)| (name, bridge_call_sites(&dir)))
        .collect();

    let mut regressions = Vec::new();
    let mut gains = Vec::new();
    for (name, layer, ceiling) in INVENTORY {
        // The lens crate is where window code belongs; it may grow.
        if *layer != Layer::CorePending {
            continue;
        }
        let now = measured.get(*name).copied().unwrap_or_default();
        if now > *ceiling {
            regressions.push(format!("{name}: {now} files, ceiling {ceiling}"));
        } else if now < *ceiling {
            gains.push(format!("{name}: {now} files, ceiling still {ceiling}"));
        }
    }

    assert!(
        regressions.is_empty(),
        "core crates reached for the paired window in new files: {regressions:#?}\n\
         A core command answers the same on the Server and the desktop; it cannot need \
         a window (ds-lens-core-boundary.md §4 L0)."
    );
    assert!(
        gains.is_empty(),
        "these ceilings are now too high — lower them in INVENTORY so the fence keeps \
         the ground gained: {gains:#?}"
    );
}

#[test]
fn the_target_layers_are_exactly_the_window_and_the_binary() {
    let lens: Vec<&str> = INVENTORY
        .iter()
        .filter(|(_, layer, _)| *layer == Layer::Lens)
        .map(|(name, _, _)| *name)
        .collect();
    let host: Vec<&str> = INVENTORY
        .iter()
        .filter(|(_, layer, _)| *layer == Layer::Host)
        .map(|(name, _, _)| *name)
        .collect();
    assert_eq!(
        lens,
        vec!["ds-cli-map"],
        "the lens layer is the window's crate; a second one is a design decision, \
         not a refactor (ds-lens-core-boundary.md §4 L0a)"
    );
    assert_eq!(
        host,
        vec!["ds"],
        "one binary is the host: the desktop is Server + window"
    );
}
