---
name: ds-solar-portfolio
description: Run and inspect one membership-pinned governed Solar portfolio through deployed `ds`, not city batches.
metadata:
  ds-chapters: project, solar
---

# Work with one exact Solar portfolio

Use the `ds` skill for installation discovery and live command contracts, and
use `ds-project-context` when the active project is not already established.
The paired application owns project identity, governed portfolio membership,
prepared inputs, native run storage, and publication state. Do not recreate
any of those from local files, browser storage, APIs, or remembered commands.

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
silently. Supply currency, horizon, representative strategy, report
intents, or other assumptions only when the live descriptor explicitly offers
them and the user or governed portfolio supplies the values. Never invent
XAF, 25 years, a representative city, or report defaults.

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

For a file, use the discovered portfolio export command. Export only a result
or report declared by the same closed batch and choose a new destination;
never reconstruct an aggregate JSON or draft in the skill.

The application publishes the governed aggregate itself, from the run that
sealed it. There is no separate publication command to discover or request.
Read the publication state on the run's own result receipt: a successful
calculation whose publication did not queue stays successful and says so
explicitly. Report that state with the result and follow its remedy; never
present it as a failed calculation, and never treat a sealed local result or a
city Sync Center row as proof the governed portfolio copy exists.

When the installed CLI lacks a needed operation, follow the `ds` skill's live
feedback procedure. Do not compensate with direct bridge calls, source-tree
inspection during delivery, or a skill-local program.
