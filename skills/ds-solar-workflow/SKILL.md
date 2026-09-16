---
name: ds-solar-workflow
description: Seed a project's governed Solar cities, then run and deliver single-city or explicit city-batch Solar results through deployed `ds`, not governed portfolios.
metadata:
  ds-chapters: project, solar
---

# Run the native Solar city lifecycle

Use the `ds` skill for installation discovery and live command contracts, and
use `ds-project-context` when the active project is not already established.
The selected native or explicitly paired project owns city authority. Use the
headless seed, capture, preparation and project lifecycle for server work.
Never reconstruct project inputs from browser storage, caches, APIs or fixtures.

For a governed aggregate with a portfolio id and membership revision, stop and
use `ds-solar-portfolio`. Repeated city contexts form an explicit city batch;
they do not become a portfolio merely because more than one city was run.

## Seed the project's cities first, if they are absent

When the project has no Solar cities yet, or the user asks to add governed
cities to it — "seed Solar into this project", "copy the standard cities",
"which cities would be added" — discover the seeding commands rather than
preparing a city that does not exist. Seeding copies authored city inputs from
a governed source into the project; preparation caches inputs for cities the
project already has. They are different requests.

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

Observe progress and completion through the discovered lifecycle commands for
that same run id. Do not switch to a newer run, merge receipts, or infer success
from one city. Cancel only when the user requests cancellation or continuing is
unsafe, and return the cancellation receipt.

## Read calculated evidence

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
available; `--system` selects the exact scenario. HTML includes cards and BOQ
tables; JSON also includes Plant chart options. These files are not publication proof.
The `solar-dashboard` MCP profile exposes both leaves.

When a user needs a document, export only an artifact declared by that run's
closed receipt to a new destination. Do not rebuild Markdown, charts, DOCX, or
JSON in the skill. An operator-interpreted final report is a separate explicit
import: use only the exact file the user selected and only the live command's
required confirmation. Import creates local review state only. Submit it for
publication only through the separate discovered final-submit command and only
when the user explicitly asks to submit that exact run and city. Do not present
a draft as final or claim an imported final is published.

Read Sync Center state through its discovered status command when publication
matters. A sealed local calculation remains valid while upload is pending or
failed; never call it published without a successful publication receipt.

## Keep the headless route separate

When the user explicitly wants a headless city run, discover the live input
capture, input preparation and artifact-runner descriptors. Capture establishes
the selected headless project's authority; never substitute a caller-supplied
project or root. Preparation may use only an already verified local reference
cache named through the live command. It must not be given a provider URL,
weather token, API key, browser cache, fixture input, or invented owner flags.
For a fresh server, seed the captured cities into a local Solar workspace and
discover reference acquisition. It derives the site/equipment request, uses the
signed-in native project, and verifies the cache before preparation. Never pass
provider credentials or substitute fixture bundles.

Keep the captured intake private and pass the resulting prepared directory
unchanged to the discovered offline artifact runner. Verify engine identity and
the closed preparation and batch receipts through the live CLI contracts. Never
combine these artifacts with a paired run receipt, and never describe cache-hit
preparation as a complete fresh-server route.

When live discovery proves a needed operation is absent, follow the `ds`
skill's feedback procedure. Do not compensate with direct bridge calls or a
skill-local program.

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
follow its exact selected-project and confirmation contract. Never treat a
local result as published, substitute current inputs for a captured run, or
automatically rebase a cloud conflict.
