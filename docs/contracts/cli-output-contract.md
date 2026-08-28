# Output contract — `ds-cli-output/v1`

**Status:** active contract. Scripts and agents depend on every rule here.

## Streams

| Stream | Carries |
|---|---|
| stdout | the answer, and nothing else |
| stderr | diagnostics, progress, human-mode refusals |

`stdout` is always parseable in `--output json` mode — including for a
refusal, because a refusal is a result an agent must read. In human mode a
refusal goes to stderr instead, so a person piping stdout receives only
answers.

Asserted by `machine_output_is_stdout_and_diagnostics_are_stderr`.

## The envelope

Success:

```json
{
  "v": 1,
  "command": "dsgrid.inspect",
  "contract": 1,
  "status": "ok",
  "data": { }
}
```

Failure:

```json
{
  "v": 1,
  "command": "dsgrid.inspect",
  "contract": 1,
  "status": "error",
  "error": {
    "class": "invalid_input",
    "code": "model_not_found",
    "message": "cannot read `/no/such/file.dsgrid`",
    "retryable": false,
    "remedy": "check the path; --model takes a .dsgrid file",
    "next": ["ds dsgrid inspect --help"],
    "detail": { "detail": "entity not found" }
  }
}
```

Three independent versions, on purpose:

- **`v`** — the envelope's own version. Raised only for a breaking change to
  the envelope, never for a change inside `data`.
- **`contract`** — *that command's* input/output version. One command can
  break compatibility without touching any other.
- the binary's release version, from `ds version`.

`data` is compact by default. `--pretty` indents it; indentation is bytes a
machine does not read.

`more` appears only when there is something more — a projection not requested,
or a collection that was truncated. Its absence means "this is all of it",
which is itself information.

## Exit codes

| Code | Class | Meaning | Retry? |
|---|---|---|---|
| 0 | `ok` | ran and answered | — |
| 1 | `internal` | a defect in `ds`; report it | no |
| 2 | `invalid_input` | unknown command or flag, missing input, unparseable or out-of-bounds value | not unchanged |
| 3 | `unavailable` | real command, missing prerequisite: engine, external tool, desktop, data asset | after the remedy |
| 4 | `unauthorized` | no verified principal, or not permitted | after signing in |
| 5 | `conflict` | stale view, or two effects collided | after re-reading |
| 6 | `failed` | ran and failed; the work did not happen | maybe |

`error.class` always names the same class the exit code does — asserted by
`exit_code_and_envelope_class_always_agree`. A caller branching on either
reaches the same conclusion.

**Engineering failure is not execution failure.** A command that correctly
computes "this structure fails its criteria" exits 0. The domain answer lives
in `data`; the exit code reports only whether the command could answer.

## Error codes

`error.code` is stable, snake_case, and never localized or reworded. It is the
field to branch on. Every code a command can emit is listed in its help under
`REFUSALS`, with the situation and the remedy — so failure can be planned for
rather than discovered.

Errors carry no stack traces. `detail` is bounded structured context, present
only when a code alone would leave a caller stuck.

## Global flags

Recognized at any position, ahead of command routing:

| Flag | Effect |
|---|---|
| `--output human\|json` | stdout format; default `human` |
| `--pretty` | indent JSON |
| `--no-color` | never emit ANSI; `NO_COLOR` does the same |
| `--yes` | pre-confirm an effectful command |
| `--help`, `-h` | help at whatever depth was named |
| `--version`, `-V` | build identity |

Colour is emitted only when stdout is a terminal, `NO_COLOR` is unset, the
format is human, and `--no-color` was not passed. Agent shells and CI get
clean bytes without asking.

## Effects and confirmation

Every command declares an effect class. The vocabulary is shared with the
desktop agent bridge — one set of words for one question. The retired `ds-mcp`
used the same words, which is where several of them came from; it is not a
source a reader can check anything against today.

| Effect | Meaning | `--yes`? |
|---|---|---|
| `discovery` | reads nothing outside the process | no |
| `read_only` | reads state, writes nothing | no |
| `proposal` | drafts a document a human applies; spends model credit | no |
| `local_file_write` | writes a file in the operator's workspace | no |
| `local_ui` | changes the paired desktop's visible state | no |
| `artifact_write` | produces a durable artifact of record | **yes** |
| `machine_write` | changes software or settings on this machine | **yes** |
| `global_write` | mutates shared project state | **yes** |

`machine_write` is the class for a command that reaches outside the operator's
workspace into the machine itself — installing a component, or writing a
user-level host configuration file. That is why `workstation install`,
`workstation configure` and `mcp install` are gated while `report export`,
which writes into a directory the caller named, is not.

Confirmation is enforced once, in `registry::dispatch`, on the **command's**
declared effect — so a handler cannot forget it, and it does not depend on
which flags an invocation carries. A command whose writing path is gated is
gated on every invocation, including the one that only prints what it would
do. Read-only commands stay frictionless.

Consent is never inferred from prose. `--yes` is the only way to pre-confirm.

## Authority

| Authority | Requires |
|---|---|
| `none` | nothing; runs offline and signed out |
| `desktop_pairing` | a running DS GridDesign session — proves a transport, not a person |
| `desktop_user` | that session, signed in |
| `project` | a verified principal bound to a confirmed project |

Possession of a file, a descriptor or a project id is never authorization.

## Input parsing

Strict. An undeclared flag is an error with a near-miss suggestion. A value
outside a declared choice set is an error listing the accepted values. A
missing required input is an error naming what is missing. Every one is exit
2 with a stable code.

Operands are used only where the operand *is* the subject —
`ds capabilities dsgrid`. Commands taking engineering inputs use named flags,
so position never has to be guessed.

## Determinism

Collections are emitted in a deterministic order — canonical order where the
domain defines one, lexical otherwise. Two runs over the same input produce
byte-identical stdout.
