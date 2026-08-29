---
name: ds-design-tag-groups
description: Assign the governed `city` or `phasing` tag across many transformers through `ds`, and export the digest-pinned tag document a per-city report groups by.
metadata:
  ds-chapters: design
---

# Govern a project's city and phasing tags

`city` and `phasing` are two reserved tag definitions with a batch surface.
Everything goes through `ds design group`, which reuses the application's own
design-collaboration client. The generic `ds design tag set` refuses these two
and points here — a governed group has exactly one write door.

1. Read the vocabulary: `ds design group list --transformers a,b --output json`.
   Per group: `defined`, `allowed`, `needsModel`, and what each transformer
   carries today.
2. Plan: `ds design group preview --group city --transformers a,b --value
   kigali --output json`. Read `.data.outcomes` back to the user — one row per
   transformer — and keep `.data.digest`.
3. Commit only with the user's intent: `ds design group apply … --digest
   <digest> --yes`, or `ds design group unassign … --digest <digest> --yes`
   for a clearing (previewed with no `--value`).

**Match the value, never repair it.** Take values from `allowed`. `Phase 1` and
`phase 1` are different values and the server refuses the one the project did
not define; do not lowercase, title-case or pluralise on the user's behalf.

**A `phasing` batch will come back `partial`.** Its second home is the DS Grid
model's alignment, `ds` holds no model session, and so every named transformer
is listed as outstanding. Report that as the true state and point the user at
the application to finish it. Never describe a phasing batch as done because
its tags landed.

**A stale digest is not a retry.** `design_plan_stale` means the project moved;
preview again and show the new plan before applying.

## Exporting for a report

A per-city report is grouped by the tag group named exactly `city`.
`ds design group export --transformers a,b --output json` returns that
authority as one document plus the `sha256` over its exact bytes.

Save it verbatim, for example
`ds design group export --transformers a,b --output json | jq -r .data.document > tags.json`.
Never parse and re-serialize it: the digest is over those bytes and the report
pins it.

The export refuses when the project has no group named exactly `city`, or two
of them. Report the refusal and the group ids it names; do not pick one, and do
not fall back to a city read from a filename, a coordinate or a Solar document.
`.data.excluded` names any value the document could not carry, with the reason.
