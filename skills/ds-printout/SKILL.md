---
name: ds-printout
description: "Produce or resume DS transformer sheets, project atlases, district MV and custom-area printouts through visual QA and verified delivery. Use for a finished print set, not isolated styling or redesign. Default to a signed-in headless server; pair a desktop only where required."
metadata:
  ds-chapters: reports
  ds-mcp-profile: printing
---

# DS Printout specialist

Deliver a verified print set from current project data or reusable outputs:
`ds` for live operations, `ds-map-composition` for cartographic judgment.

## Establish the assignment and host

Recover project, source revision, audience, geography, papers, formats,
destination and earlier decisions; build the subject/layout/format matrix.
“All” means complete inventory, resolved pagination and explicit exclusions.
Keep authorization, input/recipe revisions, receipts, QA coverage and next
action in host task state or the deliverables directory.

Default to a signed-in headless server/workstation. Verify executable, lane and
fenced selected project through `auth.status` and `auth.project.use`. Discover
`report.project.export`, project output settings, `report.layout.*`, and
`data.project-cache.status|seed`. A paired desktop is needed only where the live
contract names an app-owned operation; never borrow its credentials or lane.
Native exports acquire exact MV heads read-only; model publication is a
separate lifecycle. A district/project sheet can use a held `.dsgrid` and
staged designs: see `ds-map-composition`.

Distinguish local proof, saved project recipe, queued publication and verified
online delivery. Preserve existing authorization; ask only for material added
scope while continuing independent work. One writer owns rooms and recipes.

## Prepare sources and context

Inspect the design/model inventory through its owner; verify CRS, extent,
attributes and canonical identities. Printing authorizes no redesign,
renumbering, invented ratings or construction approval. Preserve existing
infrastructure, dirty rooms, model versions and unrelated outputs.

Use `ds-assets` for governed documents and logos, retaining bytes/digests.
Keep unknown contract numbers or signatures unresolved; never inherit approvals
or branding from another project. Missing required evidence is a named
delivery limit.

Acquire only context needed for the printed extent and purpose, including
margins and disconnected clusters. Ready-empty, incomplete and stale differ.
Seed through the governed project context route; reads never fetch missing
providers silently. National bundles are machine holdings; buildings and
contours are project context. Do not seed every dataset by habit or substitute
reference transformers for the project network. Check carried/omitted layers,
coverage, freshness and source digests; optional exclusions only where the
map’s purpose survives.

Source rules: [sources and context](references/sources-and-context.md).

## Author sheets from the contract

Start from the published paper/family default, then compare the project recipe.
Keep source identity/revision when adopting a project copy. Preserve globals,
live-map styles, unselected output policy and per-subject exceptions. Compose
each paper independently: a reduced A0 is not a readable A3. Prepare and
inspect a concrete recipe before any required approval.

Read the live schema and current optimistic revision. `report.project.export`
supports local same-paper recipe proofs (discover their inputs): they keep
governed provenance, leave saved recipes intact and cannot publish. Save a
governed recipe before requesting publication. An error does not imply
rollback: read back ambiguous writes before retrying.

Follow these printing contracts:

- The drawn printable border owns independent schedule containers. Use measured
  edges, physical margins and explicit corner anchors. Join schedules with flow
  only when requested; snapping to information/legend retains the saved gap.
- Pole/customer schedules use selected source columns and an explicit
  Design/AsBuilt phase; keep requested fields and consultant X/Y, invent no
  coordinates or meter numbers. LV line number is not a default column;
  workbook presentation is for Network Information unless requested.
- Headings never widen columns: indexed headings use order letters with a full
  key above each panel by default. Never abbreviate headings by hand.
- Few-category values never widen columns: by default the customers schedule
  prints Category/Meter type as `1`/`2` with a legend under the table
  (`table.value_key`); other tables print literally unless told to
  ([sheet furniture](references/sheet-furniture.md#values-and-legend)).
- Optimize the full network’s vertical fit. Split panels may differ in height
  and row count; their actual footprints obstruct the map, not a padded
  rectangle. Flow followers stay with their movable root. Where authorized,
  schedule type may shrink up to 30%; equal fits prefer larger type. Exact
  bounds and controls are in the live schema; the bounded search is a
  heuristic, not an optimum.
- Protect titles, scale, legend and furniture in map fitting. Include existing
  and proposed MV context with distinct pens and meaningful legend labels.
  Retain engineering hierarchy, centered transformer names and requested scale
  treatment. Keep geographic context subordinate to the network.

Individual sheets and exceptions: [transformer sheets](../../ds-map-composition/references/transformer-sheets.md),
[outlier adjustments](../../ds-map-composition/references/outlier-transformers.md);
combined A0/A3 sets and numbering: [drawing collections](references/drawing-collections.md).

## Render, inspect, refine

Render representative density/geography cases for every layout/format through
the intended route before batching. Check identity, source/recipe revisions,
print-context digests, carried/omitted layers and warnings. Hidden map labels
are placement notes; furniture overflow and missing schedule rows stay warnings.
A local proof proves neither publication nor remote renderer parity.

Inspect actual pages and readable crops: margin first, then title block,
legend/schedule edges, dense junctions, clipping, overlap, units, table scope,
existing/proposed line categories; required rows and fields survive, adjacency
holds, the full network fits vertically. Ground scale and quantities in owner
output. A successful render is not visual QA.

Revise, rerender and inspect. Review every page of a small set; for a large
set review templates and outliers and record exact coverage — sampling never
becomes an every-page claim. After a workflow change, run one blind
smaller-model trial: give the job and limits, not source or command syntax;
its real artifact attempt is the evidence.

## Deliver and recover

Run the complete matrix through the owner batch route, reusing only fresh
outputs with matching dependencies. Match identities to output IDs, paper
dimensions, revisions, hashes and receipts; flag missing, duplicate, failed
and stale rows.
Keep A0/A3 multipage collections separate and preserve project-wide drawing
numbers on individual and grouped sheets, per the owner’s bundling contract.

Deliver to the requested destination. Local-only work avoids publication
queues; online, queued is incomplete until cross-machine/server readback.
Report exact paths, counts, QA coverage and material limitations.

After interruption, revalidate identity, inventory, inputs and live contracts.
Protect dirty work and reuse unchanged dependencies. Recipe changes invalidate
prints, not source archives. Follow typed remedies and retryability; never strip
required data to appease an old validator. Report confirmed gaps through
`ds feedback submit`, not a local gap ledger.

Detailed execution and handoff: [procedure](references/procedure.md),
[delivery record](references/delivery-record.md), [acceptance](references/acceptance.md).

Stops at: the human eye and the printer — visual acceptance and the physical
print job are the operator's; `ds` composes, renders and captures.
