---
name: ds-solar-portfolio
description: Run and inspect one membership-pinned governed Solar portfolio through deployed `ds`, not city batches.
metadata:
  ds-chapters: project, solar
---

# Work with one exact Solar portfolio

Use the `ds` skill for installation discovery and live command contracts, and
use `ds-project-context` when the active project is not already established.
Prefer the installed headless commands when available. Rust owns explicit
project authorization, governed membership, sealed city inputs, calculation,
native run storage, and publication state. Do not recreate any of those from
local files, browser storage, APIs, or remembered commands.

## Headless calculation and publication

For an approved native Server workspace, discover the closed application
operations through `solar.application.schema`; `solar.application` executes
them under an explicit project. This route supports portfolio calculation and
verified artifact inventories and reads without pairing. Discover each selected
operation rather than copying request schemas or credentials into the skill.

For typed MCP, use `solar-portfolio-batch` for calculation/publication and
`solar-delivery` for the governed catalog. Discover `solar.portfolio.list`, `solar.portfolio.calculate`, and
`solar.portfolio.publish` through live descriptors. The headless catalog uses
an explicit project and lane without changing the selected project. Retain
the exact portfolio id, ordered cities, and membership revision it returns.

Calculation consumes an already verified project city-batch directory and
its exact source run id. Use `ds-solar-workflow` to obtain a closed batch if
needed. Every frozen portfolio member must have its pinned result in that
source. A source batch may contain additional cities; the Rust owner consumes
only the governed ordered membership. Never rewrite a source closure or
silently drop a declared member to force acceptance.
Choose a new output directory and run id using the calculation descriptor.
The owner derives assumptions from sealed city artifacts and emits the
portfolio result, French APD draft, charts, and closed batch.

The calculation receipt says `publication: not_requested`. Publish the same
closed output through the discovered publication command only when authorized.
It rechecks governed membership and sends the exact sealed result, declared
drafts and chart bytes. For native application runs, discover `portfolio_publish`
and retain its exact batch id and digest. Require its output count along with
the published receipt; a historical result-only publication does not prove the
complete bundle was delivered.
Require a verified online publication receipt before claiming synchronization;
A successful local calculation is separate from publication and must be
reported separately.

Older installed surfaces may offer only the paired lifecycle below. Report
headless commands absent instead of pairing or changing authority to force
success on a server.

## Freeze the portfolio identity

Discover the portfolio-list command and read its current descriptor before
invoking it. Select the exact portfolio id the user intended and retain its
name, ordered city ids, and membership revision. Refuse a list row that omits
any of those identity fields. City order is semantic. Never substitute a
same-name portfolio, a superset, the newest artifact, or every city currently
available.

If the portfolio or a declared city is unavailable, stop with the returned
refusal. Do not silently shorten the membership. An ordinary repeated-city run
is not a portfolio run and must not be presented as one.

## Prepare every frozen member

Discover the Solar prepare command and invoke it for every ordered city id in
the frozen portfolio membership, including members that appear to have been
prepared before. The paired application owns freshness and may reuse valid
prepared input; the skill must not infer readiness from an earlier receipt,
local cache, or remembered run.

Require the prepare receipt to declare every frozen member ready before
starting the portfolio run. A missing, stale, partial, failed, or extra member
is a refusal: report it and do not calculate a shortened membership. Keep the
same frozen membership revision throughout preparation. Do not obtain source
data directly, inspect a cache, or add authentication options that the live
prepare descriptor does not declare.

## Run and observe

Discover the portfolio-capable Solar run command and invoke it with the frozen
portfolio id and exact membership revision returned by the selected list row.
If the desktop reports that the revision changed, list again and ask the user
to confirm the new ordered membership; never retry with the new revision
silently. Choose exactly one graph strategy declared by the live contract:
first member, round-robin, or one exact member of the frozen portfolio.
Currency, horizon and discount rate are governed prepared-input facts, not
launch flags. Language and report intent belong to a later report operation;
do not invent or pass them to portfolio calculation.

Treat the launch response as a job receipt, not a calculated result. Use the
discovered lifecycle commands to observe that exact run id. A portfolio is
ready only when the run receipt reports a committed aggregate for every
declared member. Any missing, failed, stale, digest-mismatched, or extra city
means there is no valid portfolio artifact; report the complete refusal rather
than reading a successful subset.

## Inspect and export sealed output

For an on-screen answer, discover the bounded portfolio-result read command
and request only the sections needed. Keep the result's portfolio id,
membership revision, ordered members, input digest, result digest, currency,
and horizon with any figures you report. Distinguish portfolio ratios from
city means and minima; do not average LCOE, payback, or DSCR when the aggregate
labels a ratio-of-totals or consolidated-cashflow result.

Keep the sealed graph provenance with any graph claim. A v3 round-robin result
truthfully has no single representative city; use each available or unavailable
graph's declared member id. Never replace that null with the first city or
attribute every graph to one member.

For a file, use the discovered portfolio export command. Export only a result
or report declared by the same closed batch and choose a new destination;
never reconstruct an aggregate JSON or draft in the skill.

The paired lifecycle publishes the governed aggregate itself, from the run
that sealed it. The headless lifecycle uses the explicit publication command
described above.
Read the publication state on the run's own result receipt: a successful
calculation whose publication did not queue stays successful and says so
explicitly. Report that state with the result and follow its remedy; never
present it as a failed calculation, and never treat a sealed local result or a
city Sync Center row as proof the governed portfolio copy exists.

When the installed CLI lacks a needed operation, follow the `ds` skill's live
feedback procedure. Do not compensate with direct bridge calls, source-tree
inspection during delivery, or a skill-local program.
