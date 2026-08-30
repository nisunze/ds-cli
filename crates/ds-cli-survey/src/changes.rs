//! One bounded page from the selected project's fenced Survey changes feed.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{SURVEY_ENTRIES_CHANGES_MAX_LIMIT, SurveyEntriesChangesRequest};
use serde_json::{Value, json};

const FORM: Arg = Arg::value("form", "<form-slug>", "Exact governed form slug.").required();
const UPDATED_AFTER: Arg = Arg::value(
    "updated-after",
    "<RFC3339>",
    "Inclusive lower replication clock; continuation pages must reuse it unchanged.",
)
.required();
const LIMIT: Arg = Arg::value(
    "limit",
    "<1-500>",
    "Maximum rows in this one page; continuation pages must reuse it unchanged.",
)
.default("100");
const CURSOR: Arg = Arg::value(
    "cursor",
    "<opaque-cursor>",
    "Exact opaque next_cursor from the preceding incomplete page; no whitespace, maximum 4096 bytes.",
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
        when: "the form, updated-after clock, limit, or other fixed request value violates the closed changes grammar",
        remedy: "pass one exact form, an RFC3339 lower clock, and a limit from 1 through 500",
    },
    Refusal {
        code: "survey_entries_changes_cursor_invalid",
        when: "the supplied opaque cursor is malformed or does not match this authority and unchanged request",
        remedy: "reuse the exact next_cursor with identical --updated-after and --limit, or restart from the last completed checkpoint",
    },
    Refusal {
        code: "survey_entries_changes_fence_expired",
        when: "the immutable BigQuery page fence carried by an incomplete cursor has expired",
        remedy: "discard the incomplete cursor and restart from the last previously completed checkpoint, never this expired feed's upper_fence",
    },
    Refusal {
        code: "survey_entries_changes_too_expensive",
        when: "the bounded changes query exceeds the backend query budget",
        remedy: "keep the last completed checkpoint unchanged; repair partitioning or indexing, or raise the governed backend query budget, then restart there",
    },
    Refusal {
        code: "survey_entries_changes_too_large",
        when: "the bounded changes response exceeds its byte limit",
        remedy: "lower --limit and restart from the last completed checkpoint",
    },
    Refusal {
        code: "survey_entries_changes_mirror_invalid",
        when: "the governed Survey mirror cannot represent valid change evidence",
        remedy: "repair or update the governed mirror; an unchanged retry is not a remedy",
    },
    Refusal {
        code: "survey_entries_changes_snapshot_unavailable",
        when: "the immutable BigQuery table version for this cursor is temporarily unavailable",
        remedy: "retry the identical page request with the exact same cursor",
    },
    Refusal {
        code: "survey_entries_changes_unavailable",
        when: "the fenced changes service or its durable cursor-signing configuration is unavailable on this deployment",
        remedy: "configure the governed deployment and durable changes cursor signing key, then retry from the last completed checkpoint",
    },
    Refusal {
        code: "survey_entries_changes_sync_failed",
        when: "Survey data cannot be synchronized before reading changes",
        remedy: "retry without changing the page request and report repeated sync failures",
    },
    Refusal {
        code: "survey_entries_changes_failed",
        when: "the fenced changes service fails temporarily",
        remedy: "retry without changing the page request and report repeated failures",
    },
    Refusal {
        code: "survey_entries_scope_not_found",
        when: "the selected project or governed form is unavailable to the verified user",
        remedy: "verify the selected project and pass one exact available form slug",
    },
    Refusal {
        code: "survey_entries_changes_refused",
        when: "the backend coarsely refuses an already validated changes request without a recognized service code",
        remedy: "verify the form and restart from the last completed checkpoint",
    },
    Refusal {
        code: "survey_entries_changes_auth_rejected",
        when: "the fixed changes route rejects the verified identity or form authority",
        remedy: "verify account and form authority in the selected project",
    },
    Refusal {
        code: "survey_entries_changes_transient",
        when: "the fixed changes service is temporarily unavailable without a recognized service code",
        remedy: "retry the identical page request without advancing its checkpoint",
    },
    Refusal {
        code: "survey_entries_changes_unreadable",
        when: "the changes response violates its closed identity, clocks, geometry, ordering, paging, or consistency contract",
        remedy: "retry once without advancing the checkpoint, then update ds if it persists",
    },
    Refusal {
        code: "native_profile_not_configured",
        when: "the exact packaged native profile is unavailable",
        remedy: "install one complete ds release",
    },
    Refusal {
        code: "native_profile_digest_mismatch",
        when: "the packaged catalogue differs from the build pin",
        remedy: "reinstall one complete ds release",
    },
    Refusal {
        code: "native_profile_unsafe",
        when: "the packaged native catalogue is unsafe or malformed",
        remedy: "reinstall one complete ds release",
    },
    Refusal {
        code: "headless_signed_out",
        when: "the selected lane has no restorable native user",
        remedy: "run ds auth login --email <address>",
    },
    Refusal {
        code: "headless_project_not_selected",
        when: "the user has no audience-fenced project selection",
        remedy: "run ds auth project use --project <exact-id>",
    },
    Refusal {
        code: "project_context_stale",
        when: "the saved project belongs to another identity, lane, or audience",
        remedy: "select the project again with ds auth project use",
    },
    Refusal {
        code: "native_state_unsafe",
        when: "protected native state is unsafe or unreadable",
        remedy: "repair the owner-only DS config directory",
    },
    Refusal {
        code: "native_state_unavailable",
        when: "protected native state cannot be accessed",
        remedy: "repair the owner-only DS config directory",
    },
    Refusal {
        code: "native_state_protection_unavailable",
        when: "this build has no protected-state adapter",
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
        when: "revoked identity cleanup cannot clear project context",
        remedy: "repair protected state and run auth logout",
    },
    Refusal {
        code: "auth_rejected",
        when: "native identity restoration rejects the saved credential before the changes call",
        remedy: "verify the account and sign in again if the credential was revoked",
    },
    Refusal {
        code: "auth_revoked",
        when: "Firebase permanently revokes the native session",
        remedy: "sign in again interactively",
    },
    Refusal {
        code: "auth_identity_mismatch",
        when: "Firebase returns an identity outside the bound session",
        remedy: "sign in again and report a repeated mismatch",
    },
    Refusal {
        code: "auth_transient",
        when: "native identity restoration is temporarily unavailable before the changes call",
        remedy: "retry without changing local state",
    },
    Refusal {
        code: "auth_response_unreadable",
        when: "native identity restoration returns an unreadable response before the changes call",
        remedy: "retry once, then sign in again or update ds if it persists",
    },
];

pub static COMMAND: Command = Command {
    id: "survey.entries.changes",
    path: &["survey", "entries", "changes"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Read one fenced page of Survey changes headlessly.",
    purpose: "Validates one exact form, RFC3339 inclusive lower clock, bounded limit, and optional opaque cursor before profile or auth access; restores the native user; loads only its audience-fenced selected project under lease; releases the lease before one fixed Survey changes call; and verifies typed coalesced mirror rows and immutable page-fence consistency. It never auto-paginates. An incomplete page must be continued with identical updated-after and limit plus the exact next_cursor, without advancing the checkpoint. Only a complete page's upper_fence may become the next checkpoint. The inclusive lower clock can safely replay exact-boundary evidence, so automation must apply idempotently by doc_id plus firestore_updated_at; a tombstone removes the corresponding local live row. This is not Firestore snapshot or mutation history. There is no project, URL, method, body, token, field/media/deletion projection, force, caller-authority, or Desktop override.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[FORM, UPDATED_AFTER, LIMIT, CURSOR, LANE],
    output: "Lane, selected-project identity, exact form and canonical inclusive updated_after, effective limit, typed coalesced rows with optional geometry and tombstones, upper_fence, next_cursor, has_more/complete, and explicit immutable BigQuery mirror-fence consistency. If has_more, retain the previous completed checkpoint and reuse identical updated_after and limit with exact next_cursor. Only when complete may upper_fence become the next checkpoint.",
    examples: &[
        Example {
            command: "ds survey entries changes --form lv_poles_survey --updated-after 2026-08-30T00:00:00Z --output json",
            note: "Reads one page from the selected project; an incomplete result must not advance the checkpoint.",
            runnable: false,
        },
        Example {
            command: "ds survey entries changes --form lv_poles_survey --updated-after 2026-08-30T00:00:00Z --limit 500 --cursor '<exact-next-cursor>' --output json",
            note: "Continues one immutable feed fence with the exact prior request and cursor; it does not auto-loop.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    // Profile discovery belongs after the closed local parser.
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

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
