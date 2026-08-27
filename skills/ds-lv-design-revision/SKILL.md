---
name: ds-lv-design-revision
description: Revise one transformer’s LV design through `ds`, preserving built infrastructure and staging before save. For LV extensions and drafting reruns.
---

# Revise an LV transformer without redesigning installed work

Use `ds-project-context` only to establish or deliberately switch the project.
Then work through the live `ds` command contracts; do not copy a remembered
flag surface into the workflow.

The application owns the state. The CLI requests the smallest typed
transition and reads its receipt; it does not keep a second transformer, map,
selection, process setup, or version history. Svelte renders app-owned state,
WASM/Rust provides deterministic geometry evidence, and the model/operator
decides what that evidence means. Never drive the UI as a puppet or rebuild
raw API/IndexedDB steps in the CLI.

## Preserve the constructed network first

1. Confirm the exact active project and transformer, then read the current
   room summary and dirty/server version state.
2. Before introducing any new geometry, dry-run a property update over every
   existing feature to set `drafting_status=approved`. Apply it locally and
   re-read the room. Require every pre-existing layer count to be approved.
   This is the freeze boundary: drafting presets may replace or generate
   draft work, while already constructed infrastructure remains authoritative.
   Existing assembly behavior at a genuine new contact may still apply.
3. Do not save yet. Approval, candidate creation, processing, inspection, and
   version verification are one staged revision until the user authorizes the
   specific project write.

## Compare linework as evidence, not truth

Copy the authoritative design line layer and the exact incoming named layer to
local layers. Use the line comparison/difference operation with explicit,
conservative tolerances and keep its bounded receipt.

The numeric result is a candidate layer. Interpret it with the visible map and
the task’s engineering meaning:

- Coincident or closely aligned portions may be the same surveyed asset.
- A crossing or criss-cross is not covered merely because it intersects.
- A parallel line is intrinsically ambiguous. It may be survey drift, a valid
  opposite-side-road feeder, or new construction beside an existing line.
- Inspect endpoint connectivity, intended junctions, continuity through the
  wider network, road-side context, known construction status, provenance,
  and relevant attributes. A tolerance sweep can show sensitivity but cannot
  decide semantics.
- Heal an endpoint only when it represents an intended electrical contact and
  the gap is credible measurement/drafting error. Never widen a tolerance just
  to make a visually inconvenient difference disappear.

When meaning remains ambiguous, leave the piece in a local review layer and
focus it once with `ds map zoom --layer <layer-id>`, then ask the operator. Do
not promote it. This conservative stop is part of the workflow, not a tool
failure.

## Promote and process only the accepted extension

Promote reviewed candidate lines through the application’s existing
local-selection-to-design creation contract. New rows must enter `lv_lines` as
`draft`. Re-read the staged room and require both invariants:

- every pre-existing feature remains `approved`;
- only the accepted new line rows are `draft`.

Discover the project Fast LV setup with the smallest adequate inventory
`--limit` before changing it; selected sources and effective settings remain
complete even when available-source suggestions are truncated. Configure the
exact semantic customer source requested. “Additional customers from
`edcl_customers_survey`” means retain current design customers and add that
survey Point layer; it does not mean the similarly named as-built survey.
Before processing, materialize survey data through the application-owned
Working Area with `ds map survey download --entire-project`. Require the exact
active project, `working_area.fullProjectLoad=true`, the intended form's
bounded cached count, and `rows_returned=0`. This is not cross-project
migration: migration changes project records; Working Area downloads the
active project's survey forms into the desktop cache used by WASM.

Select the drafting preset through the application-owned project setup, first
with a dry run, and verify the returned layer key and local count.

Run the processor only after these receipts agree. Treat a selector that
matched nothing, a reported fallback from differential to full processing,
unexpected feature-count contraction, or warnings about approved assets as a
hard inspection gate. Processing stages locally; it is not a save.

Once a derived local layer has been promoted or rejected, refresh `ds map
view` and remove only its exact `this_session=true` layer id. Do not leave
intermediate current/incoming/difference layers in the map, and never remove
pinned project layers.

## Begin a version deliberately; save the working copy separately

Before entering a new engineering edit, decide explicitly whether this work
needs a new version. If it does, require a clean saved room and run `ds map
design version begin --transformer <name> --reason <text> --yes`. The caller
supplies only transformer name and reason. ds-brain assigns the next
`v<number>` and writes bounded version metadata; no layer/report data crosses
that operation. Never ask the user to type a version number.

For an upload that will overwrite an existing transformer, enable the explicit
version option and supply its required reason only when the operator intends
that bump. Never infer version intent from Replace, Save, Process, Report, or a
dirty room. Deliberate v0 means no explicit version has yet been begun.

Immediately before save, re-check the project, transformer, dirty room,
approved/draft split, customer source, preset, and process receipt. Save only
with authority for that exact transformer and the CLI-required confirmation.
Save advances an optimistic concurrency generation only. It must not create a
version, change version metadata, or stamp `v_first`/`v_last`. Reporting must
not create a version either.

Feature lineage is authored by the editing/compute boundary under the current
deliberate version, before Save transports the room. At v1 and later, require
known-example evidence: an unchanged row preserves both stamps, a changed row
preserves `v_first` and advances `v_last`, and a new row receives the current
version in both fields. Until that edit-boundary receipt exists, treat lineage
claims as unverified and report the missing contract through the `ds` skill's
live feedback procedure; do not resurrect save-time stamping.

If a necessary live command is absent or refuses the governed operation,
follow the `ds` skill's feedback procedure. Do not compensate with source
inspection, raw store access, direct bridge calls, or UI automation during a
delivery run.

When this proven single-transformer flow is extended to the Huye backup batch,
normalize a terminal Windows copy suffix ` (2)` out of the archive filename
stem before transformer matching. Treat the suffixed and unsuffixed files as
one transformer collision/revision choice, never as two transformer names.
