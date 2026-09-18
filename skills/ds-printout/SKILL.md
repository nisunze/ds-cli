---
name: ds-printout
description: "Produce or resume DS transformer sheets, project atlases, district MV and custom-area printouts through visual QA and verified delivery. Use for a finished print set, not isolated styling or redesign. Default to a signed-in headless server; pair a desktop only where required."
metadata:
  ds-chapters: reports
  ds-mcp-profile: printing
---

# DS Printout specialist

Deliver a verified print set from current project data or reusable outputs.
Use `ds` for live operations and `ds-map-composition` for cartographic judgment.

## Establish the assignment and host

Recover project, source revision, audience, geography, papers, formats,
destination and earlier decisions. Build the subject/layout/format matrix.
“All” requires complete inventory, resolved pagination and explicit exclusions.
Keep authorization, input/recipe revisions, receipts, QA coverage and next
action in host task state or the deliverables directory.

Default to a signed-in headless server/workstation. Verify executable, lane and
fenced selected project through `auth.status` and `auth.project.use`. Discover
`report.project.export`, project output settings, `report.layout.*`, and
`data.project-cache.status|seed`. A paired desktop is needed only where the live
contract names an app-owned operation. Never borrow its credentials or lane.
Native project exports can acquire exact MV heads read-only; model publication
is a separate lifecycle. A district/project sheet can use a held `.dsgrid` file
and staged designs: see `ds-map-composition`.

Distinguish a local proof, saved project recipe, queued publication and verified
online delivery. Preserve existing authorization; ask only for material added
scope while continuing independent work. One writer owns rooms and recipes.

## Prepare sources and context

Inspect current design/model inventory through its owner. Verify CRS, extent,
attributes and canonical identities. Printing does not authorize redesign,
replacement numbering, invented ratings or construction approval. Preserve
existing infrastructure, dirty rooms, model versions and unrelated outputs.

Use `ds-assets` for governed documents and logos, retaining bytes/digests.
Keep unknown contract numbers or signatures unresolved. Never inherit approvals
or branding from an unrelated project. Missing required evidence remains a
named delivery limit.

Acquire only context needed for the printed extent and purpose, including
margins and disconnected clusters. Ready-empty, incomplete and stale differ.
Seed through the governed project context route; reads never fetch missing
providers silently. National bundles are machine holdings;
buildings and contours are project context. Do not seed every dataset by habit, or substitute
reference transformers for the project network. Check carried/omitted
layers, coverage, freshness and source digests. Optional exclusions are acceptable
only where the map’s purpose survives.

Full source rules: [sources and context](references/sources-and-context.md).

## Author sheets from the contract

Start from the published paper/family default, then compare the project recipe.
Keep source identity/revision when adopting a project copy. Preserve globals,
live-map styles, unselected output policy and per-subject exceptions. Compose
each paper independently: mechanically reducing A0 does not establish readable
A3. Prepare and inspect a concrete recipe before any required approval.

Read the live schema and current optimistic revision. `report.project.export` supports local same-paper
recipe proofs; discover their exact inputs. They keep governed source
provenance, leave saved recipes intact and cannot publish. Save a governed recipe before requesting publication. An error does
not imply rollback: read back ambiguous writes before retrying.

Follow these printing contracts:

- The drawn printable border owns independent schedule containers. Use measured
  edges, physical margins and explicit corner anchors. Join schedules with flow
  only when requested; snapping to information/legend retains the saved gap.
- Plain pole/customer schedules use selected available source columns and an
  explicit Design/AsBuilt phase. Preserve requested fields and consultant X/Y;
  do not invent coordinates or missing meter numbers. LV line number is not
  a default print column. Workbook/Excel presentation is for Network Information
  unless explicitly requested for schedules.
- Headings never widen columns. Default indexed headings use order letters with
  a full untruncated key above each panel. Never abbreviate headings by hand.
- Optimize the full network’s vertical fit. Adjacent split panels may have
  different heights and row counts; their actual footprints obstruct the map,
  not a padded rectangle. Flow followers remain with their movable root.
  Where authorized, schedule type may decrease by up to 30%; equal fits prefer
  larger type. Discover exact bounds and controls in the live schema; the
  bounded search is a heuristic, not a global optimum.
- Protect titles, scale, legend and furniture in map fitting. Include existing
  and proposed MV context with distinct pens and meaningful legend labels.
  Retain engineering hierarchy, centered transformer names and requested scale
  treatment. Keep geographic context subordinate to the network.

See [transformer sheets](../../ds-map-composition/references/transformer-sheets.md)
and [outlier adjustments](../../ds-map-composition/references/outlier-transformers.md)
for individual sheets and exceptions, and [drawing collections](references/drawing-collections.md) for compounded
A0/A3 sets and project-wide numbering.

## Render, inspect, refine

Render representative density/geography cases for every layout/format through
the intended route before batching. Check identity, source/recipe revisions,
print-context digests, carried/omitted layers and warnings. Hidden map labels are informational
placement notes; furniture overflow and missing schedule rows remain warnings.
A local proof establishes neither publication nor remote renderer parity.

Inspect actual pages and readable crops. Margin first, then title block,
legend/schedule edges, dense junctions, clipping, overlap, units, table scope and
existing/proposed line categories. Confirm required rows and fields survive,
adjacency remains and the full network fits vertically. Ground scale and
quantities in owner output. A successful render is not visual QA.

Revise, rerender and inspect. Review every page of a small set. For a large set,
review templates and outliers and record exact coverage; sampling never becomes
an every-page claim. For workflow changes, run one independent blind
smaller-model trial after implementation: give the job and limits, not source or
command syntax; its real artifact attempt is the evidence.

## Deliver and recover

Run the complete matrix through the owner batch route. Reuse only fresh outputs
with matching dependencies. Match identities to output IDs, paper dimensions,
revisions, hashes and receipts; detect missing, duplicate, failed and stale rows.
Keep A0/A3 multipage collections separate and preserve project-wide drawing
numbers on individual and grouped sheets. Follow the owner’s bundling contract.

Deliver files to the requested destination. For local-only work avoid publication
queues. For online work, queued is incomplete: verify cross-machine/server
readback. Report exact paths, counts, QA coverage and material limitations.

After interruption, revalidate identity, inventory, inputs and live contracts.
Protect dirty work and reuse unchanged dependencies. Recipe changes invalidate
prints, not source archives. Follow typed remedies and retryability; never strip
required data to appease an old validator. Report confirmed gaps through
`ds feedback submit`, not a local gap ledger.

Use [procedure](references/procedure.md), [delivery record](references/delivery-record.md)
and [acceptance](references/acceptance.md) for detailed execution and handoff.

Stops at: the human eye and the printer — visual acceptance and the physical
print job are the operator's; `ds` composes, renders and captures.
