use super::*;
fn parse(command: &Command, args: &[&str]) -> Inputs {
    ds_cli_contract::parse(
        command,
        &args.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>(),
    )
    .unwrap()
}
#[test]
fn send_and_reply_translate_identically_and_keep_keys() {
    let args = [
        "--conversation",
        "c",
        "--text",
        "Ready",
        "--key",
        "retry-1",
        "--upload",
        "u1",
        "--upload",
        "u2",
    ];
    let send = request("send", &parse(&SEND, &args)).unwrap();
    let reply = request("reply", &parse(&REPLY, &args)).unwrap();
    assert_eq!(send, reply);
    assert_eq!(send.payload()["client_message_id"], "retry-1");
    assert_eq!(send.payload()["upload_ids"], json!(["u1", "u2"]));
}
#[test]
fn every_command_has_a_real_owner_request() {
    let samples = [
        (&CONFIG, vec![]),
        (&PEOPLE, vec!["--query", "ali"]),
        (&CONVERSATIONS, vec!["--project", "project-a"]),
        (&PROJECT, vec!["--project", "project-a"]),
        (&DIRECT, vec!["--uid", "peer"]),
        (&PERSONAL, vec![]),
        (&READ, vec!["--conversation", "c"]),
        (
            &SEND,
            vec!["--conversation", "c", "--text", "hi", "--key", "k"],
        ),
        (
            &REPLY,
            vec!["--conversation", "c", "--text", "answer", "--key", "r"],
        ),
        (
            &MARK_READ,
            vec!["--conversation", "c", "--read-cursor", "opaque"],
        ),
        (&NOTIFICATIONS, vec![]),
        (&ACK, vec!["--id", "n1", "--id", "n2"]),
    ];
    for (command, args) in samples {
        let call = request(command.path[1], &parse(command, &args)).unwrap();
        assert!(call.payload()["action"].is_string(), "{}", command.id);
        assert!(
            !call
                .payload()
                .as_object()
                .unwrap()
                .contains_key("sender_uid")
        );
    }
}
#[test]
fn read_cursor_modes_are_exclusive_and_never_reencoded() {
    let args = ["--conversation", "c", "--since", "opaque"];
    assert_eq!(
        request("read", &parse(&READ, &args)).unwrap().payload()["since_cursor"],
        "opaque"
    );
    let args = [
        "--conversation",
        "c",
        "--since",
        "opaque",
        "--cursor",
        "older",
    ];
    assert_eq!(
        request("read", &parse(&READ, &args)).unwrap_err().code(),
        "messaging_cursor_conflict"
    );
}
#[test]
fn acknowledgement_addresses_only_ids_and_does_not_dismiss() {
    assert_eq!(
        request("acknowledge", &parse(&ACK, &["--id", "n1", "--id", "n2"]))
            .unwrap()
            .payload(),
        json!({"action":"mark_read","ids":["n1","n2"]})
    );
}
#[test]
fn human_tables_bound_text_and_make_missing_pages_visible() {
    let table = render(&json!({"result":{"messages":[{"id":"m","text":"a\nb"}]},"more":true}));
    assert!(table.contains("a b"));
    assert!(table.contains("More rows"));
    assert!(render(&json!({"result":{"messages":[]}})).contains("(empty)"));
}
#[test]
fn producer_configuration_is_messages_v1_and_server_bounded() {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../../../../ds-brain/docs/contracts/messaging-wire/messages-v1/config.json"
    ))
    .unwrap();
    assert_eq!(fixture["data"]["wire_contract"], "messages-v1");
    assert_eq!(fixture["data"]["default_page_size"], 40);
    assert_eq!(fixture["data"]["max_page_size"], 50);
}
#[test]
fn installation_controls_use_only_redacted_identity_and_known_epoch() {
    assert_eq!(
        request("installations", &parse(&INSTALLATIONS, &[]))
            .unwrap()
            .payload(),
        json!({"action":"list_installations"})
    );
    let hash = "a".repeat(64);
    let epoch = "b".repeat(32);
    let input = parse(
        &REVOKE_INSTALLATION,
        &["--installation-hash", &hash, "--epoch", &epoch],
    );
    let request = request("installation-revoke", &input).unwrap();
    request.validate().unwrap();
    assert_eq!(
        request.payload(),
        json!({"action":"revoke_installation", "installation_hash":hash, "epoch":epoch})
    );
    assert_eq!(INSTALLATIONS.effect, Effect::ReadOnly);
    assert_eq!(REVOKE_INSTALLATION.effect, Effect::GlobalWrite);
    assert!(
        DOMAIN
            .commands
            .iter()
            .any(|c| c.id == REVOKE_INSTALLATION.id)
    );
    assert!(
        ds_cli_contract::parse(&REVOKE_INSTALLATION, &["--installation-hash".into(), hash])
            .is_err()
    );
}
#[test]
fn installation_human_view_does_not_claim_delivery_or_complete_inventory() {
    let shown = render(
        &json!({"result":{"installations":[{"installation_hash":"h", "platform":"legacy", "user_agent":"a\nb", "refreshed_at":"date"}], "completion":"unknown", "delivery_state":"not_proven"}}),
    );
    assert!(shown.contains("a b"));
    assert!(shown.contains("completeness unknown"));
    assert!(shown.contains("delivery is not proven"));
    assert!(!shown.contains("epoch"));
    assert!(render(&json!({"result":{"revoked":false,"installation_hash":"h"}})).contains("false"));
}
#[test]
fn malformed_revoke_refuses_before_native_profile_or_credentials() {
    let request = Request::RevokeInstallation {
        installation_hash: "a".repeat(64),
        epoch: "missing".into(),
    };
    assert_eq!(
        ds_cli_auth::messaging::execute("not-a-lane", &request)
            .unwrap_err()
            .code(),
        "messaging_request_invalid"
    );
}
