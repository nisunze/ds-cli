//! One bounded page from the selected project's fenced Survey changes feed.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{SURVEY_ENTRIES_CHANGES_MAX_LIMIT, SurveyEntriesChangesRequest};
use serde_json::{Value, json};

const FORM: Arg = Arg::value("form", "<form-slug>", "Exact governed form slug.").required();
const UPDATED_AFTER: Arg = Arg::value(
    "updated-after",
    "<RFC3339>",
    "Inclusive lower clock; reuse unchanged across continuation pages.",
)
.required();
const LIMIT: Arg = Arg::value(
    "limit",
    "<1-500>",
    "Rows in this page; reuse unchanged across continuations.",
)
.default("100");
const CURSOR: Arg = Arg::value(
    "cursor",
    "<opaque-cursor>",
    "Exact next_cursor for an incomplete page; no whitespace, maximum 4096 bytes.",
);
const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "survey_entries_changes_invalid",
        when: "form, lower clock, limit, or another fixed value violates the closed grammar",
        remedy: "pass an exact form, RFC3339 lower clock, and limit from 1 through 500",
    },
    Refusal {
        code: "survey_entries_changes_cursor_invalid",
        when: "the cursor is malformed or does not match this authority and request",
        remedy: "reuse it with identical --updated-after and --limit, or restart at the last completed checkpoint",
    },
    Refusal {
        code: "survey_entries_changes_fence_expired",
        when: "an incomplete cursor's immutable BigQuery fence has expired",
        remedy: "discard it and restart at the prior completed checkpoint, never the expired upper_fence",
    },
    Refusal {
        code: "survey_entries_changes_too_expensive",
        when: "the changes query exceeds its backend budget",
        remedy: "retain the completed checkpoint; repair partitioning/indexing or raise the governed budget, then restart there",
    },
    Refusal {
        code: "survey_entries_changes_too_large",
        when: "the response exceeds its byte limit",
        remedy: "lower --limit and restart at the last completed checkpoint",
    },
    Refusal {
        code: "survey_entries_changes_mirror_invalid",
        when: "the Survey mirror cannot represent valid change evidence",
        remedy: "repair or update the mirror; do not retry unchanged",
    },
    Refusal {
        code: "survey_entries_changes_snapshot_unavailable",
        when: "this cursor's BigQuery table version is temporarily unavailable",
        remedy: "retry the identical page with the same cursor",
    },
    Refusal {
        code: "survey_entries_changes_unavailable",
        when: "the changes service or durable cursor signing is not configured",
        remedy: "configure the deployment and signing key, then restart at the completed checkpoint",
    },
    Refusal {
        code: "survey_entries_changes_sync_failed",
        when: "Survey data cannot synchronize before the read",
        remedy: "retry the same page and report repeated failures",
    },
    Refusal {
        code: "survey_entries_changes_failed",
        when: "the changes service fails temporarily",
        remedy: "retry the same page and report repeated failures",
    },
    Refusal {
        code: "survey_entries_scope_not_found",
        when: "the selected project or form is unavailable to this user",
        remedy: "verify the selected project and exact available form slug",
    },
    Refusal {
        code: "survey_entries_changes_refused",
        when: "the backend refuses the request without a recognized service code",
        remedy: "verify the form and restart at the last completed checkpoint",
    },
    Refusal {
        code: "survey_entries_changes_auth_rejected",
        when: "the route rejects identity or form authority",
        remedy: "verify the account and selected-project form authority",
    },
    Refusal {
        code: "survey_entries_changes_transient",
        when: "the service is temporarily unavailable without a recognized code",
        remedy: "retry the identical page without advancing its checkpoint",
    },
    Refusal {
        code: "survey_entries_changes_unreadable",
        when: "the response violates its closed identity, data, paging, or consistency contract",
        remedy: "retry once without advancing the checkpoint; update ds if it persists",
    },
    Refusal {
        code: "native_profile_not_configured",
        when: "the packaged native profile is unavailable",
        remedy: "install one complete ds release",
    },
    Refusal {
        code: "native_profile_digest_mismatch",
        when: "the packaged catalogue differs from the build pin",
        remedy: "reinstall one complete ds release",
    },
    Refusal {
        code: "native_profile_unsafe",
        when: "the packaged native catalogue is unsafe",
        remedy: "reinstall one complete ds release",
    },
    Refusal {
        code: "headless_signed_out",
        when: "the selected lane has no restorable native user",
        remedy: "run ds auth login --email <address>",
    },
    Refusal {
        code: "headless_project_not_selected",
        when: "the user has no audience-fenced selected project",
        remedy: "run ds auth project use --project <exact-id>",
    },
    Refusal {
        code: "project_context_stale",
        when: "the saved project belongs to another identity, lane, or audience",
        remedy: "select the project again with ds auth project use",
    },
    Refusal {
        code: "native_state_unsafe",
        when: "protected native state is unsafe",
        remedy: "repair the owner-only DS config directory",
    },
    Refusal {
        code: "native_state_unavailable",
        when: "protected native state cannot be accessed",
        remedy: "repair the owner-only DS config directory",
    },
    Refusal {
        code: "native_state_protection_unavailable",
        when: "the build has no protected-state adapter",
        remedy: "install a supported native ds build",
    },
    Refusal {
        code: "native_state_root_invalid",
        when: "the configured state root is not absolute",
        remedy: "unset it or provide an absolute path",
    },
    Refusal {
        code: "native_state_conflict",
        when: "another native operation holds the state lease",
        remedy: "retry after that operation finishes",
    },
    Refusal {
        code: "native_cleanup_required",
        when: "revoked-session cleanup cannot clear project context",
        remedy: "repair protected state and run auth logout",
    },
    Refusal {
        code: "auth_rejected",
        when: "identity restoration rejects the saved credential",
        remedy: "verify the account; sign in again if the credential was revoked",
    },
    Refusal {
        code: "auth_revoked",
        when: "Firebase permanently revokes the native session",
        remedy: "sign in again interactively",
    },
    Refusal {
        code: "auth_identity_mismatch",
        when: "Firebase returns an identity outside this session",
        remedy: "sign in again; report a repeated mismatch",
    },
    Refusal {
        code: "auth_transient",
        when: "identity restoration is temporarily unavailable",
        remedy: "retry without changing local state",
    },
    Refusal {
        code: "auth_response_unreadable",
        when: "identity restoration returns an unreadable response",
        remedy: "retry once; sign in again or update ds if it persists",
    },
];

pub static COMMAND: Command = Command {
    id: "survey.entries.changes",
    path: &["survey", "entries", "changes"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Read one fenced page of Survey changes headlessly.",
    purpose: "Validates the form, inclusive lower clock, limit, and cursor before auth; uses only the restored user's selected project; releases its lease before the fixed changes call; and verifies one immutable-fence page. It never auto-paginates. Continue incomplete pages with unchanged updated-after/limit and exact next_cursor without advancing the checkpoint. Only a complete upper_fence advances it. Apply rows idempotently by doc_id plus firestore_updated_at; tombstones remove live rows. This is coalesced mirror state, not Firestore history. No project, transport, projection, force, authority, or Desktop override exists.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[FORM, UPDATED_AFTER, LIMIT, CURSOR, LANE],
    output: "Selected project/form, canonical lower clock and limit, rows with optional geometry and tombstones, upper fence, cursor/completion, and immutable mirror consistency. For an incomplete page, retain the prior checkpoint and reuse the same clock/limit with its cursor; only a complete upper fence advances it.",
    examples: &[
        Example {
            command: "ds survey entries changes --form lv_poles_survey --updated-after 2026-08-30T00:00:00Z --output json",
            note: "Reads one page; an incomplete result does not advance the checkpoint.",
            runnable: false,
        },
        Example {
            command: "ds survey entries changes --form lv_poles_survey --updated-after 2026-08-30T00:00:00Z --limit 500 --cursor '<exact-next-cursor>' --output json",
            note: "Continues the prior immutable fence without auto-looping.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // Parse every caller-controlled byte before profile discovery, local auth,
    // selected-project state, or network work.
    let request = parse(inputs)?;
    let headless = ds_cli_auth::survey_entries_changes(inputs.require("lane")?, &request)?;
    let changes = headless.changes();
    let rows = changes
        .rows()
        .iter()
        .map(|row| {
            let geometry = row
                .geometry_json()
                .map(serde_json::from_str::<Value>)
                .transpose()
                .map_err(|_| {
                    Failure::unavailable(
                        "survey_entries_changes_unreadable",
                        "the verified Survey change geometry could not be projected as JSON",
                    )
                    .remedy("retry once without advancing the checkpoint, then update ds")
                })?;
            Ok(json!({
                "doc_id": row.doc_id(),
                "geometry": geometry,
                "created_by": row.created_by(),
                "is_deleted": row.is_deleted(),
                "firestore_updated_at": row.firestore_updated_at(),
            }))
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(json!({
        "lane": headless.lane(),
        "project": {
            "ds_project": headless.project_id(),
            "project_name": headless.project_name(),
            "status": headless.project_status(),
        },
        "form": changes.form(),
        "updated_after": changes.updated_after(),
        "limit": request.limit(),
        "upper_fence": changes.upper_fence(),
        "rows": rows,
        "next_cursor": changes.next_cursor(),
        "has_more": changes.has_more(),
        "complete": changes.complete(),
        "consistency": {
            "source": changes.consistency().source(),
            "sync": changes.consistency().sync(),
            "fence": changes.consistency().fence(),
            "firestore_snapshot": changes.consistency().firestore_snapshot(),
            "immutable_page_fence": changes.consistency().immutable_page_fence(),
        },
    }))
}

fn parse(inputs: &Inputs) -> Result<SurveyEntriesChangesRequest, Failure> {
    let limit = inputs
        .require("limit")?
        .parse::<u16>()
        .ok()
        .filter(|limit| (1..=SURVEY_ENTRIES_CHANGES_MAX_LIMIT).contains(limit))
        .ok_or_else(|| invalid("`--limit` must be an integer from 1 through 500"))?;
    SurveyEntriesChangesRequest::new(
        inputs.require("form")?,
        inputs.require("updated-after")?,
        Some(limit),
        inputs.value("cursor").map(str::to_owned),
    )
    .map_err(|_| {
        invalid("the form, updated-after, limit, or cursor violates the closed changes grammar")
    })
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("survey_entries_changes_invalid", message)
        .remedy("read `ds survey entries changes --help` and pass only the bounded typed flags")
}

pub fn render(data: &Value) -> String {
    let project = data["project"]["ds_project"].as_str().unwrap_or("project");
    let form = data["form"].as_str().unwrap_or("form");
    let rows = data["rows"].as_array().map_or(0, Vec::len);
    let lower = data["updated_after"].as_str().unwrap_or("updated_after");
    let limit = data["limit"].as_u64().unwrap_or(100);
    if data["has_more"].as_bool().unwrap_or(false) {
        let cursor = data["next_cursor"].as_str().unwrap_or("<missing>");
        format!(
            "{project}/{form}  {rows} changes  INCOMPLETE — do not advance the checkpoint\nreuse identical --updated-after {lower} and --limit {limit} with this exact next_cursor:\n{cursor}\nidempotently dedupe/apply by doc_id+firestore_updated_at; tombstones remove local live rows\n"
        )
    } else {
        let upper = data["upper_fence"].as_str().unwrap_or("upper_fence");
        format!(
            "{project}/{form}  {rows} changes  COMPLETE — upper_fence {upper} may become the next checkpoint\ninclusive lower clocks may replay exact-boundary evidence; idempotently dedupe/apply by doc_id+firestore_updated_at; tombstones remove local live rows\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::args::parse;

    fn inputs(arguments: &[&str]) -> Inputs {
        parse(
            &COMMAND,
            &arguments
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn exact_clock_and_default_bound_parse_before_auth() {
        let parsed = super::parse(&inputs(&[
            "--form",
            "lv_poles_survey",
            "--updated-after",
            "2026-08-30T02:00:00.120000000+02:00",
        ]))
        .unwrap();
        assert_eq!(parsed.form(), "lv_poles_survey");
        assert_eq!(parsed.updated_after(), "2026-08-30T00:00:00.12Z");
        assert_eq!(parsed.limit(), 100);
        assert_eq!(parsed.cursor(), None);
        assert_eq!(COMMAND.arg("limit").unwrap().default, Some("100"));
    }

    #[test]
    fn local_grammar_refuses_invalid_values_before_auth() {
        let too_long = "x".repeat(4097);
        for arguments in [
            vec![
                "--form",
                "bad-form",
                "--updated-after",
                "2026-08-30T00:00:00Z",
            ],
            vec!["--form", "poles", "--updated-after", "not-a-time"],
            vec![
                "--form",
                "poles",
                "--updated-after",
                "2026-08-30T00:00:00Z",
                "--limit",
                "0",
            ],
            vec![
                "--form",
                "poles",
                "--updated-after",
                "2026-08-30T00:00:00Z",
                "--limit",
                "501",
            ],
            vec![
                "--form",
                "poles",
                "--updated-after",
                "2026-08-30T00:00:00Z",
                "--cursor",
                "white space",
            ],
            vec![
                "--form",
                "poles",
                "--updated-after",
                "2026-08-30T00:00:00Z",
                "--cursor",
                &too_long,
            ],
        ] {
            assert_eq!(
                super::parse(&inputs(&arguments)).unwrap_err().code(),
                "survey_entries_changes_invalid"
            );
        }
    }

    #[test]
    fn descriptor_has_only_the_closed_changes_grammar() {
        let names = COMMAND
            .args
            .iter()
            .map(|arg| arg.name)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            names,
            std::collections::BTreeSet::from(["cursor", "form", "lane", "limit", "updated-after",])
        );
        for forbidden in [
            "project",
            "url",
            "method",
            "body",
            "token",
            "fields",
            "media",
            "deleted",
            "include-deleted",
            "force",
            "authority",
            "desktop-descriptor",
            "auto-pagination",
        ] {
            assert!(COMMAND.arg(forbidden).is_none(), "unexpected --{forbidden}");
        }
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.effect, Effect::LocalAuthState);
    }

    #[test]
    fn human_continuation_and_checkpoint_semantics_are_explicit() {
        let incomplete = render(&json!({
            "project": { "ds_project": "project-a" },
            "form": "poles",
            "updated_after": "2026-08-30T00:00:00Z",
            "limit": 100,
            "upper_fence": "2026-08-30T01:00:00Z",
            "rows": [{}, {}],
            "next_cursor": "opaque.cursor",
            "has_more": true,
        }));
        assert!(incomplete.contains("INCOMPLETE — do not advance the checkpoint"));
        assert!(incomplete.contains("reuse identical --updated-after"));
        assert!(incomplete.contains("--limit 100"));
        assert!(incomplete.contains("opaque.cursor"));
        assert!(incomplete.contains("tombstones remove local live rows"));
        assert!(!incomplete.contains("may become the next checkpoint"));

        let complete = render(&json!({
            "project": { "ds_project": "project-a" },
            "form": "poles",
            "updated_after": "2026-08-30T00:00:00Z",
            "limit": 100,
            "upper_fence": "2026-08-30T01:00:00Z",
            "rows": [],
            "next_cursor": null,
            "has_more": false,
        }));
        assert!(complete.contains("COMPLETE"));
        assert!(complete.contains("upper_fence 2026-08-30T01:00:00Z may become"));
        assert!(complete.contains("inclusive lower clocks may replay"));
        assert!(complete.contains("doc_id+firestore_updated_at"));
    }
}
