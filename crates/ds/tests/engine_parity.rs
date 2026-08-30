//! Parity between what `ds` declares and what the engines actually accept.
//!
//! `ds report export` translates its own flags into the reporter's typed
//! request. Those field names are a hand copy of the engine's published
//! schema, and a hand copy that nobody checks drifts silently: the flag keeps
//! working, the field it writes stops being the one the engine reads, and the
//! failure surfaces as a confusing refusal months later.
//!
//! So the copy is checked, against the engine installed on this machine, at
//! the version actually installed: a contract test that invokes the owner and
//! compares field for field. `bridge_parity.rs` applies the same discipline to
//! the paired application, against its checked-out source rather than a
//! running binary.
//!
//! When the engine is not present the check cannot run. It says so loudly
//! rather than passing quietly: every skip names the binary, the three places
//! the lookup looked, and the override that fixes it. CI builds `ds-report`
//! from the `ds-network-reporter` checkout and `ds-solar` from the `ds-solar`
//! checkout, points `DS_REPORT_BIN` / `DS_SOLAR_BIN` at them, re-runs this
//! suite with `--nocapture` and fails the job if `SKIPPED` appears — so a skip
//! is a local condition and never a green CI run.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The reporter engine: the binary name `ds` looks for, and its override.
const REPORT: (&str, &str) = ("ds-report", "DS_REPORT_BIN");
/// The solar engine, likewise.
const SOLAR: (&str, &str) = ("ds-solar", "DS_SOLAR_BIN");

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

/// Where an engine binary is, resolved the way `ds` itself resolves it.
///
/// A hand mirror of `ds_cli_exec::External::locate` — override, then a sibling
/// of the running `ds`, then `PATH` — because that function reads
/// `current_exe()`, which inside a test binary is `target/debug/deps/…` rather
/// than the `ds` under test. So the sibling step anchors on `CARGO_BIN_EXE_ds`
/// instead, which is the executable this suite actually spawns.
///
/// One lookup, used both to decide whether a check can run and to invoke the
/// engine. There were two, and they disagreed: the availability check asked
/// `ds`, while `engine_help` knew only override and `PATH`, so a machine
/// carrying nothing but a packaged sibling engine panicked instead of running
/// the check.
fn engine(name: &str, env_override: &str) -> Option<PathBuf> {
    let file_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };

    if let Some(raw) = std::env::var_os(env_override) {
        // An override that does not resolve is an operator error worth
        // surfacing, not a reason to fall through to a different binary.
        let path = PathBuf::from(raw);
        return path.is_file().then_some(path);
    }

    let sibling = Path::new(env!("CARGO_BIN_EXE_ds")).with_file_name(&file_name);
    if sibling.is_file() {
        return Some(sibling);
    }

    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&file_name))
        .find(|candidate| candidate.is_file())
}

/// Whether an engine is reachable.
///
/// `ds`'s own descriptor is the authority on availability rather than a second
/// copy of the rule, and the resolved path is asserted to agree with it: an
/// engine `ds` can see but this suite cannot invoke — or the reverse — is
/// exactly the drift that made the two lookups diverge before.
fn available(command: &str, (name, env_override): (&str, &str)) -> bool {
    let (descriptor, code) = ds(&["capabilities", command, "--output", "json"]);
    let declared = code == 0 && descriptor["data"]["command"]["availability"] == "available";
    let resolved = engine(name, env_override);
    assert_eq!(
        declared,
        resolved.is_some(),
        "`ds` reports `{command}` as available={declared}, but this suite resolved \
         `{name}` as {resolved:?}. The engine lookups have drifted; fix this \
         helper against ds_cli_exec::External::locate."
    );
    declared
}

fn reporter_available() -> bool {
    available("report.tasks", REPORT)
}

/// A skip that names the binary, every place the lookup looked, and the
/// remedy — in the `SKIPPED:` shape the CI step greps for, so a suite that
/// proved nothing fails the job instead of reporting green.
fn skip((name, env_override): (&str, &str), proves: &str) {
    let sibling = Path::new(env!("CARGO_BIN_EXE_ds")).with_file_name(name);
    eprintln!(
        "SKIPPED: the {name} engine is not installed on this machine\n  \
         {proves}\n  \
         Looked in: ${env_override}, then {}, then PATH.\n  \
         Set {env_override} to a built `{name}` to run it locally; CI builds \
         one so the proof is real there.",
        sibling.display()
    );
}

const REPORT_PROVES: &str =
    "This check proves `ds report export` writes the field names the installed engine reads.";

#[test]
fn report_export_flags_cover_every_required_engine_field() {
    if !reporter_available() {
        skip(REPORT, REPORT_PROVES);
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
        if !matches!(
            name,
            "export_transformer_report" | "export_combined_transformer_report"
        ) {
            // This test owns only `report export`'s two fixed subcommands.
            // `export_compounded_report` belongs to `report bundle`; local
            // admin enrichment is a distinct file task and is intentionally
            // not smuggled through transformer-report convenience flags.
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
        skip(REPORT, REPORT_PROVES);
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

fn solar_available() -> bool {
    available("solar.engine", SOLAR)
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
        skip(
            SOLAR,
            "This check proves every flag `ds solar` forwards is one the installed engine accepts.",
        );
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
        ("compare", &["--left", "--right", "--json"]),
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
///
/// The same [`engine`] lookup the availability check used, so this can only
/// run against the binary `ds` would have called.
fn engine_help(subcommand: &str) -> String {
    let binary = engine(SOLAR.0, SOLAR.1)
        .expect("solar_available() resolved the engine before this was called");
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
