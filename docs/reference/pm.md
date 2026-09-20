# `ds pm` — reference

Tier-4 reference. `ds pm <command> --help` is the contract; this document is
the part that does not belong in any command's help because it is true of all
of them.

## Where Project Work is

Not on disk, and not reachable with a credential this process holds.

The project's plan is a governed graph behind ds-brain, which is the only
gateway and the only authority: it decides who may write, it arbitrates two
people accepting the same assignment request in the same second, and it refuses
a command authored against a revision that has since moved. So every command
here is one named semantic operation the *paired application* performs under the
session it already holds. `ds` sends a request and receives an outcome. It never
receives a credential, and it never runs code inside the application —
`docs/reference/desktop.status.md` has the pairing argument in full.

That is also why there is no `--project` flag anywhere in this domain. The
active project is the one the application has open; a project id passed as an
argument would be a claim `ds` has no standing to make.

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

## Reads cost the project nothing extra

A read paints the same shared Project Work surface the application's own
launchers read: cache first, reconciled once, no polling. A CLI session
therefore adds no project reads to a plan the application already has open.

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

After a successful write the shared surface is brought forward, so the
application's launchers do not keep painting one revision behind a change this
process just made.

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

`respond` answers as the application's signed-in user. There is no flag for who
is answering, because answering for somebody else is the one thing this must not
allow — and it is why a contributor who may not edit the schedule can still run
it.

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

These five commands run headless on the CLI-selected project (`ds auth project
use`) and never through a window. Each takes `--id`: the same id replays the
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
application. `tests/bridge_parity.rs` asserts it, in both the CLI and the
application's allowlist.

**Authoring a record.** `ds pm record list` and `ds pm record read` are
reads. A record is authored on the Records surface, where the person writing it
can see what it will be attached to.

**A project id argument, a token, and a Firestore path.** See above.

## Refusals worth planning for

| Code | Means |
|---|---|
| `desktop_not_paired` | no DS GridDesign session on this machine |
| `desktop_signed_out` | running, but signed out or with no project open |
| `work_not_permitted` | this user may read the plan but not change it |
| `work_revision_conflict` | the plan moved; re-read and decide again |
| `nothing_to_update` | an update with no change flag — refused before a round trip |
| `invalid_assignment` | `--request`, `--owner` and `--withdraw` are three different intents |
| `invalid_date` | a schedule flag that is not `YYYY-MM-DD` |
| `invalid_task_shape` | a child/root/milestone was given contradictory parent or date flags |
| `task_already_placed` | asked for admission of a task already in the WBS |
| `admission_already_requested` | the task already awaits the PM |
| `admission_not_requested` | admit/decline on a task with no open request |
| `parent_unknown` | `--under` names no task of this project |
| `not_task_proposer` / `not_task_assignee` | the request or the hours are somebody else's to write |
| `invalid_hours` | `--hours`/`--days` negative, malformed, or past the published bound |
| `proposal_created_not_requested` | the task was created but the request refused; `request-admission` finishes it |

`--start 01-09-2026` is refused here rather than at the engine on purpose: a
transposed day and month is the commonest scheduling mistake there is, and
`2026-01-09` for the ninth of September is a perfectly valid date that quietly
schedules the wrong week.
