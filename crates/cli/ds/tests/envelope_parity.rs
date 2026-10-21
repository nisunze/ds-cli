//! The DS envelope v1, replayed through ds-cli's own definition.
//!
//! The kernel states the envelope in `ds_command_kernel::envelope` and records
//! its corpus, `tests/fixtures/ds-envelope-cases.json`: per case a command id,
//! a contract version and either the `data` of a success or the `refusal` of
//! an error, with the exact envelope both externals must emit — `ds` on
//! stdout under `--output json` and the desktop shell to the webview. This
//! test replays that corpus through `ds_cli_contract::{success_envelope,
//! error_envelope}` and compares bytes, so the desktop door's envelope is the
//! CLI's envelope until ds-cli-contract re-exports the kernel's.
//!
//! The corpus is read from the sibling kernel checkout by relative path, the
//! way `bridge_parity.rs` reads the desktop's sources: this workspace links
//! the kernel by path and `pins/ds-command-kernel.rev` names the tip, so a
//! missing corpus is a broken checkout, not a reason to skip.
//!
//! Byte-for-byte: the recorded envelope is compared as the file spells it,
//! only its whitespace removed, so member order is proven without depending
//! on whether this build's `serde_json` preserves map order.

use std::path::PathBuf;

use ds_cli_contract::outcome::{ExitClass, Failure, error_envelope, success_envelope};
use ds_command_kernel::envelope::Class;
use serde_json::Value;

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../ds-command-kernel/tests/fixtures/ds-envelope-cases.json")
}

fn corpus_text() -> String {
    let path = corpus_path();
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "the kernel's envelope corpus is not beside this workspace ({}): {error}",
            path.display()
        )
    })
}

/// The exit classes in the order `ds` numbers them, paired with the kernel's.
const EXIT_CLASSES: [ExitClass; 6] = [
    ExitClass::Internal,
    ExitClass::InvalidInput,
    ExitClass::Unavailable,
    ExitClass::Unauthorized,
    ExitClass::Conflict,
    ExitClass::Failed,
];

fn exit_class(token: &str) -> ExitClass {
    EXIT_CLASSES
        .into_iter()
        .find(|class| class.token() == token)
        .unwrap_or_else(|| panic!("{token} is not an exit class"))
}

/// The recorded refusal as ds-cli's `Failure`, field for field.
fn failure(refusal: &Value) -> Failure {
    let class = exit_class(
        refusal["class"]
            .as_str()
            .expect("a refusal names its class"),
    );
    let mut failure = Failure::new(
        class,
        refusal["code"].as_str().expect("a refusal names its code"),
        refusal["message"]
            .as_str()
            .expect("a refusal carries a message"),
    );
    if let Some(remedy) = refusal["remedy"].as_str() {
        failure = failure.remedy(remedy);
    }
    for command in refusal["next"].as_array().into_iter().flatten() {
        failure = failure.next(command.as_str().expect("a next command is text"));
    }
    if let Some(detail) = refusal.get("detail") {
        failure = failure.detail(detail.clone());
    }
    failure
}

/// The envelope ds-cli prints for one case, compact.
fn answer(case: &Value) -> String {
    let id = case["id"].as_str().expect("case id");
    let request = &case["request"];
    let command = request["command"]
        .as_str()
        .expect("a case names its command");
    let contract = u32::try_from(request["contract"].as_u64().expect("a contract version"))
        .expect("a contract version fits u32");
    match request["kind"].as_str() {
        Some("success") => serde_json::to_string(&success_envelope(
            command,
            contract,
            request["data"].clone(),
        ))
        .expect("encodes"),
        Some("error") => {
            let failure = failure(&request["refusal"]);
            serde_json::to_string(&error_envelope(command, contract, &failure)).expect("encodes")
        }
        other => panic!("{id}: {other:?} is not a case kind"),
    }
}

/// `text` with every whitespace character outside a JSON string removed:
/// the compact form of the document exactly as the file spells it.
fn compact_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for character in text.chars() {
        if in_string {
            out.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
        } else if character == '"' {
            in_string = true;
            out.push(character);
        } else if !character.is_whitespace() {
            out.push(character);
        }
    }
    out
}

/// The text of the JSON object at the start of `body`, balanced and
/// string-aware.
fn leading_object<'a>(body: &'a str, id: &str) -> &'a str {
    assert!(body.starts_with('{'), "{id}: not an object");
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in body.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &body[..=index];
                }
            }
            _ => {}
        }
    }
    panic!("{id}: the object is unterminated")
}

/// The text of the `expected` object recorded for the case named `id`, as
/// the file spells it: the member after the case's `request`, so a `detail`
/// that itself carries an `expected` key is not mistaken for it.
fn recorded_expected(corpus: &str, id: &str) -> String {
    let needle = format!("\"id\": \"{id}\"");
    let at = corpus
        .find(&needle)
        .unwrap_or_else(|| panic!("{id} is not in the corpus"));
    let rest = &corpus[at..];
    let request = rest
        .find("\"request\":")
        .unwrap_or_else(|| panic!("{id} records no request"))
        + "\"request\":".len();
    let request = leading_object(rest[request..].trim_start(), id);
    let after_request = &rest[rest.find(request).expect("request text") + request.len()..];
    let start = after_request
        .find("\"expected\":")
        .unwrap_or_else(|| panic!("{id} records no expected envelope"))
        + "\"expected\":".len();
    leading_object(after_request[start..].trim_start(), id).to_string()
}

#[test]
fn ds_cli_answers_every_recorded_case_byte_for_byte() {
    let text = corpus_text();
    let corpus: Value = serde_json::from_str(&text).expect("cases parse");
    assert_eq!(corpus["schema"], "ds.envelope-cases/v1");
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 21, "the corpus shrank to {}", cases.len());
    let mut drift = Vec::new();
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let recorded = compact_text(&recorded_expected(&text, id));
        let actual = answer(case);
        // The structure first, for a readable failure; then the bytes.
        assert_eq!(
            serde_json::from_str::<Value>(&actual).expect("ds-cli emits JSON"),
            case["expected"],
            "{id}: ds-cli answers another envelope"
        );
        if recorded != actual {
            drift.push(format!(
                "{id}:\n  recorded: {recorded}\n  ds-cli:   {actual}"
            ));
        }
    }
    assert!(
        drift.is_empty(),
        "ds-cli's envelope drifted from the kernel's corpus in {} case(s):\n{}",
        drift.len(),
        drift.join("\n")
    );
}

#[test]
fn the_corpus_covers_every_class_both_ways() {
    let corpus: Value = serde_json::from_str(&corpus_text()).expect("cases parse");
    let mut successes = 0;
    let mut classes = std::collections::BTreeSet::new();
    for case in corpus["cases"].as_array().expect("cases") {
        match case["request"]["kind"].as_str() {
            Some("success") => successes += 1,
            Some("error") => {
                classes.insert(
                    case["request"]["refusal"]["class"]
                        .as_str()
                        .expect("class")
                        .to_string(),
                );
            }
            other => panic!("{other:?} is not a case kind"),
        }
    }
    assert!(successes > 0, "no success case");
    for class in Class::ALL {
        assert!(
            classes.contains(class.token()),
            "no error case records class {}",
            class.token()
        );
    }
}

/// The kernel's `Class` and ds-cli's `ExitClass` are one vocabulary: the same
/// tokens in the same exit-code order, retryable on the same classes.
#[test]
fn the_kernel_class_is_the_exit_class() {
    assert_eq!(Class::ALL.len(), EXIT_CLASSES.len());
    for (position, (kernel, exit)) in Class::ALL.into_iter().zip(EXIT_CLASSES).enumerate() {
        assert_eq!(kernel.token(), exit.token());
        assert_eq!(kernel.retryable(), exit.retryable(), "{}", kernel.token());
        assert_eq!(
            usize::from(exit.code()),
            position + 1,
            "{} is not numbered in the kernel's order",
            exit.token()
        );
        assert_eq!(Class::from_token(exit.token()), Some(kernel));
    }
    assert_eq!(ExitClass::Success.token(), "ok");
    assert_eq!(
        Class::from_token("ok"),
        None,
        "success is not a refusal class"
    );
}
