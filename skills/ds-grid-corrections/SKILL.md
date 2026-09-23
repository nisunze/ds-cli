---
name: ds-grid-corrections
description: Translate review comments or computed findings on any spotted DS Grid model into scoped, cherry-picked typed corrections through live ds CLI/MCP.
metadata:
  ds-chapters: grid-model
  ds-mcp-profile: grid-corrections
---

# Correct a spotted line from comments or findings

## Use after the first spotted revision

Use the `ds` skill and live descriptors first. The same workflow applies to
greenfield DS spotting and imported PLS-CADD models once a spotted model
exists. An engineer's comment or a report finding identifies the issue;
the model's stable IDs, surveyed geometry, design policy, and reports are
evidence. This skill does not define pole spacing, clearance, sag, support
type, protection, or survey thresholds. Read those from the project and ask
for the missing engineering decision when evidence cannot resolve it.

## Interpret before mutating

1. Pin the exact working `.dsgrid` SHA-256 and authored revision. Read the
   relevant alignments, angle points, structures, tension sections, survey
   facts, clearance and structure-usage reports through live `dsgrid.run`,
   `dsgrid.analyse.clearance`, and `dsgrid report structures` descriptors.
   Keep comments linked to exact model IDs and source evidence. Do not use a
   PLS filename, engineering number, coordinate proximity, or model type name
   as an authority for a design decision.
2. Translate each comment to a small typed `GridCommand` proposal. A general
   replacement names the exact placed structures and a reviewed type ID;
   leave all others out. A clearance repair selects its affected supports and
   tension sections. Re-stringing between angle points names the actual
   ordered support path and complete physical attachment set, then invokes
   `update_tension_section_supports`, `create_tension_section_path`, or
   `create_tension_section_set_path` as applicable. A route deviation uses
   reviewed model-CRS survey vertices with `replace_alignment_route_restationed`
   and retains the existing physical support XY; move or add only supports
   explicitly required by the new geometry. Do not generate a route from an
   expropriation comment alone. Survey or terrain acquisition follows its own
   evidenced command and validation path.
3. Record each proposal with a comment or finding ID, selected alignment IDs, current
   structure/section IDs, command ID, expected outcome, evidence, and open
   engineering decisions. Read each proposed command's live descriptor.
   Existing spotting and native PLS results are inputs, not commands to run
   again by default. Keep already satisfactory intervals and all unrelated
   lines out of the transaction.

## Execute a reviewed selection

Write a revision-pinned batch in the ordinary `dsgrid.apply-batch` format.
Give each selected command a stable `review_ref` beside `command_id` and
`command`; the guarded path requires it and returns it in the receipt. Use
`comment:<id>` for a human comment or `finding:<report>/<id>` for a computed
failure. The reference conveys provenance, not an engineering rule.
Write a guard file with the exact source package digest, the selected
alignment IDs, and an explicit list of permitted command kinds:

```json
{
  "source_sha256": "sha256:<64 hex characters>",
  "review_state": "draft",
  "alignment_ids": ["<exact alignment ID>"],
  "level": "alignment",
  "allowed_command_kinds": ["retype_structure", "update_tension_section_supports"]
}
```

Use `ds dsgrid apply-correction --model <source.dsgrid> --batch <batch.json>
--guard <guard.json> --select <command-id> --dry-run --output json`. Repeat
`--select` to cherry-pick multiple commands. The engine applies the selected
IDs in original batch order. Inspect the resulting revision, affected-table
summary, and any refusal. Then use the same inputs with `--out <new.dsgrid>`;
the source file is never overwritten. This command is available through the
grid-model MCP chapter and the focused `grid-corrections` typed profile from
the same live declaration.

The `level` is `project`, `alignment`, or `angle_interval`. Project scope
requires every alignment in this exact package. Interval scope requires
`intervals: [{"alignment_id": "...", "from_structure_id": "...",
"to_structure_id": "...", "include_start_structure": false,
"include_end_structure": false}]`, with associated, ordered angle/terminus
route nodes. The two inclusion flags independently decide whether each
boundary support may be retyped or described as part of the correction.
Their route-node XY stays fixed; relocating a boundary needs a wider scope.
Route vertices outside the open interval must remain exactly where they were.
Put pegged, built, or drafter
approved supports in `preserved_structure_ids`; use
`preserved_route_node_ids` and `preserved_section_ids` for other fixed work.
Supply `preservation_source_ref` whenever intervals or preserved IDs are
present. These IDs and source references must come from reviewed project
facts; do not infer them from a type name or an empty staking status.

`review_state: draft` permits dry-run only. To write a new package, author
`review_state: approved` and an exact `decision_ref` in the guard. This
correction review state is distinct from a model row's `drafting_status =
approved` compatibility lock; the latter is represented here by the
corresponding preserved asset IDs and its evidence reference.

The guard checks the source package digest as an optimistic edit pin, not as
the identity of a reusable structure model or a native library member.
It rejects unreviewed command kinds, missing targets, shared supports that
also belong to an unselected alignment, and changes to unselected canonical
rows. A selected alignment does not grant permission to change a shared
physical support on another line. Add all affected alignments only after
reviewing that wider scope. `apply-correction` requires `--guard`.

After a correction, rerun the relevant clearance and structure checks on
the resulting revision. Where native PLS-CADD acceptance is needed, hand the
exact new package and receipts to the PLS owner for export, Restore/reopen,
and native checks. DS import/sync supports only characterized native edits;
do not claim that a section or route edit has reached PLS until a strict
export or a fresh native re-import proves it.
