//! `ds pls reference-closure` — what a workspace actually references.
//!
//! A live PLS-CADD workspace binds its structures, cables and criteria by
//! absolute runtime paths. Moving or reissuing that workspace means knowing
//! which of those references translate to canonical asset-class identities and
//! which do not — before the move, not after.
//!
//! The task answers that. It is read-only and mutates nothing.

use std::path::PathBuf;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_tasks::{
    InspectPlsReferenceClosureRequest, inspect_pls_reference_closure,
    inspect_pls_reference_closure_request_schema,
};
use serde_json::{Value, json};

use crate::{bounded_limit, numeric, task_failure};

pub static COMMAND: Command = Command {
    id: "pls.reference-closure",
    path: &["pls", "reference-closure"],
    contract: 1,
    summary: "Translate a live workspace's reference closure.",
    purpose: "\
Walks a PLS-CADD workspace and reports every authored reference it contains, \
translated from the absolute runtime binding to a canonical asset-class and \
leaf identity. Run it before moving or reissuing a workspace: it names the \
references that will not survive the move while there is still time to fix \
them. It reads the workspace and writes nothing.",
    chapter: Chapter::PlsCadd,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("workspace", "<dir>", "The PLS-CADD workspace root.").required(),
        Arg::switch(
            "findings-only",
            "Return only references that need attention.",
        ),
        Arg::value("offset", "<n>", "Start at this translation.").default("0"),
        Arg::value(
            "limit",
            "<n>",
            "Return at most this many translations; the task bounds this at 32.",
        ),
    ],
    output: "\
Workspace file count and byte total, the component-identity verdicts, and the \
translations. `more.next_offset` when the closure continues.",
    examples: &[Example {
        command: "ds pls reference-closure --workspace ./workspace --findings-only --output json",
        note: "The short answer: only what needs attention.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "workspace_not_found",
            when: "--workspace does not name a directory",
            remedy: "check the path; it takes a PLS-CADD workspace root",
        },
        Refusal {
            code: "invalid_number",
            when: "--offset is not a whole number, or --limit is outside the task's bound of 32",
            remedy: "the refusal carries the accepted range; offset starts at 0",
        },
        Refusal {
            code: "task_refused",
            when: "the task ran and refused — an unreadable workspace, or a limit outside its bound",
            remedy: "read detail.code and detail.detail for the task's own reason",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let raw = inputs.require("workspace")?;
    let root = PathBuf::from(raw);
    if !root.is_dir() {
        return Err(
            Failure::invalid("workspace_not_found", format!("`{raw}` is not a directory"))
                .remedy("check the path; it takes a PLS-CADD workspace root"),
        );
    }

    let request = InspectPlsReferenceClosureRequest {
        workspace_root: root,
        findings_only: inputs.switch("findings-only"),
        offset: numeric(inputs.value("offset"), 0)?,
        limit: bounded_limit(
            inputs.value("limit"),
            &inspect_pls_reference_closure_request_schema(),
            "limit",
        )?,
    };

    let result = inspect_pls_reference_closure(&request)
        .map_err(|error| task_failure(&error.code, &error.detail))?;

    serde_json::to_value(&result).map_err(|error| {
        Failure::internal(
            "result_unserializable",
            "the task result could not be encoded",
        )
        .detail(json!({ "detail": error.to_string() }))
    })
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{}\n  {} file(s) · {} bytes\n\n",
        data["workspace_root"].as_str().unwrap_or(""),
        data["workspace_file_count"],
        data["workspace_bytes"],
    );
    out.push_str(&format!(
        "  component identity preserved  {}\n  component refs owner-bound    {}\n",
        data["component_identity_preserved"], data["component_references_owner_bound"],
    ));
    out.push_str(&format!(
        "\n  source components   {} file(s), {} owner(s)\n  translated          {} file(s), {} owner(s)\n",
        data["source_component_file_count"],
        data["source_component_owner_count"],
        data["translated_component_file_count"],
        data["translated_component_owner_count"],
    ));
    if let Some(translations) = data["translations"].as_array() {
        out.push_str(&format!("\n{} translation(s):\n", translations.len()));
        for translation in translations {
            out.push_str(&format!(
                "  {:<40} {}\n",
                translation["authored_reference"].as_str().unwrap_or(""),
                translation["location_class"].as_str().unwrap_or(""),
            ));
        }
    }
    out
}
