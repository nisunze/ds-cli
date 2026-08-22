//! Parity between what `ds` declares and what the engines actually accept.
//!
//! `ds report export` translates its own flags into the reporter's typed
//! request. Those field names are a hand copy of the engine's published
//! schema, and a hand copy that nobody checks drifts silently: the flag keeps
//! working, the field it writes stops being the one the engine reads, and the
//! failure surfaces as a confusing refusal months later.
//!
//! So the copy is checked, against the engine installed on this machine, at
//! the version actually installed. This is the same discipline `ds-mcp`
//! applies to its own hand-authored schemas — a contract test that invokes
//! the owner and compares field for field.
//!
//! When the engine is not present the check cannot run. It says so loudly
//! rather than passing quietly, and CI installs the engine so the proof is
//! real there.

use std::process::Command;

use serde_json::Value;

fn ds(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    (
        serde_json::from_str(&stdout).unwrap_or(Value::Null),
        output.status.code().unwrap_or(-1),
    )
}

/// Whether the reporter engine is reachable, using `ds`'s own resolution
/// rules rather than a second copy of them.
fn reporter_available() -> bool {
    let (descriptor, code) = ds(&["capabilities", "report.tasks", "--output", "json"]);
    code == 0 && descriptor["data"]["command"]["availability"] == "available"
}

fn skip(reason: &str) {
    eprintln!(
        "SKIPPED: {reason}\n  \
         This check proves `ds report export` writes the field names the \
         installed engine reads.\n  \
         Set DS_REPORT_BIN to a built `ds-report` to run it locally; CI \
         builds one so the proof is real there."
    );
}

#[test]
fn report_export_flags_cover_every_required_engine_field() {
    if !reporter_available() {
        skip("the ds-report engine is not installed on this machine");
        return;
    }

    // What `ds report export` declares it accepts.
    let (descriptor, code) = ds(&["capabilities", "report.export", "--output", "json"]);
    assert_eq!(code, 0);
    let flags: Vec<String> = descriptor["data"]["command"]["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("name").to_string())
        .collect();

    // What the engine says it requires.
    let (index, code) = ds(&["report", "tasks", "--output", "json"]);
    assert_eq!(code, 0, "the engine's task index did not load");

    for task in index["data"]["tasks"].as_array().expect("tasks") {
        let name = task["name"].as_str().expect("task name");
        if name == "export_compounded_report" {
            // `report bundle` accepts the reporter's complete typed request;
            // unlike report.export it intentionally has no convenience-field
            // translation to keep archive layout and digests one document.
            continue;
        }
        let required: Vec<&str> = task["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            !required.is_empty(),
            "task `{name}` declares no required fields; the schema did not load"
        );

        for field in required {
            // The engine's snake_case field maps to a kebab-case flag, with
            // one deliberate exception: `transformers` (plural, combined) and
            // `transformer` (singular) are both reached through the repeated
            // `--transformer` flag, because a caller should not have to know
            // which task pluralizes.
            let expected = match field {
                "transformers" => "transformer".to_string(),
                other => other.replace('_', "-"),
            };
            assert!(
                flags.contains(&expected),
                "the engine's task `{name}` requires `{field}`, but `ds report export` \
                 declares no `--{expected}`.\n\
                 Either add the flag, or — if it is only reachable through \
                 --request — say so explicitly here with the reason."
            );
        }
    }
}

#[test]
fn report_export_writes_only_fields_the_engine_declares() {
    if !reporter_available() {
        skip("the ds-report engine is not installed on this machine");
        return;
    }

    // Every optional flag `ds` offers must correspond to a property the
    // engine's schema actually has. A flag writing a field the engine ignores
    // is worse than a missing flag: it looks like it worked.
    let (descriptor, _) = ds(&["capabilities", "report.export", "--output", "json"]);
    let flags: Vec<&str> = descriptor["data"]["command"]["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .filter_map(|input| input["name"].as_str())
        .collect();

    // Flags that are `ds`'s own, not the engine's request fields.
    const DS_OWNED: &[&str] = &["task", "request", "result"];

    let mut engine_properties: Vec<String> = Vec::new();
    for task in [
        "export_transformer_report",
        "export_combined_transformer_report",
    ] {
        let (schema, code) = ds(&["report", "tasks", "--task", task, "--output", "json"]);
        assert_eq!(code, 0, "schema for `{task}` did not load");
        if let Some(properties) = schema["data"]["request_schema"]["properties"].as_object() {
            engine_properties.extend(properties.keys().cloned());
        }
    }
    assert!(!engine_properties.is_empty(), "no engine properties loaded");

    for flag in flags {
        if DS_OWNED.contains(&flag) {
            continue;
        }
        let field = flag.replace('-', "_");
        let matched = engine_properties.contains(&field)
            // `--transformer` feeds `transformer` or `transformers`.
            || engine_properties.contains(&format!("{field}s"))
            // `--format` feeds `formats`; `--admin-bounds` feeds
            // `admin_bounds_asset`.
            || engine_properties
                .iter()
                .any(|property| property.starts_with(&field));
        assert!(
            matched,
            "`ds report export` declares `--{flag}`, but no engine request \
             property corresponds to it. A flag the engine ignores looks like \
             it worked."
        );
    }
}

/// Whether the solar engine is reachable, using `ds`'s own resolution rules
/// rather than a second copy of them.
fn solar_available() -> bool {
    let (descriptor, code) = ds(&["capabilities", "solar.engine", "--output", "json"]);
    code == 0 && descriptor["data"]["command"]["availability"] == "available"
}

#[test]
fn solar_engine_flags_are_real() {
    // The headless artifact commands translate their own flags into
    // `ds-solar`'s. Those names are a hand copy, and a hand copy nobody checks
    // drifts silently: the flag keeps working, the engine stops receiving it,
    // and the failure surfaces months later as a confusing refusal. The first
    // version of `ds solar verify-weather` sent `--dataset` to an engine that
    // only accepts `--file`.
    //
    // So every flag `ds` forwards is checked against the engine's own help, at
    // the version actually installed.
    if !solar_available() {
        skip("the ds-solar engine is not installed on this machine");
        return;
    }

    // The flag names `ds` actually forwards, per subcommand. Deliberately a
    // hand list rather than derived from the Command spec: `ds`'s own flag
    // names and the engine's are allowed to differ, and this is the table that
    // records which pairs are intended.
    let forwarded: &[(&str, &[&str])] = &[
        (
            "run",
            &[
                "--prepared",
                "--out",
                "--city",
                "--concurrency",
                "--run-id",
                "--charts",
            ],
        ),
        ("verify-weather", &["--file"]),
    ];

    for (subcommand, flags) in forwarded {
        let help = engine_help(subcommand);
        for flag in *flags {
            assert!(
                help.contains(flag),
                "`ds solar` forwards `{flag}` to `ds-solar {subcommand}`, but the \
                 installed engine's help does not mention it.\n\
                 Either the engine renamed it, or the mapping was a guess.\n\
                 Engine help:\n{help}"
            );
        }
    }
}

/// One `ds-solar` subcommand's help, from the engine `ds` itself resolves.
fn engine_help(subcommand: &str) -> String {
    let binary = std::env::var("DS_SOLAR_BIN").unwrap_or_else(|_| "ds-solar".to_string());
    let output = Command::new(binary)
        .args([subcommand, "--help"])
        .output()
        .expect("the solar engine runs");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
