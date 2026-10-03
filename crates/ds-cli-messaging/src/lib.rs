//! Conversations and notifications. Every handler calls the closed native
//! client; brain alone owns messaging rules and state.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Domain, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_client_core::messaging::Command as Request;
use serde_json::{Value, json};
const LANE: Arg = ds_cli_contract::spec::LANE;
const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<ds-project>",
    "Explicit project address; never the saved selection.",
);
const LIMIT: Arg = Arg::value(
    "limit",
    "<count>",
    "Requested page bound; brain returns the applied limit.",
)
.default("40");
const CURSOR: Arg = Arg::value(
    "cursor",
    "<cursor>",
    "Opaque next cursor from the preceding page.",
);
const SINCE: Arg = Arg::value(
    "since",
    "<read-cursor>",
    "Opaque server message cursor for catch-up; exclusive with --cursor.",
);
const CONVERSATION: Arg = Arg::value(
    "conversation",
    "<conversation-id>",
    "Conversation id from list or resolve.",
)
.required();
const TEXT: Arg = Arg::value(
    "text",
    "<text>",
    "Message content; brain enforces its configured byte limit.",
)
.required();
const KEY: Arg = Arg::value(
    "key",
    "<idempotency-key>",
    "Unique client message id; preserve it and the content across retries.",
)
.required();
const UPLOAD: Arg = Arg::repeated(
    "upload",
    "<upload-id>",
    "Finalized messaging upload id, repeated in authored order.",
);
const READ_CURSOR: Arg = Arg::value(
    "read-cursor",
    "<cursor>",
    "Exact cursor returned by a served message.",
)
.required();
const IDS: Arg = Arg::repeated(
    "id",
    "<notification-id>",
    "Caller-owned notification id; repeat for a bounded batch.",
)
.required();
const QUERY: Arg = Arg::value(
    "query",
    "<prefix>",
    "Canonical email or display-name prefix.",
)
.required();
const UID: Arg = Arg::value(
    "uid",
    "<uid>",
    "Immutable UID returned by people discovery.",
)
.required();
const OWN: &[Refusal] = &[
    Refusal {
        code: "messaging_request_invalid",
        when: "brain rejected the request or cursor",
        remedy: "read the command contract; use server ids and cursors",
    },
    Refusal {
        code: "messaging_idempotency_mismatch",
        when: "the key already names different content",
        remedy: "retry the original content or choose a new key",
    },
    Refusal {
        code: "messaging_not_permitted",
        when: "brain refused identity or membership",
        remedy: "restore the intended account or request project access",
    },
    Refusal {
        code: "messaging_not_found",
        when: "the conversation is not visible",
        remedy: "list or resolve it again",
    },
    Refusal {
        code: "messaging_conflict",
        when: "the state transition conflicted",
        remedy: "read current state and use its cursor",
    },
    Refusal {
        code: "messaging_paused",
        when: "the operator paused messaging",
        remedy: "wait for the operator to resume it",
    },
    Refusal {
        code: "messaging_not_configured",
        when: "this lane has no configured messaging",
        remedy: "use a configured lane",
    },
    Refusal {
        code: "messaging_route_unavailable",
        when: "the gateway does not publish this route",
        remedy: "install matching gateway and brain routes",
    },
    Refusal {
        code: "messaging_rate_limited",
        when: "the caller exhausted a brain rate budget",
        remedy: "wait then retry the same key",
    },
    Refusal {
        code: "messaging_response_unreadable",
        when: "brain answered outside the wire shape",
        remedy: "update ds and brain together",
    },
    Refusal {
        code: "messaging_unavailable",
        when: "brain could not serve messaging",
        remedy: "retry later with the same key",
    },
    Refusal {
        code: "messaging_cursor_conflict",
        when: "both cursor modes were supplied",
        remedy: "choose --cursor or --since",
    },
    Refusal {
        code: "invalid_integer",
        when: "limit is not an integer",
        remedy: "pass an integer page limit",
    },
];
const BASE: &[Refusal] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals;
const REFUSALS: [Refusal; BASE.len() + OWN.len()] = {
    let mut out = [OWN[0]; BASE.len() + OWN.len()];
    let mut i = 0;
    while i < BASE.len() {
        out[i] = BASE[i];
        i += 1;
    }
    let mut j = 0;
    while j < OWN.len() {
        out[i + j] = OWN[j];
        j += 1;
    }
    out
};
pub static CONFIG: Command = Command {
    id: "messaging.config",
    path: &["messaging", "config"],
    contract: 1,
    summary: "Inspect messaging availability and limits.",
    purpose: "Reads the messages-v1 configuration for the authenticated account.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[LANE],
    output: "The brain configuration, including wire_contract and server limits.",
    examples: &[Example {
        command: "ds messaging config",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PEOPLE: Command = Command {
    id: "messaging.people",
    path: &["messaging", "people"],
    contract: 1,
    summary: "Find a colleague to start a personal conversation.",
    purpose: "Searches the canonical account directory; an explicit project narrows to eligible project people.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[QUERY, PROJECT_ARG, LIMIT, LANE],
    output: "People with immutable UID, messageable, truncated and applied limit.",
    examples: &[Example {
        command: "ds messaging people --query ali",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static CONVERSATIONS: Command = Command {
    id: "messaging.conversations",
    path: &["messaging", "conversations"],
    contract: 1,
    summary: "Find unread personal and project conversations.",
    purpose: "Lists account-wide direct conversations and current project groups. An optional project includes its group; an unauthorized project is omitted by the server. The cursor pages direct conversations only.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, CURSOR, LIMIT, LANE],
    output: "Conversations with kind, unread and read_cursor; directs_truncated and next_direct_cursor.",
    examples: &[Example {
        command: "ds messaging conversations --project demo",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PROJECT: Command = Command {
    id: "messaging.project",
    path: &["messaging", "project"],
    contract: 1,
    summary: "Open the conversation of an exact project.",
    purpose: "Resolves the project conversation under current canonical membership. A project address supplies no authority.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[PROJECT_ARG.required(), LANE],
    output: "The conversation id, project, unread and caller read state.",
    examples: &[Example {
        command: "ds messaging project --project demo",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static DIRECT: Command = Command {
    id: "messaging.direct",
    path: &["messaging", "direct"],
    contract: 1,
    summary: "Open a personal conversation with a colleague.",
    purpose: "Resolves or creates the deterministic direct conversation for the caller and target UID. Project selection never scopes it.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[UID, LANE],
    output: "The conversation id, kind, peer and caller state.",
    examples: &[Example {
        command: "ds messaging direct --uid colleague --yes",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PERSONAL: Command = Command {
    id: "messaging.personal",
    path: &["messaging", "personal"],
    contract: 1,
    summary: "Open retained personal notification history.",
    purpose: "Resolves the caller-owned read-only notifications conversation. Current Project Work and personal notices are in messaging notifications; no producer currently appends to this retained history.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[LANE],
    output: "The caller-owned notifications conversation id and state.",
    examples: &[Example {
        command: "ds messaging personal",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static READ: Command = Command {
    id: "messaging.read",
    path: &["messaging", "read"],
    contract: 1,
    summary: "Read a conversation thread, older pages or since a cursor.",
    purpose: "History is newest first with --cursor for older pages. --since returns the latest bounded catch-up window oldest first; truncated means older unseen rows were skipped, so recover them through history pages. The two cursor modes are exclusive. Reading does not mark read.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[CONVERSATION, CURSOR, SINCE, LIMIT, LANE],
    output: "Messages with server read_cursor. History: has_more, next_cursor, latest_read_cursor, limit. Since: truncated, latest_read_cursor, limit.",
    examples: &[Example {
        command: "ds messaging read --conversation conversation-id --limit 20",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static SEND: Command = Command {
    id: "messaging.send",
    path: &["messaging", "send"],
    contract: 1,
    summary: "Send a message with a retry-safe idempotency key.",
    purpose: "Appends to the named conversation as the signed-in caller. Retry the same key and content to receive the original message. Brain owns eligibility, membership, attachment policy and idempotency.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[CONVERSATION, TEXT, KEY, UPLOAD, LANE],
    output: "The original message with read_cursor and created; false means the retry was absorbed.",
    examples: &[Example {
        command: "ds messaging send --conversation conversation-id --text \"Ready for review\" --key review-1 --yes",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static REPLY: Command = Command {
    id: "messaging.reply",
    path: &["messaging", "reply"],
    contract: 1,
    summary: "Reply in a conversation using an idempotency key.",
    purpose: "Appends the next message to the named thread using send_message. Brain has conversation threads, with no parent-message reply metadata or nested subthreads. Retry exactly the same content and key.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[CONVERSATION, TEXT, KEY, UPLOAD, LANE],
    output: "The message with server read_cursor and created.",
    examples: &[Example {
        command: "ds messaging reply --conversation conversation-id --text \"I will check it\" --key reply-1 --yes",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static MARK_READ: Command = Command {
    id: "messaging.mark-read",
    path: &["messaging", "mark-read"],
    contract: 1,
    summary: "Mark a conversation read through an observed message.",
    purpose: "Advances the caller read state monotonically using a cursor from a served message. It cannot mark another person read and never changes delivery acknowledgements.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[CONVERSATION, READ_CURSOR, LANE],
    output: "The persisted read_cursor and advanced flag.",
    examples: &[Example {
        command: "ds messaging mark-read --conversation conversation-id --read-cursor server-cursor --yes",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static NOTIFICATIONS: Command = Command {
    id: "messaging.notifications",
    path: &["messaging", "notifications"],
    contract: 1,
    summary: "List Project Work comment and personal notifications.",
    purpose: "Reads the caller-owned canonical notification spine, newest first, including PM comment notices and personal reminders. This is distinct from retained notification conversation history.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[CURSOR, LIMIT, LANE],
    output: "Notifications with id, kind, title, body, project_id, route and read state; has_more, next_cursor and limit.",
    examples: &[Example {
        command: "ds messaging notifications --limit 20",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static ACK: Command = Command {
    id: "messaging.acknowledge",
    path: &["messaging", "acknowledge"],
    contract: 1,
    summary: "Acknowledge observed notifications idempotently.",
    purpose: "Marks only the caller-owned notification ids read. Already-read and missing ids are reported separately. Does not dismiss notices or advance conversation cursors.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[IDS, LANE],
    output: "Requested, changed, unchanged and missing counts.",
    examples: &[Example {
        command: "ds messaging acknowledge --id notification-id --yes",
        note: "Use JSON for agents; --output human prints a short table.",
        runnable: false,
    }],
    refusals: &REFUSALS,
    reference: None,
    search: &["messaging", "chat", "thread", "notification", "inbox"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static DOMAIN: Domain = Domain {
    id: "messaging",
    summary: "Conversations, messages and personal notifications.",
    commands: &[
        &CONFIG,
        &PEOPLE,
        &CONVERSATIONS,
        &PROJECT,
        &DIRECT,
        &PERSONAL,
        &READ,
        &SEND,
        &REPLY,
        &MARK_READ,
        &NOTIFICATIONS,
        &ACK,
    ],
};
fn optional(inputs: &Inputs, key: &str) -> Option<String> {
    inputs.value(key).map(str::to_owned)
}
fn limit(inputs: &Inputs) -> Result<i64, Failure> {
    inputs.require("limit")?.parse().map_err(|_| {
        Failure::invalid("invalid_integer", "--limit must be an integer")
            .remedy("pass an integer page limit")
    })
}
/// The only translation: declared arguments to a closed owner request.
pub fn request(id: &str, inputs: &Inputs) -> Result<Request, Failure> {
    let conversation = || inputs.require("conversation").map(str::to_owned);
    Ok(match id {
        "config" => Request::Config,
        "people" => Request::People {
            query: inputs.require("query")?.into(),
            project: optional(inputs, "project"),
            limit: limit(inputs)?,
        },
        "conversations" => Request::Conversations {
            project: optional(inputs, "project"),
            cursor: optional(inputs, "cursor"),
            limit: limit(inputs)?,
        },
        "project" => Request::ResolveProject {
            project: inputs.require("project")?.into(),
        },
        "direct" => Request::ResolveDirect {
            uid: inputs.require("uid")?.into(),
        },
        "personal" => Request::ResolveNotifications,
        "read" => {
            if inputs.value("cursor").is_some() && inputs.value("since").is_some() {
                return Err(Failure::invalid(
                    "messaging_cursor_conflict",
                    "--cursor and --since are exclusive",
                )
                .remedy("choose one cursor mode"));
            }
            Request::Read {
                conversation: conversation()?,
                cursor: optional(inputs, "cursor"),
                since: optional(inputs, "since"),
                limit: limit(inputs)?,
            }
        }
        "send" | "reply" => Request::Send {
            conversation: conversation()?,
            key: inputs.require("key")?.into(),
            text: inputs.require("text")?.into(),
            uploads: inputs.repeated("upload").to_vec(),
        },
        "mark-read" => Request::MarkRead {
            conversation: conversation()?,
            cursor: inputs.require("read-cursor")?.into(),
        },
        "notifications" => Request::Notifications {
            cursor: optional(inputs, "cursor"),
            limit: limit(inputs)?,
        },
        "acknowledge" => Request::Acknowledge {
            ids: inputs.repeated("id").to_vec(),
        },
        _ => unreachable!("registered messaging command"),
    })
}
fn execute(id: &str, inputs: &Inputs) -> Result<Value, Failure> {
    let result = ds_cli_auth::messaging::execute(inputs.require("lane")?, &request(id, inputs)?)?;
    Ok(
        json!({"result":result,"more": result["has_more"] == true || result["truncated"] == true || result["directs_truncated"] == true}),
    )
}
pub fn config(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("config", inputs)
}
pub fn people(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("people", inputs)
}
pub fn conversations(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("conversations", inputs)
}
pub fn project(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("project", inputs)
}
pub fn direct(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("direct", inputs)
}
pub fn personal(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("personal", inputs)
}
pub fn read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("read", inputs)
}
pub fn send(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("send", inputs)
}
pub fn reply(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("reply", inputs)
}
pub fn mark_read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("mark-read", inputs)
}
pub fn notifications(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("notifications", inputs)
}
pub fn acknowledge(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute("acknowledge", inputs)
}
pub fn render(data: &Value) -> String {
    let result = &data["result"];
    let mut out = String::new();
    for key in ["conversations", "messages", "notifications", "people"] {
        if let Some(rows) = result[key].as_array() {
            out.push_str("ID / UID\tKIND / SENDER\tUNREAD / TEXT\n");
            for row in rows {
                let id = row["id"]
                    .as_str()
                    .or_else(|| row["uid"].as_str())
                    .unwrap_or("");
                let kind = row["kind"]
                    .as_str()
                    .or_else(|| row["sender_email"].as_str())
                    .unwrap_or("");
                let detail = row["text"]
                    .as_str()
                    .or_else(|| row["title"].as_str())
                    .or_else(|| row["display_name"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("unread={}", row["unread"]));
                let detail: String = detail
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .take(100)
                    .collect();
                out.push_str(&format!("{id}\t{kind}\t{detail}\n"));
            }
            if rows.is_empty() {
                out.push_str("(empty)\n");
            }
            if data["more"] == true {
                out.push_str(
                    "More rows exist; inspect JSON for the server cursor or truncation flag.\n",
                );
            }
            return out;
        }
    }
    if let Some(message) = result["message"].as_object() {
        return format!(
            "MESSAGE\tCREATED\n{}\t{}\n",
            message["id"], result["created"]
        );
    }
    if result["id"].is_string() {
        return format!(
            "CONVERSATION\tKIND\tUNREAD\n{}\t{}\t{}\n",
            result["id"], result["kind"], result["unread"]
        );
    }
    if result["requested"].is_number() {
        return format!(
            "REQUESTED\tCHANGED\tUNCHANGED\tMISSING\n{}\t{}\t{}\t{}\n",
            result["requested"], result["changed"], result["unchanged"], result["missing"]
        );
    }
    if result["read_cursor"].is_string() {
        return format!(
            "READ CURSOR\tADVANCED\n{}\t{}\n",
            result["read_cursor"], result["advanced"]
        );
    }
    format!(
        "AVAILABLE\tWIRE CONTRACT\n{}\t{}\n",
        result["available"], result["wire_contract"]
    )
}

#[cfg(test)]
mod tests {
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
            "../../../../ds-brain/docs/contracts/messaging-wire/messages-v1/config.json"
        ))
        .unwrap();
        assert_eq!(fixture["data"]["wire_contract"], "messages-v1");
        assert_eq!(fixture["data"]["default_page_size"], 40);
        assert_eq!(fixture["data"]["max_page_size"], 50);
    }
}
