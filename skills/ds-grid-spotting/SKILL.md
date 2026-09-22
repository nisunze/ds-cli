---
name: ds-grid-spotting
description: Plan DS Grid structure spotting once across a route, then diagnose and revise only affected angle-to-angle intervals and tension sections through the live ds CLI/MCP.
metadata:
  ds-chapters: grid-model
  ds-mcp-profile: grid
---

# Spot and revise a DS Grid line

Use the `ds` skill first. The model and its design policy are the authority for project preferences, prohibitions, span limits, clearance cases, and type weights. This skill defines the *order and scope of computation*, not engineering constants. Read the live descriptors and the exact model revision before each step. In MCP, use `ds_catalog`, the grid-model chapter's `describe` and `invoke`, or the typed grid profile; do not manufacture a command from this text.

## Initial pass: pay the large search cost once

1. Validate the one working model. Read `project_plan`, `spotting_graph`, the project policy and the structure catalog through `ds dsgrid run`. Fix termini, tap/T-off, transformer and eligible angle supports first. Preserve their stable IDs.
2. Ask `ds dsgrid describe --kind operations --id plan_optimum_spotting` for the current request. Build a request for each alignment from that model's authored IDs and rules. The initial optimization may search the full alignment. Independent sections between fixed anchors can use native parallel workers; `plan_optimum_spotting_batch` plans independent alignments against *one immutable revision*, returns ordered proposals/refusals, and never applies or numbers them. Use it where the model and server budget permit.
3. Apply accepted commands through the revision-pinned `dsgrid.apply-batch` door, with a dry run first. Keep one active local working model. A journal/package revision records edits; a governed project version is a separate intentional publication.
4. Read `spotting_graph` again. Its per-alignment/per-tension-section edges and chainage-numbering proposal are the support topology; every placed support must have an engineering number. Confirm actual stringing separately: a graph edge alone does not assert a conductor is strung.

Do not repeat the full optimization merely because a later report found a local failure.

## Subsequent passes: inspect and repair locally

1. Run `dsgrid.analyse.clearance` for the affected alignment and the correct vertical/horizontal case. It reports violating survey points, span endpoints, code, deficit, and unsolved sections. Read `spotting_graph` to locate each finding's incident tension section and the consecutive fixed angle/tap/transformer anchors enclosing it. Group adjacent findings by that anchor-to-anchor interval. Start with the smallest affected section; expand to the anchor interval when moving a support changes neighboring spans or section boundaries.
2. Use `compute_support_demands` on the incident tension section and `screen_structure_usage` with `structure_ids` for affected supports. A suspension with a negative signed weight span or upward reaction is an uplift finding, even when the capacity screen says `unknown`; that status is never a pass. First inspect whether moving/removing the valley-bottom support and landing supports on the faces can fix the geometry; only then compare a rated strain/H-pole/steel alternative. Check both adjacent spans and clearance after any move. These are DS proposals, not authoritative PLS-CADD strength. Use `terrain_anomaly_analysis` where a spike/dip or waterfall makes a clearance result suspect; retain legitimate steep terrain until surveyed evidence proves an error.
3. Revise position, type, or stringing through the current typed command or a validated engine command via `dsgrid.apply-batch`. Recalculate the changed section and adjacent support demands; then rerun clearance for that alignment and usage for affected structures. Continue only where the finding or boundary effect persists. Do not discard satisfactory intervals or restart at the first difficult point. The next fixed angle anchor is a clean restart boundary.
4. If a prohibited polygon makes an interval infeasible, keep ordinary proposed supports outside it. An explicitly admitted fixed angle/tap/transformer overlap is reported as an exemption. An opt-in `bounded_interval_fallback` may carry a crossing between two consecutive fixed anchors at a declared temporary span ceiling; retain its polygon/span warning and `partial_with_warnings` status. It does not establish structural strength. If exclusions are excessive, ambiguous, or require a semantic route decision, record the exact polygon and interval for engineer review instead of weakening all exclusions or repeatedly searching the whole line.
5. After each meaningful local edit, export through the strict DS/PLS gate and compare fresh native PLS-CADD clearance and structure-usage/strength reports against the same model revision. PLS-CADD 16.81 is the native strength authority. A refused export or absent native report leaves the DS result a proposal. Feed only measured differences back into the affected section or anchor interval.

`dsgrid.analyse.clearance` currently scopes by alignment, while `compute_support_demands` scopes to one section and `screen_structure_usage` can scope to selected structures. Grouping by angle interval is an orchestration rule; do not claim a nonexistent angle-interval CLI filter. Native batch planning is read-only and parallel; mutation and final numbering remain revision-gated.
