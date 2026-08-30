//! `ds governance architecture list` — the graph, filtered, without the map.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{
    DELIVERY_STATES, DESCRIPTOR_ARG, INVALID_TEXT, LIST, READ_TIMEOUT, invoke, list_request,
    require_fields,
};

static REFUSALS: &[ds_cli_contract::spec::Refusal] = &[
    ds_cli_desktop::ops::NOT_PAIRED,
    ds_cli_desktop::ops::AMBIGUOUS,
    ds_cli_desktop::ops::UNREACHABLE,
    ds_cli_desktop::ops::PAIRING_REJECTED,
    ds_cli_desktop::ops::REFUSED,
    ds_cli_desktop::ops::UNSUPPORTED,
    ds_cli_desktop::ops::UNREADABLE,
    crate::SIGNED_OUT,
    crate::CONTRACT_MISMATCH,
    crate::NOT_PERMITTED,
    crate::NOT_FOUND,
    INVALID_TEXT,
];

pub static COMMAND: Command = Command {
    id: "governance.architecture.list",
    path: &["governance", "architecture", "list"],
    contract: 1,
    summary: "Filter architecture nodes and edges without opening the map.",
    purpose: "\
Answers the questions the graph exists to answer — what is implemented and \
where its evidence lives, what a user asked, what is being built, what is \
only a wish, and what is a recommended refactor — narrowed to one chapter or \
one delivery state. A filter the caller does not pass is omitted rather than \
sent empty, so an unfiltered listing is the whole live graph. Archived nodes \
and edges stay out unless asked for: removal archives, it never deletes.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "chapter",
            "<id>",
            "Keep only this chapter's nodes and edges.",
        ),
        Arg::value(
            "state",
            "<delivery-state>",
            "Keep only nodes in this delivery state.",
        )
        .choices(DELIVERY_STATES),
        Arg::switch(
            "include-archived",
            "Also return archived nodes and edges; they are excluded by default.",
        ),
        DESCRIPTOR_ARG,
    ],
    output: "\
`revision`, then `nodes` and `edges` as the authority holds them: each node \
with its chapter, lane, delivery state, evidence, owner and repository; each \
edge with its kind, confidence and evidence.",
    examples: &[
        Example {
            command: "ds governance architecture list --chapter survey-lifecycle --state wishlist --output json",
            note: "One chapter, one state. Omitting both returns the whole live graph.",
            runnable: false,
        },
        Example {
            command: "ds governance architecture list --state user_question --output json",
            note: "A hypothetical request: Record this form-factory question in the Survey lifecycle chapter. Read the chapter's questions first.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/governance.md"),
    availability: ds_cli_desktop::ops::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let body = list_request(
        inputs.value("chapter"),
        inputs.value("state"),
        inputs.switch("include-archived"),
    )?;
    let reply = invoke(
        inputs.value("desktop-descriptor"),
        &LIST,
        body,
        READ_TIMEOUT,
    )?;
    require_fields(&reply, LIST.operation, &["revision", "nodes", "edges"])?;
    Ok(reply)
}

pub fn render(data: &Value) -> String {
    let nodes = data["nodes"].as_array().map_or(&[][..], Vec::as_slice);
    let edges = data["edges"].as_array().map_or(&[][..], Vec::as_slice);
    let mut out = format!(
        "revision {} · {} · {}\n",
        data["revision"].as_u64().unwrap_or(0),
        ds_cli_desktop::ops::plural(nodes.len() as u64, "node"),
        ds_cli_desktop::ops::plural(edges.len() as u64, "edge"),
    );
    for node in nodes {
        out.push_str(&format!(
            "  {:<28} {:<20} {}\n",
            clip(node["id"].as_str().unwrap_or("?"), 28),
            node["delivery_state"].as_str().unwrap_or("—"),
            clip(node["label"].as_str().unwrap_or(""), 44),
        ));
    }
    for edge in edges {
        out.push_str(&format!(
            "  {:<28} {} → {}  ({})\n",
            clip(edge["id"].as_str().unwrap_or("?"), 28),
            edge["from"].as_str().unwrap_or("?"),
            edge["to"].as_str().unwrap_or("?"),
            edge["confidence"].as_str().unwrap_or("?"),
        ));
    }
    out
}

fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_delivery_states_are_the_closed_set_the_contract_names() {
        let state = COMMAND
            .args
            .iter()
            .find(|arg| arg.name == "state")
            .expect("state flag");
        assert_eq!(state.choices, DELIVERY_STATES);
        assert!(
            state.choices.contains(&"recommended_refactor")
                && state.choices.contains(&"user_question"),
            "the non-delivery claims are part of the vocabulary, not an afterthought"
        );
    }

    #[test]
    fn a_node_and_an_edge_both_reach_the_human_tier() {
        let text = render(&json!({
            "revision": 7,
            "nodes": [{ "id": "survey-form-factory", "delivery_state": "user_question", "label": "Form Factory" }],
            "edges": [{ "id": "e1", "from": "a", "to": "b", "confidence": "inferred" }],
        }));
        assert!(text.contains("revision 7 · 1 node · 1 edge"), "{text}");
        assert!(text.contains("survey-form-factory"), "{text}");
        assert!(text.contains("a → b  (inferred)"), "{text}");
    }
}
