//! `ds governance architecture get` — the snapshot at the current head.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{
    DESCRIPTOR_ARG, GET, READ_REFUSALS, READ_TIMEOUT, get_request, invoke, require_fields,
};

pub static COMMAND: Command = Command {
    id: "governance.architecture.get",
    path: &["governance", "architecture", "get"],
    contract: 1,
    summary: "The governed architecture snapshot at the current revision.",
    purpose: "\
Reads the whole governed architecture graph and the revision it is at, \
without opening the map. The revision is what every mutation is fenced \
against: plan a change against the number this returns, and confirm that \
same number with `apply`. Read access follows Governance Architecture \
access; editing it does not.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[DESCRIPTOR_ARG],
    output: "\
`snapshot` — the graph exactly as the authority holds it, nodes and edges \
with their chapter, delivery state, evidence and confidence — and `revision`, \
the head this snapshot is.",
    examples: &[Example {
        command: "ds governance architecture get --output json",
        note: "Read `.data.revision` before planning anything; it is the fence a later apply confirms.",
        runnable: false,
    }],
    refusals: READ_REFUSALS,
    reference: Some("docs/reference/governance.md"),
    availability: ds_cli_desktop::ops::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let reply = invoke(
        inputs.value("desktop-descriptor"),
        &GET,
        get_request(),
        READ_TIMEOUT,
    )?;
    require_fields(&reply, GET.operation, &["snapshot", "revision"])?;
    Ok(reply)
}

/// The current head, for a caller that must fence against it.
///
/// Separate from [`run`] because `preview` needs the number and not the
/// snapshot: a proposal that names no `expected_revision` is previewed
/// against the head the server holds right now, and saying so needs one
/// bounded read rather than a second code path.
pub fn head(descriptor: Option<&str>) -> Result<i64, Failure> {
    let reply = invoke(descriptor, &GET, get_request(), READ_TIMEOUT)?;
    require_fields(&reply, GET.operation, &["snapshot", "revision"])?;
    reply["revision"]
        .as_i64()
        .ok_or_else(|| crate::mismatch(GET.operation, "the reply carries no `revision`"))
}

pub fn render(data: &Value) -> String {
    let snapshot = &data["snapshot"];
    let count = |field: &str| {
        snapshot[field]
            .as_array()
            .map(Vec::len)
            .or_else(|| snapshot[field].as_object().map(serde_json::Map::len))
    };
    let mut out = format!("revision  {}\n", data["revision"].as_u64().unwrap_or(0));
    for field in ["nodes", "edges"] {
        if let Some(count) = count(field) {
            out.push_str(&format!("{field:<9} {count}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_declares_no_input_beyond_the_descriptor() {
        assert_eq!(COMMAND.args.len(), 1);
        assert_eq!(COMMAND.args[0].name, "desktop-descriptor");
        assert_eq!(COMMAND.effect, Effect::ReadOnly);
        assert!(!COMMAND.effect.needs_confirmation());
    }

    #[test]
    fn the_human_tier_names_the_fence_first() {
        let text = render(&serde_json::json!({
            "revision": 12,
            "snapshot": { "nodes": [1, 2, 3], "edges": [1] },
        }));
        assert!(text.starts_with("revision  12\n"), "{text}");
        assert!(text.contains("nodes     3"), "{text}");
        assert!(text.contains("edges     1"), "{text}");
    }
}
