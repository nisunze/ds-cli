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
restored user or device credential, for the audience-fenced project
`ds auth project use` selected. No pairing, no device link, no paired
application — `ds pm` answers on a bare Server exactly as it answers beside a
desktop (`ds auth status` shows the credential and the selected project).

What the graph MEANS is not decided here either. The plan, a task list, one
task, which project command a flag becomes and what the engine's answer
means are `ds_command_kernel::project_management` — one fold for the CLI,
MCP, the Server and the page, so the dashboard and the attention list cannot
disagree about the same task by host.

That is also why there is no `--project` flag anywhere in this domain. The
project is the one selected for this credential and lane; a project id passed
as an argument would be a claim `ds` has no standing to make. `--lane`
selects the native credential lane (`stable` by default, `canary` where the
box is linked on canary).

Until 2026-09-20 eight of these nine commands relayed through the paired
desktop (`requires: window`). That path is retired: `--desktop-descriptor`
is no longer an input, and a caller that still passes it is told
`requires_window_retired`.

## The shape of a session

```bash
ds pm plan                                   # what is this project, and what needs attention
ds pm task list --state blocked              # find the work item
ds pm task read --task T-0007                # read it, with its residuals and records
ds pm task update --task T-0007 --delivery in_progress --progress 40 --yes
```

`ds pm plan` is the cheapest place to start and the one that makes the rest
usable without further reading: it publishes the project's own **field-model
vocabulary**, and `--delivery`, `--review` and `--closeout` take their values
from it. The engine owns those lists. This CLI keeps no copy — a hardcoded list
is how a client once offered `task` for a node that was a milestone.

## Reads are one round trip each

A read fetches the selected project's canonical graph (`get_graph`) once and
folds it in the kernel; `ds pm record list|read` and `ds pm task read`
additionally fetch the project's context (`get_context`) — the records the
graph does not carry, and which the browser never asked for, so this is the
first `ds` that lists a project's records at all. The server answers at most
100 records per context read; a project past that lists with
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

## What is deliberately absent

**A messaging door.** Assigning work, answering a request and changing a
delivery state all *cause* notifications, and they flow through the canonical
notification spine as side effects of the governed action. What `ds` cannot do
is send a message: `messages-v1` is human-only, and a domain that could compose
one would be the same mistake as a domain that could run code inside the
application.

**Authoring a record, here.** `ds pm record list` and `ds pm record read` are
reads. Authoring one from the terminal is the correspondence contract's door
(`pm.record.create|reply`, ds-brain `docs/contracts/correspondence.md`), which
lands beside these two.

**A project id argument, a token, a window and a Firestore path.** See above.

## Refusals worth planning for

| Code | Means |
|---|---|
| `headless_signed_out` | no restored native credential on this lane — `ds auth login` or `ds auth link begin` |
| `headless_project_not_selected` | no project selected for this credential and lane — `ds auth project use --project <id>` |
| `project_not_visible` | the selected project is not one this account is a member of |
| `pm_refused` | ds-brain or the engine declined the command by name; `detail.service_message` says what |
| `work_not_permitted` | this user may read the plan but not change it |
| `work_revision_conflict` | the plan moved; re-read and decide again |
| `task_not_found` / `record_not_found` | no such id in the selected project |
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
