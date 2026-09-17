---
name: ds-solar-workflow
description: Seed a project's governed Solar cities, then run and deliver single-city or explicit city-batch Solar results through deployed `ds`, not governed portfolios.
metadata:
  ds-chapters: project, solar
---

# Run the native Solar city lifecycle

Use `ds` for installation and live contracts, and `ds-project-context` if the
project is unknown. Native or explicitly paired projects own city authority.
Use headless seed, capture, preparation and project operations on Server.
Never reconstruct inputs from browser storage, caches, APIs or fixtures.

Discover native Server requests through `solar.application.schema`, then execute
with `solar.application` (MCP profile `solar-application`). Preserve project and
workspace identity. Completed progress survives restart; inspect interrupted
work before retrying with a new run ID.

Use `ds-solar-portfolio` for governed aggregates; a city batch is not one.

## Seed the project's cities first, if they are absent

Discover seeding when cities are absent or the user requests governed cities.
Seeding copies authored inputs into the project; preparation caches inputs for
existing cities.

Preview first and inspect every city action, source/destination digest, input
document and excluded media reference. Echo the returned plan digest to apply.
By default changed cities remain untouched. When the user explicitly requests
replacement, use the live overwrite option on both preview and apply: it binds
replacement and removal of obsolete input documents to the preview. Retain the
plan as the reviewable record; existing authorization need not be asked again.
A changed digest requires a fresh preview and inspection, never blind retry.

Report applied/skipped cities and committed document counts exactly. Resolve
classified sizing tables and city maps through exact project tag assets, and
transformer maps through transformer assets. Discover the Solar network form
commands: resolution seeds editable values, even when every geographic source
is missing. Save manual overrides through that form. Solar owns copies of the classified
inputs and maps; refresh them explicitly and preserve operator edits. Discover
the map-copy command for manually composed images. It retains verified local bytes for offline runs; draft packages include these maps with relative image links. Verify copied byte hashes
and publish changed inputs through normal sync before claiming them online. Maps are optional for calculation. Read the
live form contract for customer-category columns and engineering inputs.

## Freeze the city request

Discover the existing cities and retain the exact context ids in
the operator's order. Refuse duplicate, missing, or substituted cities. Read
current readiness rather than assuming a previous run left usable inputs.

Prepare only the requested contexts. Preparation may refresh authenticated
weather or reference data inside the paired application; the skill never
handles provider credentials, cache paths, or raw cache records. Require a
successful prepared receipt for every requested city before launching compute.
If any city is not ready, report its exact refusal and do not silently run the
successful subset.

## Launch and observe one exact run

Discover the city-capable run command and pass only the explicit contexts and
options authorized by the user. Omit optional chart, concurrency, language, or
serial settings unless the live contract and request supply them. Treat the
launch response as a job receipt and retain its exact run id.

Observe that exact run through discovered lifecycle commands. Never substitute
a newer run, merge receipts, or infer batch success from one city. Cancel only
on the user's request or when unsafe; retain the cancellation receipt.

## Read calculated evidence

The prompting draft is the endpoint of automated delivery. Compose a final
document only as a separate deliberate task under close human supervision.

Use the bounded result reader for a small semantic field projection. Use the
named dashboard-section reader when the question needs Site, Plant, BOQ,
Finance, or another canonical report-input section. Preserve city id, run id,
input/result digest, units, and any unavailable markers with reported values.
Never convert a missing or malformed value to zero, and never compute a
portfolio total from city reads.

For headless `solar.results.read` contract 2, discover its live descriptor and
provide the closed batch directory plus explicit project, batch run and city.
`solar.dashboard.compose` uses the same verified source and writes JSON and
standalone HTML to a new private directory. Site, Plant, Finance and BOQ are
available; `--system` selects the exact scenario. These files are not publication proof.
The `solar-dashboard` MCP profile exposes both leaves.

When a user needs a document, export only an artifact declared by that run's
closed receipt to a new destination. Do not rebuild Markdown, charts, DOCX, or
JSON in the skill. For an existing Server run, discover application operations
`documents`, `batch_artifact_read`, and `report_bundle`: inventory, verified
reads, and portable prompting packages. Discover the workspace and retain
returned batch pins. Read each schema before execution. Paired exports are
not Server delivery; their Desktop refusal does not prove headless absence.

An operator-interpreted final report is a separate explicit
import: use only the exact file the user selected and only the live command's
required confirmation. Import creates local review state only. Submit it for
publication only through the separate discovered final-submit command and only
when the user explicitly asks to submit that exact run and city. Do not present
a draft as final or claim an imported final is published.

Discover Sync Center status when publication matters. Pending or failed uploads
do not invalidate sealed calculations; only a published receipt proves sync.

## Keep the headless route separate

For headless city runs, discover capture, preparation and artifact-runner
contracts. Preserve captured project/root authority. Prepare only from a
verified local reference cache named by the live command, never provider URLs,
weather tokens, API keys, browser caches, fixtures or invented flags.
For a fresh server, seed the captured cities into a local Solar workspace and
discover reference acquisition. It derives the site/equipment request, uses the
explicit project authority, and verifies the cache before preparation. Never pass
provider credentials or substitute fixture bundles.

Preparation also emits a validated `ds.solar.server-submission/v1` envelope.
For shared runtime work, discover `server.solar.submit` and use that envelope
unchanged, not the prepared input or publication claim.

Keep intake private; pass prepared directories unchanged to the artifact
runner. Verify engine identity and closures. Never mix paired receipts or
claim cache-hit preparation proves a fresh server.

Report proven absent operations through the `ds` skill's feedback procedure;
never bypass them with bridge calls or skill-local programs.

## Offline project work

For headless local authoring and draft delivery, discover `solar.project.init`
and `solar.project.city.create` for an editable city, or `solar.project.seed`
for complete intakes. Creation accepts absent geography and incomplete inputs;
repeating it preserves existing edits. Use the city read/write and network seed
form commands to compose the inputs, then `solar.project.run`.
These use a private local workspace and existing
verified reference cache; no Desktop or cloud access is required. Inspect
`solar.project.result` and `solar.project.outbox` before discussing publication.
Discover `solar.project.sync` for native authenticated background publication;
pass its explicit project and lane, and follow its confirmation contract.
Detached workers retain that project and leave saved selection unchanged. Never treat a
local result as published, substitute current inputs for a captured run, or
automatically rebase a cloud conflict.
