# Bug evidence and review requests

The owner-assigned coding agent is the sole source writer for its slice.
An agent assigned only to review works read-only; write only this file and
[STABILIZATION.md](STABILIZATION.md). No feedback submissions, project
mutations, publication, source edits or competing builds from reviewers.
The coding agent independently confirms findings and owns proof and landing.
Owner update, 2026-10-12: Codex owns the immediate printing fast path; Claude
is unavailable. Preserve unrelated held edits. Use bounded independent Codex
proofs and installed CLI/MCP evidence, not another review campaign.
The coding agent re-reads both files between slices and incorporates new
evidence; reviewers mark a request claimed here to avoid duplicate work.

## Immediate printing fast path (owner, 2026-10-12)

Work from [STABILIZATION.md](STABILIZATION.md#immediate-printing-fast-path-owner-2026-10-12).
These are live owner reports, not completed fixes. Delete each evidence entry
when its actual acceptance passes; delete this file only when no unresolved
evidence remains. Composition stays in the CLI; the Desktop lists and previews
the governed catalogue. Run all supported printing preview pages through the
live Linux Desktop's admitted `ds` controls and retain timings, errors and exact
source/project/revision fences in owning test or output receipts. Do not claim
Windows acceptance from Linux proof.

1. **Cloud compute selected for local context.** Deployed Windows reports
   `Building footprints: downstream https://ds-network-reporter-bulk-rjnfdov67a-ue.a.run.app/v1/buildings/fetch returned 404`
   and `Elevation contours · 10 m / 50 m: downstream https://ds-network-reporter-bulk-rjnfdov67a-ue.a.run.app/v1/contours/fetch returned 404`.
   Source tracing confirms `ds-project-data::datasets`, CLI `CliProvider`,
   `project-data/buildings.ts` and `reports/transformer-print-context.ts` invoke
   Brain `query_print_context`, whose `tile_orchestrator.go` selects Reporter
   bulk. This is the current default, not proved local compute. Use retained
   project inputs and a shared native local provider; missing local inputs name
   acquisition needs without remote-compute fallback. Prove actual local output
   and zero Brain/Reporter compute calls, warm/offline reuse and unused-input
   negative cases. Preserve source identity, authored contour parameters and
   datum; do not substitute a different footprint provider silently.
2. **Preview blocks navigation, loses reusable work and repeats data reads.**
   Owner prefers native backend jobs to continue while navigating away. The
   supplied screenshot shows Printing setup's A0 transformer preview and LV,
   MV and overview groups. General `busy` disables the entire setup fieldset;
   view destruction discards preview adoption. Retain jobs/results under exact
   subject/layout/input fences, keep navigation available, and reuse the shared
   transformer/MV holdings for every consuming page. Prove navigation during
   work, safe return/adoption, stale result refusal and counted shared reads.
   Live Linux reproduction on 2026-10-12: the declared
   `desktop.printing.preview` for `czgmdwth_gisagara` / `agahehe`, using the
   existing DS-captured A0 draft at
   `ds-web/out/printing-fast-path-20261012/live/captured-a0-draft.json`, did not
   answer within 60 seconds on the earlier Native/frontend pair. The old Vite
   process had source watching disabled and did not consume the current source.
   After the matching local restart (Native SHA-256
   `2221c5dabbe2351331f1f79f5c539d9683c487aa8343059ea97c5a06b79507cc`),
   the same request reached the renderer in 16.49 seconds but refused
   `Live printing preview exceeds the paired bridge 8 MiB response bound`.
   This explicit draft is not the project's configured template and proves no
   finished print delivery. Owner's 2026-10-12 ruling: headless native rendering
   writes disk artifacts, then CLI/Desktop consume a bounded path/manifest;
   do not return large SVG/page bytes inline or make the renderer face the UI.
   Use private disk-backed expirable preview storage, default 24 hours with an
   admitted seven-day option. Retain exact input/result/source fences and
   verified bytes; unchanged consuming requests reuse without engine/API work.
   MV pages preserve native order and can be consumed as they become ready.
   The restarted `map.ui.open` reached the printing page but waited for unrelated
   asynchronous Svelte rendering until the bridge timed out; prove the new
   presentation acknowledgement through the live paired command after reload.
3. **MV pagination refuses the owner's preview.** Exact report:
   `DsRefused: the MV plan and profile preview did not render: Network Reporter plan and profile preview failed: alignment aln-0c77f3e8a31d74e9-38: no plan-feasible continuation path within 558 m per sheet; increase the horizontal scale denominator or inspect route bends and long spans`.
   Reproduce `grid_plan_profile::station_page_windows` on the captured request,
   trace plan feasibility and fix the demonstrated defect without silently
   changing authored scale or geometry. Prove valid continuation, long-span and
   bend negatives, complete page coverage, readable preview and actual timing.
4. **Front Matter has no clear preview section.** Owner requests MV and
   applicable LV front matter grouped and previewable from the governed
   catalogue, leaving composition in the CLI. Reuse native subject/category
   facts and preview contracts; prove discovery, grouping and rendered pages.
5. **Analysis response lacks its transport digest.** Exact report:
   `DesktopBoundaryError: analysis response is missing its SHA-256 header`.
   Trace the producing native/binary boundary and all preview callers. Emit and
   verify the exact response bytes' declared digest; preserve mismatch/tamper
   refusals. Prove real native response handling and every supported Linux
   preview page, including subject changes and navigation while rendering.

## Queued next: characterized NUM 15 normalization

Review only the failing Huye 120 PLS-CADD 19.01 → 16.81 conversion in
STABILIZATION.md §2 / the moved private-corpus entry below. Trace the conversion planner's renumbering
and NUM normalization into the strict 16.81 writer. Existing evidence is in
ds-network/out/stabilization/network-full-suite.log; the two Huye failures
report NUM 15 where the writer requires 14. Do not launch a competing build.

Find the exact native record differences and whether the existing characterized
normalization can preserve them. Give path/line evidence, a minimal regression
fixture specification, negative controls for uncharacterized data, and the
smallest owner fix. Separate a demonstrated implementation error from data
requiring native PLS-CADD evidence. Maximum two findings; no speculative parser
relaxation, skipping tests, source writes or solver claims. Claim this after
the current bounded review; the coding agent will implement confirmed evidence.

**Claimed (Claude, 2026-10-07 13:37 UTC):** NUM 15 normalization review.

**Findings (Claude, 14:20 UTC; read-only; members extracted with `ds pls backup-extract` into scratch):**

1. **The exact native differences are two suffix records, and the gate is
   right to refuse (executed).**
   - **Compared:** the `huye_gisagara_120_backup.bak` NUM
     (`VERSION='15'`, `SOURCE='PLS-CADD Version 19.01'`, 450 PIs) against
     the `GISAGARA-HUYE PUs.MV DES.bak` NUM (`VERSION='14'`, 16.20, 11 PIs).
   - **Identical in shape:** the prefix line (`1 0 0 -1 …`), the PI count
     line, every PI point and attribute record, the start station, the
     offsets, the 20 side-profile rows and the station equations.
   - **Differences:**
     - after `… ; back margin, ahead margin, number start/stop station pairs`,
       the next record has 7 numbers in v14 (`65 64 0 71 11 15.24 1000`) and
       8 in v15 (`2354 2363 0 2717 450 15 1000 0`; extra trailing `0`);
     - the custom coordinate-system record gains a trailing quoted string in
       v15 (`'custom' 'EDCL' 'EDCL Rwanda' '' 0 0 0.000000 0 ''`).
   - **The gate is correct.** `num_materialize.rs` keeps the source prefix
     and suffix verbatim (`:117-177`) and refuses anything but v14
     (`require_num_v14`, `:187`). Relabelling a v15 shell as 14 would carry
     the two v15 records into a v14 file.
   - **The gap is a missing characterized NUM v15 → v14 suffix normalization.**
     The cable writer has one (`pls_cadd_wire.rs:1562`, `:1661`, `:1733`);
     `ds-io` has none for NUM (its only NUM normalization is XY order,
     `num_align.rs:90`). The path is reached since ds-network `06d3785f`
     (2026-10-04): station renumbering forces the whole-design export
     (see the moved private-corpus entry below).
   - **Smallest fix:** a NUM normalization beside the cable one in
     `ds-io/src/pls_cadd_native`, applied before `require_num_v14`. It drops
     only the trailing `0` of the counts record and the trailing `''` of the
     coordinate-system record, and refuses any other value.
   - **Regression fixture:** take `tests_support.rs` `sample_num` (v14) and
     make a v15 twin (header `15`, a counts record with an 8th `0`, a
     coordinate-system record with `''`). Expect normalization back to the
     v14 bytes.
   - **Negative controls:** an 8th field of `1`, or a trailing `'X'`, must
     refuse.
2. **Two different failures, plus one fact that needs native evidence.**
   - **Scope:** only the 19.01 backup is a NUM 15 failure. The 16.20
     `GISAGARA-HUYE PUs.MV DES.bak` NUM is already v14; it fails the
     post-export re-read with 41 `section_phase_slot_missing` blockers
     (`network-full-suite.log:2352`), which is a separate stringing
     defect.
   - **Hypothesis (unexecuted):** the counts record's 5th field equals the
     PI count (11 and 450 here). A whole-design rewrite that changes the PI
     count must keep that record consistent; check `verify_materialized_num`.
   - **Native evidence needed:** before dropping the two v15 fields as
     defaults, confirm with one native PLS-CADD 16.81 save of a 19.01 NUM
     that 16.81 writes neither field. Implementation can proceed behind the
     refuse-unless-default rule.

**Moved from OPEN.md §11 (owner, 2026-10-07; fix with the NUM findings above):**

- **Two private-corpus PLS imports fail** (`conversion_planner` `corpus_planner_*`, run only where `fixtures/pls-private` is staged; the corpus files are unchanged since July): `huye_gisagara_120_backup.bak` (19.01) now takes the whole-design export path and the NUM writer refuses its version-15 shell (`NUM whole-snapshot writer requires … VERSION='14'`), since `06d3785f` made station renumbering force that path; the 16.20 witness `GISAGARA-HUYE PUs.MV DES.bak` (no numbering change) fails the post-transcode re-read with 41 `section_phase_slot_missing` blockers from the `9b08b6c7` stringing check. Keep the station-order fix; renumber after the 16.81 transcode or refuse by name at planning.

## Suspension ↔ strain retype leaves the owner's model unsaveable

**Owner report:** retyping structure 292 fails with "model failed validation
with 3 issue(s); first: ATTACHMENT_SET_PARTIAL_STRINGING
[tension_section_supports] … strings structure str-423815a32fed4309-292 at
sequence 1 without a resolved attachment point". The owner expects
suspension → strain, and back, to be instant and free of issues.

**Still reproduces (dry run, current dev `ds`, 2026-10-09):**

```
ds dsgrid structure retype --package local-aad500c1e08847db.dsgrid --structure 292 --type j-w-S325.014 --dry-run
```

It returns 4 `unresolved_retype_attachments` and 0 `tension_section_splits`:
`sec-423815a32fed4309-258`, `-258-slot-1` and `-258-slot-2` on `30kV` slots
0–2, and `sec-…-636` on `gw`. The engine paths are
`reconcile_interior_retype` and `join_released_anchor_sections` (ds-network
`ds-grid-engine`, `tests/retype_duty_change.rs`); the synthetic fixtures pass,
the real model does not.

**Witness:** 292 is interior sequence 1 of each three-support section
(291 → 292 → 293). Structure 618 (`j-w-S325.014`) is an endpoint on both
sides: section 29 ends on `30kV [set 2]`, section 361 starts on
`30kV [set 3]`, all phase slots preserved; its arriving section crosses
alignments, so station comparison would be wrong. `project_structure_library`
gives `[set 2]` dead_end at 1.571 rad, `[set 3]` at 4.712 rad, `GW` shield.

**Next:** characterize the import frame of attachment `local_x` and the PLS
strain azimuths (1.571/4.712 rad) against the 618 `[set 2]`/`[set 3]`
witness before `resolve_directional_strain` can trust the facings, and check
anchor insertion for the same mismatch.

**Regression target:** the Nyamagabe reference, 292 → `j-w-S325.014`, splits
into 291→292 and 292→293 halves with inherited sag and criteria and no
unresolved attachments; the reverse retype heals back to one section per
set. The parked alternative at
`ds-network/out/parked-numbering-retype-20261008-1119/` is superseded for
`reconcile_retype`.

## Unpaired strain ends report a one-sided wind span (residual of `4ba8ab15`)

Executed 2026-10-07 on the local Gisagara Model 1 seed (revision 30,
`ds dsgrid run --operation analyze_model_defaults`): the `4ba8ab15` factor-of-two
wind span and 115 uplifts no longer reproduce. Paired boundaries combine both
sides (191 of 193; median wind span 71.5 m) and uplift is the whole-support
resultant (`structure_usage.rs:820-826, 960-985`, `390bf15d`).

One residual: at `str-…-216` (`st-deded789…`, a three-way strain junction) and
`str-…-308` (`st-dd93738b…`), `opposite_strain_end`
(`ds-grid-engine/src/structure_usage.rs:714-754`) leaves strain ends unpaired,
so the structure reports one side's wind span (13.1–25.0 m and 26.4–70.8 m).
First decide between differing back/ahead cable ids and the 3.142-rad tap set by
reading `cable_id` and `attachment_point_id` of sections 91, 215, 376 and 503 at
`str-216`. Regression: `str-6` keeps its four joints; `str-216` pairs opposite
same-cable ends, and any side left unpaired is named in a row advisory, never
reported as the structure's wind span. Negative controls: a single-sided
terminal dead-end stays one realization without advisory; a perpendicular tap
keeps separate realizations; uplift, cable-modulus and capacity blockers are
unchanged. No solver approval; tolerances unchanged.

Unrelated full CLI gate boundary: report.layout.render help is 2545 bytes against
2480-byte allowance; Claude owns template-preview changes. Codex must leave its
source and tests unchanged. Continue owning controls/transport checks; do not
claim the unrestricted workspace suite passed until that owner closes it.
