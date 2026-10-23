//! Fixed application requests against an installed Core's descriptor: the
//! declared inputs and choices decide, never a version number.
use super::*;
use serde_json::json;

fn status(choices: &[&str], extra: Value) -> Value {
    let mut descriptor = json!({"id":"design.status","contract":1,"availability":"available",
        "requires":"server","effect":"local_auth_state",
        "inputs":[{"name":"project","kind":"value","required":true},
                  {"name":"lane","kind":"value","required":false,"default":"stable"},
                  {"name":"read","kind":"value","required":false,"choices":choices},
                  {"name":"out","kind":"value","required":false}]});
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            descriptor[key] = value.clone();
        }
    }
    descriptor
}

fn config(document: bool) -> Value {
    let mut inputs = vec![
        json!({"name":"project","kind":"value","required":true}),
        json!({"name":"lane","kind":"value","required":false,"default":"stable"}),
        json!({"name":"sheet","kind":"value","required":true}),
        json!({"name":"out","kind":"value","required":false}),
    ];
    if document {
        inputs[2]["required"] = json!(false);
        inputs.push(json!({"name":"document","kind":"switch","required":false}));
        inputs.push(json!({"name":"read","kind":"value","required":false,
            "choices":["held","refresh","acquire"],"default":"acquire"}));
    }
    json!({"id":"design.config.read","contract":1,"availability":"available","requires":"server","inputs":inputs})
}

#[test]
fn an_older_core_without_a_mode_or_input_answers_core_update_required() {
    let older = status(&["held", "refresh"], json!({}));
    let refusal = admit_request("design.status", &older, &json!({"read":"acquire"}), "p").unwrap();
    assert_eq!(refusal.code, CORE_UPDATE_REQUIRED);
    assert!(
        refusal.message.contains("--read acquire"),
        "{}",
        refusal.message
    );
    assert!(refusal.remedy.contains("Update DS Core"));
    assert!(!refusal.retryable);
    let older = config(false);
    let refusal = admit_request(
        "design.config.read",
        &older,
        &json!({"document":true,"read":"held"}),
        "p",
    )
    .unwrap();
    assert_eq!(refusal.code, CORE_UPDATE_REQUIRED);
    assert!(
        refusal.message.contains("--document"),
        "{}",
        refusal.message
    );
}

#[test]
fn a_core_declaring_the_inputs_admits_the_request_whatever_its_numbers() {
    for contract in [1, 2, 9] {
        let newer = status(
            &["held", "refresh", "acquire"],
            json!({"contract": contract, "future_descriptor_field": {"x": 1}}),
        );
        for read in ["held", "refresh", "acquire"] {
            assert_eq!(
                admit_request("design.status", &newer, &json!({"read":read}), "p"),
                None,
                "{contract} {read}"
            );
        }
    }
    let mut newer = config(true);
    newer["inputs"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"future-input","kind":"switch","required":false}));
    for read in ["held", "refresh", "acquire"] {
        assert_eq!(
            admit_request(
                "design.config.read",
                &newer,
                &json!({"document":true,"read":read}),
                "p"
            ),
            None
        );
    }
}

#[test]
fn a_skewed_descriptor_and_invalid_values_keep_their_existing_refusals() {
    let mut other = status(&["held", "refresh", "acquire"], json!({}));
    other["id"] = json!("design.other");
    assert_eq!(
        admit_request("design.status", &other, &json!({"read":"acquire"}), "p")
            .unwrap()
            .code,
        "engine_incompatible"
    );
    let newer = config(true);
    assert_eq!(
        admit_request(
            "design.config.read",
            &newer,
            &json!({"document":"yes","read":"held"}),
            "p"
        )
        .unwrap()
        .code,
        "inputs_invalid"
    );
    assert_eq!(
        admit_request("unknown.command", &newer, &json!({}), "p")
            .unwrap()
            .code,
        super::super::CODE_UNKNOWN
    );
}
