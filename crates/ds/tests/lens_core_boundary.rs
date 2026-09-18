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
//!
//! The crate inventory answers "who *can* open a window". It cannot answer
//! the question an operator actually asks — "can this command run on my
//! server?" — because a crate is not a command: one `ds-cli-design`
//! dependency line stands for twenty-eight desktop-bound commands, and
//! `ds-cli-map` holds server commands next to window ones. That fact is now
//! declared per command, as `Requires` on the descriptor, and the second half
//! of this suite pins it the same way: a window command may only live where
//! the bridge does, the declared fact may never disagree with the paired
//! availability that implements it, and the per-crate counts are ceilings
//! that may only fall.

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
/// they always should have had, and `ds-cli-feedback`, `ds-cli-tile` and
/// `ds-cli-style` followed — the last two by collapsing two routes to the
/// native one they already had, and in `ds style`'s case already defaulted to.
/// `ds-cli-sre` left on 2026-09-18 as well: both of its commands were pure
/// server reads of platform health, which is exactly the thing a server must
/// be able to read.
const INVENTORY: &[(&str, Layer, usize)] = &[
    ("ds", Layer::Host, 1),
    ("ds-cli-map", Layer::Lens, 4),
    // ── core, pending a headless form ───────────────────────────────────────
    // Project documents: preview, read, classify and ingest still ask the
    // paired application for bytes.
    ("ds-cli-assets", Layer::CorePending, 10),
    // Device pairing discovery reads the running application's descriptor.
    ("ds-cli-auth", Layer::CorePending, 1),
    // Native elevation on a point file or over an area, and attaching the
    // project's pinned Rwanda hierarchy to one: local file compute whose
    // engine and component manager live in the desktop shell. The two national
    // boundary READS left on 2026-09-18 — they are gateway reads with no
    // project and nothing to render.
    ("ds-cli-data", Layer::CorePending, 4),
    // Immutable attachments on design objects are paired-only (audit item 5).
    ("ds-cli-design", Layer::CorePending, 1),
    // Model preparation reads the application's active project cache.
    ("ds-cli-dsgrid", Layer::CorePending, 1),
    // Solar runs read the workspace the application holds open.
    ("ds-cli-solar", Layer::CorePending, 2),
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

// ── the declared fact: which commands still need the window ────────────────

/// How many window commands each crate declares today.
///
/// A ceiling, never a target, exactly like `INVENTORY`'s file counts — and
/// measured the same way, by reading the source rather than the registry,
/// because a crate is a directory and the registry does not know which one a
/// command came from. What is counted is a *declaration site*: a `Command`
/// value carrying `requires: Requires::Window`. `ds-cli-design` builds three
/// of its paired commands from one shared constructor, so its twenty-eight
/// registered window commands are twenty-six declarations here. Lowering a
/// number is the entire point of the host-transparency backlog; raising one
/// is a new desktop-bound command, which is the thing this suite exists to
/// refuse.
///
/// `ds-cli-desktop` is on this list and deliberately not on `INVENTORY`: it
/// *is* the bridge, so it cannot "depend on" it, but its commands are window
/// commands like any other and its count has to be able to fall too.
///
/// Measured 2026-09-18 against the run tip.
const WINDOW_COMMANDS: &[(&str, usize)] = &[
    // The bridge's own domain: pairing, sync, printing, published artifacts.
    ("ds-cli-desktop", 25),
    // The lens crate. Window commands are its purpose; its server commands
    // (the machine-local layer catalogue) are not counted here.
    ("ds-cli-map", 39),
    // ── core, pending a headless form ───────────────────────────────────────
    ("ds-cli-assets", 9),
    ("ds-cli-auth", 1),
    ("ds-cli-data", 4),
    ("ds-cli-design", 26),
    ("ds-cli-dsgrid", 1),
    ("ds-cli-solar", 16),
    ("ds-cli-work", 9),
];

/// Non-test source lines in this crate that declare a window command.
fn window_declarations(crate_dir: &Path) -> usize {
    source_lines(crate_dir, |line| {
        line.contains("requires: Requires::Window")
    })
}

/// Count non-comment source lines that match, under this crate's `src`.
///
/// Comments are skipped deliberately: both of the things counted here are
/// discussed in prose right next to the code that does them — the one command
/// that reaches the window without declaring a paired availability explains
/// itself in a comment — and a ledger that counted its own documentation
/// would be unmaintainable.
fn source_lines(crate_dir: &Path, matches: impl Fn(&str) -> bool + Copy) -> usize {
    source_lines_by_file(crate_dir, matches).values().sum()
}

/// The same count, kept per file, for the checks that compare two counts.
///
/// Per file rather than per crate because a crate total hides a swap: a
/// command that drops `Requires::Window` while another gains it leaves every
/// crate total unmoved, and one of those two commands is then described to an
/// operator as running somewhere it does not. A `Command` value and the
/// availability it hands over are written in one file, so the file is the
/// smallest unit where the two halves can be compared without parsing Rust.
fn source_lines_by_file(
    crate_dir: &Path,
    matches: impl Fn(&str) -> bool + Copy,
) -> BTreeMap<PathBuf, usize> {
    fn walk(dir: &Path, matches: &dyn Fn(&str) -> bool, hits: &mut BTreeMap<PathBuf, usize>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, matches, hits);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let count = text
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .filter(|line| matches(line))
                    .count();
                if count > 0 {
                    hits.insert(path, count);
                }
            }
        }
    }
    let mut hits = BTreeMap::new();
    walk(&crate_dir.join("src"), &matches, &mut hits);
    hits
}

#[test]
fn a_window_command_lives_only_where_the_window_is_reachable() {
    let mut incoherent = Vec::new();
    for (name, dir) in crate_dirs() {
        if window_declarations(&dir) == 0 {
            continue;
        }
        if name == BRIDGE || declares_bridge(&dir.join("Cargo.toml")) {
            continue;
        }
        incoherent.push(name);
    }

    assert!(
        incoherent.is_empty(),
        "these crates declare `Requires::Window` but cannot reach {BRIDGE}, so the \
         declaration is a claim the crate cannot honour: {incoherent:?}\n\
         Either the command is server-first and the declaration is wrong, or it \
         belongs in ds-cli-map (ds-lens-core-boundary.md §4 L0a)."
    );
}

#[test]
fn the_declared_fact_never_disagrees_with_the_availability_behind_it() {
    let root = workspace_root();
    let mut disagreements = Vec::new();
    for (_, dir) in crate_dirs() {
        let declared =
            source_lines_by_file(&dir, |line| line.contains("requires: Requires::Window"));
        let paired = source_lines_by_file(&dir, |line| {
            line.contains("availability:") && line.contains("paired")
        });
        let files: BTreeSet<&PathBuf> = declared.keys().chain(paired.keys()).collect();
        for file in files {
            let declared = declared.get(file).copied().unwrap_or_default();
            let paired = paired.get(file).copied().unwrap_or_default();
            if declared != paired {
                let shown = file.strip_prefix(&root).unwrap_or(file);
                disagreements.push(format!(
                    "{}: {declared} declare window, {paired} use a paired availability",
                    shown.display()
                ));
            }
        }
    }

    assert!(
        disagreements.is_empty(),
        "the descriptor and the implementation disagree about which commands need \
         the window: {disagreements:#?}\n\
         `requires: Requires::Window` and a paired availability are two halves of \
         one fact. A command that asks the application for its answer declares it; \
         one that no longer does drops both in the same change."
    );
}

#[test]
fn window_command_counts_never_grow() {
    let measured: BTreeMap<String, usize> = crate_dirs()
        .into_iter()
        .map(|(name, dir)| (name, window_declarations(&dir)))
        .collect();
    let frozen: BTreeMap<&str, usize> = WINDOW_COMMANDS.iter().copied().collect();

    let mut regressions = Vec::new();
    let mut gains = Vec::new();
    for (name, ceiling) in &frozen {
        let now = measured.get(*name).copied().unwrap_or_default();
        if now > *ceiling {
            regressions.push(format!("{name}: {now} window commands, ceiling {ceiling}"));
        } else if now < *ceiling {
            gains.push(format!(
                "{name}: {now} window commands, ceiling still {ceiling}"
            ));
        }
    }
    let arrivals: Vec<&String> = measured
        .iter()
        .filter(|(name, count)| **count > 0 && !frozen.contains_key(name.as_str()))
        .map(|(name, _)| name)
        .collect();

    assert!(
        arrivals.is_empty(),
        "these crates gained their first window command: {arrivals:?}\n\
         A new desktop-bound command is a decision, not a refactor — give it its \
         headless owner instead (ds-lens-core-boundary.md §4 L0)."
    );
    assert!(
        regressions.is_empty(),
        "window commands multiplied: {regressions:#?}\n\
         Every one of these is a command an operator cannot run on a server."
    );
    assert!(
        gains.is_empty(),
        "these ceilings are now too high — lower them in WINDOW_COMMANDS so the \
         fence keeps the ground gained: {gains:#?}"
    );
}

#[test]
fn the_window_ledger_and_the_bridge_inventory_name_the_same_crates() {
    let ledger: BTreeSet<&str> = WINDOW_COMMANDS.iter().map(|(name, _)| *name).collect();
    let inventory: BTreeSet<&str> = INVENTORY
        .iter()
        // The one binary links the bridge because it *is* Server + window; it
        // registers no command of its own that needs one.
        .filter(|(_, layer, _)| *layer != Layer::Host)
        .map(|(name, _, _)| *name)
        .collect();

    let untracked: Vec<&&str> = inventory.difference(&ledger).collect();
    assert!(
        untracked.is_empty(),
        "these crates depend on {BRIDGE} but declare no window commands: {untracked:?}\n\
         If the last one is gone, drop the dependency and leave INVENTORY too — \
         that is the whole point of the entry."
    );

    let unexplained: Vec<&&str> = ledger
        .difference(&inventory)
        .filter(|name| ***name != *BRIDGE)
        .collect();
    assert!(
        unexplained.is_empty(),
        "these crates declare window commands without an INVENTORY entry \
         explaining why they may reach the bridge: {unexplained:?}"
    );
}

/// How many window commands each *domain* registers, as the surface reports
/// them.
///
/// [`WINDOW_COMMANDS`] above counts declaration sites, which is all a file
/// scan can see, and a shared constructor is the gap in that: `ds-cli-design`
/// builds three of its paired commands from one `command(...)` function, so
/// twenty-eight registered commands are twenty-six lines there. Registering a
/// fourth would add a window command an operator cannot run on a server while
/// every line count stayed still. So the backlog is pinned a second time, at
/// the other end — by the number `ds capabilities --requires window` gives the
/// caller who asks.
///
/// Two ledgers because a crate is not a domain and a line is not a command;
/// the same rule governs both, and the same rule governs the totals:
/// 130 declaration sites, 133 registered commands, and both may only fall.
///
/// Measured 2026-09-18 against the run tip.
const WINDOW_BACKLOG: &[(&str, u64)] = &[
    ("assets", 9),
    ("auth", 1),
    ("data", 4),
    ("design", 28),
    ("desktop", 26),
    ("dsgrid", 1),
    ("map", 39),
    ("solar", 16),
    ("work", 9),
];

/// What the filter answers for the whole surface. Pinned beside the rows so a
/// domain cannot be quietly dropped from the ledger to hide its commands.
const WINDOW_BACKLOG_TOTAL: u64 = 133;

#[test]
fn the_registered_window_backlog_never_grows() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["capabilities", "--requires", "window", "--output", "json"])
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("`ds capabilities --requires window` is JSON");
    let data = &answer["data"];

    let measured: BTreeMap<&str, u64> = data["domains"]
        .as_array()
        .expect("per-domain counts")
        .iter()
        .map(|domain| {
            (
                domain["id"].as_str().expect("domain id"),
                domain["commands"].as_u64().expect("domain count"),
            )
        })
        .collect();
    let frozen: BTreeMap<&str, u64> = WINDOW_BACKLOG.iter().copied().collect();

    let mut regressions = Vec::new();
    let mut gains = Vec::new();
    for (domain, ceiling) in &frozen {
        let now = measured.get(domain).copied().unwrap_or_default();
        if now > *ceiling {
            regressions.push(format!(
                "{domain}: {now} window commands, ceiling {ceiling}"
            ));
        } else if now < *ceiling {
            gains.push(format!(
                "{domain}: {now} window commands, ceiling still {ceiling}"
            ));
        }
    }
    let arrivals: Vec<&&str> = measured
        .keys()
        .filter(|domain| !frozen.contains_key(*domain))
        .collect();

    assert!(
        arrivals.is_empty(),
        "these domains gained their first window command: {arrivals:?}\n\
         A new desktop-bound command is a decision, not a refactor — give it its \
         headless owner instead (ds-lens-core-boundary.md §4 L0)."
    );
    assert!(
        regressions.is_empty(),
        "window commands multiplied: {regressions:#?}\n\
         Every one of these is a command an operator cannot run on a server, and \
         one added through a shared constructor moves no line count."
    );
    assert!(
        gains.is_empty(),
        "these ceilings are now too high — lower them in WINDOW_BACKLOG so the \
         fence keeps the ground gained: {gains:#?}"
    );

    let total = data["matched"].as_u64().expect("matched total");
    assert!(
        total <= WINDOW_BACKLOG_TOTAL,
        "the surface reports {total} window commands, above the frozen \
         {WINDOW_BACKLOG_TOTAL}"
    );
    assert_eq!(
        total, WINDOW_BACKLOG_TOTAL,
        "the backlog fell to {total} — lower WINDOW_BACKLOG_TOTAL so the fence \
         keeps the ground gained"
    );
}
