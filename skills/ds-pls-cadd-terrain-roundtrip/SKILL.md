---
name: ds-pls-cadd-terrain-roundtrip
description: Round-trip a native PLS-CADD backup through DS Grid to revise deviation-route PIs and terrain evidence and return a fresh PLS-CADD handoff. For PLS-CADD alignment and terrain work, not LV design or structure repair.
---

# Revise PLS-CADD route and terrain without losing native identity

Use the deployed `ds` CLI as the only DS interface. Start with the `ds` skill,
then discover the installed command contracts. The `.bak`, deviation routes,
survey points, CRS, vertical datum, and operator rulings are evidence; never
replace one with an inferred value merely to make a conversion run.

Read [references/terrain-round-trip.md](references/terrain-round-trip.md) for
the staged import/revision/export/operator-return workflow. When a PLS import,
export, restore, terrain interpolation, or report behaves unexpectedly, also
read [references/native-failure-modes.md](references/native-failure-modes.md).

## Keep three identities separate

- A PLS-CADD alignment PI is a vertex in the ordered alignment route and is
  emitted into native PI geometry. It is not automatically a terrain feature
  code and is not a placed structure.
- Terrain observations carry plan coordinates, elevation, classification and
  provenance. A point whose elevation is interpolated from the effective
  surface is derived evidence, not a surveyed observation.
- A placed structure has independent plan position, derived station/offset,
  orientation and library identity. A route revision must not silently move a
  structure or rename its exact resource leaf.

If the operator says “code the angle points as PI,” determine whether `PI`
means native alignment vertices, a project feature-code token, or both. Author
only the meaning the evidence establishes. To make points manually movable as
PLS-CADD angle points, the canonical route must contain those vertices; merely
labelling terrain points `PI` is insufficient.

## Non-negotiable gates

1. Preserve every supplied file and record its digest before conversion.
2. Import from the `.bak` container, not a hand-unpacked approximation.
3. Require an explicit horizontal CRS and declared vertical datum. For
   non-standard EDCL coordinates use the project-approved Custom/EDCL
   definition, never a guessed EPSG code.
4. Inspect, plan, then convert into a new directory. Validate the resulting
   `.dsgrid` before editing.
5. Apply one typed, revision-pinned engine command at a time. Dry-run first;
   every committed step writes a new `.dsgrid` and never overwrites its parent.
6. Refuse elevation interpolation when the engine reports missing effective
   ground coverage. Acquire or author verified terrain evidence instead.
7. Export through a digest-pinned `dsgrid-exchange` plan to a new PLS-CADD
   `.bak`. DS Grid validation does not prove that PLS-CADD can open it.
8. Require native Restore into a fresh directory and reopen in the declared
   PLS-CADD version before handing the model to the operator.
9. The operator manually moves PIs and readjusts structure positions in
   PLS-CADD, saves, and returns a new native backup. Treat that returned backup
   as a new authority candidate; re-import and compare it rather than assuming
   which rows PLS changed.
10. Report native open, reference closure, terrain/route changes, structure
    movement, analysis coverage, checks, and engineering approval separately.

Do not repair a missing live command with direct store access, a skill-local
parser, hand-edited PLS bytes, or UI automation. Use the `ds` skill's bounded
feedback procedure with the exact observable acceptance behavior.
