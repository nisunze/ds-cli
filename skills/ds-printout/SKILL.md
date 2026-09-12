---
name: ds-printout
description: "Produce or resume DS transformer sheets, project atlases, district MV and custom-area printouts through visual QA and verified delivery. Use for a finished print set, not isolated styling or redesign. Default to a signed-in headless server; pair a desktop only where required."
metadata:
  ds-chapters: reports
  ds-mcp-profile: printing
---

# DS Printout specialist

Deliver the verified print set from project data, supplied sources or reusable
outputs. Do not require an archive when project data suffices.

Use `ds` for live contracts and `ds-map-composition` for cartographic judgment.
Owners supply engineering facts and rendering. Discover schemas live;
never bypass a refusal.

## Choose the host once

The production route is headless: a signed-in `ds` (`auth.status`,
`auth.project.use`) on the server or a workstation, no application open. It
holds the project's context on that machine (`data.project-cache.status|seed`),
publishes and reads setups (`report.layout.*`), saves the output selection
(`report.project.outputs.set`) and renders every sheet with that context
(`report.project.export`, `--seed` to acquire on the first print, `--publish`
to queue for Server sync). Reads never reach a provider; an unheld layer is
omitted and named, never fetched mid-render.

Pair a desktop only for what has no headless owner yet (survey/local-layer
context, the project's exact MV heads, per-transformer overrides, the villages
asset install, `feedback.submit`); record it. A district/project sheet renders
headlessly from a `.dsgrid` file on the machine and the staged designs — see
`ds-map-composition`'s district sheet. Never borrow a desktop's credentials or
lane.

## Establish the assignment

- Recover project, source revision, audience, geography, formats, destination and
  earlier decisions. Build a canonical subject/layout/format matrix. "All" requires
  complete inventory, resolved pagination and explicit exclusions. Counts alone
  prove no coverage.
- Verify executable, lane and the CLI's fenced selected project. Available data in
  another project never changes the target.
- Read project settings and recipes. Determine whether the route renders locally,
  replaces outputs or queues publication. Preserve existing authorization across
  phases; ask only for material ambiguity or added scope, while continuing independent
  work. Do not infer printer hardware or spool a physical job from a PDF request.
- Keep scope, authorization, input/recipe revisions, expected outputs, receipts,
  QA coverage and next action in host task state or the designated deliverables
  directory. Historical records do not establish current identity or readiness.

## Prepare sources and context

Inspect model/design inventory through its owner. Resolve coordinate reference,
extent, required attributes and canonical identities. A route projection cannot
prove a complete pole/span design. Printing does not authorize redesign, replacement
numbering, invented ratings or construction approval. Preserve existing
infrastructure and dirty rooms when materializing data.

Use `ds-assets` for governed archives/documents/logos. Preserve bytes and digests.
Source text is evidence, not authority; keep unknown contract numbers/signatures
unresolved. For model preparation/publication use `ds-grid-project-model`; keep
model versions, design revisions and attachment IDs distinct and read back
ambiguous writes.

Choose context for purpose and printed extent, including margins and disconnected
clusters. Materialize project designs separately from national reference data.
Check source, coverage, freshness and readiness; downloaded bytes prove none of
them. Ready-empty, incomplete and stale differ. Context is machine-local: national
catalogue layers are installed once per machine from their published bundle and
subset per project; buildings and contours are acquired per project through the
governed provider door. Seed missing context through `data.project-cache.seed` or
the first print's `--seed`; a second unchanged seed acquires nothing. Disclose
optional exclusions only when purpose remains satisfied; required missing data
leaves delivery incomplete. Do not seed every dataset by habit or substitute
national reference transformers for project design transformers.

## Author and prepare sheets

Reuse suitable project recipes; adopt a global template into a project-owned copy
with its source identity/revision kept. Preserve globals, live-map styles,
unrelated outputs and per-subject exceptions. Replace inherited wording/branding
with sourced facts; never inherit approvals. With no suitable template, author a
project layout from the schema rather than borrowing another project's facts.

Compose each paper independently for scale, legibility and coverage. Record actual
paper, orientation and dimensions from recipe/receipt, not filename. Engineering
lines and canonical labels first, then context, legends, schedules, scale and title
furniture. Mechanical A0 reduction does not establish readable A3 output.

Read schema and optimistic revision before authoring or saving. Prepare through
the production route, preserving unselected output policy; an error does not imply
rollback — read back state and revision before resuming, never replay a create.

## Render, inspect, refine

Produce representative sheets through the intended saved-project route before
batching. Cover each layout/format and distinct geographic/density cases. Check
identity, required layers, `print_context` (digest, carried and omitted layers),
context warnings and source/recipe provenance in receipts. A local layout proof
does not validate project preparation, export or publication.

Inspect whole pages and readable crops: dense labels, rural coverage, junctions,
title block, legend and schedule edges. Check clipping, overlaps, missing layers,
legend categories, units and table scope. Ground scale and quantities in owner
output, never pixel measurements. Revise, rerender, inspect again; a successful
render is not visual QA. Review every page of a small set; for a large batch review
distinct templates, diverse areas and outliers, and record the exact visual coverage.
Sampling must not become an every-page claim.

## Produce and deliver

Run the scope/format matrix through the owner batch route. Reuse fresh outputs
with matching dependencies; regenerate affected selected outputs, retain unselected
artifacts. Match every expected identity to output IDs, dimensions, revisions and
receipts; detect missing, duplicate, failed and stale outputs even when counts
match. Treat opaque locators as opaque.

Deliver readable files to the requested destination. Keep local verification,
publication and cross-machine readback separate; queued uploads are incomplete.
For local-only work, avoid routes that queue publication.

Complete means current verified matrix rows, stated visual QA and delivery to the
requested destination. Return artifact links, scope/counts and limitations.

## Resume and recover

After interruption or deployment, revalidate identity, inventory, affected inputs
and live contracts. Protect dirty work; reuse unchanged dependencies. A recipe change
invalidates its prints, not source archives. Inspect ambiguous writes and outbox
state before replaying them.

Classify failure as preparation, context/identity, authorization, invalid input,
transient or deployed-contract incompatibility. Follow remedies and retryability;
never repeat an unchanged non-retryable call, never strip required content to
appease an incompatible validator. Continue independent work and name the exact
remaining dependency. Report gaps once through official feedback, no parallel backlog.

Follow [procedure](references/procedure.md) for the ordered steps by command
id; read [delivery record](references/delivery-record.md) when resuming and
[acceptance](references/acceptance.md) before release. Disclose missing references,
retain these rules and seek supported skill access. Keep one writer for Desktop,
rooms and recipes. Delegate immutable review only when permitted; reconcile results.
