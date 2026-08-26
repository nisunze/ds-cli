---
name: ds-solar-workflow
description: Run and deliver single-city or explicit city-batch Solar work through deployed `ds`. Use for city results and reports, not governed portfolio aggregation.
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
