//! The protocol boundary, pinned.
//!
//! This repository is `ds-cli`. MCP is one way to reach it, not what it is:
//! `ds mcp serve` is this same executable answering JSON-RPC on stdio, and it
//! reaches a command the way anything else does — by running `ds <argv>` and
//! reading the envelope. `ds-cli-mcp` therefore links no domain crate, and no
//! domain crate knows it exists. A protocol that replaces MCP is a sibling
//! leaf crate written against the same two things: the `Command` descriptor in
//! `ds-cli-contract`, and `run_cli`.
//!
//! That held by construction and by reading, which is exactly the state
//! `process_boundary.rs` was written about: a structural claim that is true
//! until the afternoon somebody adds one line, with every other test still
//! green. Two leaks had already appeared that way. `DS_MCP_CHILD` was read
//! inside `ds-cli-auth`, so the auth domain branched on a variable named for
//! a protocol it must not know; `DS_MCP_SCHEMA_ONLY` did the same to `ds
//! capabilities`. Neither fact was about MCP — one is "no human is at a
//! terminal", the other is "a schema is enough" — and the next protocol would
//! have arrived as an edit to auth and to meta rather than to the adapter.
//! They are now `DS_CLI_NONINTERACTIVE` and `DS_CLI_SCHEMA_ONLY`.
//!
//! So this suite pins the inventory instead of the prose. What it forbids is
//! *code* coupling: a crate name, a module path, a protocol-named variable.
//! Prose is not coupling — a doc comment in `ds-cli-contract` saying "MCP uses
//! this when it projects a descriptor into a tool" documents a consumer and
//! costs nothing to delete, so nothing here objects to the word MCP in a
//! comment. See docs/contracts/protocol-boundary-contract.md.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The crates that speak a host protocol rather than a domain. Nothing below
/// this line may name them.
const PROTOCOL_CRATES: &[&str] = &["ds-cli-mcp", "ds-cli-skills"];

/// Every package permitted to depend on a protocol crate, and why.
///
/// Adding a row is a deliberate act: it means a domain now compiles against a
/// protocol, and the next protocol will cost an edit here.
const PROTOCOL_CONSUMERS: &[(&str, &str, &str)] = &[
    (
        "ds",
        "ds-cli-mcp",
        "the binary registers `mcp serve`/`mcp install` as one ordinary domain \
         in crates/ds/src/registry.rs, with the same Entry shape as every other \
         domain, so removing the protocol is removing rows",
    ),
    (
        "ds",
        "ds-cli-skills",
        "the binary registers the skills domain and reads the receipt-verified \
         bundle shipped beside this executable",
    ),
    (
        "ds-cli-mcp",
        "ds-cli-skills",
        "an MCP host is served its skills over the same connection; the skills \
         crate is a bundle reader and depends on no protocol in return",
    ),
];

/// Tokens that are code coupling to a protocol, not prose about one.
///
/// `DS_MCP_` is included because an environment variable named for a protocol
/// is the cheapest way to smuggle that protocol into a domain: the compiler
/// never sees the edge, so neither the crate graph nor a reviewer catches it.
const COUPLING_TOKENS: &[&str] = &["ds_cli_mcp", "ds_cli_skills", "DS_MCP_"];

/// Files allowed to contain a coupling token, and why.
const COUPLING_OWNERS: &[(&str, &str)] = &[
    (
        "crates/ds/src/registry.rs",
        "the binary's one registration table, where every domain is named once",
    ),
    (
        "crates/ds/src/meta.rs",
        "`ds diagnostics` reports the receipt-verified skill bundle shipped beside \
         this executable; the reader is called, and reads nothing back about a host",
    ),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root resolves")
}

fn rust_sources(root: &Path, into: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(root).unwrap_or_else(|error| {
        panic!("could not read {}: {error}", root.display());
    });
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            rust_sources(&path, into);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            into.push(path);
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("inside the workspace")
        .to_string_lossy()
        .replace('\\', "/")
}

/// Each crate directory name paired with the text of its manifest.
fn manifests() -> Vec<(String, String)> {
    let root = workspace_root();
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).expect("crates/ exists") {
        let path = entry.expect("a crate directory").path();
        let manifest = path.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .expect("a crate directory name")
            .to_string_lossy()
            .into_owned();
        found.push((
            name,
            std::fs::read_to_string(&manifest).expect("a UTF-8 manifest"),
        ));
    }
    assert!(
        found.len() > 20,
        "only {} manifests found; the walk is not reaching the tree",
        found.len()
    );
    found
}

/// A manifest declares a dependency when the crate name starts a line. A
/// mention inside a comment or a description does not.
fn declares(manifest: &str, dependency: &str) -> bool {
    manifest.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with(dependency)
            && line[dependency.len()..]
                .trim_start()
                .starts_with(['=', '.'])
    })
}

#[test]
fn only_the_pinned_packages_may_depend_on_a_protocol_crate() {
    let declared: BTreeSet<(&str, &str)> = PROTOCOL_CONSUMERS
        .iter()
        .map(|(package, protocol, _)| (*package, *protocol))
        .collect();

    let mut found = BTreeSet::new();
    for (name, manifest) in manifests() {
        for protocol in PROTOCOL_CRATES {
            if name == *protocol {
                continue;
            }
            if declares(&manifest, protocol) {
                found.insert((name.clone(), (*protocol).to_string()));
            }
        }
    }

    let undeclared: Vec<&(String, String)> = found
        .iter()
        .filter(|(package, protocol)| !declared.contains(&(package.as_str(), protocol.as_str())))
        .collect();
    assert!(
        undeclared.is_empty(),
        "these packages now compile against a host protocol: {undeclared:?}\n\
         A domain reaches a host by being called, never by calling one. If this is \
         genuinely intended, add it to `PROTOCOL_CONSUMERS` in this file with the \
         reason. See docs/contracts/protocol-boundary-contract.md."
    );

    let stale: Vec<&(&str, &str)> = declared
        .iter()
        .filter(|(package, protocol)| {
            !found.contains(&((*package).to_string(), (*protocol).to_string()))
        })
        .collect();
    assert!(
        stale.is_empty(),
        "these pinned consumers no longer depend on a protocol crate; remove them: {stale:?}"
    );
}

#[test]
fn the_protocol_adapter_links_no_domain() {
    // The property the whole boundary rests on. `ds-cli-mcp` may link the
    // contract and the skills bundle and nothing else of ours: with no domain
    // in its graph it *cannot* call one in process, so the only route it has
    // is the one the CLI publishes. That is what makes replacing the protocol
    // a rewrite of one leaf rather than a pass over every domain.
    let manifest =
        std::fs::read_to_string(workspace_root().join("crates/ds-cli-mcp/Cargo.toml"))
            .expect("the ds-cli-mcp manifest");
    let permitted = ["ds-cli-contract", "ds-cli-skills"];

    for (name, _) in manifests() {
        if !name.starts_with("ds") || permitted.contains(&name.as_str()) || name == "ds-cli-mcp" {
            continue;
        }
        assert!(
            !declares(&manifest, &name),
            "`ds-cli-mcp` now links `{name}`. The adapter reaches a command by running \
             `ds <argv>` and reading the envelope, exactly as a person at a terminal does; \
             linking a domain gives the protocol a second, private route into it."
        );
    }
}

#[test]
fn no_domain_carries_a_protocol_token() {
    let root = workspace_root();
    let owners: BTreeSet<&str> = COUPLING_OWNERS.iter().map(|(path, _)| *path).collect();

    let mut sources = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).expect("crates/ exists") {
        let path = entry.expect("a crate directory").path();
        let name = path.file_name().expect("a name").to_string_lossy().into_owned();
        if PROTOCOL_CRATES.contains(&name.as_str()) {
            continue;
        }
        let source = path.join("src");
        if source.is_dir() {
            rust_sources(&source, &mut sources);
        }
    }
    assert!(
        sources.len() > 50,
        "only {} source files found; the walk is not reaching the tree",
        sources.len()
    );

    let mut offences = Vec::new();
    for path in sources {
        let text = std::fs::read_to_string(&path).expect("a UTF-8 source file");
        let file = relative(&root, &path);
        if owners.contains(file.as_str()) {
            continue;
        }
        for token in COUPLING_TOKENS {
            if text.contains(token) {
                offences.push(format!("{file}: {token}"));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "these files couple a domain to a host protocol: {offences:?}\n\
         Name the fact, not the protocol: a child with no terminal is \
         `DS_CLI_NONINTERACTIVE`, not `DS_MCP_CHILD`. Otherwise the next protocol \
         arrives as an edit to this domain instead of to the adapter. See \
         docs/contracts/protocol-boundary-contract.md."
    );
}

#[test]
fn every_pinned_consumer_and_owner_states_its_reason() {
    for (package, protocol, reason) in PROTOCOL_CONSUMERS {
        assert!(
            reason.len() > 40,
            "`{package}` -> `{protocol}` needs a real justification, not a placeholder"
        );
    }
    for (path, reason) in COUPLING_OWNERS {
        assert!(
            reason.len() > 20,
            "`{path}` needs a real justification, not a placeholder"
        );
        let file = workspace_root().join(path);
        assert!(file.is_file(), "`{path}` is pinned but does not exist");
        // An exemption nobody needs is an exemption nobody reads. Drop it, so
        // the list stays the short inventory it is meant to be.
        let text = std::fs::read_to_string(&file).expect("a UTF-8 source file");
        assert!(
            COUPLING_TOKENS.iter().any(|token| text.contains(token)),
            "`{path}` carries no protocol token any more; remove its exemption"
        );
    }
}
