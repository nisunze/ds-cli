use super::*;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect};
use ds_cli_contract::{Context, Format, Output};
use std::collections::BTreeSet;

const COMMANDS: &[&Command] = &[
    &list::COMMAND,
    &register::COMMAND,
    &rename::COMMAND,
    &remove::COMMAND,
];

fn parse(command: &'static Command, tokens: &[&str]) -> Inputs {
    let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
    ds_cli_contract::parse(command, &tokens).expect("declared inputs")
}

#[test]
fn a_kernel_refusal_keeps_the_kernel_s_own_code() {
    // The vocabulary is closed and shared with the browser and the
    // desktop. Renaming one here would mean an agent that planned for
    // `unknown_layer` has to learn a second word for the same fact.
    for code in [
        "malformed_descriptor",
        "duplicate_layer",
        "unknown_layer",
        "scope_mismatch",
        "unsupported_source_kind",
    ] {
        let failure = refuse(StoreError::Refused {
            code: code.into(),
            message: "why".into(),
        });
        assert_eq!(failure.code(), code);
        assert_eq!(failure.message(), "why");
    }
    // A code this build has never heard of is still a refusal, not a
    // panic and not a success.
    assert_eq!(
        refuse(StoreError::Refused {
            code: "from_a_newer_kernel".into(),
            message: "why".into(),
        })
        .code(),
        "local_layer_refused"
    );
    assert_eq!(
        refuse(StoreError::Payload("not geojson".into())).code(),
        "invalid_payload"
    );
    assert_eq!(
        refuse(StoreError::Store("unreadable".into())).code(),
        "local_layer_refused"
    );
}

#[test]
fn every_refusal_a_command_can_raise_is_one_it_declares() {
    // The per-command lists are what a caller reads before it runs
    // anything; the shared set is what any catalogue file can carry.
    for command in COMMANDS {
        let declared: BTreeSet<&str> = command
            .refusals
            .iter()
            .map(|refusal| refusal.code)
            .collect();
        for shared in STORE_REFUSALS {
            assert!(
                declared.contains(shared.code),
                "`{}` does not declare `{}`",
                command.id,
                shared.code
            );
        }
    }
    let declared =
        |command: &Command| -> BTreeSet<&str> { command.refusals.iter().map(|r| r.code).collect() };
    assert!(declared(&register::COMMAND).contains("invalid_payload"));
    assert!(declared(&remove::COMMAND).contains("confirmation_required"));
    assert!(declared(&remove::COMMAND).contains("unknown_layer"));
    assert!(declared(&rename::COMMAND).contains("unknown_layer"));
    // A listing cannot name a layer it was not given, so it does not
    // advertise a refusal a caller could never see.
    assert!(!declared(&list::COMMAND).contains("unknown_layer"));
}

#[test]
fn the_family_is_machine_local_and_needs_no_principal() {
    for command in COMMANDS {
        assert_eq!(command.authority, Authority::None, "{}", command.id);
        assert_eq!(command.chapter, Chapter::Survey, "{}", command.id);
        assert!(
            matches!((command.availability)(), Availability::Available),
            "`{}` is gated on something a local file is not",
            command.id
        );
        assert!(
            command.purpose.contains("achine-local"),
            "`{}` does not say it is machine-local",
            command.id
        );
    }
    assert_eq!(list::COMMAND.effect, Effect::ReadOnly);
    for command in [&register::COMMAND, &rename::COMMAND, &remove::COMMAND] {
        assert_eq!(command.effect, Effect::LocalFileWrite, "{}", command.id);
    }
    // The one thing a caller must not assume: a Server on another host
    // cannot read the path `--file` names.
    assert!(
        register::COMMAND
            .purpose
            .contains("a remote Server never reads a client path")
    );
}

#[test]
fn a_removal_is_refused_until_it_is_confirmed() {
    // `local_file_write` is not a gated effect class, so the gate is this
    // command's own and has to be proved here rather than assumed from the
    // declaration.
    let inputs = parse(&remove::COMMAND, &["--layer", "sketch-1"]);
    let unconfirmed = Context {
        confirmed: false,
        output: Output::resolve(Format::Json, false, true),
    };
    let failure = remove::run(&inputs, &unconfirmed).expect_err("must refuse");
    assert_eq!(failure.code(), "confirmation_required");
    assert!(!remove::COMMAND.effect.needs_confirmation());
    assert!(
        remove::COMMAND
            .examples
            .iter()
            .any(|example| example.command.contains("--yes"))
    );
}

#[test]
fn the_scope_is_a_lane_and_an_account_with_defaults_that_need_no_sign_in() {
    let default = scope(&parse(&list::COMMAND, &[])).expect("declared defaults");
    assert_eq!(default.host, Host::Native);
    assert_eq!(default.lane.as_deref(), Some("stable"));
    assert_eq!(default.uid.as_deref(), Some("local"));
    let named = scope(&parse(
        &list::COMMAND,
        &["--lane", "canary", "--account", "uid-7"],
    ))
    .expect("named scope");
    assert_eq!(named.lane.as_deref(), Some("canary"));
    assert_eq!(named.uid.as_deref(), Some("uid-7"));
}

#[test]
fn a_listed_row_carries_the_origin_and_never_a_capability() {
    // The kernel already bounds what a row says; this proves the CLI does
    // not widen it on the way out.
    let projected = row(&json!({
        "id": "sketch-1",
        "name": "Access road",
        "sourceName": "roads.zip",
        "sourceLayerName": "Access road",
        "sourceKind": "import",
        "geometryType": "LineString",
        "featureCount": 12,
        "visible": true,
        "createdAt": 1_758_000_000_000i64,
        "origin": {"kind": "import_file", "label": "roads.zip → Access road"},
    }));
    let keys: BTreeSet<&str> = projected
        .as_object()
        .expect("a row is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "layer",
            "name",
            "source_name",
            "source_layer_name",
            "source_kind",
            "geometry_type",
            "feature_count",
            "visible",
            "created_at",
            "origin"
        ])
    );
    assert_eq!(projected["origin"]["label"], "roads.zip → Access road");
    let text = serde_json::to_string(&projected).unwrap();
    for secret in ["handleKey", "handle_key", "color", "schema", "styleDoc"] {
        assert!(!text.contains(secret), "a listing leaked {secret}");
    }
}

#[test]
fn the_geometry_choices_are_the_ones_the_store_can_read() {
    let geometry = register::COMMAND
        .arg("geometry")
        .expect("a declared geometry");
    assert!(geometry.required);
    for word in geometry.choices {
        assert!(
            ds_layer_store::prepared::geometry_type(word).is_some(),
            "`{word}` is offered but the store cannot read it"
        );
    }
    assert_eq!(geometry.choices.len(), 3);
}
