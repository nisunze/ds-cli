---
name: ds-design-tag-groups
description: Batch-assign any eligible project tag definition across transformers through `ds`, and export a digest-pinned projection for an explicit ordered consumer grouping.
metadata:
  ds-chapters: design
---

# Batch-edit project tags

`ds design group` discovers active, single-valued choice definitions that apply
to LV transformers. Definition ids are project-authored metadata; do not infer
special behavior from names such as `city` or `phasing`. Single-object edits may
also use the ordinary `ds design tag set` path.

1. Read the vocabulary: `ds design group list --transformers a,b --output json`.
   Per group: `defined`, `allowed`, `needsModel`, and what each transformer
   carries today.
2. Select the exact `.data.groups[].group` definition id, then plan: `ds design
   group preview --group <definition-id> --transformers a,b --value <allowed-value>
   --output json`. Read `.data.outcomes` back to the user — one row per
   transformer — and keep `.data.digest`.
3. Commit only with the user's intent: `ds design group apply … --digest
   <digest> --yes`, or `ds design group unassign … --digest <digest> --yes`
   for a clearing (previewed with no `--value`).

**Match the value, never repair it.** Take values from `allowed`. `Phase 1` and
`phase 1` are different values and the server refuses the one the project did
not define; do not lowercase, title-case or pluralise on the user's behalf.

If a plan carries model evidence, report its returned model state and
outstanding rows exactly. Never infer a model requirement from the definition
id or fabricate a receipt.

**A stale digest is not a retry.** `design_plan_stale` means the project moved;
preview again and show the new plan before applying.

## Exporting for a report

A consumer chooses grouping with an explicit ordered list of definition ids.
`ds design group export --transformers a,b --definition-ids region,wave
--output json` returns that projection plus the `sha256` over its exact bytes.
Use `--definition-ids` with the ids approved for that report; omit it only when
the intended result is one explicit untagged group.

Save it verbatim, for example
`ds design group export --transformers a,b --output json | jq -r .data.document > tags.json`.
Never parse and re-serialize it: the digest is over those bytes and the report
pins it.

Report a missing, archived, inapplicable, or ambiguous selected definition; do
not substitute a similarly named definition or fall back to a filename,
coordinate, or consumer document. `.data.excluded` names values the projection
could not carry, with the reason.
