//! Thin native adapters for recoverable shared Project Notes.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_client_core::project_management::Command as PM;
use ds_command_kernel::project_management::project_notes as owner;
use serde_json::{Value, json};
use std::io::Read;
pub(super) const INPUT:Arg=Arg::value("input","<json-file>","Closed native note request JSON, at most 256 KiB; see the Shared Project Notes request examples in docs/reference/pm.md. No transport envelope, author or timestamp overrides.").required();
const NOTE: Arg = Arg::value("note", "<note-id>", "Exact same-project note identity.").required();
const VERSION: Arg = Arg::value(
    "version",
    "<integer>",
    "Exact positive recoverable note version; omit to read current.",
);
const PARENT: Arg = Arg::value(
    "parent",
    "<note-id>",
    "Read direct subnotes of this exact same-project note; no recursive acquisition. Set draft.memory.parent with an exact id/version in typed creation/update JSON.",
);
const CLAIM: Arg = Arg::value(
    "claim",
    "<claim-id>",
    "One individually retrievable claim at the selected note/version.",
);
const TERM: Arg = Arg::repeated(
    "term",
    "<words>",
    "Bounded text relevance, including archived note search; terms do not imply authority or acceptance.",
);
const TAG: Arg = Arg::repeated(
    "tag",
    "<project-tag>",
    "Topical tags; equivalent case, spacing, hyphen and underscore spellings normalize consistently.",
);
const TAG_MATCH: Arg = Arg::value(
    "tag-match",
    "<any|all>",
    "Match any or all supplied topical tags.",
)
.default("any")
.choices(&["any", "all"]);
const TYPE: Arg = Arg::repeated(
    "type",
    "<note-type>",
    "Filter session_summary,decision,constraint,correction,rejected_assumption,observation,proposal,mutation_outcome,context_consolidation.",
);
const STATUS: Arg = Arg::repeated(
    "status",
    "<lifecycle>",
    "Filter active,superseded,withdrawn,discarded,irrelevant,archived; lifecycle is separate from topical tags.",
);
const SORT: Arg = Arg::value(
    "sort",
    "<created|updated>",
    "Sort by server creation or update time, never decision authority.",
)
.default("updated")
.choices(&["created", "updated"]);
const DIRECTION: Arg = Arg::value("direction", "<asc|desc>", "Sort direction.")
    .default("desc")
    .choices(&["asc", "desc"]);
const CURSOR: Arg = Arg::value(
    "cursor",
    "<opaque>",
    "Continue only the same bounded owner query; no invented offset.",
);
const LIMIT: Arg = Arg::value(
    "limit",
    "<1..50>",
    "Bound returned notes/history; scanning and truncation remain explicit.",
)
.default("20");
pub(crate) const INVALID: Refusal = Refusal {
    code: "project_note_invalid",
    when: "the note request, provenance, scope, evidence, lifecycle or bound is invalid",
    remedy: "use the closed note fields; retain source/version pins and never relabel a proposal or inference as a decision",
};
pub(crate) const UNREADABLE: Refusal = Refusal {
    code: "project_note_input_unreadable",
    when: "the selected request is unreadable or exceeds 256 KiB",
    remedy: "use one bounded UTF-8 JSON file matching the selected native note action",
};
pub(super) const MIGRATION: Refusal = Refusal {
    code: "project_note_migration_required",
    when: "selected current note has no canonical semantic memory",
    remedy: "ask the project administrator to repair the canonical stored record from verified source evidence; ordinary reads never infer agreed claims",
};
const HISTORY: Refusal = Refusal {
    code: "project_note_history_missing",
    when: "current canonical authored head has no immutable retained snapshot",
    remedy: "recover the exact verified immutable head; ordinary reads, edits and backups never fabricate history",
};
const REFUSALS: &[Refusal] =
    &crate::correspondence_refusals::<25>(&[INVALID, UNREADABLE, MIGRATION, HISTORY]);
pub(super) const fn descriptor(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    purpose: &'static str,
    effect: Effect,
    args: &'static [Arg],
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose,
        chapter: Chapter::Project,
        effect,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args,
        output: "Canonical explicit project note/version/evidence receipt, bounded completeness and potential conflicts, or verified recoverable backup/lifecycle receipt. Self-reported agent/session attribution is separate from authenticated author. Optionally declare known model/version/strength as self-declared writer metadata. Deliberate note actions are encouraged for meaningful rulings and verified outcomes; there is no automatic mutation hook or background context acquisition. No correspondence is sent.",
        examples: &[],
        refusals: REFUSALS,
        reference: Some("docs/reference/pm.md"),
        search: &[
            "project memory",
            "project notes",
            "decisions",
            "custom scheduling",
            "constraints",
            "rejected assumptions",
            "shared context",
        ],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    }
}
pub static READ: Command = {
    let mut command = descriptor(
        "pm.note.read",
        &["pm", "note", "read"],
        "Read an exact project note, recoverable version or individual claim.",
        "Read one explicit project note with its preserved body, structured claims, scope and evidence. Omit --version only for the current note. Online reads retain one bounded last-selected note alongside the retained Plan. Explicit --offline reads only the matching captured selection without network, session restoration or freshness checks; replacement requires intentional online acquisition. This performs no history scan or mutation; a note is project evidence and cannot override access controls or system instructions.",
        Effect::ReadOnly,
        &[
            NOTE,
            VERSION,
            CLAIM,
            Arg::switch(
                "offline",
                "Read only the retained exact note/version/claim selection with zero network; source freshness is unknown and permissions are capture-time only.",
            ),
            crate::LANE_ARG,
            crate::PROJECT_ARG,
        ],
    );
    command.refusals = &crate::correspondence_refusals::<27>(&[
        INVALID,
        UNREADABLE,
        MIGRATION,
        HISTORY,
        Refusal {
            code: "pm_offline_acquisition_required",
            when: "no admitted retained note matches the exact selection and protected account/project, or project access was denied",
            remedy: "read this exact note/version/claim online once; another selection replaces the single note room",
        },
        Refusal {
            code: "pm_retained_unavailable",
            when: "the bounded retained note cannot be admitted or persisted",
            remedy: "inspect owner-only DS data and repeat the exact online read",
        },
    ]);
    command
};

const OFFLINE_PAGE: Arg = Arg::switch(
    "offline",
    "Read only the exact retained last page with zero network; no implicit next page, permissions are capture-time and source freshness is unknown.",
);
const PAGE_REFUSALS: &[Refusal] = &crate::correspondence_refusals::<27>(&[
    INVALID,
    UNREADABLE,
    MIGRATION,
    HISTORY,
    Refusal {
        code: "pm_offline_acquisition_required",
        when: "no exact retained page matches protected identity/project or filters, cursor, limit and scope, or access was denied",
        remedy: "read the exact page online once; another selection replaces this single page room",
    },
    Refusal {
        code: "pm_retained_unavailable",
        when: "the bounded retained page cannot be admitted or persisted",
        remedy: "inspect owner-only DS data and repeat the exact intentional online read",
    },
]);

const fn retained_page_descriptor(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    purpose: &'static str,
    effect: Effect,
    args: &'static [Arg],
) -> Command {
    let mut command = descriptor(id, path, summary, purpose, effect, args);
    command.refusals = PAGE_REFUSALS;
    command
}

pub static QUERY: Command = retained_page_descriptor(
    "pm.note.query",
    &["pm", "note", "query"],
    "Search shared notes or read direct subnotes with bounded filters.",
    "Online reads retain one fixed last-query page. Explicit --offline reuses only its exact filters/cursor/limit under the same protected account/project, with unknown freshness and capture-time permissions. Read one bounded same-project note page; --parent selects direct subnotes without recursive acquisition. Use normalized topical tags and explicit any/all matching. Lifecycle filters do not alter topical tags. Sort is creation/update time only, not authority or automatic supersession. Return scan/completeness/truncation explicitly; a partial empty page is not proof of no context.",
    Effect::ReadOnly,
    &[
        PARENT,
        TERM,
        TAG,
        TAG_MATCH,
        TYPE,
        STATUS,
        SORT,
        DIRECTION,
        CURSOR,
        LIMIT,
        OFFLINE_PAGE,
        crate::LANE_ARG,
        crate::PROJECT_ARG,
    ],
);
pub static CONTEXT: Command = retained_page_descriptor(
    "pm.note.context",
    &["pm", "note", "context"],
    "Retrieve applicable active rulings and scoped conflicts before acting.",
    "Online reads retain one fixed last-context page; explicit --offline reuses only the exact request and captured context revision, without network or session restoration. Captured incomplete or migration-required context remains incomplete. Consumes a closed context JSON: query fields terms/tags/tag_match/note_types/lifecycle/sort/direction/cursor/limit and optional scope. The existing project owner combines text relevance and affected objects/structures/conditions; exact-tag-only lookup is insufficient. Routine context is active only, scans at most 200 and reports incomplete results. Potential overlapping issue/scope conflicts retain exact sources and explicit supersession; newest does not win. References are inspected on demand, never fetched speculatively.",
    Effect::ReadOnly,
    &[INPUT, OFFLINE_PAGE, crate::LANE_ARG, crate::PROJECT_ARG],
);
pub static UPDATE: Command = descriptor(
    "pm.note.update",
    &["pm", "note", "update"],
    "Revise a project note with its reviewed version and stable operation.",
    "Closed Write JSON requires operation_id,id,expected_version,reason,draft. Draft contains subject/body/tags and memory schema ds.project-note-memory/v1, note_type,active lifecycle, provenance,scope,individual claims,evidence,supersedes,unresolved. Each claim has id,semantic kind,issue_key,text,mandatory provenance and exact evidence/supersession refs. User instructions/decisions cannot be inferred from proposals. Server owns author/time/permissions, version CAS and identical replay. Non-active transitions use verified backup retirement.",
    Effect::GlobalWrite,
    crate::authored_args![INPUT, crate::LANE_ARG, crate::PROJECT_ARG],
);
pub static VERSIONS: Command = descriptor(
    "pm.note.versions",
    &["pm", "note", "versions"],
    "List recoverable note revisions and their authors, reasons and hashes.",
    "Read bounded immutable history for one exact shared note. Evidence can retain a selected version; changed notes never replace their earlier provenance.",
    Effect::ReadOnly,
    &[NOTE, CURSOR, LIMIT, crate::LANE_ARG, crate::PROJECT_ARG],
);
pub static BACKUP: Command = descriptor(
    "pm.note.backup",
    &["pm", "note", "backup"],
    "Create and verify a recoverable exact-version note backup.",
    "Closed Backup JSON: operation_id,sources[{id,version}]. The existing project owner retains and verifies exact contents, full histories, metadata, evidence and relationships. A verified backup receipt with SHA is required before retirement; this action itself retires nothing.",
    Effect::GlobalWrite,
    crate::authored_args![INPUT, crate::LANE_ARG, crate::PROJECT_ARG],
);
pub static RETIRE: Command = descriptor(
    "pm.note.retire",
    &["pm", "note", "retire"],
    "Retire scoped notes only after their recoverable backup verifies.",
    "Closed Retire JSON: operation_id,sources[{id,version}],backup_id,backup_sha256,lifecycle,reason,replacement?{id,version}. Lifecycle is superseded/withdrawn/discarded/irrelevant/archived. The server re-verifies exact backup bytes and reviewed source versions before any transition. Backup failure prevents retirement; age alone never retires an active constraint.",
    Effect::GlobalWrite,
    crate::authored_args![INPUT, crate::LANE_ARG, crate::PROJECT_ARG],
);
pub static RESTORE: Command = descriptor(
    "pm.note.restore",
    &["pm", "note", "restore"],
    "Restore backed-up shared notes as new recoverable revisions.",
    "Closed Restore JSON: operation_id,backup_id,backup_sha256,sources[{id,version}],expected_versions[{id,version}]. Current fences cover exactly the restored identities. The owner verifies the backup and creates new versions without replacing history or evidence.",
    Effect::GlobalWrite,
    crate::authored_args![INPUT, crate::LANE_ARG, crate::PROJECT_ARG],
);
pub static CONSOLIDATE: Command = descriptor(
    "pm.note.consolidate",
    &["pm", "note", "consolidate"],
    "Publish concise context from exact source versions; sources stay.",
    "Closed Consolidate JSON: operation_id,id,reason,draft,sources[{id,version}]. Draft must be active context_consolidation memory with explicit claim provenance/scope/evidence. Consolidation preserves disagreements, unresolved questions and immutable source refs; it never promotes a proposal into an accepted decision or retires sources. Retirement is a separate verified-backup operation.",
    Effect::GlobalWrite,
    crate::authored_args![INPUT, crate::LANE_ARG, crate::PROJECT_ARG],
);
fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid(INVALID.code, e.to_string()).remedy(INVALID.remedy)
}
fn selected<T: serde::de::DeserializeOwned>(i: &Inputs) -> Result<T, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(i.require("input")?)
        .map_err(|e| Failure::invalid(UNREADABLE.code, e.to_string()).remedy(UNREADABLE.remedy))?
        .take(owner::MAX_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Failure::invalid(UNREADABLE.code, e.to_string()).remedy(UNREADABLE.remedy))?;
    if bytes.len() > owner::MAX_REQUEST_BYTES {
        return Err(invalid("selected note request exceeds 256 KiB"));
    }
    serde_json::from_slice(&bytes).map_err(invalid)
}
pub(super) fn ask(i: &Inputs, request: owner::Command) -> Result<Value, Failure> {
    request.payload(i.require("project")?).map_err(invalid)?;
    let command = PM::ProjectNotes { command: request };
    let report = if matches!(
        &command,
        PM::ProjectNotes {
            command: owner::Command::Write(_) | owner::Command::Consolidate(_)
        }
    ) {
        ds_cli_auth::project_management_for_project(
            i.require("lane")?,
            i.require("project")?,
            &command,
        )?
    } else {
        crate::authoring::project(i, i.require("lane")?, i.require("project")?, command)?
    };
    Ok(report.result().clone())
}
pub fn structured_create(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut r: owner::Write = selected(i)?;
    crate::authoring::draft(i, &mut r.draft)?;
    if r.expected_version.is_some() {
        return Err(invalid(
            "creation has no expected_version; use note update to revise",
        ));
    }
    ask(i, owner::Command::Write(r))
}
pub fn update(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut r: owner::Write = selected(i)?;
    crate::authoring::draft(i, &mut r.draft)?;
    if r.expected_version.is_none() {
        return Err(invalid("update needs the exact reviewed expected_version"));
    }
    ask(i, owner::Command::Write(r))
}
pub fn read(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let selected = owner::Read {
        id: i.require("note")?.into(),
        version: i
            .value("version")
            .map(|v| crate::integer(v, "version", 1, i64::MAX - 1).map(|v| v as u64))
            .transpose()?,
        claim_id: i.value("claim").map(str::to_owned),
    };
    ds_cli_auth::pm_graph_source::read_note(
        i.require("lane")?,
        i.require("project")?,
        &selected,
        i.switch("offline"),
    )
}

fn query_request(i: &Inputs) -> Result<owner::Query, Failure> {
    let q=serde_json::from_value(json!({"parent_id":i.value("parent"),"terms":i.repeated("term"),"tags":i.repeated("tag"),"tag_match":i.require("tag-match")?,"note_types":i.repeated("type"),"lifecycle":i.repeated("status"),"sort":i.require("sort")?,"direction":i.require("direction")?,"cursor":i.value("cursor"),"limit":crate::integer(i.require("limit")?,"limit",1,50)?})).map_err(invalid)?;
    Ok(q)
}
pub fn query(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_cli_auth::pm_graph_source::read_query(
        i.require("lane")?,
        i.require("project")?,
        &query_request(i)?,
        i.switch("offline"),
    )
}
pub fn context(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_cli_auth::pm_graph_source::read_context(
        i.require("lane")?,
        i.require("project")?,
        &selected(i)?,
        i.switch("offline"),
    )
}
pub fn versions(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(
        i,
        owner::Command::Versions(owner::Versions {
            id: i.require("note")?.into(),
            cursor: i.value("cursor").map(str::to_owned),
            limit: crate::integer(i.require("limit")?, "limit", 1, 50)? as u32,
        }),
    )
}
pub fn backup(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(i, owner::Command::Backup(selected(i)?))
}
pub fn retire(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(i, owner::Command::Retire(selected(i)?))
}
pub fn restore(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ask(i, owner::Command::Restore(selected(i)?))
}
pub fn consolidate(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut request: owner::Consolidate = selected(i)?;
    crate::authoring::draft(i, &mut request.draft)?;
    ask(i, owner::Command::Consolidate(request))
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(c: &Command, tokens: &[&str]) -> Inputs {
        ds_cli_contract::args::parse(c, &tokens.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .unwrap()
    }
    #[test]
    fn maintained_reference_examples_are_valid_closed_native_requests() {
        let section = include_str!("../../../docs/reference/pm.md")
            .split("## Shared Project Notes")
            .nth(1)
            .unwrap();
        let examples = section
            .split("```json\n")
            .skip(1)
            .map(|block| block.split("```").next().unwrap())
            .collect::<Vec<_>>();
        let write: owner::Write = serde_json::from_str(examples[0]).unwrap();
        owner::Command::Write(write)
            .payload("project-example")
            .unwrap();
        let context: owner::Context = serde_json::from_str(examples[1]).unwrap();
        owner::Command::Context(context)
            .payload("project-example")
            .unwrap();
    }
    #[test]
    fn query_flags_reach_owner_with_any_all_terms_sort_and_lifecycle_distinct() {
        let i = parse(
            &QUERY,
            &[
                "--project",
                "p",
                "--tag",
                "MV Staking",
                "--tag",
                "switching",
                "--tag-match",
                "all",
                "--term",
                "transformer quantities",
                "--type",
                "constraint",
                "--status",
                "archived",
                "--sort",
                "created",
                "--direction",
                "asc",
                "--limit",
                "7",
            ],
        );
        let q = query_request(&i).unwrap();
        q.validate().unwrap();
        assert_eq!(q.tag_match, owner::TagMatch::All);
        assert_eq!(q.tags, vec!["MV Staking", "switching"]);
        assert_eq!(q.terms, vec!["transformer quantities"]);
        assert_eq!(q.lifecycle, vec![owner::Lifecycle::Archived]);
        assert_eq!(q.sort, owner::Sort::Created);
        assert_eq!(q.direction, owner::Direction::Asc);
        assert_eq!(q.limit, 7);
    }
    #[test]
    fn parent_selector_is_explicit_and_reaches_existing_owner() {
        let i = parse(&QUERY, &["--project", "p", "--parent", "parent-note"]);
        let q = query_request(&i).unwrap();
        q.validate().unwrap();
        assert_eq!(q.parent_id.as_deref(), Some("parent-note"));
        let payload = owner::Command::Query(q).payload("p").unwrap();
        assert_eq!(payload["action"], "project_note_query");
        assert_eq!(payload["project_note"]["parent_id"], "parent-note");
        let unselected = query_request(&parse(&QUERY, &["--project", "p"])).unwrap();
        assert!(unselected.parent_id.is_none());
    }
    #[test]
    fn invalid_enum_and_bound_refuse_before_auth_or_a_project_read() {
        let i = parse(&QUERY, &["--project", "p", "--type", "agreed-by-inference"]);
        assert_eq!(query_request(&i).unwrap_err().code(), INVALID.code);
        let i = parse(&QUERY, &["--project", "p", "--limit", "51"]);
        assert!(query_request(&i).is_err());
    }
    #[test]
    fn creation_accepts_only_canonical_input_and_declared_effects() {
        assert!(
            ds_cli_contract::args::parse(
                &crate::note::CREATE,
                &[
                    "--project".into(),
                    "p".into(),
                    "--title".into(),
                    "Review".into(),
                    "--body".into(),
                    "Observed".into()
                ]
            )
            .is_err()
        );
        let i = parse(
            &crate::note::CREATE,
            &["--project", "p", "--input", "not-opened.json"],
        );
        assert_eq!(i.value("input"), Some("not-opened.json"));
        assert!(
            READ.effect == Effect::ReadOnly
                && CONTEXT.effect == Effect::ReadOnly
                && RETIRE.effect == Effect::GlobalWrite
        );
    }
    #[test]
    fn offline_read_is_explicit_exact_and_self_describing() {
        let i = parse(
            &READ,
            &[
                "--project",
                "p",
                "--note",
                "n",
                "--version",
                "2",
                "--claim",
                "c",
                "--offline",
            ],
        );
        assert!(i.switch("offline"));
        assert_eq!(i.value("note"), Some("n"));
        assert_eq!(i.value("version"), Some("2"));
        assert_eq!(i.value("claim"), Some("c"));
        assert!(
            READ.refusals
                .iter()
                .any(|r| r.code == "pm_offline_acquisition_required")
        );
        assert!(
            READ.refusals
                .iter()
                .any(|r| r.code == "pm_retained_unavailable")
        );
        assert!(!parse(&READ, &["--project", "p", "--note", "n"]).switch("offline"));
        for command in [&CONTEXT, &QUERY] {
            assert!(command.args.iter().any(|a| a.name == "offline"));
            assert!(
                command
                    .refusals
                    .iter()
                    .any(|r| r.code == "pm_offline_acquisition_required")
            );
            assert!(
                command
                    .refusals
                    .iter()
                    .any(|r| r.code == "pm_retained_unavailable")
            );
        }
        assert!(
            parse(
                &QUERY,
                &["--project", "p", "--cursor", "exact-boundary", "--offline"]
            )
            .switch("offline")
        );
        assert!(
            parse(
                &CONTEXT,
                &["--project", "p", "--input", "not-opened.json", "--offline"]
            )
            .switch("offline")
        );
    }
}
