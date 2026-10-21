//! What `ds install` promises in its descriptors: the words that find it and
//! the refusals it can actually keep.

use ds_cli_installs::{list, policy, retire, show};

/// The four refusal codes this domain used to declare and could never emit.
/// The route reaches the inventory through the shared native client, which
/// reports a refused user, a rejected request and a malformed answer under
/// its own codes; a declaration that spelt them install-shaped was a promise
/// nothing kept.
const NEVER_EMITTED: &[&str] = &[
    "install_not_permitted",
    "install_invalid_selection",
    "install_revision_conflict",
    "unreadable_response",
];

fn codes(command: &ds_cli_contract::spec::Command) -> Vec<&'static str> {
    command
        .refusals
        .iter()
        .map(|refusal| refusal.code)
        .collect()
}

#[test]
fn the_enforcement_command_answers_to_the_words_an_operator_types() {
    // "licence" and "block" are in the summary and match from there; the
    // rule forbids repeating them. These are the words that were NOT there.
    for word in ["ban", "lock out", "disable", "machine", "deactivate"] {
        assert!(
            policy::COMMAND.search.contains(&word),
            "`install.policy` cannot be found by `{word}`: {:?}",
            policy::COMMAND.search
        );
    }
    for command in [&list::COMMAND, &show::COMMAND] {
        assert!(
            command.search.contains(&"licence"),
            "`{}` cannot be found by the British spelling: {:?}",
            command.id,
            command.search
        );
    }
}

#[test]
fn no_install_command_declares_a_refusal_the_route_cannot_emit() {
    for command in [
        &list::COMMAND,
        &show::COMMAND,
        &policy::COMMAND,
        &retire::COMMAND,
    ] {
        let declared = codes(command);
        for never in NEVER_EMITTED {
            assert!(
                !declared.contains(never),
                "`{}` declares `{never}`, which nothing on its route emits",
                command.id
            );
        }
        // What the route DOES emit for a refused user and a rejected request.
        assert!(
            declared.contains(&"auth_rejected"),
            "{}: {declared:?}",
            command.id
        );
        assert!(
            declared.contains(&"auth_input_invalid"),
            "{}: {declared:?}",
            command.id
        );
        // Every one of them parses a number.
        assert!(
            declared.contains(&"invalid_number"),
            "{}: {declared:?}",
            command.id
        );
    }
}

#[test]
fn the_commands_that_name_one_install_declare_that_it_may_not_exist() {
    for command in [&show::COMMAND, &policy::COMMAND, &retire::COMMAND] {
        let not_found = command
            .refusals
            .iter()
            .find(|refusal| refusal.code == "install_not_found")
            .unwrap_or_else(|| panic!("`{}` never says an install can be absent", command.id));
        assert_eq!(not_found.when, "no registered install has this id");
        assert_eq!(not_found.remedy, "run ds install list");
    }
    assert!(
        !codes(&list::COMMAND).contains(&"install_not_found"),
        "a list names no install, so none can be not found"
    );
}
