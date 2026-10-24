# `ds pm` — reference

Tier-4 reference. `ds pm <command> --help` is the contract; this document is
the part that does not belong in any command's help because it is true of all
of them.

## Where Project Management is

The governed plan remains with ds-brain; a verified native capture can also be retained for explicit offline reading.

The project's plan is a governed graph behind ds-brain, which is the only
gateway and the only authority: it decides who may write, it arbitrates two
people accepting the same assignment request in the same second, and it refuses
a command authored against a revision that has since moved. `POST /api/v1/pm`
is published on both gateway lanes and authenticates from the bearer alone, so
every command here is one governed action the native client sends under the
restored user or device credential, for the project named by this command's
required `--project`. No pairing or paired application is needed. The gateway
decides whether that credential may act on the named project.

What the graph MEANS is not decided here either. The plan, a task list, one
task, which project command a flag becomes and what the engine's answer
means are `ds_command_kernel::project_management` — one fold for the CLI,
MCP, the Server and the page, so the dashboard and the attention list cannot
disagree about the same task by host.

Every PM command requires `--project <exact-id>`. A saved CLI selection is
never consulted, so two interleaved commands for different projects keep their
own request context. `--lane` selects the native credential lane (`stable` by
default, `canary` where the box is linked on canary).

Until 2026-09-20 eight of these nine commands relayed through the paired
desktop (`requires: window`). That path is retired: `--desktop-descriptor`
is no longer an input.

## Retained plan reads

A normal `ds pm plan --project <exact-id>` still reads one current canonical
Graph through the authenticated PM authority and retains that exact capture.
`--offline` reads only its retained graph under the same protected UID, lane,
credential audience, credential instance and explicit project. It performs no
session restore, source acquisition or freshness request. Missing, denied,
corrupt or differently scoped holdings refuse; they never substitute an empty
plan or another account's capture.

The result's `retained` metadata preserves the captured graph/schedule revisions,
body SHA-256 and capture time. Offline source freshness is `unknown` and
permission freshness is `captured_only`; it grants no current write authority.
Graph revision alone is not a freshness basis for class-redacted correspondence,
permissions or live subject state. A local PM write attempt marks existing
holdings `known_invalidated`, including attempts that might partially commit;
access denial durably prevents offline reuse. No unused holding is created.
All writes retain their live authority path. Shared `pm note read --offline` also consumes one exact retained id/version/claim selection through this owner; a different selector requires its own intentional live acquisition. Context and query reads remain live.

## The shape of a session

```bash
ds pm plan --project <exact-id>                                   # what needs attention
ds pm member list --project <exact-id>                            # who may be assigned
ds pm task list --project <exact-id> --state blocked              # find the work item
ds pm task read --project <exact-id> --task T-0007                # read it
ds pm task update --project <exact-id> --task T-0007 --delivery in_progress --progress 40 --yes
```

`ds pm plan` is the cheapest place to start and the one that makes the rest
usable without further reading: it publishes the project's own **field-model
vocabulary**, and `--delivery`, `--review` and `--closeout` take their values
from it. The engine owns those lists. This CLI keeps no copy — a hardcoded list
is how a client once offered `task` for a node that was a milestone.

## Reads are one round trip each

A plan or task read fetches the named project's canonical graph (`get_graph`) once and
folds it in the kernel; `ds pm task read` additionally fetches the project's
context (`get_context`) for the records that reference the task. The record
commands read through the correspondence contract's own actions
(`record_list`, `record_read`, `record_thread`): one server filter, one
bounded page, the record's attachments and blocked tasks projected beside it.
The server scans at most 1000 records per list; past that the list says
`truncated: true`. Deletion inventory and read use their own server actions to
inspect durable backup snapshots without folding the live graph.
`ds pm member list` makes a separate bounded server read of the project-side
assignment roster, including each account's active and assignable status.
It does not infer membership from `pm plan.permissions.admin`: a platform
administrator can edit a plan without being listed on that project.

Graph and record reads are bounded and report their bounds. On task and record
lists, `--limit` is a page, the matched `total` is returned, and a page smaller
than the total says so rather than ending quietly. Deletion inventory uses an
opaque `next_cursor` instead of a total. Detail commands cap each related
collection at 250 rows and report its full `*Total`; task descriptions and
record bodies carry an explicit truncation flag when cut.

## Writes are the same governed commands the surfaces send

There is no second pipeline. Most task and record writes load the current
graph, build the same project command the Plan sheet would build, and commit
it under optimistic concurrency against the revision they were authored on.
Task deletion and backup restore take the explicit plan revision the operator
reviewed. They send that exact revision and a stable command id directly to
the server, so a lost response can be retried against its command ledger.

| | |
|---|---|
| Effect | `global_write` — dispatch requires `--yes` |
| Atomicity | `ds pm task update` sends every flag as ONE saved draft against ONE base revision: it all lands, or none of it does |
| Conflict | the plan moved while the command was in flight → re-read and decide again; nothing is merged silently |
| Warnings | an accepted change that pushed a dependency out is reported, never swallowed |

A write that the engine evaluated and declined answers `pm_refused` with the
engine's own sentence in `detail.service_message` (and its `violations`); a
200 with `applied: false` is never reported as a receipt.

### Retrying a create

`ds pm task create --id <task-id>` mints that exact id. Re-running the same
command with the same `--id` after a lost answer is refused as "already exists"
rather than creating the work item twice. Without `--id` an id is minted for
you, and a retry creates a second item — so pass one for any unattended use.

### Recovering a deleted Project Work task

A task or milestone deletion removes the selected item, its hierarchy
descendants, and their attached dependencies and residuals. An assigned
descendant refuses the cascade. The server saves an exact pre-delete backup
atomically with an accepted deletion; the result carries its `backup_id`.
Review the item and current plan before deleting:

```bash
ds pm task read --project <exact-id> --task T-0007 --output json
ds pm plan --project <exact-id> --output json
ds pm task delete --project <exact-id> --task T-0007 \
  --base-revision <reviewed-plan-revision> --command-id <stable-delete-id> --yes --output json
```

Recovery stays in the same named project and native credential lane:

```bash
ds pm deletion inventory --project <exact-id> --output json
ds pm deletion read --project <exact-id> --backup <backup-id> --output json
ds pm plan --project <exact-id> --output json
ds pm deletion restore --project <exact-id> --backup <backup-id> \
  --base-revision <reviewed-plan-revision> --command-id <stable-id> --yes --output json
```

`inventory` pages with `--limit` and the returned `next_cursor`; `read` can
open one captured document with `--item-key` from `summary.entries`. Restore
verifies every snapshot and commits the whole graph at the reviewed head, or
refuses it. A moved plan needs a new review and revision. Reuse the same
`--command-id` when a response is lost so the server replays that decision.
Likewise, retry a lost delete response with the exact original task, revision
and command id; do not create a second id for the same intended deletion.

### Attaching a file to a task

```bash
ds pm task attach --project <exact-id> --task T-0007 --file ./crossing-survey.pdf --yes --output json
```

One command puts one local file of any size (1 byte up to Cloud Storage's
5 TiB object maximum) on an existing task: the server reserves an attachment
row, the file streams from disk straight to storage in resumable chunks (never
held in memory, never through ds-brain), and the server checks the
stored size and type before the row turns `ready`. Only a ready row on the
named task is reported, with the local `sha256`. The media type is named from
the file extension unless `--content-type` gives one. Read a large attachment
back with `ds assets download --asset sys:pm_attachment:<id>`, which answers
its generation-pinned signed URL without fetching it.
There is no Assets ingest or separate link step.

An interrupted upload leaves its row `uploading`. Attaching the same file
(same name and size) to the same task again restarts that row's upload and
completes it, so a retry never adds a second row; the receipt says
`restarted: true`. Your other unfinished rows of that file (`uploading` or
`failed`) are then detached from the task and listed under `reclaimed`; any
that refuse are listed under `unreclaimed`. Only your own rows are touched.
To remove one unfinished row explicitly, read its id and `detail.version`
from its attachment entry in `ds pm task read --timeline` and run
`ds pm task detach --expected-version <that version>`.

`ds pm task create --attach` uses the same upload for each file. If one
fails, the task stays created and the refusal lists the ready receipts;
attach the remaining files with `ds pm task attach`, not a second create.

### Attaching a file to a record or project note

```bash
ds pm record attach --project <exact-id> --record R-0031 --file ./EPCGIS_1.RAR --yes --output json
ds pm note create --project <exact-id> --input note.json --attach ./EPCGIS_1.RAR --yes --output json
```

A record, including a project note, takes files through the same resumable
upload as a task. `record attach` reads the record first, so nothing is
reserved on an id that is not a record you may read. Attaching the same file
again after an interruption completes your unfinished row (`restarted: true`)
instead of adding one. A record has no relationship detach, so an older
unfinished row of the same file is left for the server's own reclaim of your
abandoned uploads. `ds pm record read` lists the record's own uploads under
`pmAttachments`, each with its `state` (`ready` is a delivered file,
`uploading` an interrupted one) and the `sys:pm_attachment:<id>` that
`ds assets read` and `ds assets preview` open.

`record create --attach` checks every file before filing anything, files the
record, then uploads each file and returns them under `uploads`. If one
fails, the record stays filed and `record_attachments_incomplete` carries its
`recordId` and the ready receipts; attach the remaining files with
`ds pm record attach`, never a second record. `ds pm note create --attach`
does the same for a project note (`note_attachments_incomplete` carries the
`noteId`); a note needs no source asset, so it is the record to create when a
file such as an approved reference archive is itself what is being recorded.

## Assignment is a request, not a decree

The three rules are the engine's, and this domain does not flatten them:

* **The current holder is untouched while a request is open.** Asking never
  orphans work, and a request everyone declines leaves the plan exactly as it
  was.
* **Several people may be asked, and the first to accept holds it.** That is how
  work actually gets picked up in a field organisation. `--request` repeats.
* **Declining removes only you.** The request stays open for everyone else, and
  it carries no reason — a required justification is how declining becomes
  socially expensive and therefore stops being real.

```bash
ds pm task assign  --task T-0007 --request pilot@example.com --request field@example.com --yes
ds pm task respond --task T-0007 --response accept --yes     # or decline
ds pm task assign  --task T-0007 --withdraw --yes            # cancel the open request
ds pm task assign  --task T-0007 --owner lead@example.com --yes   # direct transfer
```

`--owner` is the other, rarer thing: a transfer of accountability that keeps the
former holder as a collaborator rather than removing them from work they know
about.

`respond` answers as the signed-in native credential's user. There is no flag
for who is answering, because answering for somebody else is the one thing this
must not allow — and it is why a contributor who may not edit the schedule can
still run it.

Every person named must be an **active member** of the project; an elevated
platform account that can read every project is not thereby a member of any,
and the engine refuses the request by name (`pm_refused`, "every task assignee
must be an active project member"). Read `ds pm member list` before assigning.

## Checklist and issue items

`pm task checklist read` reads one named task through `task_read`, including
server-owned item states, promoted-child completion and the location overlay.
It returns at most 50 concise items by default; `--limit` admits up to 200 and
`--details` includes their bodies, answers, geometry and links. `more` explicitly
marks a shorter projection. No plan-wide read or background refresh is started.

```bash
ds pm task checklist read --project <exact-id> --task T4 --details --output json
ds pm task checklist edit --project <exact-id> --task T4 --input item-edit.json --id item-edit-001 --base-revision 17
ds pm task checklist promote --project <exact-id> --task T4 --item I2 --child T4-child-2 --id promote-001 --base-revision 18
```

The edit file is a closed item patch, at most 192 KiB. It contains `action`
(`add`, `edit`, `check`, `uncheck`, `move`, `remove`) and stable `item_id`.
Optional fields are `checklist_kind` (`checklist` or `issues`), `text`, `body`,
`answer` (check only), `geometry`, `clear_geometry`, `links`, `attachment_ids`
and zero-based `index` (add or move). Omitting a field preserves it; empty arrays
explicitly clear the selected item's links or asset references. Assets must
already belong to the task. Content belongs to add/edit, and move requires an
index. The server validates geometry, evidence, list state and permissions.

```json
{"action":"edit","item_id":"I2","body":"Field evidence verified","attachment_ids":["asset-1"]}
```

Edits and promotions send one existing `commit_command` without first reading
a graph. Preserve the exact `--id`, reviewed `--base-revision` and intent after
a lost reply; both the command id and committed revision must match the receipt.
There is no automatic rebase. Promotion creates the explicit child and retains
the item's evidence and location atomically. `--request <email>` uses existing
assignment requests and may be repeated.

## Existing task geometry

A task may carry stored WGS84 geometry and typed object links. Discover
`pm.task.geometry.read` to inspect them and `pm.task.geometry.clear` for an
explicitly confirmed removal. Clear preserves links unrelated to the stored
Grid geometry. Ordinary task creation, dry-run preview, and checklist operations
remain available with their existing project authority and revision fences.

Core no longer exposes geometry authoring from model objects:
`pm.task.geometry.set` and the `--geometry-from`, `--package`, and `--buffer-m`
inputs on task creation/checklist editing are absent. The
[restoration backlog](../../../ds-architecture/backlog/DECOUPLING.md#decoupling-004--restore-pm-geometry-authoring)
names the independent owner boundary required to restore that workflow.

## Which model version: exact DS Grid model links

A task, milestone or project note can name a whole DS Grid project model at
one exact catalog version. A catalog version stays open until a bump
supersedes it, so the link pins one immutable **catalog revision** and carries
that revision's version and `.dsgrid` digest, read from the catalog — never a
display name. ds-brain checks the pin again inside the Project Work write.
The contract is ds-brain
`docs/contracts/work-links-and-spatial-anchors.md` (DS Grid model versions).

```bash
ds pm model-link add    --task T4 --model <model-id> --version 3 --yes            # pins version 3's newest revision now
ds pm model-link add    --note N1 --model <model-id> --revision <revision-id> --yes
ds pm model-link list   --task T4 --output json                                   # links[]: model_id, revision_id, model_version, model_digest, attached_by/at
ds pm model-link list   --model <model-id> --output json                          # every task, milestone and note linking it
ds pm model-link remove --task T4 --model <model-id> --revision <revision-id> --yes
```

- `--version N` alone pins the newest revision of version N **as it stands
  now**; a later save into version N does not move the link. `--revision`
  pins exactly; with both, they must agree (`model_version_mismatch`).
- Two revisions of one model are two links; `remove` takes the exact revision
  and leaves the others. Adding a link that is already there changes nothing
  and keeps its first attribution.
- A link outlives retirement of its model: it can still be read and removed,
  but a retired model cannot be newly linked.
- Task geometry links (`dsgrid_structure`, `dsgrid_alignment`) are separate
  and untouched by model-link operations.
- Linking freezes nothing. This is not the design-version `vN` marker, and an
  immutable external submission stays a Project Work record.
- `list --model` reads project notes from the bounded context page;
  `notes_truncated: true` says the page was cut.

## A third party proposes; the PM admits

The plan is not only the schedule editor's to write. A member with the
project's ordinary contribution permission — a consultant, a contractor, a
surveyor — proposes work about themselves, and the PM decides
(`ds-brain/docs/contracts/task-proposals.md`):

```bash
ds pm task propose --title "Survey the Kabuga feeder extension" --hours 12 --days 3 --note "site visit" --yes
ds pm task request-admission --task <id> --yes           # a task made elsewhere, or after a decline
ds pm plan --output json                                 # .data.proposals[], .data.dashboard.proposalsPending
ds pm task admit   --task <id> --under <parent-task-id|root> --yes
ds pm task decline --task <id> --reason "out of scope this phase" --yes
ds pm task log-hours --task <id> --hours 4 --note "first day on site" --yes
```

`propose` is one gesture for two governed commands: it creates an Inbox task
with you as its responsible person and the estimate you state, then asks the
schedule editors to admit it. `admit` is the same reviewed reparent a drag on
the Plan sheet performs, plus the decision, in one commit; `decline` leaves
the task in the Inbox with the reason on it, and the proposer may revise the
estimate and ask again. The estimate (`--hours`, `--days`) and the append-only
hours log are the facts billing and duration learning read; nothing here
edits or deletes an hours entry, and only an assignee writes one.

These five commands run headless on the project each request names and never
through a window. Each takes `--id`: the same id replays the
ledger rather than repeating the write, which is how a lost answer is retried
safely — the minted id is in every receipt. `ds pm plan` flags a task whose
logged hours pass its estimate (`over_estimate`) beside the late ones, and
publishes the bounds (`vocabulary.maxEstimatedHours` …) the flags are checked
against before the round trip.

## What is deliberately absent

**A messaging door.** Assigning work, answering a request and changing a
delivery state all *cause* notifications, and they flow through the canonical
notification spine as side effects of the governed action. What `ds` cannot do
is send a message: `messages-v1` is human-only, and a domain that could compose
one would be the same mistake as a domain that could run code inside the
application.

**A record authored against nothing.** `ds pm record create` without a
source is refused: the Records-surface doctrine — the person writing it can
see what it will be attached to — holds in a headless world too.

**Setting `responded` by hand.** It is derived from a reply in the thread;
what a person may set is a waiver, with a reason, and it is not withdrawn.

**A token, a window and a Firestore path.** See above.

## Refusals worth planning for

| Code | Means |
|---|---|
| `headless_signed_out` | no credential is connected on this lane — `ds account connect`, approved in the Desktop |
| `missing_input` | `--project` was omitted; name the exact project on this call |
| `project_not_visible` | the named project is not one this account may access |
| `pm_refused` | ds-brain or the engine declined the command by name; `detail.service_message` says what |
| `work_not_permitted` | this user may read the plan but not change it |
| `work_revision_conflict` | the plan moved; re-read and decide again |
| `task_not_found` / `record_not_found` | no such id in the named project |
| `invalid_choice` | a state, priority, type, placement or scheduling value outside the vocabulary `ds pm plan` publishes |
| `nothing_to_update` | an update with no change flag — refused before a round trip |
| `invalid_assignment` | `--request`, `--owner` and `--withdraw` are three different intents |
| `invalid_date` | a schedule flag that is not `YYYY-MM-DD` |
| `invalid_task_shape` | a child/root/milestone was given contradictory parent or date flags |
| `model_link_source_invalid` | `model-link` needs exactly one `--task` or `--note`, and a note must be a project note |
| `model_link_invalid` | a model or revision id is not an exact catalog id, neither `--revision` nor `--version` was given, or the item holds 128 links |
| `model_version_not_found` | the project's catalog has no such model, revision or version |
| `model_version_mismatch` | `--version` disagrees with the catalog version of `--revision` |
| `attachment_file_invalid` | `--file` is unreadable, empty, above the 5 TiB storage maximum, changed while it was read, or unnamed, or `--content-type` is not one `type/subtype` |
| `attachment_unverified` | the server did not verify a ready attachment on the named task or record; read it before repeating |
| `record_attachments_incomplete` | `record create --attach` filed the record but one upload was not verified ready; attach the rest with `ds pm record attach` |

`--start 01-09-2026` is refused here rather than at the engine on purpose: a
transposed day and month is the commonest scheduling mistake there is, and
`2026-01-09` for the ninth of September is a perfectly valid date that quietly
schedules the wrong week.


The authenticated collaboration index is also available headlessly:
`ds pm project list --status active --limit 50 --output json`. It returns the
existing public Project Work index independently of the machine's selected
project. Repeat with the exact `next_cursor` as `--cursor`; statuses are
`active`, `archived` and `testing`. Assignment still requires an eligible member.

Use `ds pm task comments --project <id> --task <task-id> --limit 50 --output json`
for a task's existing context comments; follow `next_cursor` with `--cursor`.
`ds pm task comment --project <id> --task <task-id> --id <stable-comment-id>
--body "Review text" --yes` adds an ordinary comment of at most 4000 UTF-8 bytes.
This uses the existing comment action and leaves review state unchanged. After
an uncertain reply, read comments and look for the same id before choosing a new
id; duplicate creation is refused without a second comment.

## Selected note checklists and scoped copies

`ds notes checklist read --note <id>` reads one account-private note;
`ds pm note checklist read --project <id> --note <id>` reads one project note.
Both return the owner's `kind`, `items`, `total`, `completed` and note `version`.
`--limit` is 1–200 (default 50), with explicit `more`; `--details` includes item
body, answer, geometry, links and the canonical overlay. These reads acquire
only the selected note, without graph, thread, attachment or asset queries.

Each family's `checklist edit` uses `--note`, `--input <json>`, `--version`,
`--id` and `--yes`; project notes also require `--project`. The closed JSON
item edit is the same `add|edit|check|uncheck|move|remove` contract as task
checklists. The command sends one write without an automatic preliminary read.
Keep the exact version, id and payload after an uncertain reply. A new stale
edit refuses; an exact replay returns its original version receipt. Project
model pins cannot be copied into account-private notes. Asset references must
already be held by the note; this control does not ingest a file implicitly.

`ds pm document copy --project <id> --input copy.json --id <command-id>
--base-revision <revision> --dry-run` previews a selected scoped copy. The JSON
contains `source_kind`, `source_id`, `source_version`, `target_kind`,
`target_id`, and explicit `attachment_ids` or `exclude_attachments`; kinds are
`task`, `project_note`, `personal_note`. A task source pins its graph revision;
a note source pins its version. A task destination requires the reviewed
`--base-revision`; use 0 for note-only copies. Apply with `--yes` and without
`--dry-run`. The source remains intact. Privacy, selected assets and portable
body/checklist/geometry are revalidated atomically by the owner. Private to
project files require the existing ingest boundary; a refusal never silently
drops evidence. An exact replay, including a later preview of that same
intent, may return its original applied receipt.

`ds pm mail-thread ingest --project <id> --input mail.json --id <command-id>
--dry-run` previews explicitly selected, already-ingested EML assets. Input
contains `record_id`, exact `message_count` (1–32), and `sources` with
`asset_id`, `version`, lowercase `sha256` and optional `generation`. Apply
with `--yes`. The owner checks complete MIME/header lineage and returns an
immutable correspondence record and source-pinned manifest with exact message
and part counts. Truncated metadata, missing sources or unselected references
refuse. The command never fetches a mailbox, URL or an unselected attachment.

These controls acquire data only for the selected consuming operation.
Constructing or describing an intent makes zero API calls. Note edits, copy
previews/applies/replays and mail previews/applies/replays each use one fixed
owner request, with no automatic catalogue, graph, asset or thread acquisition.

The typed MCP profile `project-checklists` exposes these six note/copy/mail
controls and the three task checklist controls from the same descriptors.
Use `ds mcp serve --exposure commands --profile project-checklists`; it keeps
the broad `project` profile at its existing tool budget.


## Shared Project Notes

`pm task read --offline` uses the last matching captured task and optional timeline.
`pm note query --offline` and `pm note context --offline` use their last exact
captured request/page. Filters, cursor, scope, limit and project remain exact
selectors; changing them requires intentional acquisition. Each response keeps
its independently observed source revisions, partial/migration-needed flags,
capture-time permission scope and unknown remote freshness. Offline operation
restores no session and makes no API request. Normal online acquisition remains
separate from the general cheap `isFresh` integration obligation.

Shared creation uses the one canonical `--input` form. The older title/body
creation route is retired. Ordinary current reads and writes require canonical
memory; missing memory returns a named migration-required refusal rather than
interpreting text or silently capturing an older head.

The one-off canonical migration has completed and its command surfaces are
retired. Stored adoption receipts remain representation provenance; they do not
renumber authored versions or turn literal observations into agreed claims.
Unclassified observations carry no inferred claims or project-wide authority.
Exact old plain versions remain readable as literal documents with semantic
memory unrecorded; selecting a claim from them refuses. Queries encountering
unadopted rows mark migration needs and incomplete results instead of silently
turning plain text into decisions. Use a later explicit authored update to record
properly sourced claims and scope.

For subnotes, set `draft.memory.parent` to an exact `{"id":"parent-note","version":1}` in the existing typed creation/update JSON; omit it for a root or explicit detach. Updating still requires the reviewed `expected_version` of the child; a newly selected/changed parent pin requires the current version of the direct parent. An unchanged historic pin remains intact. Read direct children with `ds pm note query --parent parent-note --project PROJECT --output json`. The bounded page can be incomplete; it is not a recursive subtree or authority inheritance.

Use notes actively to retain scoped user rulings, corrections, observations and
verified outcomes. Recording is deliberate: commands do not file notes or fetch
context in the background. A proposal or inference is not a user decision.

`ds pm note create --project <id> --input decision.json --yes` accepts a closed
Write request (no transport envelope). This minimal example records an explicit
user instruction; substitute the actual instruction, author, scope and evidence:

```json
{
  "operation_id": "stake-ruling-20261013",
  "id": "stake-fill-in-rule",
  "reason": "Record the user's explicit staking-table rule",
  "draft": {
    "subject": "MV fill-in transformer supply",
    "body": "Fill-ins may receive replacement protection, but no transformer supply entry.",
    "tags": ["MV", "staking"],
    "memory": {
      "schema": "ds.project-note-memory/v1",
      "note_type": "constraint",
      "lifecycle": "active",
      "provenance": {"kind": "user_instruction", "source_author": "Project owner"},
      "scope": {"kind": "workstream", "workstream": "MV staking"},
      "evidence": [{"kind":"external","locator":"owner-conversation:Gisagara:fill-in-supply"}],
      "claims": [{
        "id": "no-fill-in-supply",
        "kind": "constraint",
        "issue_key": "fill-in-transformer-supply",
        "text": "Do not supply transformers at fill-ins.",
        "provenance": {"kind": "user_instruction", "source_author": "Project owner"},
        "evidence": [{"kind":"external","locator":"owner-conversation:Gisagara:fill-in-supply"}]
      }]
    }
  }
}
```

For `note update`, retain the id, provide a new operation_id and the reviewed
`expected_version`. An exact replay retains the original receipt; a deliberate
revision creates the next version. Use `note read --note <id> --version <n>
--claim <id>` to retrieve one preserved ruling. Never silently overwrite a stale
version.

Memory `note_type` is session_summary, decision, constraint, correction,
rejected_assumption, observation, proposal, mutation_outcome or
context_consolidation. Claim `kind` is instruction, decision, constraint,
rejected_assumption, correction, observation, proposal or inference. Each claim
requires its own `provenance`, whose kind is user_instruction, user_decision,
observation, proposal or inference; optional source_author, agent_label and
session_ref are self-reported. Authenticated author and time are server-owned.

Scope kinds are project, design, asset, workstream, structures, conditions or
preference. Optional selectors are workstream, object_refs
(`{"kind":"lv_transformer","id":"<exact-id>"}` or mv_model), asset_ids,
structure_ids, conditions and user_ref. These are exact identities or bounded
conditions, not geometry or permission overrides. Evidence entries use kind
note/record with id and positive version; asset uses id, asset_version and exact
sha256 (optional actual revision_id); external uses locator and is explicitly
unverified, never fetched by a routine context read. Supersession refs retain
note_id, version and optional claim_id. Do not invent evidence identities.

`ds pm note context --project <id> --input context.json` consumes a closed query,
for example:

```json
{"terms":["transformer supply"],"tags":["staking"],"tag_match":"any","lifecycle":["active"],"sort":"updated","direction":"desc","scope":{"kind":"workstream","workstream":"MV staking"},"limit":20}
```

It returns active applicable claims and potential conflicts with exact source
versions. A truncated/incomplete page cannot prove absence; newest does not win.
`note query` provides terms, any/all tags, type/status and created/updated sort
flags. `note versions` lists recoverable history.

Lifecycle writes each consume a separate JSON file: backup has operation_id and
sources `[{"id":"<note>","version":1}]`; retire adds backup_id, verified
backup_sha256, lifecycle (superseded/withdrawn/discarded/irrelevant/archived), reason
and optional replacement `{id,version}`. Restore has operation_id, backup_id,
backup_sha256, sources and expected_versions. Consolidate has operation_id, id,
reason, an active context_consolidation draft and exact sources. Consolidation
leaves source notes intact; retirement requires a verified recoverable backup.

Voluntary attribution uses draft.authoring_model with only known model, version
and/or strength, or the supported write's --authoring-model,
--authoring-version and --authoring-strength flags. Each supplied label is trimmed
and at most 200 UTF-8 bytes; omit unknown values. For example,
`"authoring_model":{"model":"GPT-6"}` belongs beside draft.subject, not inside
human provenance. Output labels it source:self_declared, not verified identity.
Read commands have no attribution flags. A later writer's omitted declaration
is not inherited from an earlier model. Discover each command before use;
unsupported owner paths refuse declarations rather than silently dropping them.

## Deliberate private note authoring

Use shared `ds pm note create --project PROJECT` for project decisions,
constraints and verified delivery outcomes. Use `ds notes create` only when
choosing to keep the material account-private. No command files notes
implicitly or copies private content into a project.

```sh
ds notes create --input private-note.json --yes --output json
ds notes read --note NOTE --output json
ds notes update --note NOTE --version 1 --input complete-replacement.json --yes --output json
```

The JSON is the complete authored draft, without a transport envelope:

```json
{"title":"Personal follow-up","content":{"format":"markdown","markdown":"Review the field photographs."},"checklist":[{"id":"review","text":"Review photographs","checked":false}]}
```

Allowed draft fields are title, folder, content, checklist, checklist_kind,
tags, links, geometry, reminder, canvas, pinned and archived. Markdown is at
most 128 KiB; the whole request is at most 512 KiB. Server-owned identities,
versions, timestamps, authors, trash and attachment state are not draft fields.
Private links cannot adopt DS-object pins. Attachment references must already
be ready on the selected existing note; creation cannot reference attachments.

Update is a full authored replacement: omitted authored fields clear or take
owner defaults. Read the selected note first and retain the fields you need.
Legacy simple checklist rows preserve rich fields of matching items; explicit
rich clears use the item control. The existing reminder behavior applies to
an authored reminder; this command does not invent a new scheduler. Private
authoring metadata is not added to a schema that does not store it.

Create allocates a new identifier and is not idempotent. If the outcome is
uncertain, inspect the explicit private list before repeating creation. After
an uncertain update, read that note: the expected-version fence prevents an
old reviewed revision overwriting a new one.

## Private metadata pages

`ds notes list --limit 50` requests one bounded metadata page, not the full
workspace or note bodies. Use the returned `next_cursor` with `--cursor` for
another explicit page. Cursors bind the authenticated account and query flags;
there is no eager next-page acquisition or total-count query. Stable ID order
is a live traversal, not an immutable collection snapshot: a concurrent insert
before the current cursor requires a fresh first-page read to discover it.

`--term` matches title, folder and tags only within that page. The result marks
that search scope and keeps acquired, scanned, matching, displayed and remaining
source-page information distinct. An empty filtered page can still carry a
continuation cursor. Read the selected note explicitly for its body or evidence.
Later UI infinite scrolling should consume this same cursor contract.

## Canonical private-note records

Ordinary selected reads and mutations require the canonical stored ID, geometry
representation and history baseline. A named `notes_migration_required` refusal
requires administrator recovery from verified source evidence; reads do not heal
older data. Metadata pages remain available and mark `migration_required`
without acquiring note bodies. The one-off migration commands are retired.

Retained migration receipts describe representation changes without changing
an authored version, actual authorship, timestamps, geometry, reminders or
evidence. The observed baseline can report earlier history as `not_assessed`;
no current read or mutation invents past writers or a complete history chain.

## Private history and backup pages

`ds notes versions --note NOTE --limit 50` acquires one immutable-header page.
Use its `next_cursor` explicitly for the next page. The cursor retains the first
page's source-head version, so a new revision cannot intrude into an older
traversal. `current_version` remains separately observed. Chain status is
complete, incomplete or unknown; continuation alone cannot prove the entire
history. Historical bodies require a selected version read.

```sh
ds notes backup --note NOTE --version 3 --id OPERATION --yes --output json
ds notes backup read --note NOTE --backup BACKUP --backup-sha256 SHA --out /absolute/private-backup.json --output json
ds notes backup restore --note NOTE --version 4 --backup BACKUP --backup-sha256 SHA --id RESTORE_OPERATION --yes --output json
```

Backup captures and independently verifies every available revision within
512 KiB and the 400-revision recovery write budget. Larger archives refuse as
a whole. The artifact omits the internal owner UID; original revision hashes
remain server pins, and the public archive has its own measured SHA. Export
writes exact checked bytes to a new owner-only JSON file; existing files remain.
Without `--out`, read returns the descriptor rather than a large private dump.

Restore selects an existing verified server backup for the same existing note,
recovers genuinely retained missing history, checks immutable collisions and
appends the next current version. It keeps current identity, creation attribution,
archive organization and operational reminder. It does not import an arbitrary
file, migrate accounts, fabricate missing history or acquire attachment bytes.
The stable operation id enables exact retry without duplicate backup/restoration.
