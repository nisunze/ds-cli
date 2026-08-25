//! Context size is a testable interface budget.
//!
//! The principal consumers of `ds` are coding agents whose entire working
//! memory is spent on whatever this binary prints. A help screen that doubles
//! is not a cosmetic regression — it is a capability the agent no longer has
//! room to use. So every tier has a byte ceiling.
//!
//! The ceilings are *derived*, not flat. A command that genuinely takes twelve
//! inputs must be allowed to describe twelve inputs; what a budget is for is
//! catching prose — an explanation that grew, an example that became a
//! tutorial, an architecture note that should have been a cross-link. So each
//! allowance is per declared thing, with a small frame.
//!
//! The most important assertion here is not any single number. It is
//! [`root_help_scales_with_domains_not_commands`]: root help must stay
//! proportional to the number of *domains*, never to the number of commands.
//! That is what makes the whole stack fit behind one executable.

use std::process::Command;

use serde_json::Value;

mod common;

fn ds(args: &[&str]) -> (String, String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

fn bytes(args: &[&str]) -> usize {
    ds(args).0.len()
}

fn json(args: &[&str]) -> Value {
    let (stdout, _, _) = ds(args);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("`ds {}` is not JSON ({error}): {stdout}", args.join(" ")))
}

fn assert_within(label: &str, args: &[&str], ceiling: usize) {
    let size = bytes(args);
    assert!(
        size <= ceiling,
        "{label} is {size} bytes, over its {ceiling}-byte budget.\n\
         This is the interface getting more expensive for every caller.\n\
         Either move the new text down a tier, or raise the budget \
         deliberately in this test with a reason."
    );
}

/// Every registered command, walked from the live surface rather than a list
/// kept by hand. A hardcoded list silently stops covering the commands added
/// after it was written — which is exactly when a budget matters most.
fn every_command() -> Vec<Value> {
    let index = json(&["capabilities", "--output", "json"]);

    let mut ids: Vec<String> = Vec::new();
    for domain in index["data"]["domains"].as_array().expect("domains") {
        let id = domain["id"].as_str().expect("domain id");
        let commands = json(&["capabilities", id, "--output", "json"]);
        for command in commands["data"]["commands"].as_array().expect("commands") {
            ids.push(command["id"].as_str().expect("id").to_string());
        }
    }
    for meta in ["capabilities", "doctor", "version"] {
        ids.push(meta.to_string());
    }

    ids.iter()
        .map(|id| json(&["capabilities", id, "--output", "json"])["data"]["command"].clone())
        .collect()
}

/// A command's allowance, scaled to what it declares.
fn command_help_ceiling(command: &Value) -> usize {
    const FRAME: usize = 1_200;
    const PER_INPUT: usize = 180;
    const PER_REFUSAL: usize = 220;

    let inputs = command["inputs"].as_array().map_or(0, Vec::len);
    let refusals = command["refusals"].as_array().map_or(0, Vec::len);
    FRAME + PER_INPUT * inputs + PER_REFUSAL * refusals
}

#[test]
fn root_help_is_cheap() {
    // Root help is the only screen that answers no specific question, and
    // every agent reads it. It is the most expensive text in the product.
    //
    // Raised from 1,400 when the `work` domain landed and from 1,520 when the
    // `survey` domain landed: a domain costs root help one line forever, which
    // is the trade `root_help_scales_with_domains_not_commands` exists to
    // price. The flat ceiling stays at that scaled one (800 + 80 × 10 domains)
    // so it still catches prose creeping into the frame.
    assert_within("root help", &["--help"], 1_600);
}

#[test]
fn root_help_scales_with_domains_not_commands() {
    // The invariant that lets one executable hold the whole stack.
    //
    // Root help is allowed a fixed frame plus one line per domain. It may not
    // grow when a domain gains a command, and it may not grow faster than one
    // line per domain. If this fails, someone has started listing commands at
    // the root — the exact failure this CLI exists to avoid.
    let index = json(&["capabilities", "--output", "json"]);
    let domains = index["data"]["domains"]
        .as_array()
        .expect("domain list")
        .len();

    const FRAME: usize = 800;
    const PER_DOMAIN: usize = 80;
    let ceiling = FRAME + PER_DOMAIN * domains;
    let size = bytes(&["--help"]);
    assert!(
        size <= ceiling,
        "root help is {size} bytes for {domains} domain(s); the budget is \
         {FRAME} + {PER_DOMAIN}/domain = {ceiling}.\n\
         Root help must list domains, never commands."
    );
}

#[test]
fn root_help_names_no_command() {
    // Tier separation, asserted directly rather than inferred from size.
    //
    // The check is on the command's full invocation path, not its leaf. A
    // leaf-word check reads as stricter and is in fact useless: "run" and
    // "inspect" are ordinary English verbs, and a domain summary that says
    // "run them offline" would fail a test that is supposed to be about
    // *listing commands*. What must not appear at the root is a caller's next
    // invocation — `solar run`, `dsgrid inspect` — because that is what
    // listing a command actually looks like.
    let (root, _, _) = ds(&["--help"]);
    for command in every_command() {
        let id = command["id"].as_str().expect("id");
        // The meta commands are root-level by design and are named there.
        if !id.contains('.') {
            continue;
        }
        let path: Vec<&str> = command["path"]
            .as_array()
            .expect("path")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let invocation = path.join(" ");
        assert!(
            !root.contains(&invocation),
            "root help names the command `ds {invocation}`. Root help lists \
             domains and how to drill down — nothing below that tier."
        );
        assert!(
            !root.contains(id),
            "root help names the command id `{id}`. Command ids belong to \
             `ds capabilities`, not to the root screen."
        );
    }
}

#[test]
fn domain_help_scales_with_its_own_commands() {
    let index = json(&["capabilities", "--output", "json"]);
    for domain in index["data"]["domains"].as_array().expect("domains") {
        let id = domain["id"].as_str().expect("domain id");
        let commands = domain["commands"].as_u64().unwrap_or(0) as usize;
        assert_within(
            &format!("`ds {id} --help`"),
            &[id, "--help"],
            260 + 140 * commands,
        );
    }
}

#[test]
fn domain_help_carries_no_command_prose() {
    // Domain help is an index: one line per command. The paragraph belongs to
    // the command's own help, one tier down.
    let index = json(&["capabilities", "--output", "json"]);
    for domain in index["data"]["domains"].as_array().expect("domains") {
        let id = domain["id"].as_str().expect("domain id");
        let (domain_help, _, _) = ds(&[id, "--help"]);
        let commands = json(&["capabilities", id, "--output", "json"]);

        for command in commands["data"]["commands"].as_array().expect("commands") {
            let descriptor = json(&[
                "capabilities",
                command["id"].as_str().expect("id"),
                "--output",
                "json",
            ]);
            let purpose = descriptor["data"]["command"]["purpose"]
                .as_str()
                .expect("purpose");
            let opening: String = purpose
                .split_whitespace()
                .take(6)
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                !domain_help.contains(&opening),
                "`ds {id} --help` contains a command's purpose prose; it should \
                 carry only the one-line summary."
            );
        }
    }
}

#[test]
fn command_help_is_bounded() {
    for command in every_command() {
        let path: Vec<String> = command["path"]
            .as_array()
            .expect("path")
            .iter()
            .map(|part| part.as_str().expect("part").to_string())
            .collect();
        let mut args: Vec<&str> = path.iter().map(String::as_str).collect();
        args.push("--help");

        let ceiling = command_help_ceiling(&command);
        let size = bytes(&args);
        assert!(
            size <= ceiling,
            "`ds {} --help` is {size} bytes against a {ceiling}-byte budget \
             ({} inputs, {} refusals).\n\
             The allowance already scales with what the command declares, so \
             this is prose. Move it to the command's reference document and \
             cross-link.",
            path.join(" "),
            command["inputs"].as_array().map_or(0, Vec::len),
            command["refusals"].as_array().map_or(0, Vec::len),
        );
    }
}

#[test]
fn command_descriptors_are_bounded() {
    // The machine tier carries the same facts with less framing, so it gets
    // the same allowance plus a little for JSON's own punctuation.
    for command in every_command() {
        let id = command["id"].as_str().expect("id");
        let ceiling = command_help_ceiling(&command) + 600;
        let size = bytes(&["capabilities", id, "--output", "json"]);
        assert!(
            size <= ceiling,
            "the descriptor for `{id}` is {size} bytes against a {ceiling}-byte budget"
        );
    }
}

#[test]
fn discovery_indexes_are_cheap_in_json() {
    let index = json(&["capabilities", "--output", "json"]);
    let domains = index["data"]["domains"].as_array().expect("domains");

    assert_within(
        "capabilities domain index",
        &["capabilities", "--output", "json"],
        400 + 140 * domains.len(),
    );

    for domain in domains {
        let id = domain["id"].as_str().expect("domain id");
        let commands = domain["commands"].as_u64().unwrap_or(0) as usize;
        assert_within(
            &format!("`ds capabilities {id}`"),
            &["capabilities", id, "--output", "json"],
            300 + 220 * commands,
        );
    }

    assert_within(
        "capabilities search",
        &[
            "capabilities",
            "--search",
            "model inspect",
            "--output",
            "json",
        ],
        1_200,
    );
}

#[test]
fn errors_are_short() {
    // A refusal has to carry a code, a remedy and a next step — and stop
    // there. An agent that has to read a paragraph to learn a file was
    // missing has lost more context than the failure was worth.
    let (stdout, _, _) = ds(&[
        "dsgrid",
        "inspect",
        "--model",
        "/definitely/not/here.dsgrid",
        "--output",
        "json",
    ]);
    assert!(
        stdout.len() <= 800,
        "error envelope is {} bytes, over its 800-byte budget: {stdout}",
        stdout.len()
    );
}

#[test]
fn default_results_are_bounded() {
    // The default projection of a command over a real model must stay small.
    // Everything larger is opt-in through an explicit projection.
    let model = common::fixture();
    let (stdout, stderr, code) = ds(&["dsgrid", "inspect", "--model", &model, "--output", "json"]);
    assert_eq!(code, 0, "inspect failed: {stdout}{stderr}");
    assert!(
        stdout.len() <= 1_500,
        "default inspect result is {} bytes, over its 1500-byte budget",
        stdout.len()
    );
}
