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

Inspect model/design inventory through its owner; resolve CRS, extent, required
attributes and canonical identities. Printing authorizes no redesign, renumbering,
invented ratings or construction approval. Use `ds-assets` for governed archives
and `ds-grid-project-model` for model preparation, preserving bytes and digests.

Choose context for purpose and printed extent. Check source, coverage, freshness
and readiness — downloaded bytes prove none of them, and ready-empty, incomplete
and stale differ. Context is machine-local; seed what is missing through
`data.project-cache.seed` or the first print's `--seed`, never every dataset by
habit. Required missing data leaves delivery incomplete.

Full rules: [sources and context](references/sources-and-context.md).

## Author and prepare sheets

Start from the published paper/family default, compare the project recipe, and
follow `ds-map-composition` for measured schedule adjacency and furniture
alignment. Adopt the template into a project-owned copy keeping its source
identity/revision. Preserve globals, live-map styles, unrelated outputs and
per-subject exceptions; replace inherited wording/branding with sourced facts and
never inherit approvals. With no suitable template, author from the schema rather
than borrowing another project's facts.

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

Sheet furniture obeys three standing rules; full text and the schedule's field
list in [sheet furniture](references/sheet-furniture.md).

- Tables, schedules and legends **hug the sheet border at one 5 mm margin** and
  grow inward. Never float a frame in unused width, even when nothing overlaps.
- A heading **never widens its column**. `table.heading_mode` defaults to
  `auto_index`: order letters in the header, full names in the key above the
  panel. Never hand-shorten a heading or drop a column to fit one.
- A printed schedule **carries the fields its workbook carries**. Bind it to the
  exported table; drop only the as-built-only fields (`meter_type`, `nid`, phone
  numbers) when the project is not as-built, and never drop a column for width.

Inspect whole pages and readable crops before batching, checking clipping,
overlaps, missing layers, legend categories, units, table scope and furniture
alignment. A successful render is not visual QA. Review every page of a small set;
for a batch, templates and outliers, recording exact coverage — sampling never
becomes an every-page claim. Checklist: [visual QA](references/visual-qa.md).

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

Classify the failure and follow its remedy and retryability. Never repeat an
unchanged refusal or strip required content to appease an older validator.
Continue independent work and report the remaining dependency through feedback.

Follow [procedure](references/procedure.md) for the ordered steps by command
id; read [delivery record](references/delivery-record.md) when resuming and
[acceptance](references/acceptance.md) before release. Disclose missing references,
retain these rules and seek supported skill access. Keep one writer for Desktop,
rooms and recipes. Delegate immutable review only when permitted; reconcile results.
