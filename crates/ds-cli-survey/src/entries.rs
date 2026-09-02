//! One bounded spatial selection from the selected project's Survey mirror.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{SURVEY_ENTRIES_SELECT_MAX_LIMIT, SurveyEntriesSelectRequest};
use serde_json::{Value, json};

const FORM: Arg = Arg::value("form", "<form-slug>", "Exact governed form slug.").required();
const BBOX: Arg = Arg::value(
    "bbox",
    "<west,south,east,north>",
    "Closed WGS84 bounding box with west < east and south < north.",
)
.required();
const LIMIT: Arg = Arg::value(
    "limit",
    "<1-500>",
    "Maximum selected rows; truncated results must use a narrower bounding box.",
)
.default("100");
const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "survey_entries_invalid",
        when: "the form, bounding box, or limit violates the closed local grammar",
        remedy: "pass one exact form, four ordered WGS84 coordinates, and a limit from 1 through 500",
    },
    Refusal {
        code: "survey_entries_scope_not_found",
        when: "the selected project or governed form is unavailable to the verified user",
        remedy: "verify the selected project and pass one exact available form slug",
    },
    Refusal {
        code: "survey_entries_too_expensive",
        when: "the selection exceeds the backend query budget",
        remedy: "narrow --bbox before retrying",
    },
    Refusal {
        code: "survey_entries_too_large",
        when: "the selection exceeds the bounded response limit",
        remedy: "narrow --bbox or lower --limit before retrying",
    },
    Refusal {
        code: "survey_entries_sync_failed",
        when: "Survey data cannot be synchronized before selection",
        remedy: "retry without changing the selection and report repeated sync failures",
    },
    Refusal {
        code: "survey_entries_mirror_invalid",
        when: "the governed Survey mirror cannot represent the selection safely",
        remedy: "repair or update the governed mirror; an unchanged retry is not a remedy",
    },
    Refusal {
        code: "survey_entries_unavailable",
        when: "the bounded selection service is unavailable on this deployment",
        remedy: "retry later without changing the selection",
    },
    Refusal {
        code: "survey_entries_failed",
        when: "the bounded selection service fails temporarily",
        remedy: "retry without changing the selection and report repeated failures",
    },
    Refusal {
        code: "survey_entries_refused",
        when: "the backend refuses the already validated bounded selection",
        remedy: "narrow --bbox or lower --limit, then verify the governed form state",
    },
    Refusal {
        code: "survey_entries_auth_rejected",
        when: "the fixed selection route rejects the verified identity or form authority",
        remedy: "verify account and form authority in the selected project",
    },
    Refusal {
        code: "survey_entries_transient",
        when: "the fixed selection service or its required mirror sync is temporarily unavailable",
        remedy: "retry without changing local state",
    },
    Refusal {
        code: "survey_entries_unreadable",
        when: "the selection response violates its closed identity, geometry, consistency, order, or digest contract",
        remedy: "retry once, then update ds if it persists",
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
        when: "native identity restoration rejects the saved credential before selection",
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
        when: "native identity restoration is temporarily unavailable before selection",
        remedy: "retry without changing local state",
    },
    Refusal {
        code: "auth_response_unreadable",
        when: "native identity restoration returns an unreadable response before selection",
        remedy: "retry once, then sign in again or update ds if it persists",
    },
];

pub static COMMAND: Command = Command {
    id: "survey.entries.select",
    path: &["survey", "entries", "select"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Select bounded Survey geometry headlessly.",
    purpose: "Validates one exact form, WGS84 bounding box, and limit before profile or auth access; restores the native user; loads only its audience-fenced selected project under lease; releases the lease before the fixed Survey selection call; and verifies the returned mutable mirror rows and digest. There is no project, URL, method, body, token, cursor, geometry-document, field, media, deletion, force, caller-authority, or Desktop override.",
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[FORM, BBOX, LIMIT, LANE],
    output: "Lane, selected-project identity, exact form and bounding box, at most 500 typed geometry rows, truncation/completeness, a selection digest, and explicit mutable-mirror consistency. A truncated result requires a narrower --bbox; no cursor or mutable apply is provided.",
    examples: &[
        Example {
            command: "ds survey entries select --form lv_poles_survey --bbox '29.70,-2.05,29.80,-1.95' --output json",
            note: "Returns at most 100 spatially intersecting rows from the selected project's governed form.",
            runnable: false,
        },
        Example {
            command: "ds survey entries select --form customers --bbox '30.00,-1.99,30.02,-1.97' --limit 500 --output json",
            note: "Raises the row bound; if truncated, narrow the bounding box instead of expecting pagination.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // Parse every caller-controlled byte before profile discovery, local auth,
    // project-context access, or network work.
    let request = parse(inputs)?;
    let headless = ds_cli_auth::survey_entries_select(inputs.require("lane")?, &request)?;
    let selection = headless.selection();
    let rows = selection
        .rows()
        .iter()
        .map(|row| {
            let geometry = serde_json::from_str::<Value>(row.geometry_json()).map_err(|_| {
                Failure::unavailable(
                    "survey_entries_unreadable",
                    "the verified Survey geometry could not be projected as JSON",
                )
                .remedy("retry once, then update ds if it persists")
            })?;
            Ok(json!({
                "doc_id": row.doc_id(),
                "geometry": geometry,
                "created_by": row.created_by(),
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
        "form": selection.form(),
        "bbox": selection.bbox(),
        "rows": rows,
        "truncated": selection.truncated(),
        "complete": selection.complete(),
        "selection_digest": selection.selection_digest(),
        "consistency": {
            "source": selection.consistency().source(),
            "sync": selection.consistency().sync(),
            "snapshot": selection.consistency().snapshot(),
            "mutable": selection.consistency().mutable(),
        },
    }))
}

fn parse(inputs: &Inputs) -> Result<SurveyEntriesSelectRequest, Failure> {
    let bbox = parse_bbox(inputs.require("bbox")?)?;
    let limit = inputs
        .require("limit")?
        .parse::<u16>()
        .ok()
        .filter(|limit| (1..=SURVEY_ENTRIES_SELECT_MAX_LIMIT).contains(limit))
        .ok_or_else(|| invalid("`--limit` must be an integer from 1 through 500"))?;
    SurveyEntriesSelectRequest::new(inputs.require("form")?, bbox, Some(limit)).map_err(|_| {
        invalid("the form, bounding box, or limit violates the closed Survey selection grammar")
    })
}

fn parse_bbox(raw: &str) -> Result<[f64; 4], Failure> {
    let values = raw
        .split(',')
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid("`--bbox` must contain exactly four decimal coordinates"))?;
    values
        .try_into()
        .map_err(|_| invalid("`--bbox` must contain west,south,east,north"))
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("survey_entries_invalid", message)
        .remedy("read `ds survey entries select --help` and pass only the bounded typed flags")
}

pub fn render(data: &Value) -> String {
    let project = data["project"]["ds_project"].as_str().unwrap_or("project");
    let form = data["form"].as_str().unwrap_or("form");
    let rows = data["rows"].as_array().map_or(0, Vec::len);
    if data["truncated"].as_bool().unwrap_or(false) {
        format!(
            "{project}/{form}  {rows} rows  TRUNCATED — narrow --bbox; this is not a complete selection\n"
        )
    } else {
        format!(
            "{project}/{form}  {rows} rows  complete mutable mirror selection (not a snapshot)\n"
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
    fn exact_bbox_and_default_bound_parse_before_auth() {
        let parsed = super::parse(&inputs(&[
            "--form",
            "lv_poles_survey",
            "--bbox",
            "29.7,-2.05,29.8,-1.95",
        ]))
        .unwrap();
        assert_eq!(parsed.form(), "lv_poles_survey");
        assert_eq!(parsed.bbox(), [29.7, -2.05, 29.8, -1.95]);
        assert_eq!(
            parsed.limit(),
            ds_client_core::SURVEY_ENTRIES_SELECT_DEFAULT_LIMIT
        );
        assert_eq!(COMMAND.arg("limit").unwrap().default, Some("100"));
    }

    #[test]
    fn local_grammar_refuses_invalid_bounds_before_auth() {
        for arguments in [
            vec!["--form", "bad-form", "--bbox", "29,-2,30,-1"],
            vec!["--form", "poles", "--bbox", "29,-2,30"],
            vec!["--form", "poles", "--bbox", "30,-2,29,-1"],
            vec!["--form", "poles", "--bbox", "29,-1,30,-2"],
            vec!["--form", "poles", "--bbox", "NaN,-2,30,-1"],
            vec!["--form", "poles", "--bbox", "29,-2,30,-1", "--limit", "0"],
            vec!["--form", "poles", "--bbox", "29,-2,30,-1", "--limit", "501"],
        ] {
            assert_eq!(
                super::parse(&inputs(&arguments)).unwrap_err().code(),
                "survey_entries_invalid"
            );
        }
    }

    #[test]
    fn descriptor_has_only_the_closed_selection_grammar() {
        let names = COMMAND
            .args
            .iter()
            .map(|arg| arg.name)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            names,
            std::collections::BTreeSet::from(["bbox", "form", "lane", "limit"])
        );
        for forbidden in [
            "project",
            "url",
            "method",
            "body",
            "token",
            "cursor",
            "wkt",
            "geojson",
            "fields",
            "media",
            "deleted",
            "include-deleted",
            "force",
            "authority",
            "desktop-descriptor",
        ] {
            assert!(COMMAND.arg(forbidden).is_none(), "unexpected --{forbidden}");
        }
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.effect, Effect::LocalAuthState);
        let refusals = COMMAND
            .refusals
            .iter()
            .map(|refusal| refusal.code)
            .collect::<std::collections::BTreeSet<_>>();
        for code in [
            "survey_entries_scope_not_found",
            "survey_entries_too_expensive",
            "survey_entries_too_large",
            "survey_entries_sync_failed",
            "survey_entries_mirror_invalid",
            "survey_entries_unavailable",
            "survey_entries_failed",
            "survey_entries_refused",
            "survey_entries_auth_rejected",
            "survey_entries_transient",
            "survey_entries_unreadable",
        ] {
            assert!(refusals.contains(code));
        }
    }

    #[test]
    fn human_truncation_is_an_explicit_narrow_bbox_requirement() {
        let output = render(&json!({
            "project": { "ds_project": "project-a" },
            "form": "poles",
            "rows": [{}, {}],
            "truncated": true,
        }));
        assert!(output.contains("TRUNCATED"));
        assert!(output.contains("narrow --bbox"));
        assert!(output.contains("not a complete selection"));
    }
}
