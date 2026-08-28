//! Every refusal code a command can emit must be documented.
//!
//! `ds` promises that a caller can plan for failure from `--help` alone: the
//! REFUSALS section lists each code with the situation that produces it and
//! the remedy. That promise is only worth anything if the list is complete,
//! and completeness is exactly the property that rots — a handler grows a new
//! `Failure::invalid("something_new", …)` and nothing notices.
//!
//! So this test reads the domain crates' own source, collects every literal
//! error code they can construct, and requires each one to appear in some
//! command's declared refusals. It is source analysis rather than execution
//! because most of these codes are reached only in situations a test cannot
//! reliably produce — a full disk, a killed engine, a corrupted package.
//!
//! Codes that are genuinely internal-only are listed in [`NOT_A_REFUSAL`],
//! with the reason. That list is the escape hatch, and it is deliberately
//! short: putting a code there is a claim that a caller can never see it.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod common;

/// Codes a caller cannot reach, with why.
const NOT_A_REFUSAL: &[(&str, &str)] = &[
    (
        "missing_declared_input",
        "raised only if a command declares an input required and the parser \
         then fails to supply it — a defect in ds, not a situation a caller \
         can create",
    ),
    (
        "unmapped_task",
        "raised only if a validated --task choice has no engine subcommand \
         behind it, which the choice list makes unreachable",
    ),
    (
        "unmapped_choice",
        "raised only if a validated --target/--mode/--container choice has no          engine value behind it, which the choice list makes unreachable",
    ),
    (
        "callee_wait_failed",
        "raised only if the OS cannot report on a child ds itself spawned",
    ),
    (
        "undeclared_bridge_argument",
        "raised only if a ds map handler builds an argument key its own \
         BridgeOp does not declare — a defect in ds caught at the boundary, \
         and one tests/bridge_parity.rs proves cannot be a schema drift",
    ),
];

fn ds(args: &[&str]) -> Value {
    common::json(args).0
}

/// Codes declared by each domain's commands, plus the union across all of
/// them.
///
/// Source ownership is by crate, while a number of commands share a crate and
/// can legitimately share a failure constructor. This static check therefore
/// proves the narrower, truthful invariant: every constructed caller-visible
/// code is declared by at least one command in its owner domain. Per-command
/// execution and descriptor tests cover the command-specific contract; do not
/// describe this aggregate source scan as proof of a particular command's
/// REFUSALS section.
fn declared_codes() -> (BTreeMap<String, BTreeSet<String>>, BTreeSet<String>) {
    let mut by_domain: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut all = BTreeSet::new();
    let index = ds(&["capabilities", "--output", "json"]);

    let mut targets: Vec<(String, String)> = Vec::new();
    for domain in index["data"]["domains"].as_array().expect("domains") {
        let id = domain["id"].as_str().expect("domain id");
        let commands = ds(&["capabilities", id, "--output", "json"]);
        for command in commands["data"]["commands"].as_array().expect("commands") {
            targets.push((
                id.to_string(),
                command["id"].as_str().expect("id").to_string(),
            ));
        }
    }
    for meta in common::META_COMMANDS {
        targets.push(("meta".to_string(), meta.to_string()));
    }

    for (domain, id) in targets {
        let descriptor = ds(&["capabilities", &id, "--output", "json"]);
        let command = &descriptor["data"]["command"];
        let entry = by_domain.entry(domain).or_default();
        for refusal in command["refusals"].as_array().into_iter().flatten() {
            if let Some(code) = refusal["code"].as_str() {
                entry.insert(code.to_string());
                all.insert(code.to_string());
            }
        }
        // An availability check's code is also caller-visible, through the
        // dispatch gate, so it counts as declared.
        if let Some(code) = command["unavailable"]["code"].as_str() {
            entry.insert(code.to_string());
            all.insert(code.to_string());
        }
    }
    (by_domain, all)
}

/// Literal codes constructed anywhere under `dir`.
fn constructed_codes(dir: &Path) -> BTreeSet<String> {
    const CONSTRUCTORS: &[&str] = &[
        "Failure::new(",
        "Failure::invalid(",
        "Failure::unavailable(",
        "Failure::unauthorized(",
        "Failure::conflict(",
        "Failure::failed(",
        "Failure::internal(",
        "Availability::unavailable(",
    ];

    let mut codes = BTreeSet::new();
    for file in rust_files(dir) {
        let source = std::fs::read_to_string(&file).expect("read source");
        for constructor in CONSTRUCTORS {
            let mut rest = source.as_str();
            while let Some(at) = rest.find(constructor) {
                rest = &rest[at + constructor.len()..];
                // `Failure::new` takes the class first; skip to the next
                // argument before reading the literal.
                let mut scan = rest;
                if *constructor == "Failure::new(" {
                    match scan.find(',') {
                        Some(comma) => scan = &scan[comma + 1..],
                        None => continue,
                    }
                }
                // Bound the window instead of refusing newlines. Rustfmt
                // wraps a constructor whose message is long, so the code
                // literal is routinely on the line *after* the paren — an
                // earlier version of this scan skipped exactly those and
                // reported clean while three codes went undocumented.
                // Slice on a char boundary: these sources contain em dashes
                // and arrows, and a byte-index cut lands inside one.
                let window = &scan[..char_boundary(scan, 300)];
                let Some(open) = window.find('"') else {
                    continue;
                };
                let after = &window[open + 1..];
                let Some(close) = after.find('"') else {
                    continue;
                };
                let code = &after[..close];
                if !code.is_empty()
                    && code
                        .chars()
                        .all(|character| character.is_ascii_lowercase() || character == '_')
                {
                    codes.insert(code.to_string());
                }
            }
        }
    }
    codes
}

/// The largest char boundary at or below `limit` bytes.
fn char_boundary(text: &str, limit: usize) -> usize {
    let mut end = limit.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}

fn crates_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
        .join("crates")
}

#[test]
fn every_constructible_refusal_code_is_documented() {
    let (by_domain, all_declared) = declared_codes();
    assert!(
        !all_declared.is_empty(),
        "no refusal codes were declared at all"
    );

    let exempt: BTreeSet<&str> = NOT_A_REFUSAL.iter().map(|(code, _)| *code).collect();
    let root = crates_root();

    // Domain crates map to the domain whose commands must document them.
    // `ds-cli-contract` is excluded: its codes are the parser's own
    // (unknown_flag, missing_value, invalid_choice …) and apply to every
    // command equally, so they are documented once in the output contract
    // rather than repeated in every REFUSALS section.
    let domain_crates = [
        ("ds-cli-map", Some("map")),
        ("ds-cli-dsgrid", Some("dsgrid")),
        ("ds-cli-dsgrid-exchange", Some("dsgrid-exchange")),
        ("ds-cli-library", Some("library")),
        ("ds-cli-pls", Some("pls")),
        ("ds-cli-report", Some("report")),
        ("ds-cli-solar", Some("solar")),
        ("ds-cli-work", Some("work")),
        ("ds-cli-sre", Some("sre")),
        ("ds-cli-style", Some("style")),
        ("ds-cli-tile", Some("tile")),
        ("ds-cli-feedback", Some("feedback")),
        ("ds-cli-shell", Some("shell")),
        ("ds-cli-workstation", Some("workstation")),
        ("ds-cli-mcp", Some("mcp")),
        // Shared across every calling domain; declaring it in any one of them
        // is enough for this check, and the per-command help of each caller
        // is what the domain checks above enforce.
        ("ds-cli-exec", None),
        // Also shared, and it became so: `ds-cli-desktop` is the paired-session
        // authority surface, and `ds solar prepare` borrows it to have the
        // application perform an authenticated fetch. Its pairing refusals are
        // therefore reachable from more than the `desktop` domain, and each
        // caller declares them in its own REFUSALS — which is what a reader of
        // one command's help actually needs.
        ("ds-cli-desktop", None),
    ];

    // A domain crate missing from the list above is silently unchecked, which
    // is exactly how a new domain ships undocumented codes. Prove the list
    // covers every crate on disk rather than trusting that it does.
    let listed: BTreeSet<&str> = domain_crates.iter().map(|(name, _)| *name).collect();
    for entry in std::fs::read_dir(&root).expect("read crates dir").flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "ds" || name == "ds-cli-contract" || !entry.path().is_dir() {
            continue;
        }
        assert!(
            listed.contains(name.as_str()),
            "crate `{name}` is not covered by this test. Add it to `domain_crates` \
             with the domain whose commands must document its codes."
        );
    }

    let mut undocumented: Vec<String> = Vec::new();
    for (crate_name, domain) in domain_crates {
        let dir = root.join(crate_name).join("src");
        assert!(dir.is_dir(), "crate source missing: {}", dir.display());

        let declared = match domain {
            Some(domain) => by_domain.get(domain).cloned().unwrap_or_default(),
            None => all_declared.clone(),
        };

        for code in constructed_codes(&dir) {
            if declared.contains(&code) || exempt.contains(code.as_str()) {
                continue;
            }
            match domain {
                Some(domain) => undocumented.push(format!(
                    "  {crate_name}: `{code}` (no `ds {domain}` command declares it)"
                )),
                None => undocumented.push(format!(
                    "  {crate_name}: `{code}` (no command anywhere declares it)"
                )),
            }
        }
    }

    assert!(
        undocumented.is_empty(),
        "these refusal codes can be emitted but are not documented:\n{}\n\n\
         Add each to the REFUSALS of the command that emits it — with the \
         situation and a remedy — or, if a caller truly cannot reach it, list \
         it in NOT_A_REFUSAL with the reason.",
        undocumented.join("\n")
    );
}
