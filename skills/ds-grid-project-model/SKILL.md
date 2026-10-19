---
name: ds-grid-project-model
description: Discover project models, then create, import, select and edit DS Grid working copies through the governed local-model lifecycle. Use for local `.dsgrid` working copies (including typed structure edits — describe, retype single poles to H-poles — and the structure list / staking table with REG rule findings) and immutable project versions, not PLS-CADD conversion.
metadata:
  ds-chapters: grid-model, design, map-presentation
  ds-mcp-profile: grid-local-model, grid-profile
---

# Manage a DS Grid project model

Use the `ds` skill first. Keep three states distinct:

- acquisition creates or imports a native local working copy;
- active selects the working copy; paired Profile presentation is separate;
- publication registers one immutable revision in the selected project and does not change the active model.

Core runs every package read and edit through the installed Network addon; a
missing or older addon refuses before any work. Project publication
(`dsgrid publish-version`), PLS-CADD conversion (`dsgrid-exchange`), package
assets and revision compare are not restored in Core yet: stop at that
boundary and report it, never hand-edit or bridge package bytes.

## Discover a project's models and their versions

A project may have several independently versioned DS Grid models. Their
display names are arbitrary and must not be parsed as version numbers. Discover
`dsgrid.project.list` and `dsgrid.project.versions` through the live
descriptors. List the explicit project, following every `next_cursor` while
`more` is true. For each returned opaque `model_id`, list its versions and
follow that command's cursor too. Use `--include-deleted` only when retired
models are relevant. Report the model name, ID, head revision, and actual
version values separately; pin later downloads or prints to an exact revision.
Both are in the `grid-local-model` profile.

Read the live descriptor before each command:

```text
ds capabilities dsgrid.model.list --output json
ds capabilities dsgrid.model.create-local --output json
ds capabilities dsgrid.model.import-external --output json
ds capabilities dsgrid.model.set-active --output json
```

Start with `ds dsgrid model list --output json`. Then choose exactly one local acquisition:

- `ds dsgrid model create-local ... --output json` creates an empty model and normally makes it active.
- `ds dsgrid model import-external --path <absolute.dsgrid> ... --output json` acquires a package without activating it.

Select the returned opaque id with `ds dsgrid model set-active --model <id> --output json`; repeating it is idempotent. Discover `dsgrid.profile.open` for paired presentation.

Local list, create, import and set-active need no paired app or project. Profile presentation needs the app. Never infer a project, inspect IndexedDB, bridge package bytes, or treat local activity as project authority.

## Edit a working copy

Use `grid-profile` for paired Profile jobs and cached results. Discover its
live controls. Editing, calculation and Save remain independent.

Use typed working-copy commands; never build envelopes. Discover them first:

```text
ds capabilities dsgrid.model.show --output json
ds capabilities dsgrid.structure.describe --output json
ds capabilities dsgrid.structure.retype --output json
ds capabilities dsgrid.report.structures --output json
```

Steps:

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

Never combine `--yes` with `--dry-run` or hand-edit packages. The native reader
accepts only the current schema. Retain older bytes and discover
`dsgrid.package.migrate`: pin their SHA, review the dry run, then use `--yes`
to write a new file. Validate and explicitly import it; never use an older
reader. Edits remain proposals until native PLS-CADD confirms them.

Stops at: native PLS-CADD engineering acceptance and solver approval.
