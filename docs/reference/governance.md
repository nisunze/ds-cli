# `ds governance architecture`

The governed architecture planning graph, reachable without opening the map.

Contract of record: `ds-web/docs/specs/DYNAMIC_ARCHITECTURE_PLANNING_CONTRACT.md`.
This page documents only what `ds` itself owns — the commands, the proposal
document, and how each refusal is named.

```text
ds governance architecture get
ds governance architecture list [--chapter <id>] [--state <delivery-state>] [--include-archived]
ds governance architecture history [--limit 20] [--cursor <cursor>]
ds governance architecture preview --command <proposal.json>
ds governance architecture apply --command <proposal.json> --expected-revision <N> --yes
```

Every command takes `--desktop-descriptor <path>` when more than one DS
GridDesign session is running.

## Authority

`ds` holds no credential. Each command names one closed operation on the paired
DS GridDesign bridge, and the application performs the ds-brain call under the
user it has already signed in — the same authority the UI's edit mode uses. The
graph is platform-wide, so a signed-in session is required and a selected
project is not.

Editing needs the platform administration capability. A caller without it is
refused `architecture_not_permitted`, class `unauthorized`, exit 4.

## The wire

One ds-brain door, `POST /api/v1/architecture/planning`, over the closed action
set `get | list | history | preview | apply`. The arguments a command sends
**are** that request body, `action` included, so nothing between `ds` and the
server re-spells a key. One bridge operation per action:

| Command | Bridge operation | Body keys |
|---|---|---|
| `get` | `governance.architecture.get` | `action` |
| `list` | `governance.architecture.list` | `action`, `chapter`, `state`, `include_archived` |
| `history` | `governance.architecture.history` | `action`, `limit`, `cursor` |
| `preview` | `governance.architecture.preview` | `action`, `expected_revision`, `idempotency_key`, `command` |
| `apply` | `governance.architecture.apply` | `action`, `expected_revision`, `idempotency_key`, `command` |

A key a command's operation does not declare never leaves the process, so
`list` can never carry a command and `apply` can never carry a filter.

An optional filter the caller did not pass is **omitted**, never sent as `""`
or `false`: an absent `chapter` means every chapter, while an empty one asks
for the chapter whose id is the empty string.

Responses are read permissively. Each reply is checked for the fields its
action promises and is otherwise passed through verbatim, so a field a newer
server adds reaches the caller intact rather than failing to parse.

### What the paired adapter should return

On success, ds-brain's 200 body verbatim.

On a refusal, ds-brain's error envelope — `{"error": "<code>", "message": "…"}`,
plus `violations[]` for `validation_failed` and `expected` / `current` /
`snapshot` for `revision_conflict`. Returning that envelope as the operation
result is what carries the per-violation lines and the moved head through to
the caller; a refusal thrown as prose still reaches the right named code and
the right remedy, but arrives without those structured fields.

## The proposal document

```json
{
  "expected_revision": 12,
  "idempotency_key": "stable-command-id",
  "command": {
    "id": "stable-command-id",
    "kind": "update_node",
    "target_id": "survey-form-factory",
    "node": { "delivery_state": "user_question" }
  }
}
```

`command` is required and must carry a trimmed `id`, a `target_id`, and a
`kind` from `add_node | update_node | archive_node | link_edge | update_edge |
unlink_edge`. Everything else inside `command` — `node`, `edge` and whatever
the graph vocabulary grows — travels **verbatim**: `ds` does not subset the
authority's own model.

The three top-level keys are the only ones accepted. A misspelled
`expected_rev` is refused rather than ignored, because a silently dropped fence
is an unfenced apply.

`expected_revision` is optional in the file. `preview` uses it when present and
otherwise reads the head immediately before validating. `apply` always requires
`--expected-revision`; if the file also names one and the two disagree, the
apply is refused with `proposal_revision_mismatch` rather than one silently
winning.

### The idempotency key

`idempotency_key` is optional. When absent, `ds` derives one as
`sha256:<hex>` over the canonicalized `command` — object keys sorted
recursively, so re-indenting or reordering the file does not mint a new key for
a command that has not changed.

**It is never random, and that is the point.** A retry after a network failure
must reach the server as the same command it may already have committed, so it
is answered `applied: false` instead of committing a second revision. A key
minted per invocation would make every retry a new command. Array order is
content, so two edges listed the other way round are a different command and
derive a different key.

Previewing and then applying the same file therefore carry the same key.

## Refusals

| Code | Class | Exit | Meaning |
|---|---|---|---|
| `architecture_validation_failed` | invalid_input | 2 | the shared validator refused; each violation is reported on its own line |
| `architecture_revision_conflict` | conflict | 5 | the head moved; nothing was applied |
| `architecture_not_permitted` | unauthorized | 4 | the signed-in user lacks platform administration authority |
| `architecture_not_found` | invalid_input | 2 | the command names a node, edge or revision the graph does not hold |
| `architecture_conflict` | conflict | 5 | duplicate identity, dangling edge, or a doorway the edge does not use |
| `invalid_proposal` | invalid_input | 2 | the proposal file is not one bounded command document |
| `proposal_unreadable` | unavailable | 3 | the path cannot be read, or exceeds one megabyte |
| `proposal_revision_mismatch` | invalid_input | 2 | `--expected-revision` and the file's own fence disagree |
| `confirmation_required` | invalid_input | 2 | `apply` was run without `--yes` |
| `desktop_contract_mismatch` | unavailable | 3 | the reply is not the action's documented shape |

Plus the shared pairing refusals every paired command documents:
`desktop_not_paired`, `desktop_ambiguous`, `desktop_unreachable`,
`desktop_unreadable`, `desktop_signed_out`, `desktop_refused`,
`desktop_operation_unsupported`, `pairing_rejected`.

An unknown `--chapter` is refused by the authority as `validation_failed`, not
answered with an empty list. A chapter that does not exist and a chapter with
nothing in it are different answers.

### A conflict is never retried

`architecture_revision_conflict` names the head the graph moved to and stops.
`ds` does not re-send against that head, and the refusal's `next` deliberately
offers only `get` — re-applying an edit on top of work its author never saw is
the exact failure the revision fence exists to prevent. Re-planning is a human
decision.

### An idempotent replay is a success

`apply` answering `applied: false` means the server already holds this exact
command under this key. That is exit 0, and the human tier renders it as
`idempotent`. Reporting it as a failure would send a caller to re-plan work
that is already committed.

## Worked loop

```bash
# 1. What is the head, and what does the chapter hold?
ds governance architecture get --output json
ds governance architecture list --chapter survey-lifecycle --output json

# 2. Validate the proposal. Writes nothing; reports every violation.
ds governance architecture preview --command question.json --output json

# 3. Commit it, fenced to the revision it was planned against.
ds governance architecture apply --command question.json --expected-revision 12 --yes --output json

# 4. What moved, and who moved it.
ds governance architecture history --limit 20 --output json
```
