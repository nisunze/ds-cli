//! Surface invariants, asserted through the CLI's own discovery output.
//!
//! These tests deliberately drive `ds capabilities` rather than reaching into
//! the registry. What matters is not that the internal table is well-formed —
//! it is that the *observable* contract is, because that is all a caller ever
//! sees. Driving the public surface also means these rules keep holding if
//! the registry is ever restructured.

use std::process::Command;

use serde_json::Value;

mod common;

fn ds(args: &[&str]) -> (Value, i32) {
    common::json(args)
}

/// Every command's full descriptor, fetched the way an agent would.
fn descriptors() -> Vec<Value> {
    let (index, code) = ds(&["capabilities", "--output", "json"]);
    assert_eq!(code, 0);

    let mut ids: Vec<String> = Vec::new();
    for domain in index["data"]["domains"].as_array().expect("domains") {
        let id = domain["id"].as_str().expect("domain id");
        let (commands, code) = ds(&["capabilities", id, "--output", "json"]);
        assert_eq!(code, 0, "domain index for `{id}` failed");
        for command in commands["data"]["commands"].as_array().expect("commands") {
            ids.push(command["id"].as_str().expect("command id").to_string());
        }
    }
    // The root-level meta commands are part of the surface and answer to the
    // same rules.
    for id in common::META_COMMANDS {
        ids.push(id.to_string());
    }

    ids.iter()
        .map(|id| {
            let (value, code) = ds(&["capabilities", id, "--output", "json"]);
            assert_eq!(code, 0, "descriptor for `{id}` failed");
            value["data"]["command"].clone()
        })
        .collect()
}

const EFFECTS: &[&str] = &[
    "discovery",
    "read_only",
    "proposal",
    "local_file_write",
    "local_ui",
    "artifact_write",
    "machine_write",
    "global_write",
];
const AUTHORITIES: &[&str] = &["none", "desktop_pairing", "desktop_user", "project"];
/// The closed chapter catalog, spelled the way a caller receives it. Written
/// out rather than read from the binary on purpose: a test that asks the
/// surface what its own vocabulary is proves nothing.
const CHAPTERS: &[&str] = &[
    "catalog",
    // 2026-08-28: local data preparation. Its own chapter rather than a corner
    // of `survey` or `project`, because it needs neither: converting a file on
    // a local disk has no project and no principal, and a caller looking for
    // it is not looking for either of those.
    "data",
    "project",
    "grid-model",
    "pls-cadd",
    "survey",
    "design",
    "map-presentation",
    "vector-tiles",
    "solar",
    "reports",
    "operations",
    "workstation",
];

#[test]
fn one_line_summaries_stay_one_line() {
    // Summaries are what an index costs. A summary that grows into a
    // paragraph makes every domain listing more expensive for everyone.
    let (index, _) = ds(&["capabilities", "--output", "json"]);
    for domain in index["data"]["domains"].as_array().expect("domains") {
        let summary = domain["summary"].as_str().expect("summary");
        assert!(
            summary.len() <= 70 && !summary.contains('\n'),
            "domain `{}` summary is {} chars; index summaries stay under 70 on one line",
            domain["id"],
            summary.len()
        );
    }
    for command in descriptors() {
        let summary = command["summary"].as_str().expect("summary");
        assert!(
            summary.len() <= 70 && !summary.contains('\n'),
            "`{}` summary is {} chars; index summaries stay under 70 on one line",
            command["id"],
            summary.len()
        );
    }
}

#[test]
fn every_command_is_fully_described() {
    for command in descriptors() {
        let id = command["id"].as_str().expect("id");

        let path: Vec<&str> = command["path"]
            .as_array()
            .expect("path")
            .iter()
            .map(|part| part.as_str().expect("path part"))
            .collect();
        assert_eq!(
            id,
            path.join("."),
            "`{id}` id and invocation path disagree; the dotted id is the path"
        );

        for field in ["purpose", "output"] {
            let text = command[field].as_str().unwrap_or("");
            assert!(
                text.len() > 30,
                "`{id}` has no meaningful `{field}`. A command without one \
                 forces a caller to read source or experiment."
            );
        }

        assert!(
            EFFECTS.contains(&command["effect"].as_str().expect("effect")),
            "`{id}` declares an effect outside the closed vocabulary"
        );
        assert!(
            AUTHORITIES.contains(&command["authority"].as_str().expect("authority")),
            "`{id}` declares an authority outside the closed vocabulary"
        );

        assert!(
            command["contract"].as_u64().unwrap_or(0) >= 1,
            "`{id}` has no contract version"
        );
    }
}

#[test]
fn declared_inputs_are_documented_and_unique() {
    for command in descriptors() {
        let id = command["id"].as_str().expect("id");
        let mut seen: Vec<&str> = Vec::new();
        for input in command["inputs"].as_array().expect("inputs") {
            let name = input["name"].as_str().expect("input name");
            assert!(!seen.contains(&name), "`{id}` declares `--{name}` twice");
            seen.push(name);
            assert!(
                input["summary"].as_str().unwrap_or("").len() > 5,
                "`{id}` input `--{name}` has no summary"
            );
        }
    }
}

#[test]
fn refusals_are_named_and_actionable() {
    for command in descriptors() {
        let id = command["id"].as_str().expect("id");
        let mut seen: Vec<&str> = Vec::new();
        for refusal in command["refusals"].as_array().expect("refusals") {
            let code = refusal["code"].as_str().expect("code");
            assert!(
                !seen.contains(&code),
                "`{id}` documents the refusal code `{code}` twice"
            );
            seen.push(code);
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "`{id}` refusal code `{code}` is not stable snake_case"
            );
            assert!(
                refusal["remedy"].as_str().unwrap_or("").len() > 10,
                "`{id}` refusal `{code}` has no remedy. A named failure with \
                 no way out is a dead end."
            );
        }
    }
}

#[test]
fn runnable_examples_run_and_fail_only_as_documented() {
    // An example that stops being true is worse than no example: it is
    // documentation an agent will trust. Every example marked runnable is
    // executed verbatim here.
    //
    // The guarantee is deliberately not "always exits 0". Some examples are
    // honest about a machine-dependent world — `ds desktop status` cannot
    // succeed the same way on a laptop with the application running and on a
    // CI box without it. What must always hold is that the example is a valid
    // invocation and that any failure is one the command *documented*. That
    // closes a loop nothing else does: the refusal list stops being prose and
    // becomes a claim this suite checks by execution.
    for command in descriptors() {
        let id = command["id"].as_str().expect("id");
        let must_have_runnable = matches!(
            command["effect"].as_str(),
            Some("discovery") | Some("read_only")
        );
        let examples = command["examples"].as_array().expect("examples");
        assert!(
            !must_have_runnable || examples.iter().any(|example| example["runnable"] == true),
            "`{id}` is safe to inspect but exposes no runnable example. Add a real example or preserve the canonical `ds <path> --help` example generated by the command contract."
        );
        let documented: Vec<&str> = command["refusals"]
            .as_array()
            .expect("refusals")
            .iter()
            .map(|refusal| refusal["code"].as_str().expect("code"))
            .collect();

        for example in examples {
            if !example["runnable"].as_bool().unwrap_or(false) {
                continue;
            }
            let text = example["command"].as_str().expect("example command");
            let mut args = shell_words(text);
            assert_eq!(
                args.first().map(String::as_str),
                Some("ds"),
                "example must invoke ds"
            );
            // Ask for the machine envelope so the outcome can be inspected
            // rather than guessed at from prose.
            args.push("--output".into());
            args.push("json".into());

            let output = Command::new(env!("CARGO_BIN_EXE_ds"))
                .args(&args[1..])
                .env("NO_COLOR", "1")
                .output()
                .expect("example runs");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let envelope: Value = serde_json::from_str(&stdout).unwrap_or_else(|error| {
                panic!("`{id}` example `{text}` did not emit an envelope ({error}): {stdout}")
            });

            if envelope["status"] == "ok" {
                assert_eq!(
                    output.status.code(),
                    Some(0),
                    "`{text}` returned ok but exited non-zero"
                );
                continue;
            }

            let code = envelope["error"]["code"].as_str().unwrap_or("");
            assert!(
                documented.contains(&code),
                "`{id}` example `{text}` failed with `{code}`, which is not in \
                 its documented refusals {documented:?}. Either the example is \
                 wrong or the refusal is undocumented."
            );
        }
    }
}

#[test]
fn command_help_matches_its_descriptor() {
    // Help and the machine descriptor are rendered from one declaration.
    // This proves they cannot have drifted apart.
    for command in descriptors() {
        let path: Vec<String> = command["path"]
            .as_array()
            .expect("path")
            .iter()
            .map(|part| part.as_str().expect("part").to_string())
            .collect();
        let mut args: Vec<&str> = path.iter().map(String::as_str).collect();
        args.push("--help");

        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(&args)
            .env("NO_COLOR", "1")
            .output()
            .expect("help runs");
        let help = String::from_utf8_lossy(&output.stdout);

        assert!(
            help.contains(command["summary"].as_str().expect("summary")),
            "`ds {} --help` does not carry its own summary",
            path.join(" ")
        );
        for input in command["inputs"].as_array().expect("inputs") {
            let name = input["name"].as_str().expect("name");
            let documented = if input["kind"] == "positional" {
                input["value"]
                    .as_str()
                    .expect("positional value")
                    .to_string()
            } else {
                format!("--{name}")
            };
            assert!(
                help.contains(&documented),
                "`ds {} --help` does not document its declared input `{documented}`",
                path.join(" ")
            );
        }
        for refusal in command["refusals"].as_array().expect("refusals") {
            let code = refusal["code"].as_str().expect("code");
            assert!(
                !code.is_empty(),
                "`ds {}` declares an empty refusal code",
                path.join(" ")
            );
            assert!(
                help.contains(code),
                "`ds {} --help` does not document its refusal `{code}`",
                path.join(" ")
            );
        }
    }
}

#[test]
fn every_command_reachable_by_id_and_by_path() {
    for command in descriptors() {
        let id = command["id"].as_str().expect("id");
        let (by_id, code) = ds(&["capabilities", id, "--output", "json"]);
        assert_eq!(code, 0, "`{id}` is not reachable by id");
        assert_eq!(by_id["data"]["command"]["id"], id);

        let path: Vec<&str> = command["path"]
            .as_array()
            .expect("path")
            .iter()
            .map(|part| part.as_str().expect("path part"))
            .collect();
        let mut invocation = path.clone();
        invocation.push("--help");
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(&invocation)
            .env("NO_COLOR", "1")
            .output()
            .expect("path help runs");
        assert!(
            output.status.success(),
            "`ds {} --help` did not resolve: {}",
            path.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains(command["summary"].as_str().expect("summary")),
            "`ds {} --help` did not reach its own command",
            path.join(" ")
        );
    }
}

#[test]
fn every_command_is_in_exactly_one_chapter() {
    // Exactly one is structural: `chapter` is a single required field of the
    // declaration, so a command cannot be registered without one and cannot
    // carry two. What is left to check is that the value is in the closed set
    // and that the chapters between them still account for the whole surface
    // — no command stranded, and no chapter that has quietly emptied out and
    // would be advertised to a caller with nothing behind it.
    let descriptors = descriptors();
    let mut counts: Vec<(&str, usize)> = CHAPTERS.iter().map(|name| (*name, 0)).collect();

    for command in &descriptors {
        let id = command["id"].as_str().expect("id");
        let chapter = command["chapter"].as_str().unwrap_or_else(|| {
            panic!(
                "`{id}` declares no chapter. Every command belongs to one \
                 operator concern; a command in none of them is unreachable \
                 by concern and invisible to anything that routes by chapter."
            )
        });
        let entry = counts
            .iter_mut()
            .find(|(name, _)| *name == chapter)
            .unwrap_or_else(|| {
                panic!(
                    "`{id}` declares the chapter `{chapter}`, which is outside \
                     the closed catalog {CHAPTERS:?}. The catalog is what a \
                     caller reads before it knows what it wants, so it does \
                     not grow when a command does."
                )
            });
        entry.1 += 1;
    }

    let total: usize = counts.iter().map(|(_, count)| count).sum();
    assert_eq!(
        total,
        descriptors.len(),
        "the chapters account for {total} of {} commands",
        descriptors.len()
    );
    for (chapter, count) in &counts {
        assert!(
            *count > 0,
            "the chapter `{chapter}` has no commands. An advertised concern \
             with nothing behind it costs a caller a choice it cannot use."
        );
    }
}

#[test]
fn chapters_follow_intent_where_it_parts_from_the_domain() {
    // A chapter is an operator-intent boundary, and three places it
    // deliberately disagrees with the repository layout are worth holding.
    // Everywhere else the domain is the chapter and needs no assertion here.
    let descriptors = descriptors();
    let chapter_of = |id: &str| -> String {
        descriptors
            .iter()
            .find(|command| command["id"].as_str() == Some(id))
            .and_then(|command| command["chapter"].as_str())
            .unwrap_or_else(|| panic!("`{id}` has no descriptor with a chapter"))
            .to_string()
    };

    // 1. The `map` domain splits: staging and saving an LV design is not the
    //    concern that acquiring survey data and reviewing geometry is.
    for command in &descriptors {
        let id = command["id"].as_str().expect("id");
        let Some(rest) = id.strip_prefix("map.") else {
            continue;
        };
        let expected = if rest.starts_with("design.") {
            "design"
        } else {
            "survey"
        };
        assert_eq!(
            command["chapter"].as_str(),
            Some(expected),
            "`{id}` is in chapter {:?}; every `map.design.*` command is \
             `design` and every other `map.*` command is `survey`",
            command["chapter"]
        );
    }

    // 2. `library` joins `pls`: resolving a pinned native asset is part of one
    //    PLS-CADD delivery workflow, not a chapter of its own.
    assert_eq!(chapter_of("library.resolve-native"), "pls-cadd");
    assert_eq!(chapter_of("pls.reference-closure"), "pls-cadd");

    // 3. Styling a layer is not regenerating the tile archive under it, so
    //    presentation and tiles stay separate concerns.
    assert_ne!(chapter_of("style.read"), chapter_of("tile.generate"));
}

#[test]
fn fixture_is_present() {
    // The semantic tests bind to real domain data; fail loudly and early if
    // it has moved rather than silently skipping.
    let _ = common::fixture();
}

/// Split an example command the way a shell would, honouring double quotes.
/// Examples are authored in this repository, so this only has to handle what
/// they actually use.
fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in text.chars() {
        match character {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}
