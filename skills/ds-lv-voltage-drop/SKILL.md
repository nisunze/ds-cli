---
name: ds-lv-voltage-drop
description: Check an LV transformer's voltage drop against its country rule, read the year-by-year outlook and the proposed reinforcement, and explain them. For voltage-drop questions, compliance, reinforcement timing and cost.
metadata:
  ds-chapters: design, reports
---

# Read and explain an LV voltage drop

The calculation belongs to the Rust engine (method `ds-lv-vd/1`). `ds` runs it
and returns one structured result; the agent reads that result and explains it.
Never recompute a drop, a phase, a year or a cost yourself, and never soften or
dramatise what the result states.

## Get the result

1. Establish the project with `ds-project-context`; every call names it.
2. Discover the commands, don't recall them:
   `ds capabilities --search "voltage drop" --output json`, then
   `ds capabilities design.lv.voltage-drop --output json`. Use only declared
   inputs.
3. The input is a saved transformer with its project seeds, as the LV project
   export command writes it. Nothing is written to the project.
4. A processed transformer already carries its results (`vd_*` columns and
   `vd_summary` on `tr`). Report exports include them when the project's design
   exports select the `voltage_drop` output (on by default); the export says
   whether it reports them "as processed" or "recomputed at export", and why.

## Read it in this order

1. **Is it calculated?** `vd_summary.status: reserved` means the transformer's
   authoritative `transformer_nature` tag (fill-in or upgrade) excludes it:
   its existing network is not fully surveyed. Say so, quote
   `vd_summary.calculation`, and stop. The nature comes from the tag, never
   from the transformer's name. A transformer's own `vd_calculate` overrides.
2. **The verdict** (`vd_summary.verdict`):
   - `complies` — every customer is within the limit at the design year.
   - `complies_with_reinforcement` — fails as drawn, and the proposed
     reinforcement keeps every customer within the limit. Present it as an
     action to schedule, not a failure.
   - `does_not_comply` — customers remain that no allowed reinforcement saves
     (`sizing.infeasible`, each with its reason): a design decision.
   - `incomplete` — inputs are missing; name them from `report.issues`.
3. **The rule** — the standard, clause, limit and nominal voltage come from the
   project's rule set (`vd_summary.standard`, `criterion`). Quote them; never
   assume a country. A provisional rule says so (`rule_confirmed: false`).
4. **The outlook** — `sizing.schedule[]`, one entry per year:
   `as_drawn` (worst drop, customers over the limit, transformer loading) and
   `with_plan`, plus the changes first needed that year.
   `sizing.first_failing_year` is when the violation starts. The honest
   proposal is: within the limit until that year; from it, the utility
   reinforces as scheduled.
5. **The reinforcement** — `sizing.changes[]`:
   - `lv_line`: spans one size up, never above the configured cap
     (`vd_max_recommended_abc_mm2`); `span_node_ids` names the exact spans.
   - `new_circuit`: a second cable of the largest allowed size strung on the
     existing poles from the transformer to `junction_node_id`, with the line
     cut before `cut_node_id` ("2×70"); `customers_transferred` moves to it.
   - `service_cable`, and `transformer_change` (to kVA).
   Costs use seeded prices when every size is priced, else cross-section ×
   length (`cost_basis`). Recommendations never change the design.

## Load basis — what the numbers assume

A customer's load in year y rises from its category's initial to its
saturation load over the years to saturation, times household growth. The
values come from the rule set's `category_loads` (voltage drop only), else the
project workbook's `cust_category` (`peak_power_w`, `initial_power_w`).
`report.parameters.sources` names where every value came from — quote it
when asked "why". No simultaneity factor is applied: the rule set's
`diversity` text states why.

## Run a scenario

The run-only inputs of `design.lv.voltage-drop` change what one check assumes,
never the project: `--year` (design year), `--outlook` (years to check),
`--no-outlook`, `--load <Category>=<saturation W>[:<initial W>]` (repeatable)
and `--set vd_<name>=<value>` (repeatable, `vd_*` settings only). Write each
scenario to its own `--out`, run the unchanged input once as the baseline,
and compare the rows side by side. The result's `scenario` block lists every
override; quote it with the numbers.

## Explain, don't decide

- Lead with the verdict and the first failing year, then the plan and its
  cost, then the assumption that drives it most (the per-customer load).
- A different load basis, design year or cap is a scenario: run it through the
  declared inputs, show both results side by side, and let the owner decide.
- Report a refusal by its code and remedy. A missing capability is feedback
  (`ds feedback submit`), never a workaround.

Stops at: the owner's engineering decision. Which scenario, load basis or
reinforcement plan to adopt is theirs; hand over the baseline and scenario
results side by side, with the rule-set sources quoted.
