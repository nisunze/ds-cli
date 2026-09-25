---
name: ds-grid-project-model
description: Create, import, select, edit and publish DS Grid project models through the governed local-model lifecycle. Use for local `.dsgrid` working copies (including typed structure edits — describe, retype single poles to H-poles — and the structure list / staking table with REG rule findings) and immutable project versions, not PLS-CADD conversion.
metadata:
  ds-chapters: grid-model, design
  ds-mcp-profile: grid-local-model
---

# Manage a DS Grid project model

Use the `ds` skill first. Keep three states distinct:

- acquisition creates or imports a model in the paired application's local store;
- active means one local model occupies Profile and editing;
- publication registers one immutable revision in the selected project and does not change the active model.

## Discover a project's models and their versions

A project may have several independently versioned DS Grid models. Their
display names are arbitrary and must not be parsed as version numbers. Discover
`dsgrid.project.list` and `dsgrid.project.versions` through the live
descriptors. List the explicit project, following every `next_cursor` while
`more` is true. For each returned opaque `model_id`, list its versions and
follow that command's cursor too. Use `--include-deleted` only when retired
models are relevant. Report the model name, ID, head revision, and actual
version values separately; pin later downloads or prints to an exact revision.
Both leaves are in the `grid-local-model` MCP profile.

Read the live descriptor before each command:

```text
ds capabilities dsgrid.model.list --output json
ds capabilities dsgrid.model.create-local --output json
ds capabilities dsgrid.model.import-external --output json
ds capabilities dsgrid.model.set-active --output json
ds capabilities dsgrid.publish-version --output json
```

Start with `ds dsgrid model list --output json`. Then choose exactly one local acquisition:

- `ds dsgrid model create-local ... --output json` creates an empty model and normally makes it active.
- `ds dsgrid model import-external --path <absolute.dsgrid> ... --output json` acquires a package without activating it.

Use the returned opaque model id with `ds dsgrid model set-active --model <id> --output json` only when that model should occupy Profile. Repeating it for the active model is idempotent.

Local list, create, import, and set-active require a paired application but no project. Never add or infer a project argument, inspect IndexedDB, send package bytes through the bridge, or treat local activity as project authority.

Publish separately with `ds dsgrid publish-version`. For a new project model, name the local model plus the authored display name and declared kind. For an existing project model, name its generated id and the expected head when known; do not pass a new name. Add `--yes` only after the operator authorizes this exact project write. Require the receipt to report `status: published`, its immutable revision and digest, and `active_model_changed: false`. A failed local binding after publication does not undo the committed version.

Do not retry a moved-head conflict or change projects to force publication. Re-read the project model, review the new head, and ask for a fresh publication decision.

Each publish is a revision of the model's current version; only `--bump-version` starts the next one (a submission). Put `--milestone`, `--approval submitted` and `--attach <delivered.bak>` on that revision, then mark it with `design version begin --kind mv_model --milestone <m> --expected-source <revision>`. Read history with `dsgrid project show|versions|compare`; `dsgrid project exports` holds immutable delivered files.


To import replacement content into an existing model, use one
`ds dsgrid publish-version --path <incoming.dsgrid|incoming.bak>
--replace-content --project <id> --project-model <existing-id>
--expected-head <reviewed-revision> --kind mv_line --reason <text> --yes`
operation. A `.bak` also needs `--crs` and, for multiple projects,
`--select-project <don-leaf>`. Check the receipt's source, head, result,
warnings, revision and digest. Verify the earlier version and active local
model did not change.

## Typed edits of a working copy (structure list, descriptions, H-poles)

A working copy is edited in place through typed commands, never by hand-built
envelopes. Discover them first:

```text
ds capabilities dsgrid.model.show --output json
ds capabilities dsgrid.structure.describe --output json
ds capabilities dsgrid.structure.retype --output json
ds capabilities dsgrid.report.structures --output json
```

Procedure, in this order:

1. `ds dsgrid model show --model <local-id> --output json` — read
   `head.authored_revision`; every edit below pins against it (`--revision`)
   or, omitted, against the current head; a moved head refuses
   `revision_conflict` and you re-read, never retry blindly.
2. `ds dsgrid report structures --model <local-id> --out <file.csv|.xlsx>` —
   the structure list: one row per placed structure with description, station,
   line angle (right turn positive, as PLS-CADD prints it), pole family /
   material / height / class / stays, drawing number, Table 14 foundation and
   the findings. Read `counts.findings` and `assumptions[]` (`assumed: true`
   means the engine evaluated a rule the issued standard does not carry — say
   so in your report). `--only-findings` narrows the printed rows.
3. `ds dsgrid structure describe --model <local-id> --structure <id|number> --text "<drawing no.> <assembly>, <12/14> m <material>, <angle band>, <n> stays" --dry-run`,
   then the same with `--yes`. One structure per call; the text is the
   structure's own line in the list, never parsed for values.
4. `ds dsgrid structure retype --model <local-id> --from-finding structure_type_not_allowed --type <h-pole library name> --dry-run`
   — every single pole carrying 10° ≤ |line angle| < 60°, as ONE revision.
   Read `structures[]` and `findings {cleared, remaining, created}`; a
   `command_invalid` naming a strung set the new type lacks (a T-off) means
   `--skip <id|number>` that structure and choose its type separately. Repeat
   with `--yes` only when the dry run is clean; a write that would leave or
   create the finding is refused `structure_type_not_allowed`.
5. `ds dsgrid report structures …` again — the receipt is the evidence
   (`resulting_revision`, counts, the file digest).

Never combine `--yes` with `--dry-run` or edit the package by hand. For
`package_decode_failed`, re-convert the PLS-CADD source; older additive
schemas still open (`head.prior_schema_members`). These edits are proposals
until PLS-CADD confirms after `ds dsgrid-exchange sync`.

## PLS-CADD sources

A PLS-CADD workspace or `.bak` is not a local-model import. Discover and use `dsgrid-exchange.inspect`, `dsgrid-exchange.plan`, and `dsgrid-exchange.convert` to produce a new `.dsgrid`; validate it, then acquire that package with `model import-external`. Never add a second convert-and-publish route to this workflow.

Native package lineage and governance `vN` are separate. Keep the validated
manifest identity and advance changed package content; ask the server how to
recover a legacy head. MV attachments pin a content revision and its digest;
LV attachments pin `vN`. After submission, publish the `.bak` with
`design.attachment.publish --kind mv_model --version <revision_id>`; inspect
its bindings with `design.attachment.versions|show` and verified bytes with
`design.attachment.download --out`.

A version's package files: `dsgrid.asset.list|extract` (local package) and
`dsgrid.project.asset.list|extract` (exact revision); v1's incoming backup is
`pls-original-workspace.bak`. Attach the delivered backup with
`dsgrid.asset.attach` before publishing; a later revision marks it `stale`.

Stops at: native PLS-CADD — opening, spotting or solving a model happens in the
application; `ds` publishes and reads the files around it.
