---
name: ds-solar-workflow
description: Seed a project's governed Solar cities, then run and deliver single-city or explicit city-batch Solar results through deployed `ds`, not governed portfolios.
metadata:
  ds-chapters: project, solar
---

# Run the native Solar city lifecycle

Use the `ds` skill for installation discovery and live command contracts, and
use `ds-project-context` when the active project is not already established.
The paired application owns project identity, city context, prepared inputs,
native run storage, and publication state. Do not recreate those from browser
storage, local caches, APIs, source files, or remembered command syntax.

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

Seeding is propose-then-confirm and the two halves are not interchangeable.
Preview first, always: it writes nothing, and its plan is what the operator
authorizes. Show that plan before asking for a decision, and show it whole —
the cities that would be created, the ones already present, the ones that
differ at the destination, the ones missing from the source, and every
warning. A destination that differs is never overwritten; report it and let
the operator decide.

Confirm by echoing the exact digest that plan returned, together with the same
selection. Never derive, recompute, reconstruct or guess that digest, and
never confirm a plan nobody was shown. If the digest is refused as stale, the
source or destination moved: preview again, show the new plan, and ask again.
Do not retry with a fresh digest as though the refusal were a transient error.

Report the applied and skipped cities and the documents written exactly as
returned. An idempotent second apply that writes nothing is a success, not a
failure. Network assets are reported and never seeded, so a seeded city still
needs its network maps uploaded through the normal path — say so rather than
implying the city is complete.

## Freeze the city request

Discover the relevant Solar descriptors and retain the exact context ids in
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

Use the headless artifact runner only when the caller already supplied a
prepared artifact directory and explicitly wants offline file output. That
route must not be used to extract paired-app inputs or bypass project context.
Verify its engine identity and closed batch receipt through the live CLI
contracts, and never combine its artifacts with a paired run receipt.

When live discovery proves a needed operation is absent, follow the `ds`
skill's feedback procedure. Do not compensate with direct bridge calls or a
skill-local program.
