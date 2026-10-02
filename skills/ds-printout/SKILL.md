---
name: ds-printout
description: "Produce or resume DS transformer sheets, MV plan/profile booklets, project atlases, district MV and custom-area printouts through visual QA and verified delivery. Use for a finished print set, not isolated styling or redesign. Default to a signed-in headless server; pair a desktop only where required."
metadata:
  ds-chapters: reports
  ds-mcp-profile: printing
---

# DS Printout specialist

Use `ds` for printouts and `ds-map-composition` for maps.

Choose the deliverable before discovery: individual transformer sheets and
MV booklets are different from combined LV plans/workbooks, a one-row-per-
transformer voltage-drop overview, and staking/BOQ reconciliation. Read
[delivery modes and source truth](references/delivery-modes.md) when the request
mixes them. Staking is an XLSX submission, never printed booklet pages.

Map delivery: `printing-maps`.

For MV plan/profile sets, read [booklet workflow](references/mv-plan-profile-booklet.md)
before setup or batching.

For voltage-drop A4, choose `voltage_drop_pdf`. Discover the workstation Chromium configuration and verification contracts
before headless rendering. Use the declared project export route, then verify
online publication before web preview or delivery. See `ds-lv-voltage-drop`
for calculation and remediation. Distinguish saved voltage-drop analysis JSON from the A4 report PDF in the live
output catalogue. Verify the installed build's declared outputs and actual
artifacts before claiming delivery; a successful PDF render is not a complete
engineering verdict or online publication.

## Establish the assignment and host

Pin project, sources, audience, papers, formats and destination. “All” means
complete inventory, resolved pagination and explicit exclusions. Keep receipts
and QA coverage with the delivery.

Use a signed-in headless server/workstation. Check `auth.status`; pass
`--project` on every call. Discover
`report.project.export`, project output settings, `report.layout.*`, and
`data.project-cache.status|seed`. Pair a desktop only for app-owned operations;
never borrow its credentials or lane. Native exports read exact MV heads;
model publication is separate. District/project sheets may use a held `.dsgrid`
and staged designs; see `ds-map-composition`.

Distinguish local proof, saved recipe, queued publication and verified online
delivery. Preserve authorization; ask only for added material scope. One writer
owns rooms and recipes.

## Prepare sources and context

Inspect design/model inventory through its owner; verify CRS, extent, attributes
and identities. Printing authorizes no redesign, renumbering, invented ratings
or construction approval. Preserve infrastructure, dirty rooms, model versions
and unrelated outputs.

Use `ds-assets` for governed documents/logos and their digests. Never guess
contract numbers, signatures or approvals, or borrow another project's branding.

Acquire context for the printed extent and purpose, including margins and
disconnected clusters. Ready-empty, incomplete and stale differ. Seed through
governed project context; reads never fetch missing providers silently.
National bundles are machine holdings; buildings and contours are project
context. Do not seed every dataset by habit or substitute reference
transformers for the project network. Check carried/omitted layers, coverage,
freshness and digests; exclude optional layers only if the map's purpose survives.

See [sources and context](references/sources-and-context.md).

## Author sheets from the contract

Start with the governed standard; discover `report.layout.schema` and
`report.export`. For LV, read [standard sets](references/lv-standard.md).
Preserve source identity/revision, globals, live-map styles, unselected output
policy and exceptions. Compose each paper independently; a reduced A0 is not
readable A3. Inspect the recipe before approval.

Read live schema and optimistic revision. `report.project.export` supports local
same-paper recipe proofs (discover inputs): they keep provenance, leave saved
recipes intact and cannot publish. Save a governed recipe before publication.
Read back ambiguous writes before retrying.

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
- Fit maps around titles, scale, legend and furniture. Show existing and
  proposed MV with distinct labelled pens; keep hierarchy, centered names,
  requested scale, quiet context. LV prints two ways, not mixed: standalone
  sheets categorical (cable, structure, existing/new), MV booklets bluish.
  For adjacent circuits, use [the focused-context policy](../../ds-map-composition/references/adjacent-networks.md)
  and verify multiple neighbours, edge circuits and crossings without changing
  focused schedules or voltage drop.

Sheets: [transformer sheets](../../ds-map-composition/references/transformer-sheets.md),
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

Review small sets completely; record template/outlier coverage for larger sets.
After a workflow change, run one blind
smaller-model trial: give the job and limits, not source or command syntax;
its real artifact attempt is the evidence.

## Deliver and recover

Batch the requested matrix through its owner; reuse only dependency-current
outputs. Check IDs, paper, revisions, hashes and receipts for missing,
duplicate, failed and stale members.
Keep A0/A3 multipage collections separate and preserve project-wide drawing
numbers on individual and grouped sheets, per the owner’s bundling contract.

Verify destination bytes and visibility. Queued online delivery needs remote
readback. Report paths, counts, QA coverage and material limitations.

Execution: [procedure](references/procedure.md),
[delivery record](references/delivery-record.md), [acceptance](references/acceptance.md).

Stops at: the human eye and the printer — visual acceptance and the physical
print job are the operator's; `ds` composes, renders and captures.
