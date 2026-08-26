---
name: ds-pls-cadd-terrain-roundtrip
description: Round-trip PLS-CADD construction deviations through DS Grid — route PIs, near-as-built structures/stringing, terrain evidence and review attachments. Not LV design or structure repair.
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

## Automate bounded near-as-built changes

When a construction change is minimal and almost as built, author route PIs,
placements, native section-table stringing, local review attachments, and
report/table setup automatically. These deterministic workspace operations do
not require a PLS-CADD UI step.

Use deployed `ds` contracts for DS model changes. At a native boundary, reuse
the characterized `ds-network` whole-snapshot or surgical composer; never
hand-edit coupled DON rows. If the installed CLI has not exposed that composer,
use bounded Rust glue only in an explicitly authorized delivery/coding session
and leave CLI exposure for its own coding session.

Use Windows UI only when a calculation/check needs the native PLS-CADD solver
or the user explicitly requests native acceptance. Drive that UI directly with
the Windows controller, never through `ds`.

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
7. Export through a digest-pinned `dsgrid-exchange` plan to a new self-contained
   PLS-CADD workspace. DS Grid validation does not prove native closure.
8. For a minimal near-as-built delivery, require native parser readback,
   reference closure, route/structure counts, attachment closure, and
   section-table readback. Require Windows UI only for a native calculation or
   explicitly requested native acceptance.
9. When engineering judgment requires the operator to move PIs or readjust
   structures, treat the returned saved workspace as a new authority candidate;
   re-import and compare it rather than assuming which rows PLS changed.
10. Report native acceptance, reference closure, terrain/route changes, structure
    movement, analysis coverage, checks, and engineering approval separately.

Do not repair a missing live command with direct store access, a skill-local
parser, hand-edited PLS bytes, or PLS-CADD UI authoring. Use the established
native composer only under explicit task authority, and use the `ds` skill's
bounded feedback procedure for missing CLI exposure.
