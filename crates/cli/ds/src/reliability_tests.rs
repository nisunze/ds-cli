use super::*;

fn tokens(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn host_context_is_explicit_and_only_the_server_owner_defaults_to_server() {
    for action in ["core.serve", "server.status", "feedback.list"] {
        assert_eq!(
            resolve_host_kind(Some("desktop"), action),
            HostKind::Desktop
        );
        assert_eq!(resolve_host_kind(Some("core"), action), HostKind::Server);
        assert_eq!(
            resolve_host_kind(Some("headless"), action),
            HostKind::Headless
        );
        let fallback = if action == "core.serve" {
            HostKind::Server
        } else {
            HostKind::Headless
        };
        assert_eq!(resolve_host_kind(None, action), fallback);
        assert_eq!(resolve_host_kind(Some("invalid"), action), fallback);
    }
}

#[test]
fn local_commands_record_without_delivery_and_the_existing_adapter_still_flushes() {
    for (id, only_records) in [
        ("solar.calculate-prepared", true),
        ("feedback.draft", true),
        ("feedback.submit", false),
        ("core.sync.read", true),
        ("core.store", true),
        ("core.sync.retry", false),
        ("mcp.serve", false),
    ] {
        let command = crate::registry::find_by_id(id).expect("registered command");
        assert_eq!(records_only(command.command), only_records, "{id}");
    }
}

#[test]
fn only_declared_flag_names_are_flags_and_everything_else_is_scrubbed() {
    let command = crate::registry::all_commands()
        .into_iter()
        .find(|command| command.id == "feedback.submit")
        .expect("feedback.submit is registered");
    let argv = tokens(&[
        "feedback",
        "submit",
        "--title=SECRET-TITLE",
        "--detail",
        "SECRET DETAIL",
        "--kind",
        "bug",
        "--not-declared",
        "--output",
        "json",
        "--",
        "--yes",
    ]);
    let (flags, values) = split_argv(&argv, Some(command));
    assert_eq!(
        flags,
        tokens(&["--title", "--detail", "--kind", "--output"])
    );
    assert!(values.contains(&"SECRET-TITLE".to_string()));
    assert!(values.contains(&"SECRET DETAIL".to_string()));
    assert!(values.contains(&"--not-declared".to_string()));
    assert!(
        values.contains(&"--yes".to_string()),
        "after `--` it is an operand"
    );
    assert!(
        !values.contains(&"bug".to_string()),
        "a declared choice is vocabulary"
    );
    assert!(
        !values.contains(&"submit".to_string()),
        "a path word is vocabulary"
    );
    assert_eq!(
        project_flag(&tokens(&["x", "--project", "kigali"])).as_deref(),
        Some("kigali")
    );
    assert_eq!(
        project_flag(&tokens(&["x", "--project=kigali"])).as_deref(),
        Some("kigali")
    );
    assert_eq!(project_flag(&tokens(&["x", "--", "--project", "p"])), None);
}

fn command(id: &str) -> &'static Command {
    crate::registry::find_by_id(id)
        .expect("registered command")
        .command
}

fn replayed(id: &str, argv: &[&str]) -> reporter::Replay {
    replay::replay(&tokens(argv), command(id), None).expect("a replay within its bound")
}

/// Contract §4: the replay is the invocation as typed. Path words, flags,
/// choices, project ids, names and bounds stay; a value the descriptor marks
/// sensitive is withheld, in every spelling, and named.
#[test]
fn a_replay_keeps_every_word_but_the_values_the_descriptor_marks_sensitive() {
    let listed = [
        "feedback", "list", "--view", "all", "--limit", "100", "--detail", "--output", "json",
    ];
    let replay = replayed("feedback.list", &listed);
    assert_eq!(replay.argv, tokens(&listed));
    assert!(replay.redacted.is_empty() && replay.elided.is_empty());
    assert_eq!(replay.command, "feedback.list");
    assert_eq!(replay.version, crate::build::VERSION);
    assert_eq!(replay.source_sha, crate::build::SOURCE_SHA);
    assert_eq!(replay.channel, "local");
    assert_eq!(replay.os, std::env::consts::OS);
    assert_eq!(replay.arch, std::env::consts::ARCH);

    let party = replayed(
        "pm.party.create",
        &[
            "pm",
            "party",
            "create",
            "--project",
            "rwanda_template",
            "--name",
            "Kigali Water",
            "--kind",
            "organisation",
            "--email",
            "SECRET.person@example.org",
            "--email=SECRET.other@example.org",
            "--phone",
            "SECRET-0788",
            "--notes",
            "SECRET note about a person",
            "--yes",
            "--output",
            "json",
        ],
    );
    assert_eq!(
        party.argv,
        tokens(&[
            "pm",
            "party",
            "create",
            "--project",
            "rwanda_template",
            "--name",
            "Kigali Water",
            "--kind",
            "organisation",
            "--email",
            "<redacted>",
            "--email=<redacted>",
            "--phone",
            "<redacted>",
            "--notes",
            "<redacted>",
            "--yes",
            "--output",
            "json",
        ])
    );
    assert_eq!(party.redacted, tokens(&["email", "phone", "notes"]));
    assert!(!party.encode().contains("SECRET"), "{}", party.encode());
}

/// A value no declared input takes could be anything, so it is withheld as
/// `<unbound>`; the refusal it earns does not depend on its text. An operand
/// a command declares is a value like any other.
#[test]
fn a_replay_withholds_values_no_input_takes_and_keeps_declared_operands() {
    let replay = replayed(
        "feedback.list",
        &[
            "feedback",
            "list",
            "--since",
            "2026-10-01",
            "stray",
            "--SHOUT",
            "--limit=5",
        ],
    );
    assert_eq!(
        replay.argv,
        tokens(&[
            "feedback",
            "list",
            "--since",
            "<unbound>",
            "<unbound>",
            "<unbound>",
            "--limit=5"
        ])
    );
    let switch = replayed("feedback.list", &["feedback", "list", "--detail=yes"]);
    assert_eq!(
        switch.argv,
        tokens(&["feedback", "list", "--detail=<unbound>"])
    );

    let selector = replayed(
        "capabilities",
        &["capabilities", "feedback.list", "--output", "json"],
    );
    assert_eq!(
        selector.argv,
        tokens(&["capabilities", "feedback.list", "--output", "json"])
    );
    let operands = replayed("capabilities", &["capabilities", "--", "--yes", "extra"]);
    assert_eq!(
        operands.argv,
        tokens(&["capabilities", "--", "--yes", "<unbound>"])
    );
}

/// The backlog stores one context value of at most 1000 characters: the
/// longest values are elided first, and only as many as the bound needs.
#[test]
fn an_overlong_replay_elides_its_longest_values_and_names_them() {
    let detail = "d".repeat(2_000);
    let replay = replayed(
        "feedback.submit",
        &[
            "feedback",
            "submit",
            "--title",
            "List crashed",
            "--detail",
            &detail,
            "--component",
            "ds-cli/feedback",
            "--agent",
            "codex",
        ],
    );
    assert_eq!(replay.elided, tokens(&["detail"]));
    assert_eq!(
        replay.argv,
        tokens(&[
            "feedback",
            "submit",
            "--title",
            "List crashed",
            "--detail",
            "<elided>",
            "--component",
            "ds-cli/feedback",
            "--agent",
            "codex",
        ])
    );
    assert!(replay.encode().chars().count() <= reporter::REPLAY_MAX_CHARS);
    let words: Vec<String> = (0..400).map(|_| "--detail".to_string()).collect();
    assert!(
        replay::replay(&words, command("feedback.submit"), None).is_none(),
        "a list that cannot meet the bound carries no replay rather than a partial one"
    );
}

/// A panic's location is a workspace path or a dependency's own path; the
/// machine's directories (a home directory names its user) never travel.
#[test]
fn a_panic_location_never_carries_the_machines_directories() {
    for (file, shown) in [
        (
            "crates/cli/ds-cli-feedback/src/list.rs",
            "crates/cli/ds-cli-feedback/src/list.rs:7",
        ),
        (
            "/home/someone/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/serde_json-1.0.140/src/de.rs",
            "serde_json-1.0.140/src/de.rs:7",
        ),
        (
            "C:\\Users\\someone\\.cargo\\registry\\src\\index.crates.io-1949cf8c6b5b557f\\time-0.3.41\\src\\date.rs",
            "time-0.3.41/src/date.rs:7",
        ),
        (
            "/rustc/90b35a6239c3d8bdabc530a6a0816f7ff89a0aaf/library/core/src/option.rs",
            "library/core/src/option.rs:7",
        ),
        (
            "/home/someone/elsewhere/ds-network/crates/x/src/y.rs",
            "crates/x/src/y.rs:7",
        ),
    ] {
        assert_eq!(replay::location_text(file, 7), shown);
    }
}

/// The gate that keeps the marking honest as commands are added: an input
/// that names an email address, a phone number, a password or a secret is
/// declared with a sensitive value shape, so its replay withholds it.
#[test]
fn every_personal_or_secret_input_is_declared_sensitive() {
    let mut unmarked = Vec::new();
    for command in crate::registry::all_commands() {
        for arg in command.args {
            let personal = ["email", "phone", "password", "secret"].contains(&arg.name)
                || arg.value.contains("email")
                || arg.value.contains("phone");
            if personal && arg.kind != ds_cli_contract::spec::ArgKind::Switch && !arg.is_sensitive()
            {
                unmarked.push(format!("{} --{} {}", command.id, arg.name, arg.value));
            }
        }
    }
    assert!(
        unmarked.is_empty(),
        "declare these with a value shape from ds_cli_contract::spec::SENSITIVE_VALUES: {unmarked:#?}"
    );
}
