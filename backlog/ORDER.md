# Coding order

Start order for [OPEN.md](OPEN.md), following [owner priorities](PRINCIPLES.md#3-priorities-owner-2026-10-07). This file holds order and dependencies; scope, evidence and proof stay in OPEN.md. Delete a fulfilled row with its OPEN.md entry. [OPEN-X.md](OPEN-X.md) gates only the named decision; [SEEDING.md](SEEDING.md) owns configuration, deployment and installed acceptance.

Pick the first actionable row in your track whose touched files are free. Recheck the defect through installed `ds` and owning tests; use admitted parameters/seeds before coding. “After” names an actual prerequisite, not a wait for every earlier row. A red gate blocks its affected release, not unrelated work. Work on canonical `run`, one coherent slice per repository, prove/commit/push before the next; preserve occupied edits. Cross-repository rows are successive owning slices, not a mixed checkpoint.

Each touched control must finish native CLI/MCP → shared JSON UI → replaced TypeScript deletion, preserving result views/downloads and project/principal/revision fences ([principles](PRINCIPLES.md#5-kernel-cli-and-ui-owner-2026-10-07)). Verify held work before rewriting; file `ds` friction as you go. Finish urgent authorized delivery with deployed capabilities before more builds. Confirmed exposure, data loss or a current delivery blocker may interrupt this order.

Impact: **security** = exposed artifacts; **lost** = overwritten or unpublished work; **wrong** = incorrect engineering/output; **blocked** = no delivery path; **slow** = measured delivery cost; **debt** = duplication or drift. Repository shorthand: kernel = ds-command-kernel, network = ds-network, reporter = ds-network-reporter.

## Immediate safeguards and release prerequisites

| # | Slice | OPEN.md | Repos | Impact | After |
| ---: | --- | --- | --- | --- | --- |
| 1 | Credential-free artifact URLs: authorized short-lived signed downloads (`c9df4098`) | [§1](OPEN.md#1-source-integration-and-release-code), [§7](OPEN.md#7-report-compute-export-and-combined-output) | ds-brain, kernel | **security** — listed reports remain readable by bearer URL | — |
| 2 | Reproduced source gates and final native source pins: Grid/LV tests, profile-review marker, formatting, Web/CLI/Solar/reporter checks | [§2](OPEN.md#2-green-run-and-release-gates), [§1](OPEN.md#1-source-integration-and-release-code) | network, kernel, ds-cli, ds-web, ds-solar, reporter | blocked — repair each owning gate; pin coherent release sources | — |

## Priority 1 — `ds` reliability and proactive feedback

Proceed on the specification defaults; [OPEN-X §25](OPEN-X.md#25-cli-reliability-and-feedback) gates changes to those defaults. Reporter/outbox code can be proved offline while the store slice proceeds.

| # | Slice | OPEN.md | Repos | Impact | After |
| ---: | --- | --- | --- | --- | --- |
| 3 | Teach the duty: `ds` skill "Report as you go", `feedback.submit` wording, MCP `FEEDBACK_GUIDANCE` | [§25](OPEN.md#25-cli-reliability-reporting-and-proactive-feedback-priority) | ds-cli | owner asks for feedback constantly; text-only, cheapest win | — |
| 4 | Store accepts `ds` events: `cli_command` / `cli_rollup` sources, `service = "ds"` | [§25](OPEN.md#25-cli-reliability-reporting-and-proactive-feedback-priority) | ds-sre, ds-brain | `ds` failures are invisible today | — |
| 5 | Reporter in `ds-client-core`: classify, sanitize, outbox, dedupe, rollup | [§25](OPEN.md#25-cli-reliability-reporting-and-proactive-feedback-priority) | kernel | as above | — |
| 6 | Wire CLI and MCP; `ds doctor`; `next.feedback` in envelopes | [§25](OPEN.md#25-cli-reliability-reporting-and-proactive-feedback-priority) | ds-cli | as above | 4, 5 |
| 7 | Invocation journal and `ds feedback draft --last` | [§25](OPEN.md#25-cli-reliability-reporting-and-proactive-feedback-priority) | kernel, ds-cli | makes filing one step | 6 |
| 8 | Automatic bug on crash; link closed feedback to SRE events so it stops reappearing | [§25](OPEN.md#25-cli-reliability-reporting-and-proactive-feedback-priority), [§22](OPEN.md#22-operations-product-code) | kernel, ds-cli, ds-brain | crashes become reports without anyone asking | 6 |
| 9 | SRE overview row for the `ds` client | [§25](OPEN.md#25-cli-reliability-reporting-and-proactive-feedback-priority) | ds-sre, ds-brain | owner sees CLI health at a glance | 6 |

## Priority 2 — First-release controls

| # | Slice | OPEN.md | Repos | Impact | After |
| ---: | --- | --- | --- | --- | --- |
| 10 | PLS-CADD control integration and freeze: Restore, reopen, qualification, AutoSag, reports, backup, delivery as `ds` commands; dialog catalogue packaged | [§21](OPEN.md#21-windows-release-code) | ds-cli, kernel | first release | — |
| 11 | Provider-model library lifecycle through CLI/MCP and UI; missing-equivalent export admission | [§18](OPEN.md#18-global-ds-grid-library) | kernel, network, ds-cli, ds-web | first release | — |
| 12 | Native controls and JSON UI: choose one demonstrated missing vertical in the current delivery, prove it and delete replaced TypeScript | [§23](OPEN.md#23-kernel-controls-json-ui-and-typescript-retirement) | kernel, ds-cli, ds-web | first release; debt | — |
| 13 | Windows Server public release integration; headless PDF promotion denied (`7519c28b`) | [§21](OPEN.md#21-windows-release-code) | ds-cli, ds-web | first release | — |
| 14 | Retire the interim PLS drivers | [§21](OPEN.md#21-windows-release-code) | ds-cli, ds-work | debt | 10 + [installed Windows toolkit](SEEDING.md#21-windows-native-release) |

## Priority 3 — Measured loss, wrong output and blocked delivery

| # | Slice | OPEN.md | Repos | Impact | After |
| ---: | --- | --- | --- | --- | --- |
| 15 | Self-healing publication: dead-process outbox lock (`d50cc39d`); headless export never reaches cloud (`74bdd848`); shared-publication replacement (`fd5bce6a`); bounded cloud-head reads | [§8](OPEN.md#8-sync-publication-and-edge-outbox), [§7](OPEN.md#7-report-compute-export-and-combined-output) | kernel (sync runtime), ds-cli | **lost** — computed reports silently unpublished | — |
| 16 | Preserve authored LV designs: headless save without reprocessing (`6ed42257`), batch process overwrites approved poles and cables (`38bd5091`), non-destructive topology for legacy designs (`00ad9848`), keep supplied line numbers (`4bad5ef3`) | [§9](OPEN.md#9-lv-processing-sizing-and-headless-intake) | network, kernel, ds-cli | **lost** — approved design facts overwritten | — |
| 17 | LV span REG maximum never flagged; EDCL rejected three designs (`8b2d9545`) | [§9](OPEN.md#9-lv-processing-sizing-and-headless-intake) | network | **wrong** — client rejection | — |
| 18 | Transformer protection sizing: frozen/existing transformers unchecked (`4070556f`), `minimum_tr_size` leaves protection on the smaller row (`571c4b57`), 15 kV fuse ignored (`dcacdb90`) | [§9](OPEN.md#9-lv-processing-sizing-and-headless-intake) | network (sizing), reporter | **wrong** — protection does not match selected size/voltage | — |
| 19 | Combined output: no partial archive on refusal (`4151b6b7`); Desktop/headless MV parity; true combined A0/A3 PDFs and workbook (`42f3abd1`) | [§7](OPEN.md#7-report-compute-export-and-combined-output) | kernel, reporter, ds-brain | **wrong** — incomplete output published | — |
| 20 | Model and transformer retirement without a verified backup; governed restore (`b1b3eec5`, `81ae43eb`) | [§7](OPEN.md#7-report-compute-export-and-combined-output) | kernel, ds-cli, ds-brain | **lost** — a tombstone can leave nothing to restore | — |
| 21 | Reporter/Desktop contract drift: `survey_layers_omitted` rejected (`56749e69`), printing export rejects the current result field (`88c03fbd`) | [§7](OPEN.md#7-report-compute-export-and-combined-output) | ds-web (Desktop), reporter | blocked — Desktop reports refused | — |
| 22 | Solar design runs refused: UI sends `concurrency: 0` (every ADERM city) | [§21](OPEN.md#21-windows-release-code) | ds-web, ds-solar | blocked — whole Solar design path; small fix | — |
| 23 | Project data never renders: uploads have no layer class, styles or tiles (`011ae5a6`); bare `.shp` drop freezes Desktop (`ad44f3a0`) | [§16](OPEN.md#16-map-data-datasets-and-screen-styling) | kernel, ds-web | blocked — uploaded data unusable on the map | — |
| 24 | Assets ingest through the paired Desktop fails (`e9b5858d`, `40b53f7f`); task attachment route (`e01cf456`) | [§17](OPEN.md#17-project-work-and-assets) | ds-web (Desktop), ds-cli, ds-brain | blocked — documents cannot reach tasks | — |
| 25 | Headless LV intake: Northern Hub upload (`14fd08e8`), Gisagara standards columns (`2adfa7d2`), 10-character DBF names (`83f3712b`), legacy unfenced rooms (`007c1624`), `Existing` → `existing_status` migration (`7f5ac170`) | [§9](OPEN.md#9-lv-processing-sizing-and-headless-intake) | kernel, ds-cli, ds-brain | blocked — client data cannot enter headlessly | 16 for preservation-sensitive writes |
| 26 | Vector runner names every dropped feature in `skipped` | [§16](OPEN.md#16-map-data-datasets-and-screen-styling) | network | **wrong** — silent feature loss in `ds data vector` | — |
| 27 | PLS restore acceptance: catalogued exit prompt and exact restored bytes (`1086e058`, `6f346aa3`); export-plan/convert admission parity (`e258b287`) | [§11](OPEN.md#11-pls-cadd-exchange-round-trip) | network, ds-cli | **wrong** — a dialog count reported as success | — |

## Windows engineering track — independent of the server queue

Windows owns OPEN.md blocks 12–13 and their engineering consumers. This track proceeds independently; do not wait for the server queue. Source-backed fixes and measured differences continue while OPEN-X gates only the specified policy/material choice or acceptance claim.

| # | Slice | OPEN.md | Impact |
| ---: | --- | --- | --- |
| 28 | Duty profiles undiscoverable, Model 2 cannot get its policy (`778bb573`); whole-model receipt cannot apply (`7a78525b`) | [§13](OPEN.md#13-spotting-codex-main) | blocked |
| 29 | Uplift and section-boundary loads (`523a7f6d`, `4ba8ab15`) | [§12](OPEN.md#12-ds-grid-engineering-codex-main) | **wrong** — 115 spurious uplifts |
| 30 | Stringing vs AutoSag (`e343d5ac`); creep shift (`6226c0bb`, `3d1c735c`); OPGW sections; signed native criterion-case bindings | [§12](OPEN.md#12-ds-grid-engineering-codex-main), [§15](OPEN.md#15-project-delivery-code) | **wrong** |
| 31 | Spotting legality: forbidden 10–60° band (`fb163694`), planner/attachment clearance parity (`449b4f36`) | [§13](OPEN.md#13-spotting-codex-main) | **wrong** |
| 32 | Structure usage and Method-2 artifacts (`3d2ddd7a`, `df1f53ae`, `4ff54666`) | [§12](OPEN.md#12-ds-grid-engineering-codex-main) | blocked — implement source binding and truthful unknowns; [OPEN-X §12](OPEN-X.md#12-engineering-acceptance-and-material-authority) gates unresolved material choices and acceptance |
| 33 | Clearance report case identity (`null °C`); existing conductor status on sections | [§15](OPEN.md#15-project-delivery-code), [§12](OPEN.md#12-ds-grid-engineering-codex-main) | **wrong** on print |
| 34 | Bounded laptop spotting (`6b597153`) and Profile selected-case binding | [§13](OPEN.md#13-spotting-codex-main), [§14](OPEN.md#14-plan-and-profile-authoring) | wrong/slow |

## Priority 4 — Delivery cost and remaining blockers

| # | Slice | OPEN.md | Repos | Impact | After |
| ---: | --- | --- | --- | --- | --- |
| 35 | MV map-inputs rereads the whole project: six minutes per capture (`f48bd93c`) | [§5](OPEN.md#5-print-renderer-and-booklet-code) | reporter, kernel | slow | — |
| 36 | Combined report waits on serial publication (`69f60e82`) | [§7](OPEN.md#7-report-compute-export-and-combined-output) | kernel, ds-brain | slow | 15 |
| 37 | Print sets too large: 481 MB / 714 MB PDFs (`e2cd543e`) | [§5](OPEN.md#5-print-renderer-and-booklet-code) | reporter | slow; delivery size | — |
| 38 | MV booklet owner integration without the Python bridge | [§5](OPEN.md#5-print-renderer-and-booklet-code) | reporter, ds-cli | blocked — delivery still depends on a retired bridge | — |
| 39 | Script-removal gaps: execute download plan with SHA-256 manifest; district from artifacts; MV/LV orphan join | [§7](OPEN.md#7-report-compute-export-and-combined-output) | kernel, ds-cli | blocked for agents | — |
| 40 | Data install: wetlands 422, bulk install 504, ground data never indexed; 10k caps and paging (`46dec14e`) | [§16](OPEN.md#16-map-data-datasets-and-screen-styling) | kernel, ds-cli, ds-web | blocked/slow | — |
| 41 | Voltage-drop analysis/Reporter one input digest | [§10](OPEN.md#10-voltage-drop) | network, reporter | **wrong** when stale | — |
| 42 | Typed single-sheet configuration copy (`d76f6e55`); template fixtures | [§3](OPEN.md#3-template-creation-code) | kernel, ds-cli | blocked for new projects | — |

## Priority 5 — Remaining wider changes

| # | Slice | OPEN.md | Impact |
| ---: | --- | --- | --- |
| 43 | Country neutrality: CRS from the project, country identity, report i18n, guards | [§19](OPEN.md#19-country-neutrality-and-duplicated-rules) | **wrong** — non-Rwanda CRS, identity and report semantics |
| 44 | KMZ styles into the governed resolver; no implicit Rwanda print defaults | [§4](OPEN.md#4-print-template-and-style-code) | debt |
| 45 | Remaining duplicated rules: sizing, file stems, upload transport, archive grouping (fold into the owning fix when touched) | [§19](OPEN.md#19-country-neutrality-and-duplicated-rules) | debt; drift risk |
| 46 | Sidebar gateway bypasses and one Rust communication authority | [§23](OPEN.md#23-kernel-controls-json-ui-and-typescript-retirement) | debt |
| 47 | Never-landed work: verify ancestry and adapt archive-group readers (§7), Rust-owned selection (§16), PM model-version links (§17), style family (§23) | [§7](OPEN.md#7-report-compute-export-and-combined-output), [§16](OPEN.md#16-map-data-datasets-and-screen-styling), [§17](OPEN.md#17-project-work-and-assets), [§23](OPEN.md#23-kernel-controls-json-ui-and-typescript-retirement) | debt — finished work not on `run` |
| 48 | Remaining PLS exchange fidelity items and the comparison harness | [§11](OPEN.md#11-pls-cadd-exchange-round-trip) | **wrong** in edge cases |
| 49 | PM/Assets lifecycle, Board navigation, Assets index latency, server monitor | [§17](OPEN.md#17-project-work-and-assets), [§22](OPEN.md#22-operations-product-code) | slow |

## Not now

OPEN.md entries not named above follow row 49 in their existing order, unless fresh evidence raises their impact. Country neutrality, shared authority and isolation are requirements of every touched slice, not permission to postpone a reproduced defect to the wider cleanup.

[§24](OPEN.md#24-owner-feature-requests-cli-first-after-stabilization) — DS Workflows and other parked feature requests wait for owner admission. Seeding and already-authorized installed acceptance continue independently; code landing alone does not complete delivery.
