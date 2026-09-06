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
        "invalid_lane",
        "raised only if the auth host receives a lane outside the command's parser-enforced stable/canary choices",
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
    (
        "catalog_action_invalid",
        "raised only if a parser-validated global catalog action has no match arm behind it",
    ),
    (
        "catalog_action_not_allowed",
        "raised only if a parser-validated global catalog action escapes the exact read/write allowlist that declared it",
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
                // The code argument itself must be the literal. Only leading
                // whitespace may precede it, which is what keeps the wrapped
                // form above working. Searching the whole window for the first
                // quote instead would read a *later* literal — the next
                // argument, or the next statement's map key — as this call's
                // code, and report a word like `remedy` as an undocumented
                // refusal. A constructor given a variable has no literal code
                // to document, and the code it does carry is covered from its
                // own side by `every_application_refusal_code_is_documented`.
                let trimmed = window.trim_start();
                if !trimmed.starts_with('"') {
                    continue;
                }
                let open = window.len() - trimmed.len();
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
        // Native auth is now a shared selected-project client boundary used by
        // Design, Survey/Forms, and Solar commands as well as `ds auth`.
        // Caller commands declare the relevant helper refusals.
        ("ds-cli-auth", None),
        ("ds-cli-data", Some("data")),
        ("ds-cli-design", Some("design")),
        ("ds-cli-map", Some("map")),
        ("ds-cli-dsgrid", Some("dsgrid")),
        ("ds-cli-dsgrid-exchange", Some("dsgrid-exchange")),
        ("ds-cli-library", Some("library")),
        ("ds-cli-pls", Some("pls")),
        ("ds-cli-report", Some("report")),
        ("ds-cli-solar", Some("solar")),
        ("ds-cli-work", Some("work")),
        ("ds-cli-design", Some("design")),
        ("ds-cli-sre", Some("sre")),
        ("ds-cli-survey", Some("survey")),
        ("ds-cli-style", Some("style")),
        ("ds-cli-tile", Some("tile")),
        ("ds-cli-feedback", Some("feedback")),
        ("ds-cli-shell", Some("shell")),
        ("ds-cli-workstation", Some("workstation")),
        ("ds-cli-mcp", Some("mcp")),
        // Receipt verification returns bounded diagnostic strings to doctor
        // and MCP resources; it constructs no CLI Failure/refusal codes.
        ("ds-cli-skills", None),
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

/// The paired application's half of the same invariant.
///
/// A refusal raised in DS GridDesign now reaches the caller with its own class,
/// code and remedy for *every* operation, not just an allowlisted two (see
/// `structured_desktop_refusal` in `ds-cli-desktop`). The allowlist was what
/// used to guarantee that a code crossing the bridge was one `ds` documents;
/// removing it without replacing that guarantee would let the application mint
/// codes no `--help` mentions. This test is the replacement: the *declaration*
/// is the contract, and an undeclared code is a failing build rather than a
/// surprise in production.
///
/// Source analysis, like the Rust half above, and for the same reason: these
/// refusals are reached only in situations a test cannot reliably produce.
fn ds_web() -> Option<PathBuf> {
    let root = match std::env::var_os("DS_WEB_DIR") {
        Some(explicit) => PathBuf::from(explicit),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-web"),
    };
    let root = root.canonicalize().unwrap_or(root);
    root.is_dir().then_some(root)
}

/// Every code the application can hand the bridge, read from its `cliRefusal`
/// call sites. The class is the first argument and the code the second.
fn application_refusal_codes(root: &Path) -> BTreeSet<String> {
    let mut codes = BTreeSet::new();
    let mut files = Vec::new();
    typescript_files(&root.join("src"), &mut files);
    for file in files {
        // A test may construct a refusal to assert the bridge's own behaviour;
        // that is not a code the product can emit.
        if file
            .to_str()
            .is_some_and(|path| path.contains(".test.") || path.contains("/tests/"))
        {
            continue;
        }
        let source = std::fs::read_to_string(&file).expect("read application source");
        let mut rest = source.as_str();
        while let Some(at) = rest.find("cliRefusal(") {
            rest = &rest[at + "cliRefusal(".len()..];
            let window = &rest[..char_boundary(rest, 300)];
            // Skip the class argument, then read the code literal.
            let Some(comma) = window.find(',') else {
                continue;
            };
            let after_class = &window[comma + 1..];
            let Some(open) = after_class.find('\'').or_else(|| after_class.find('"')) else {
                continue;
            };
            let quote = after_class.as_bytes()[open] as char;
            let after = &after_class[open + 1..];
            let Some(close) = after.find(quote) else {
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
    codes
}

fn typescript_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            typescript_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "ts") {
            out.push(path);
        }
    }
}

#[test]
fn every_application_refusal_code_is_documented() {
    let Some(root) = ds_web() else {
        eprintln!(
            "SKIPPED: this check proves every refusal DS GridDesign can send \
             across the bridge is one `ds` documents.\n  Set DS_WEB_DIR to the \
             ds-web checkout to run it."
        );
        return;
    };
    let codes = application_refusal_codes(&root);
    assert!(
        !codes.is_empty(),
        "no `cliRefusal` call sites were found in {}. The scan is matching \
         nothing, which would make this check vacuous — confirm the helper is \
         still named `cliRefusal` before trusting a pass.",
        root.display()
    );

    let (_, all_declared) = declared_codes();
    let undocumented: Vec<String> = codes
        .iter()
        .filter(|code| !all_declared.contains(*code))
        .map(|code| format!("  `{code}`"))
        .collect();

    assert!(
        undocumented.is_empty(),
        "DS GridDesign can refuse with these codes, but no `ds` command \
         declares them:\n{}\n\n\
         The bridge now preserves an application refusal's own class, code and \
         remedy for every operation, so an undeclared code would reach a \
         caller that cannot look it up. Add each to the REFUSALS of the \
         command whose operation raises it.",
        undocumented.join("\n")
    );
}
