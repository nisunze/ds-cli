# Coding order

The sequence in which [OPEN.md](OPEN.md) slices are started, by owner priority and then impact. This file holds **order only**: scope, evidence and proof for each slice live in the linked OPEN.md entry. When a slice lands, delete its row here and its entry there in the same commit. Owner decisions that gate a slice are in [OPEN-X.md](OPEN-X.md); operational acceptance after the code lands is in [SEEDING.md](SEEDING.md).

How to pick: take the first row whose repository is free (no other agent's uncommitted work in the files it touches) and whose "after" column is satisfied. Rows in different repositories may run in parallel; within one repository, one slice at a time, landed on `run` before the next. Blocks 12–13 are Codex's (OPEN.md "Codex main"); they are ordered here for visibility, not for another agent to start.

Impact scale: **lost** = work or data silently lost or unpublished; **wrong** = incorrect engineering or output reaches a client; **blocked** = a delivery path has no way through; **slow** = it works but costs hours; **debt** = duplication, drift or TypeScript to retire.

## Tier 0 — Make every later proof possible

| # | Slice | OPEN.md | Repos | Impact | After |
| ---: | --- | --- | --- | --- | --- |
| 1 | Green `run`: Grid station-move test, LV disjoint-feeder test, profile-review marker, formatting hunks, printing toolbar test, reporter XLSX column drop | [§2](OPEN.md#2-green-run-and-release-gates) | network, kernel, ds-web, reporter | blocked — no slice can prove itself on a red suite | — |
| 2 | Artefacts readable without a credential: short-lived signed URLs via ds-brain (`c9df4098`) | [§1](OPEN.md#1-source-integration-and-release-code), [§7](OPEN.md#7-report-compute-export-and-combined-output) | ds-brain, kernel | **security**: every listed report is public by URL | — |

## Tier 1 — Owner priority: `ds` reports its failures and everyone files feedback

| # | Slice | OPEN.md | Repos | Impact | After |
| ---: | --- | --- | --- | --- | --- |
| 3 | Teach the duty: `ds` skill "Report as you go", `feedback.submit` wording, MCP `FEEDBACK_GUIDANCE` | [§25](OPEN.md#25-cli-reliability-reporting-and-proactive-feedback-priority) | ds-cli | owner asks for feedback constantly; text-only, cheapest win | — |
| 4 | Store accepts `ds` events: `cli_command` / `cli_rollup` sources, `service = "ds"` | §25 | ds-sre, ds-brain | `ds` failures are invisible today | — |
| 5 | Reporter in `ds-client-core`: classify, sanitize, outbox, dedupe, rollup | §25 | kernel | as above | 4 |
| 6 | Wire CLI and MCP; `ds doctor`; `next.feedback` in envelopes | §25 | ds-cli | as above | 5 |
| 7 | Invocation journal and `ds feedback draft --last` | §25 | kernel, ds-cli | makes filing one step | 6 |
| 8 | Automatic bug on crash; link closed feedback to SRE events so it stops reappearing | §25, [§22](OPEN.md#22-operations-product-code) | kernel, ds-cli, ds-brain | crashes become reports without anyone asking | 6 |
| 9 | SRE overview row for the `ds` client | §25 | ds-sre, ds-brain | owner sees CLI health at a glance | 6 |

## Tier 2 — Lost and wrong: data integrity and client-facing correctness

| # | Slice | OPEN.md | Repos | Impact | After |
| ---: | --- | --- | --- | --- | --- |
| 10 | Self-healing publication: 593 sealed artifacts stuck behind a dead-process lock (`d50cc39d`); headless export that never reached the cloud (`74bdd848`); bounded cloud-head reads | [§8](OPEN.md#8-sync-publication-and-edge-outbox), §7 | kernel (sync runtime), ds-cli | **lost** — computed reports silently unpublished | 1 |
| 11 | Preserve authored LV designs: headless save without reprocessing (`6ed42257`), batch process overwrites approved poles and cables (`38bd5091`), non-destructive topology for legacy designs (`00ad9848`), keep supplied line numbers (`4bad5ef3`) | [§9](OPEN.md#9-lv-processing-sizing-and-headless-intake) | network, kernel, ds-cli | **lost** — approved design facts overwritten | 1 |
| 12 | LV span REG maximum never flagged; EDCL rejected three designs (`8b2d9545`) | §9 | network | **wrong** — client rejection | 1 |
| 13 | Transformer protection sizing: frozen/existing transformers unchecked (`4070556f`), `minimum_tr_size` leaves protection on the smaller row (`571c4b57`), 15 kV fuse ignored (`dcacdb90`) | §9 | network (sizing), reporter | **wrong** — reports break REG Tables 12–13 | 12 |
| 14 | Combined report: no partial archive on refusal (`4151b6b7`); Desktop and headless agree on MV content | §7 | kernel, reporter, ds-brain | **wrong** — incomplete output published | 10 |
| 15 | Model and transformer retirement without a verified backup; governed restore (`b1b3eec5`, `81ae43eb`) | §7 | kernel, ds-cli, ds-brain | **lost** — a tombstone can leave nothing to restore | 1 |
| 16 | Reporter/Desktop contract drift: `survey_layers_omitted` rejected (`56749e69`), printing export rejects the current result field (`88c03fbd`) | §7 | ds-web (Desktop), reporter | blocked — Desktop reports refused | 1 |
| 17 | Solar design runs refused: UI sends `concurrency: 0` (every ADERM city) | [§21](OPEN.md#21-windows-release-code) | ds-web, ds-solar | blocked — whole Solar design path; small fix | — |
| 18 | Project data never renders: uploads have no layer class, styles or tiles (`011ae5a6`); bare `.shp` drop freezes Desktop (`ad44f3a0`) | [§16](OPEN.md#16-map-data-datasets-and-screen-styling) | kernel, ds-web | blocked — uploaded data unusable on the map | — |
| 19 | Assets ingest through the paired Desktop fails (`e9b5858d`, `40b53f7f`); task attachment route (`e01cf456`) | [§17](OPEN.md#17-project-work-and-assets) | ds-web (Desktop), ds-cli, ds-brain | blocked — documents cannot reach tasks | — |
| 20 | Headless LV intake: Northern Hub upload (`14fd08e8`), Gisagara standards columns (`2adfa7d2`), 10-character DBF names (`83f3712b`), legacy unfenced rooms (`007c1624`), `Existing` → `existing_status` migration (`7f5ac170`) | §9 | kernel, ds-cli, ds-brain | blocked — client data cannot enter headlessly | 11 |
| 21 | Vector runner names every dropped feature in `skipped` | §16 | network | **wrong** — silent feature loss in `ds data vector` | — |

## Tier 3 — First-release controls named by the owner

| # | Slice | OPEN.md | Repos | Impact | After |
| ---: | --- | --- | --- | --- | --- |
| 22 | PLS-CADD control integration and freeze: Restore, reopen, AutoSag, reports, backup, delivery as `ds` commands; dialog catalogue packaged | §21 | ds-cli, kernel | first release | 1 |
| 23 | PLS restore acceptance: catalogued exit prompt, byte comparison (`1086e058`, `6f346aa3`); export plan accepts what convert cannot emit (`e258b287`) | [§11](OPEN.md#11-pls-cadd-exchange-round-trip) | network, ds-cli | **wrong** — a dialog count reported as success | 22 |
| 24 | Retire the interim PLS drivers | §21 | ds-cli, ds-work | debt | 22 |
| 25 | Provider-model library lifecycle through CLI/MCP and UI; missing-equivalent export admission | [§18](OPEN.md#18-global-ds-grid-library) | kernel, network, ds-cli, ds-web | first release | 1 |
| 26 | Native controls and JSON UI, one vertical per slice, TypeScript deleted (start with the vertical the current delivery touches) | [§23](OPEN.md#23-kernel-controls-json-ui-and-typescript-retirement) | kernel, ds-cli, ds-web | first release; debt | 1 |
| 27 | Windows Server public release integration; Windows headless PDF promotion denied (`7519c28b`) | §21 | ds-cli, ds-web | first release | 22 |

## Tier 4 — DS Grid engineering (Codex)

| # | Slice | OPEN.md | Impact |
| ---: | --- | --- | --- |
| 28 | Duty profiles undiscoverable, Model 2 cannot get its policy (`778bb573`); whole-model receipt cannot apply (`7a78525b`) | [§13](OPEN.md#13-spotting-codex-main) | blocked |
| 29 | Uplift and section-boundary loads (`523a7f6d`, `4ba8ab15`) | [§12](OPEN.md#12-ds-grid-engineering-codex-main) | **wrong** — 115 spurious uplifts |
| 30 | Stringing vs AutoSag (`e343d5ac`, 828 violations); creep shift field (`6226c0bb`, `3d1c735c`); OPGW sections | §12 | **wrong** |
| 31 | Spotting legality: forbidden 10–60° band (`fb163694`), planner/attachment clearance parity (`449b4f36`) | §13 | **wrong** |
| 32 | Structure usage and Method-2 artifacts (`3d2ddd7a`, `df1f53ae`, `4ff54666`) | §12 | blocked — gated in part by OPEN-X §12 |
| 33 | Clearance report case identity (`null °C`); existing conductor status on sections | [§15](OPEN.md#15-project-delivery-code), §12 | **wrong** on print |
| 34 | Bounded laptop spotting (`6b597153`) and Profile selected-case binding | §13, [§14](OPEN.md#14-plan-and-profile-authoring) | slow |

## Tier 5 — Slow paths and delivery cost

| # | Slice | OPEN.md | Repos | Impact | After |
| ---: | --- | --- | --- | --- | --- |
| 35 | MV map-inputs rereads the whole project: six minutes per capture (`f48bd93c`) | [§5](OPEN.md#5-print-renderer-and-booklet-code) | reporter, kernel | slow | 1 |
| 36 | Combined report waits on serial publication (`69f60e82`) | §7 | kernel, ds-brain | slow | 10 |
| 37 | Print sets too large: 481 MB / 714 MB PDFs (`e2cd543e`) | §5 | reporter | slow; delivery size | 1 |
| 38 | MV booklet owner integration without the Python bridge | §5 | reporter, ds-cli | debt; delivery depends on a retired bridge | 35 |
| 39 | Script-removal product gaps: run a download plan with a sha256 manifest, district from artefacts, MV/LV orphan join | §7 | kernel, ds-cli | blocked for agents | 2 |
| 40 | Data install: wetlands 422, bulk install 504, ground data never indexed; 10k caps and paging (`46dec14e`) | §16 | kernel, ds-cli, ds-web | blocked/slow | — |
| 41 | Voltage-drop analysis/Reporter one input digest | [§10](OPEN.md#10-voltage-drop) | network, reporter | **wrong** when stale | 11 |
| 42 | Typed single-sheet configuration copy (`d76f6e55`); template fixtures | [§3](OPEN.md#3-template-creation-code) | kernel, ds-cli | blocked for new projects | — |

## Tier 6 — Structural debt

| # | Slice | OPEN.md | Impact |
| ---: | --- | --- | --- |
| 43 | Country neutrality: CRS from the project, country identity, report i18n, guards | [§19](OPEN.md#19-country-neutrality-and-duplicated-rules) | **wrong** for every non-Rwanda project |
| 44 | KMZ styles into the governed resolver; no implicit Rwanda print defaults | [§4](OPEN.md#4-print-template-and-style-code) | debt |
| 45 | Duplicated rules: transformer sizing twice, report file stem, upload transport twice, archive grouping | §19 | debt; drift risk |
| 46 | Sidebar gateway bypasses and one Rust communication authority | §23 | debt |
| 47 | Never-landed work to adapt: archive-group readers (§7), Rust-owned selection (§16), PM model-version links (§17), style family (§23) | §7, §16, §17, §23 | debt — finished work not on `run` |
| 48 | Remaining PLS exchange fidelity items and the comparison harness | §11 | **wrong** in edge cases |
| 49 | PM/Assets lifecycle, Board navigation, Assets index latency, server monitor | §17, §22 | slow |

## Not now

Every OPEN.md entry not named above (minor print labels, batch-report JSON jobs, cloud-conflict review, Nyamagabe LV editing, remaining Profile interaction work and the like) ranks after row 49, in OPEN.md order, unless new evidence raises its impact.

[§24](OPEN.md#24-owner-feature-requests-cli-first-after-stabilization) — DS Workflows and the other owner feature requests wait until the owner admits them.
