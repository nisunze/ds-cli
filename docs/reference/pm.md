# `ds pm` — reference

Tier-4 reference. `ds pm <command> --help` is the contract; this document is
the part that does not belong in any command's help because it is true of all
of them.

## Where Project Management is

Not on disk, and not behind a window.

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
is no longer an input, and a caller that still passes it is told
`requires_window_retired`.

## The shape of a session

```bash
ds pm plan --project <exact-id>                                   # what needs attention
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

A read fetches the named project's canonical graph (`get_graph`) once and
folds it in the kernel; `ds pm task read` additionally fetches the project's
context (`get_context`) for the records that reference the task. The record
commands read through the correspondence contract's own actions
(`record_list`, `record_read`, `record_thread`): one server filter, one
bounded page, the record's attachments and blocked tasks projected beside it.
The server scans at most 1000 records per list; past that the list says
`truncated: true`.

Every read is bounded and every bound is reported. On list commands, `--limit`
is a page, the matched `total` is always returned, and a page smaller than the
total says so rather than ending quietly. Detail commands cap each related
collection at 250 rows and report its full `*Total`; task descriptions and
record bodies carry an explicit truncation flag when cut.

## Writes are the same governed commands the surfaces send

There is no second pipeline. A write loads the current graph, builds the same
project command the Plan sheet would build, and commits it under optimistic
concurrency against the revision it was authored on.

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
must be an active project member").

## Where the work is: task geometry from DS objects

A task carries one optional top-level WGS84 geometry (Point, LineString or
Polygon) and typed links. Nobody draws it from the CLI: the person or agent
that read a comment naming DS Grid structures supplies **typed references**,
the kernel resolves them deterministically against the model and shapes the
geometry, `--dry-run` shows the proposal, `--yes` writes it. Nothing parses
prose anywhere in the stack. The contract is
`ds-command-kernel/docs/contracts/task-geometry-from-objects.md`.

```bash
ds pm task create --title "Swamp crossing at 74/76/77" --kind inbox \
   --geometry-from dsgrid:local-<id>:structure:74,76,77 --dry-run --output json   # proposal, nothing written
ds pm task create --title "Swamp crossing at 74/76/77" --kind inbox \
   --geometry-from dsgrid:local-<id>:structure:74,76,77 --yes                     # task + geometry + 3 links, one revision
ds pm task geometry read  --task T4 --output json                                 # geometry, objectLinks[], subject_state
ds pm task geometry set   --task T4 --from dsgrid:local-<id>:alignment:aln-1:74..77 --dry-run
ds pm task geometry clear --task T4 --yes
```

| Reference | Resolves to |
|---|---|
| `dsgrid:local-<id>:structure:74,76,77` | structures by engineering number (then by id) of the working copy `<id>` in this machine's catalogue (`ds dsgrid model list`, the session's lane and account) |
| `dsgrid:package:structure:74` with `--package <path>` | the same, from a `.dsgrid` file — e.g. a head taken with `ds dsgrid project download` |
| `dsgrid:<src>:alignment:<id-or-label>` | the alignment's whole route |
| `dsgrid:<src>:alignment:<aln>:74..77` | the route between the two structures, by station |

| Resolved | Shape |
|---|---|
| one structure | `Point` |
| several structures | `Polygon` — their convex hull buffered by `--buffer-m` (default 25, 1..500) |
| an alignment / a range | `LineString` |

Every resolved object becomes one `ds_object` link (`object_type`
`dsgrid_structure` / `dsgrid_alignment`, `entity_id` `<model id>:<object id>`,
`object_revision` the package revision and fingerprint it was read from), so
the plan says which structures the area came from. `set` keeps the task's
other links; `clear` removes the geometry and only the DS Grid object links.

A transformer is linked as a typed subject through the work-template flow and
a survey entry through a `survey_entry` link; neither is shaped into the
task's geometry, by the links contract — a `transformer:` or `survey:`
reference is refused as `reference_invalid` and says so.

The write is the ordinary `create_task` / `update_task_fields` command every
other `ds pm` write sends, against the revision it read, through the same
headless door.

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
| `requires_window_retired` | `--desktop-descriptor` was passed; drop it |
| `nothing_to_update` | an update with no change flag — refused before a round trip |
| `invalid_assignment` | `--request`, `--owner` and `--withdraw` are three different intents |
| `invalid_date` | a schedule flag that is not `YYYY-MM-DD` |
| `invalid_task_shape` | a child/root/milestone was given contradictory parent or date flags |
| `reference_invalid` | a `--from` / `--geometry-from` is not `dsgrid:<local-<id>\|package>:structure:…` or `:alignment:…`; `detail.part` names the piece |
| `model_unknown` | the working copy is not in this machine's catalogue, or `package` was referenced without `--package` |
| `object_unresolved` / `object_ambiguous` | a number, id or label is not in the model, or names two objects (`detail` lists them) |
| `references_incompatible` | structures and an alignment, or two alignments, in one proposal |
| `too_many_objects` / `geometry_too_large` / `links_bound_exceeded` / `buffer_out_of_range` | a bound, with its number |

`--start 01-09-2026` is refused here rather than at the engine on purpose: a
transposed day and month is the commonest scheduling mistake there is, and
`2026-01-09` for the ninth of September is a perfectly valid date that quietly
schedules the wrong week.
