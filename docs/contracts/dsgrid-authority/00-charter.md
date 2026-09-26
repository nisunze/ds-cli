# 00 — Charter and conventions

Every contract in this folder inherits this one.

## Principles (owner rulings, 2026-09-20)

1. **Server-first.** A design operation is landed when it runs through the kernel with authority `none` (file operations) or `headless_project` / `headless_user` (governed reads and writes), is discoverable through `ds capabilities`, has `--help` as its contract, and is reachable by MCP through the same descriptor. `requires: window` is a defect for any design, grid, PLS, feature-code, clearance, spotting or strength operation. The application calls the same kernel commands and only previews/adjusts.
2. **DS Grid proposes, PLS-CADD confirms — until verified.** DS Grid's stringing, strength and clearance results are *proposals* with a verification level. PLS-CADD 16.81 is the authority for delivery verdicts. A DS Grid receipt never presents a proposal as a verdict.
3. **Versions are first-class.** Every PLS member read or written is named with `TYPE / VERSION / UNITS` in the receipt (DON 57, CRI 94, FEA 15, NUM 14, PPS 57, STRUCT 13, PLS-POLE 22, XYZ 5, TIN 5 for 16.81). Target version 16.81; no 20.01 writer exists and none is written; a 20.01 source (DON 72) is converted to 57 through the existing path and said so.
4. **Standards are data.** `standards/pls-feature-codes.v1.json` (feature codes, clearances per voltage class) and `standards/reg-mv-structure-rules.v1.json` (REG v7 spans, structure sequence, poles, foundations, HV rules, **reservations**) are read by the engine; values are owner-issued and never edited by code. Assumed values carry `assumed: true` and appear in receipts as assumptions.
5. **Reservations are part of the rule.** REG span heuristics (65 m wooden / 70 m other; Table 15) are respected where reasonable; obstacles and feature codes justify longer spans up to 150 m with special structures (14 m, steel, H-pole, H-pole long-span with stays — modelled today for 2 stays only, three-pole); above 150 m needs an explicit design decision. A justified long span is reported as *above heuristic, justified: <code>*, never as a violation. Every span report shows **wind span and weight span** per load case with their definitions, because the client confuses them.
6. **Receipts, not prose.** Every command returns a typed, bounded receipt with explicit truncation; refusals are named codes with remedies; nothing is silently dropped, relabelled or re-shaded.
7. **Everything through the kernel.** No CLI-side algorithm, no direct API call, no ds-web source in the server build. Work in ds-server worktrees; never on the Windows box's live checkout.
8. **No confidential test data in the repo.** The Nyamagabe workspace and the TBEA package are used for acceptance from the Drive / Desktop sandbox paths, never copied into a repository; fixtures are synthetic or public.

## Conventions

- Command ids follow the existing domains: `dsgrid.*` (engine and packages), `dsgrid-exchange.*` (PLS ↔ DS Grid), `pls.*` (native files), `design.*` (governed project objects). New sub-domains proposed here: `dsgrid feature-codes`, `dsgrid criteria`, `dsgrid structure-type`, `dsgrid spotting`, `dsgrid verify`, `design decision`.
- Every write takes `--yes`; every mutation of a package is revision-pinned (`dsgrid apply` semantics: dry run, exact revision gate, new package or in-place working copy with journal).
- Acceptance scripts in each contract run against the Nyamagabe workspace or its sandbox; results (receipts) go to the handover.
- Contracts live with the code: copy each landed contract into the owning repo's `docs/contracts/` (kernel for engine/exchange/commands, ds-brain for governed objects) in the same commit.
