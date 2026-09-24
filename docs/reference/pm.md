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

## Correspondence: parties, records, threads, blockers

The correspondence half of Project Management (ds-brain
`docs/contracts/correspondence.md`) is what turns an email, a letter, a call
or a transmittal into an object the plan can wait on. Four rules carry it:

1. **A record is always authored against something.** `ds pm record create`
   takes exactly one of `--source-asset` (the ingested .eml, screenshot or
   letter — `ds assets ingest` first), `--source-message` (an in-app
   message) or `--meeting-note` (a `note` asset); a reply is authored against
   the record it answers (`--reply-to`). A record with none of these is
   refused, `record_source_required`. The record inherits the strictest
   sensitivity of the assets it names.
2. **Parties are ids.** `ds pm party create --name … --kind organisation|person
   --role client|consultant|contractor|supplier|authority|other` once; then
   `--party <id>` on records. A duplicate name of the same kind is
   `party_exists`. `ds pm party list` is the directory, per project.
3. **Naming who owes the answer is owing a response.** `--response-owed-by
   <party-id>` or `<member-email>` with an optional `--response-due
   YYYY-MM-DD`. The response status is derived, never set: `outstanding`,
   `overdue`, `responded` (a later record in the thread with the opposite
   direction, from the owing side), `waived` (`ds pm record update --response
   waived --reason …`). `ds pm record list --awaiting` answers "who owes us
   an answer"; `--overdue` answers "what is late"; `--party` narrows to one
   counterparty.
4. **A task blocked on a record clears itself.** `ds pm task block --task T
   --on-record R` while R's answer is owed (`record_not_awaiting` otherwise).
   The moment a reply from the owing side lands, or the answer is waived,
   the blocker clears in the same commit — the plan's attention and the
   task's `awaitingCorrespondence` count move with it. `ds pm task unblock
   --reason` clears it by hand; `ds pm task read --timeline` shows the
   blocker set and cleared.

`ds pm task create --from-record R` makes a task that points back at the
record it answers; the record lists it under `resultingTasks`. A
`submission` or `transmittal` record needs at least one `--document` that
is a registered document (`ds assets classify --document-number …
--document-revision … --document-state …`).

`ds pm plan` publishes the correspondence vocabularies beside the task
ones — `channels`, `recordCategories`, `recordStates`, `recordDirections`,
`responseStatuses`, `blockerKinds`, `blockerClearedBys`, `partyKinds`,
`partyRoles` — and a `correspondence` section: the counters
`recordsOutstanding`, `recordsOverdue`, `tasksAwaitingCorrespondence`, and
the attention rows grouped by the party the answer is owed to (overdue
first, then due within seven days, with every task blocked on each). It is
`null` on a server that predates the contract, which is not the same answer
as nothing awaited.

Everything about a thread is indexed under **Assets › Correspondence**:
`ds assets tree --folder Correspondence` lists one folder per thread with
the .eml it was filed from (and its MIME parts), its registered documents,
every asset attached with `ds assets attach --record`, and the external
references among them. See `docs/reference/assets.md`.

```bash
# the shape of a thread, with synthetic names
ds pm party create --name "Acme Consultancy" --kind organisation --role consultant --yes   # → p_acme
ds assets ingest --path ./review.eml --folder correspondence/2026-09 --sensitivity internal --yes   # → a_mail
ds pm record create --category review --channel email --direction inbound \
  --subject "MV plan and profile — not approved" --happened-at 2026-09-14 \
  --party p_acme --source-asset a_mail --affects scope,schedule --yes   # → R1
ds pm record reply --reply-to R1 --category request_for_information --direction outbound \
  --subject "Questions on the span standard" --response-owed-by p_acme --response-due 2026-09-24 --yes   # → R2
ds pm task create --from-record R1 --kind parent --title "Address the review" --yes           # → T0
ds pm task block --task T0 --on-record R2 --yes
ds pm plan --output json      # .data.correspondence: R2 outstanding under Acme, T0 blocked
ds pm record reply --reply-to R2 --category response --direction inbound --party p_acme \
  --subject "Re: Questions" --source-asset a_reply --yes                       # clears T0's blocker
ds pm task read --task T0 --timeline --output json                            # blocker cleared_by: response
```

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
| `record_source_required` | a record named no `--source-asset`, `--source-message` or `--meeting-note` and is not a reply |
| `invalid_response_owner` | both a party and a person owe the answer, or the email is not a signed-in member |
| `record_not_awaiting` | the record owes no answer, so a task cannot wait on it |
| `party_exists` / `party_not_found` | a duplicate (kind, name) or `--id`; a party id the project does not hold |
| `document_required` / `document_not_registered` | a submission/transmittal without a `--document`; a document asset not registered |
| `thread_mismatch` | a reply asserted a thread that is not its parent's |
| `blocker_exists` / `blocker_not_found` | the task is already blocked on the record; or not blocked at all |
| `record_exists` | the same `--id` twice |
| `response_not_settable` | withdrawing a waiver, or setting `responded` by hand |
| `bound_exceeded` | a thread past 500 records, or a field past its bound (`detail.what`, `detail.bound`) |
| `asset_not_found` | a source, note or document asset the signed-in user may not read |
| `invalid_stamp` | `--happened-at`/`--since` is neither a day nor an RFC 3339 instant |

`--start 01-09-2026` is refused here rather than at the engine on purpose: a
transposed day and month is the commonest scheduling mistake there is, and
`2026-01-09` for the ninth of September is a perfectly valid date that quietly
schedules the wrong week.
