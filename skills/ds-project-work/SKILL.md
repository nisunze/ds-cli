---
name: ds-project-work
description: "Read a named DS project's current work and recent activity, or track authorized delivery through tasks, records and assets. Use for a grounded project briefing as well as delivery; a read-only question needs no new task."
metadata:
  ds-chapters: project,assets,design
---

# Keep the project's work current while doing it

Use the installed `ds` or its MCP projection for a real deliverable on a named
DS project. Project Work is its default delivery record. The native credential
acts as the signed-in person; every headless command names the exact project.
Never switch identity or rely on a saved or visible project to redirect a
call. An ordinary answer to a read-only question needs no new task.
An MCP typed profile may omit Project Work leaves; use the `project` and
`correspondence` profiles or the broad chapter router when available.

## Brief the person on what happened recently

Name the exact project first. Read the live descriptors for `pm.plan`,
`pm.record.list`, `pm.record.read` and, if files matter, `assets.list`. A small
briefing starts with one bounded read:

```
ds pm plan --project <exact-id> --limit 20 --output json
```

Its `recent` rows name changed tasks; `attention` names blocked or late work;
the project revision and `recentTotal` show scope. These rows are pointers,
not descriptions of what was done. Read only the relevant tasks with
`ds pm task read --project <exact-id> --task <id> --timeline --output json`.
For decisions and handovers, ask for recent project notes, then read selected
bodies rather than inferring from their subjects:

```
ds pm record list --project <exact-id> --category project_note --since <yyyy-mm-dd> --limit 20 --page 0 --output json
ds pm record read --project <exact-id> --record <id> --output json
```

Use `ds assets list` in recent order only when the briefing needs an artifact
or version; read back the selected asset before claiming it was published.
State the time window, the source IDs, the difference between a task update and
a completed deliverable, and any list truncation. If these bounded sources do
not establish an answer, say what is missing. Do not scan the whole project,
guess from filenames, or create a task or note simply to answer a question.

1. Start with `ds capabilities pm --output json`, then the live descriptors
   for the commands you need. Read `pm.plan` for permissions, attention and
   field vocabulary. Search `pm.task.list`, then `pm.task.read` for a matching
   work item before creating one. Reuse its ID and inspect its outstanding
   residuals and records; similar titles alone do not prove a match. Every
   actual deliverable should resolve to a task and an exact submitted artifact.
2. For authorized project work with no matching item, create one scoped task
   with a clear completion criterion when the signed-in person may edit the
   plan. Otherwise use `pm.task.propose` for their own Inbox work and let the
   PM admit it. A design review thread becomes one linked task through
   `design.comment.promote`; a sourced review record can create a task through
   `pm.task.create` with `--from-record`, so the record lists its resulting
   task. Reuse the same task for the response, proof and submission. Use a
   stable ID for unattended create retries. Use `pm.task.assign` only with an
   active project member and the user's authorized assignment intent: a
   request leaves the current holder until someone accepts, while an owner
   transfer changes accountability directly. `pm.task.respond` answers only
   for the signed-in person. The server issues applicable notifications after
   a committed PM write; the assignment receipt alone does not prove delivery
   of a notification. Do not invent dates, assignees, approvals or work
   performed by someone else.
3. At meaningful milestones, use `pm.task.update` to record observed delivery
   and progress. Read the project's vocabulary before setting review or
   closeout. After verified submission, record the actual document revision or
   governed design version and review state; close only when the task's
   completion criteria and residuals permit it. Read the task again after a
   write; if the plan revision moved, re-read before deciding.
4. File actual external exchanges through `pm.record.create` or
   `pm.record.reply` against their real source asset, message or meeting note.
   Read `pm.party.list` and the record thread first. A submission or
   transmittal needs a document registered with `assets.classify`, then a
   `pm.record.create` carrying that document and its real source. An MV
   model-change submission delivers the set in
   [MV submission deliverables](references/mv-submission-deliverables.md):
   staking is XLSX only, never printed, and a dated folder holds only
   artifacts made for that change. A review
   response matrix file can be registered as a document asset; keep its exact
   document revision, asset ID and each actionable row or comment ID on the
   linked task or promoted design thread. Verify any matrix asset link by
   readback. Sourced review records and design comment threads carry the
   responses; there is no dedicated response-matrix command. Link an awaiting answer to
   the task with `pm.task.block` only when a record truly owes a response; a
   reply or justified waiver resolves that blocker. Filing a record does not
   send an email or message. Put design discussion on its object with
   `design.comment.post`. Use `pm.task.comment` for ordinary task context and
   `pm.task.comments` for readback. Keep a stable comment ID across retries;
   after an uncertain reply, find that ID before claiming delivery or choosing
   another ID. Task comments leave review state unchanged.
5. Use `ds assets` for the exact submitted file, asset ID and sensitivity.
   Register uploaded documents with `assets.classify` using their real
   document number, revision and state, and read back the row. `assets.versions`
   applies only where the source exposes versions, such as a projected design
   or model; an uploaded document does not gain a version chain by ingestion.
   Register and file documents using the live asset and record contracts;
   link a catalogue asset with `assets.attach` only if the route accepts that
   asset and task. The declared link route can refuse, and there
   is no free-standing `pm.task.attach` command. Read back the task or asset
   link before claiming an attachment, and preserve the file receipt on
   refusal. Use the applicable design/model version read where the source
   owns a version; never substitute a path, draft alias or guessed version
   for that artifact.
6. When a real dependency stops delivery, read the task and record its cause
   and the next condition for progress. If the project's vocabulary supports
   `blocked`, preserve the existing description while updating its delivery
   state and description. Use `pm.task.block` only when an actual
   correspondence record owes the answer; a different dependency is ordinary
   task state, not a fabricated record. Resume and remove the stale blocker
   note only after the named condition is verified. Keep task state tied to
   project delivery evidence and review decisions.

Stops at: a permission or authority refusal — the signed-in project member
or PM must grant the required role or make the decision named by `ds`; do not
route around it.
