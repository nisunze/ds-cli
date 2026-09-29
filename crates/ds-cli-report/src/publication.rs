//! The shared compute-artifact heads, read without opening the report outbox.

use std::collections::BTreeSet;

use ds_cli_auth::sync::PublicationHeadQuery;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::compute_artifact_inventory::{Head, decode_head, valid_operation};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::project::{LANE_ARG, PROJECT_ARG};

const LIMIT_ARG: Arg =
    Arg::value("limit", "<1..100>", "Publication heads in one page.").default("25");
const CURSOR_ARG: Arg = Arg::value(
    "cursor",
    "<token>",
    "Opaque cursor returned by the preceding page.",
);
const ENGINE_ARG: Arg = Arg::value("engine", "<engine>", "Exact engine owning the head.")
    .required()
    .choices(&["network_reporter", "solar", "print", "vector_tiler"]);
const OPERATION_ARG: Arg = Arg::value(
    "operation",
    "<operation>",
    "Exact operation returned by publication list.",
)
.required();
const VARIANT_ARG: Arg = Arg::value(
    "variant",
    "<variant>",
    "Exact variant returned by publication list.",
)
.required();

const SELECTOR_INVALID: Refusal = Refusal {
    code: "publication_selector_invalid",
    when: "the project, head identity, limit, or cursor is outside the closed discovery grammar",
    remedy: "use the exact project, head identity, or cursor returned by a prior list",
};
const NOT_PERMITTED: Refusal = Refusal {
    code: "publication_not_permitted",
    when: "the native account cannot read the named project's publications",
    remedy: "check this account's access to the named project",
};
const NOT_FOUND: Refusal = Refusal {
    code: "publication_not_found",
    when: "the named head does not exist",
    remedy: "list the project's publication heads and use an exact identity",
};
const CONFLICT: Refusal = Refusal {
    code: "publication_conflict",
    when: "the shared publication head has an inconsistent record",
    remedy: "report the project and head identity; do not infer publication from local files",
};
const UNAVAILABLE: Refusal = Refusal {
    code: "publication_unavailable",
    when: "the native session or shared publication authority is unavailable",
    remedy: "retry the same read when the service is available",
};
const UNREADABLE: Refusal = Refusal {
    code: "publication_unreadable",
    when: "the returned page or head violates its bounded published contract",
    remedy: "report the project and response identity; do not use the partial page",
};
const REFUSALS: &[Refusal] = &[
    ds_cli_contract::args::INVALID_NUMBER,
    SELECTOR_INVALID,
    NOT_PERMITTED,
    NOT_FOUND,
    CONFLICT,
    UNAVAILABLE,
    UNREADABLE,
    super::project::NATIVE_PROFILE,
    super::project::NATIVE_PROFILE_DIGEST,
    super::project::NATIVE_PROFILE_UNSAFE,
    super::project::HEADLESS_SIGNED_OUT,
    super::project::AUTH_CONTEXT_MISMATCH,
    super::project::AUTH_REVOKED,
    super::project::AUTH_TRANSIENT,
];

pub static LIST: Command = Command {
    id: "report.publication.list",
    path: &["report", "publication", "list"],
    contract: 1,
    summary: "List shared project publication heads without local report state.",
    purpose: "\
Reads one bounded page of current compute-artifact heads under the restored native user \
and the explicitly named project. The server enforces project membership. Every returned \
head is checked by the shared kernel before it is shown. A cursor continues this exact \
project's list; the automatic project touch may reconcile the report queue \
first. This list itself opens no local room, Desktop or output download.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LANE_ARG, PROJECT_ARG, LIMIT_ARG, CURSOR_ARG],
    output: "Project and lane, `count`, full validated `heads` with output declarations and origins, `limit`, `has_more`, and `next_cursor`.",
    examples: &[Example {
        command: "ds report publication list --lane canary --project <exact-id> --output json",
        note: "Pass `.data.next_cursor` to the next call while `.data.has_more` is true.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["compute artifact", "remote head", "published report"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static SHOW: Command = Command {
    id: "report.publication.show",
    path: &["report", "publication", "show"],
    contract: 1,
    summary: "Read one exact shared publication head and its output declarations.",
    purpose: "\
Reads one engine/operation/variant head from the named project's shared compute-artifact \
authority under the restored native user. It validates the returned identity and all output \
declarations against the shared kernel. The automatic project touch may \
reconcile the report queue first. This head read itself downloads no output bytes.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        LANE_ARG,
        PROJECT_ARG,
        ENGINE_ARG,
        OPERATION_ARG,
        VARIANT_ARG,
    ],
    output: "Project and lane, exact `head` with revision, fingerprint, outputs and per-output origins where recorded.",
    examples: &[Example {
        command: "ds report publication show --lane canary --project <exact-id> --engine network_reporter --operation export-fill_in_buhoro --variant default --output json",
        note: "Compare each output's SHA-256 and byte count with the sealed local publication receipt.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["compute artifact", "remote head", "published report"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

#[derive(Deserialize)]
struct HeadPage {
    heads: Vec<Value>,
    #[serde(default)]
    next_cursor: Option<String>,
    has_more: bool,
    limit: u8,
}

fn unreadable() -> Failure {
    Failure::unavailable(
        "publication_unreadable",
        "the shared publication answer is invalid",
    )
    .remedy("report the project and response identity; do not use the partial page")
}

fn valid_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn validated_head(value: &Value) -> Result<Head, Failure> {
    let bytes = serde_json::to_vec(value).map_err(|_| unreadable())?;
    decode_head(&bytes).map_err(|_| unreadable())
}

fn validated_page(value: Value, requested_limit: u8) -> Result<Value, Failure> {
    let page: HeadPage = serde_json::from_value(value).map_err(|_| unreadable())?;
    if page.limit != requested_limit || page.heads.len() > usize::from(page.limit) {
        return Err(unreadable());
    }
    let cursor = page.next_cursor.filter(|cursor| !cursor.is_empty());
    if page.has_more != cursor.is_some()
        || (page.has_more && page.heads.is_empty())
        || cursor.as_deref().is_some_and(|value| !valid_cursor(value))
    {
        return Err(unreadable());
    }
    let mut seen = BTreeSet::new();
    let mut heads = Vec::with_capacity(page.heads.len());
    for raw in &page.heads {
        let head = validated_head(raw)?;
        if !seen.insert((
            head.engine.clone(),
            head.operation.clone(),
            head.variant.clone(),
        )) {
            return Err(unreadable());
        }
        heads.push(head);
    }
    Ok(json!({
        "count": heads.len(), "heads": heads, "limit": page.limit,
        "has_more": page.has_more, "next_cursor": cursor,
    }))
}

fn valid_cursor(cursor: &str) -> bool {
    !cursor.is_empty()
        && cursor.len() <= 512
        && cursor
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    if !valid_id(project) {
        return Err(
            Failure::invalid(SELECTOR_INVALID.code, SELECTOR_INVALID.when)
                .remedy(SELECTOR_INVALID.remedy),
        );
    }
    let limit = ds_cli_contract::args::integer(inputs.require("limit")?, "limit", 1, 100)? as u8;
    let cursor = inputs.value("cursor");
    if cursor.is_some_and(|value| !valid_cursor(value)) {
        return Err(
            Failure::invalid(SELECTOR_INVALID.code, SELECTOR_INVALID.when)
                .remedy(SELECTOR_INVALID.remedy),
        );
    }
    let page = ds_cli_auth::sync::publication_heads_for_project(
        inputs.require("lane")?,
        project,
        PublicationHeadQuery::List { limit, cursor },
    )?;
    let mut answer = validated_page(page, limit)?;
    answer["lane"] = json!(inputs.require("lane")?);
    answer["project"] = json!({"ds_project": project});
    Ok(answer)
}

pub fn show(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let engine = inputs.require("engine")?;
    let operation = inputs.require("operation")?;
    let variant = inputs.require("variant")?;
    if !valid_id(project) || !valid_operation(engine, operation) || !valid_id(variant) {
        return Err(
            Failure::invalid(SELECTOR_INVALID.code, SELECTOR_INVALID.when)
                .remedy(SELECTOR_INVALID.remedy),
        );
    }
    let raw = ds_cli_auth::sync::publication_heads_for_project(
        inputs.require("lane")?,
        project,
        PublicationHeadQuery::Show {
            engine,
            operation,
            variant,
        },
    )?;
    let head = validated_head(&raw)?;
    if head.engine != engine || head.operation != operation || head.variant != variant {
        return Err(unreadable());
    }
    Ok(json!({
        "lane": inputs.require("lane")?,
        "project": {"ds_project": project},
        "head": head,
    }))
}

pub fn render(data: &Value) -> String {
    let project = data["project"]["ds_project"].as_str().unwrap_or("?");
    if let Some(head) = data.get("head") {
        return format!(
            "{project}: {}/{}/{} revision {} · {} output(s)\n",
            head["engine"].as_str().unwrap_or("?"),
            head["operation"].as_str().unwrap_or("?"),
            head["variant"].as_str().unwrap_or("?"),
            head["head_revision"].as_u64().unwrap_or(0),
            head["outputs"].as_array().map_or(0, Vec::len),
        );
    }
    let mut out = format!(
        "{project}: {} publication head(s){}\n",
        data["count"].as_u64().unwrap_or(0),
        if data["has_more"].as_bool().unwrap_or(false) {
            " · more pages"
        } else {
            ""
        },
    );
    if let Some(heads) = data["heads"].as_array() {
        for head in heads {
            out.push_str(&format!(
                "  {}/{}/{} revision {} · {} output(s)\n",
                head["engine"].as_str().unwrap_or("?"),
                head["operation"].as_str().unwrap_or("?"),
                head["variant"].as_str().unwrap_or("?"),
                head["head_revision"].as_u64().unwrap_or(0),
                head["outputs"].as_array().map_or(0, Vec::len),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head() -> Value {
        json!({
            "work_id": "work1", "engine": "network_reporter", "operation": "export-test",
            "variant": "default", "input_base_fingerprint": "a".repeat(64),
            "head_revision": 1, "updated_at": "2026-09-28T14:00:00Z",
            "outputs": [{
                "output_id": "report-pdf", "format": "pdf", "content_type": "application/pdf",
                "sha256": "b".repeat(64), "size_bytes": 123,
            }],
        })
    }

    #[test]
    fn page_requires_complete_unique_heads_and_consistent_cursor() {
        let first = head();
        let good = json!({"heads": [first.clone()], "limit": 25, "has_more": false});
        assert_eq!(validated_page(good, 25).unwrap()["count"], 1);
        let duplicate =
            json!({"heads": [first.clone(), first.clone()], "limit": 25, "has_more": false});
        assert!(validated_page(duplicate, 25).is_err());
        let missing_cursor = json!({"heads": [first.clone()], "limit": 25, "has_more": true});
        assert!(validated_page(missing_cursor, 25).is_err());
        let wrong_limit = json!({"heads": [first.clone()], "limit": 24, "has_more": false});
        assert!(validated_page(wrong_limit, 25).is_err());
        let malformed_head = json!({"heads": [{"work_id": "x"}], "limit": 25, "has_more": false});
        assert!(validated_page(malformed_head, 25).is_err());
    }

    #[test]
    fn selectors_match_the_authoritys_closed_id_grammar() {
        assert!(valid_id("export-fill_in_buhoro"));
        assert!(!valid_id("../another-project"));
        assert!(!valid_id("bad id"));
        assert!(valid_cursor("cHJvamVjdA"));
        assert!(!valid_cursor("a/b"));
    }
}
