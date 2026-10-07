# Bug evidence and review requests

The coding Codex is the sole source writer. Claude or a reviewing Codex can
pick up these requests without an owner relay. Review read-only; write only
this file and [STABILIZATION.md](STABILIZATION.md). No feedback submissions,
project mutations, publication, source edits or competing builds. Codex
independently confirms each finding, implements it and owns proof and landing.
Request the next bounded review before finishing the current coding slice.
The coding agent re-reads both files between slices and incorporates new
evidence; reviewers mark a request claimed here to avoid duplicate work.

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

## Incoming finding (Claude, 2026-10-07 14:00 UTC): suspension ↔ strain retype leaves the model unsaveable (executed)

**Owner report:** the retype fails with "model failed validation with 3
issue(s); first: ATTACHMENT_SET_PARTIAL_STRINGING [tension_section_supports]
sec-423815a32fed4309-185: section … strings structure str-423815a32fed4309-292
at sequence 1 without a resolved attachment point". The owner expects
suspension → strain, and back, to be instant and free of issues.

**Reproduction** (executed as a dry run; nothing was written):

```
ds dsgrid structure retype --package <stable copy local-aad500c1e08847db.dsgrid> --out <scratch> --structure 292 --type j-w-S325.014 --dry-run --output json
```

It returns `unresolved_retype_attachments` for 4 supports:

- `sec-…-185`, `-185-slot-1` and `-185-slot-2` on set `30kV`, slots 0–2;
- `sec-…-560` on set `gw`, slot 0.

**Cause:**

- `retype_structure` (`ds-grid-engine/src/command.rs:408`) rebinds a support
  only to an identical (set label, slot).
- Structure 292's type, `a-w-S325.014` (Type A, single conductor set), has
  sets `30kV` (slots 0–2) and `gw` (slot 0).
- The target, `j-w-S325.014` ("TYPE J: H-POLE STRAIN, DEVIATION 0-15 DEG"),
  has `30kV [set 2]` (0–2), `30kV [set 3]` (0–2) and `GW` (0). No label
  matches; even `gw`/`GW` differ only in case.
- Every attachment is therefore cleared. The draft is accepted in the session,
  but `pack` refuses it (`ds-grid-exchange/src/package.rs:107`, with
  `ds-grid-model/src/validate.rs` reporting ATTACHMENT_SET_PARTIAL_STRINGING),
  so the save fails.
- The validator is right. The missing piece is the remap, so this is a gap in
  the code, not a misreading by the owner.

**An existing primitive is unused.** `GridCommand::RetypeStructuresWithSupports`
(`command.rs:419`, operation `retype_structures_with_supports`) re-binds every
support to an explicit attachment point of the new type. Its documentation
names this exact case: "split one strain set into arriving and departing
sets". No caller exists in ds-cli, ds-web or ds-grid-wasm.

**Smallest fix:** an engine proposal that derives the default mapping and
feeds `retype_structures_with_supports`:

- **Suspension → strain:** the arriving section takes one strain set and the
  departing section the other. Slot order is kept, and the ground wire is
  matched by normalized label (`gw`/`GW`).
- **Strain → suspension:** both sides map to the single set. Refuse only when
  the slot or phase counts differ.
- **Which set is which:** whether `[set 2]` or `[set 3]` is arriving must come
  from existing strung Type J structures in this model (for example 618) or
  from native evidence. Never guess it.
- **Callers:** CLI `dsgrid structure retype` and the Desktop retype picker
  both use it. The preview shows the mapping, and the retype is one
  transaction. Fall back to today's draft only when no safe mapping exists.

**Negative control (executed earlier today on a scratch package):** retyping
618 `j-w-S325.014` → `j-w-S255.012` (same set labels) kept every attachment
with no warning. Retyping 618 → `a-w-S325.014` reproduced the unresolved
attachments.

**Regression fixture:** a single-set structure strung on both sides,
retyped to a two-set strain type. Expect every support rebound and the package
to emit. Then retype it back: expect the same.

**Related:** STABILIZATION.md §4 (swift editing).

**Bounded follow-up requested:** prove the arriving/departing set convention
from already-strung native Type J witnesses such as structure 618, and check
whether structure 292 is an interior support in a continuous tension section
or an endpoint of independent sections. Give exact support sequence, neighboring
structures/stations, attachment labels/slots and native request evidence.
If an interior section must split to introduce a strain, identify the existing
atomic splitting command and its sag/criterion preservation contract. Do not
guess phase ordering from labels, edit the working copy or write source.

Codex independently read the exact immutable Nyamagabe reference through
`ds dsgrid run --operation project_table` (structures, attachment points,
tension-section supports; receipts in ds-cli/out/stabilization/retype-evidence).
292 is interior sequence 1 of each three-support section: 291 → 292 → 293.
618 is an endpoint on both sides: section 29 ends on `[set 2]`, section 361
starts on `[set 3]`, all phase slots preserved. These are sequence conventions;
618's arriving section crosses alignments, so station comparison would be
wrong. A safe 292 retype must split its sections atomically, not only rebind
four points. Please trace the existing split primitive and the reverse merge.

**Claimed (Claude, 13:52 UTC):** split primitive and reverse merge trace.

**Findings (Claude, 14:05 UTC; read-only, ds-network `d5f9d0c9`):**

1. **Split exists, but only for structure insertion.**
   - `split_at_anchor` (`ds-grid-engine/src/structure_placement.rs:867`,
     private) replaces each member section with two anchor-to-anchor halves.
     Each half clones the replaced section: cable, stringing intent, sag,
     criteria and details are unchanged.
   - Lineage is recorded as `TensionSectionSplitDelta`
     (`command.rs:834-881`, `effects.tension_section_splits`).
   - Back/ahead sets come from `resolve_directional_strain` (`:749`), which
     uses authored attachment facings (`orientation_rad`), never labels, and
     refuses uncharacterized facings. It is reached only via
     `apply_plan` / `resolve_action` (`:809-826`, `:579`, candidate sets
     `:514`) when a new structure is placed. Suspension insertion uses
     `splice_suspension` (`:833`) and does not split.
2. **Reverse merge exists, but only for deletion.**
   - Deleting a strain structure heals the run (`command.rs:4103`, owner
     2026-10-05).
   - Only the physical conductor must agree (`conductor_join_conflict`,
     `:4188-4206`: cable, nominal voltage, wires per phase, bundle). The
     incoming section's sag, criteria and details carry the joined run.
   - Branch points refuse (`:4087`).
3. **Neither is reachable from a retype.** `RetypeStructuresWithSupports`
   only rebinds supports; it never splits or merges.
   - **Executed:** `project_structure_library` for `j-w-S325.014` shows
     characterized facings: `30kV [set 2]` dead_end at 1.571 rad,
     `30kV [set 3]` dead_end at 4.712 rad, `GW` shield. So
     `resolve_directional_strain` can assign 292's back/ahead sets
     deterministically, consistent with Codex's 618 sequence evidence.
   - **Smallest owner change:** in `structure_placement`, plan a retype that
     changes duty as one transaction, with the existing structure as the
     anchor.
     - **Suspension → strain:** reuse `candidate_sets` +
       `resolve_directional_strain` + `split_at_anchor` at 292's index in
       each member path (both 30kV phase sections and the GW section, which
       binds to `GW`).
     - **Strain → suspension:** reuse the deletion heal (path join +
       `conductor_join_conflict`) while keeping the structure, then bind the
       joined path to the single suspension set.
     - **Refusals:** the same as today's insertion and deletion.
   - **Regression fixture:** the Nyamagabe reference, 292 → `j-w-S325.014`.
     Expect 291→292 and 292→293 halves with inherited sag and criteria, and
     no unresolved attachments. The reverse retype heals back to one
     section per set.

### Why even successful retypes are too slow for real-time use (measured, Claude, 2026-10-07)

**Owner:** retypes that do work take too long to use live. The edit itself is
cheap; what follows it reprocesses the whole model several times. Full
timings and trace are in STABILIZATION.md §4.

**Measured** on ds-server, debug builds (the only `ds` that reads today's
schema-5 packages); Nyamagabe M1, 1.4 MB, structure 618:

| Step | Time |
| --- | --- |
| CLI retype dry run, nothing written | 6.0–6.2 s |
| CLI retype write | 3.9–7.3 s |
| Open and validate only | 1.9 s (schema 5) / 5.5 s (schema 4) |
| Whole-model `engineering_issue_layer` | 5.6 s |

**What one Desktop retype triggers:**

1. **The edit.** `gridCommitStructureRetype` applies it in the worker
   (`ds-web/src/lib/grid/session.ts:749`). This part is cheap.
3. **Auto analysis.** The new revision re-offers it
   (`GridProfilePanel.svelte:1210-1226`; on by default since 2026-10-05).
   `executeNativeProfileCommand` (`profile-native.ts`) then:
   - runs `checkpointDsgrid()`, a full package emit;
   - sends `package_bytes: Array.from(bytes)`, a JSON array of 1.4 million
     numbers over Tauri IPC;
   - has the native side decode and validate the whole package, rebuild
     every Profile band, and run the whole-model analysis;
   - returns the full scene and up to 2,000 issues per kind, which are
     re-installed in the worker.
4. **CLI path** (`ds-cli-dsgrid/src/mutation.rs`, `structure/retype.rs`):
   decode the whole package (schema-4 copies re-run the 4 → 5 migration on
   every open), compute line angles for every structure, then re-emit and hash the whole
   package on an explicit file write.

**Remaining:** Profile analysis still costs a full package emit, a decode and
validation, a whole-scene rebuild and a whole-model analysis.
A retype that fails, as above, pays the same chain before the save refuses.

**Owner ruling (2026-10-07):** a retype, a slide/move, or any other edit must
not save the model. The model stays in memory in a dirty state for fast
interaction; persistence happens only through the Save button. A retype plus
its recalculation must take well under 0.5 s: high-performance editing, not a
sluggish Profile. This means:

- no `checkpointDsgrid()`, working checkpoint or package `pack`/validation on
  the edit path;
- a draft that fails package validation must not block the edit; it shows as
  pending issues until the user saves;
- recalculation is scoped to the affected sections, bands and spans.

**Fix direction:** send the edit, not the package (the queued replay review
below), and recompute only the affected tension sections, bands and spans.
Acceptance (STABILIZATION.md §4): retyping 618 shows the new type and usage in
well under a second in a release build.

## Native Profile transport and localized refresh

- The worker needs a read-only initialization handover of its opened snapshot,
  active journal and retained assets. Initialization after earlier edits must
  still permit undo to the opened state and must accept an invalid current draft
  without packing it or changing Save metadata.
- `ds-web/src/lib/grid/profile-native.ts` still checkpoints and transfers the
  full package on every refresh. Bind it to native revision-fenced refresh,
  capture model/generation scope, retain an admitted native head even when a
  display result becomes stale, serialize same-client work and expose refusals
  without an automatic checkpoint fallback.
- Native scene preparation still rebuilds all bands at a changed revision.
  Consume the command-receipt closure for affected bands and engineering
  refresh, retaining unchanged scene and demand inputs at section ends.

**Remaining in this vertical:** bind the held native Profile runtime and its
analysis to the bounded command-receipt scope and demand cache; remove the
per-edit package round trip. Localize terrain invalidation to the spans and
supports whose effective ground uses the changed sample, and refresh only
those Profile bands. Unknown broader effects remain reconciliation work,
not a synchronous interactive solve. Preserve the owner's tension-section-end
boundary for both CLI and user edits.

**Moved from OPEN.md §12 (owner, 2026-10-07; fix with the closure findings above: usage must combine every incident side):**

- `4ba8ab15` (major): screen_structure_usage takes one side at section-boundary structures: wind span ~0.5x PLS, 115 spurious uplifts (Gisagara calibration).

## Queued after replay: exact incident-side calibration

**Claimed (Claude, 2026-10-07 14:50 UTC):** focused replay follow-up review of
`ds-profile-runtime` at `c87444b`. The calibration trace follows it.

**Focused replay follow-up (Codex, 2026-10-07):** review the native replay/navigation
request as it appears in `ds-profile-runtime`: transaction rollback on a later
journal fence failure or projection refusal, same-revision model replacement,
undo/redo and a branch after undo, retained package assets/protection, and demand
cache advancement from native command receipts. Then trace the smallest native
Profile scene refresh that consumes those receipt scopes without preparing every
band again. Keep this read-only, at most two findings, no competing builds.

Trace the moved `4ba8ab15` finding above on the exact Gisagara calibration
fixture. Compare native whole-support conductor vectors with the screening
realizations at a tension-section boundary: attachment identities, authored
facings and coordinates, cable/phase slots, each incident demand and the final
capacity input. Identify whether the factor-of-two wind span and reported
uplifts remain reproducible under current native aggregation, or name the
precise stale assumption in the old finding. Preserve unknown capacity and
longitudinal-force blockers. Give one bounded regression and its negative
controls; no source writes, model edits, competing builds or solver approval.
Maximum two actionable findings. Claim after the current replay review.

### Findings: focused replay follow-up (Claude, 2026-10-07 15:10 UTC; read-only at `c87444b`)

**Checked, no defect:**

- **Transaction rollback.**
  - `refresh` works on a cloned session and cache (`replay.rs:59-60`).
  - It runs the style preflight before projection (`:121-129`).
  - A projection refusal hands the previous `HeldView` back
    (`lib.rs:329-343`).
  - The one fallible call after the commit, `clearance_violation_encoding`
    (`lib.rs:453`), is already required by `resolve()`
    (`profile_encoding.rs:236`), so it cannot fail after admission.
- **Model replacement.** A different `model_id` refuses (`replay.rs:33-38`).
  Revision ids are content-derived, so the same revision means the same
  content.
- **Undo, redo and a branch after undo.**
  - The runtime redoes everything, then takes the exact common
    `JournalEntry` prefix (command identity included), undoes to it and
    applies the rest with parent and result fences (`replay.rs:69-105`).
  - A pure undo keeps the redo tail.
- **Assets and protection.** Shared `Arc` assets are kept (`lib.rs:65-69`).
  Protection is covered by
  `package_derived_combined_protection_survives_native_replay`.
- **Demand cache.**
  - `advance_command` clears the cache on any parent mismatch, which covers
    the first command after an undo, and on `whole_model`
    (`structure_usage_cache.rs:102-117`).
  - `screen` clears it on any other revision (`:118-124`).
  - A pure undo applies no receipt and falls through to that clear.

**Finding 1: every refresh still prepares every band.**

- **What happens:**
  - `replay.rs:89-105` reads each applied `CommandOutcome` only for
    `cache.advance_command`. The receipt's `delta.recalculation_scope`
    (`alignments`, `demand_sections`, `context_sections`, `spans`,
    `structures`; `recalculation_scope.rs:13-24`) then goes unused.
  - `ProfileReviewView::scene` (`ds-network …/profile_engineering.rs:375-391`)
    calls `ProfileCaseProjection::prepare` whenever the revision changed
    (`profile_atlas/case_projection.rs:45-110`). That rebuilds, for the
    whole model:
    - `raw_profile_surface` for every alignment (`profile_atlas.rs:1861`);
    - `CaseCurves::prepare` for every tension section;
    - `build_chained_sections`, `layout_scene` and
      `project_profile_engineering`.
- **Smallest refresh:**
  1. Union the scopes of the applied outcomes in `refresh`.
  2. Pass the union to a new `ProfileReviewView::advance(session, options, &scope)`.
  3. In `advance`:
     - keep the per-alignment raw bands outside `scope.alignments`;
     - keep `CaseCurves` outside `demand_sections ∪ context_sections`;
     - rebuild chain placements only when a band's station bounds change;
     - lay out only the changed bands;
     - project engineering only for `scope.structures`.
  4. Fall back to the full `prepare` on `whole_model` and on navigation,
     since an undo produces no receipt.
- **Constraint:** `raw_profile_surface` returns `ProfileGeometry<'a>`
  borrowed from the snapshot. Retained per-alignment geometry must therefore
  be owned, keyed by alignment id and revision.
- **Regression test:** retype one suspension structure in the middle of a
  section. Assert two things:
  - only that structure's alignment band and its incident sections' curves
    are rebuilt (count them);
  - the scene serializes identically to a full `prepare` at the new revision.
- **Negative controls:**
  - a `whole_model` command rebuilds everything;
  - undo navigation rebuilds everything;
  - a terrain or alignment edit rebuilds exactly that alignment;
  - a station-bounds change re-chains the downstream bands.

**Finding 2: Analyze screens the whole model without the cache.**

- **What happens:**
  - `analysis::analyze_with_cache` first calls
    `session.analyze_model_defaults` (`analysis.rs:33-37`). That runs
    `structure_usage_screening_with_demands` for every structure
    (`structure_ids: []`, `default_analysis.rs:802-810`) with no cache.
  - Only after that does the cached issue layer screen again
    (`analysis.rs:56-68`).
- **Effect:** the receipt-advanced `StructureUsageScreeningCache` cannot
  make an Analyze refresh cheaper than a whole-model section-demand solve.
- **Fix:** add a `_with_cache` form of the default analysis that reads and
  fills the same `SectionDemandCache` entries, keyed by
  `(section, case, basis)`, that `screen` uses.
- **Regression test:** open, analyze, retype one structure, then refresh
  with `analyze: true`. Assert that both the default analysis and the issue
  layer report `computed_cases` equal to the incident sections × cases, and
  that the report is byte-identical to an uncached analysis.
- **Negative controls:**
  - a `whole_model` edit recomputes every case;
  - a changed `weight_span_basis` misses the cache.

### Findings: incident-side calibration for `4ba8ab15` (Claude, 2026-10-07 15:45 UTC; read-only)

**Run.**

- **Package:** the local Gisagara Model 1 seed
  `~/.cache/claude-agents/seed/gisagara-mv-model1-rev3.dsgrid`: authored
  `rev:79def71d…`, revision 30, fingerprint `fnv1a64:65d91b7dc13baa24`.
- **Not the calibration package:** the revision-81 calibration package
  (`b6d75f61…`) isn't on this host.
- **Command:** `ds dsgrid run --operation analyze_model_defaults` (case
  envelope: 7 cases, `exact_force_equivalent`). It took 4.1 s.
- **Binary:** debug build from 10:40 today. The aggregation code is
  unchanged since `390bf15d` (10-03). Today's `f72d28a6` only changes
  caching.

**Finding 1: neither the factor of two nor the 115 uplifts reproduce at
paired boundaries; the old finding's assumption is stale.**

- **Paired boundaries carry the full wind span.**
  - 191 of the 193 structures with 8 incident sections (back plus ahead
    per wire) combine both sides.
  - Their wind spans have median 71.5 m (p25 64.1, p75 78.7). Mid-section
    suspension types are on the same scale: `st-fab01188…` averages
    67.0 m and `st-d4275080…` 67.6 m.
  - The median spread across phases is 0.4%.
  - Pairing at `str-…-6`, for example: `[163, 3]`,
    `[163-slot-1, 3-slot-1]`, `[163-slot-2, 3-slot-2]`, `[256, 257]`.
- **Uplift is now decided by the whole support.**
  - 1,577 per-side upward demand case rows (`upward_load_case_count`)
    become 35 structures with whole-support uplift, out of 300 complete
    case states.
  - 3,453 case states stay incomplete (no cable modulus) and remain
    blockers. Keep those blockers, and keep the unknown-capacity
    blockers.
- **Stale assumption:** the old finding read one incident side's wind span
  and negative vertical as the structure's. Uplift is now the signed
  resultant of all incident realizations (`structure_usage.rs:820-826,
  960-985`), and opposite strain ends join one joint (`390bf15d`).
- **Not this defect:**
  - The 101 low-wind-span 4-incident rows (types `st-37909e…` and
    `st-af1099…`) belong to two-structure alignments. For example,
    `aln-…-52` holds only `str-390`/`-391`, one 2.4 m span apart.
  - So a 1.3 m wind span is correct there. Their ±137 m weight spans
    fit a short span strung at full tension.

**Finding 2: a residual one-sided wind span where strain-end pairing
fails.**

- **Where:** `str-…-216` (type `st-deded789…`) and `str-…-308`
  (`st-dd93738b…`).
- **What happens:**
  - At `str-216` each of the 8 incident sections forms its own
    realization: `215`, `215-slot-1/2`, `91`, `91-slot-1/2`, `376`,
    `503`.
  - At `str-308`, `230`, `230-slot-1/2` and `504` stay single; only
    `[432, 433]` pairs.
  - The structure's wind span is then one side's: 13.1–25.0 m at
    `str-216`, 26.4–70.8 m at `str-308`.
- **Why it can fail:**
  - `opposite_strain_end` (`structure_usage.rs:714-754`) pairs only when
    all of these hold:
    - the attachments carry the same cable (`used` is filtered by
      cable, `:802-806`);
    - they have the same type and slot;
    - their local coordinates are bit-identical;
    - their orientations differ by π ± 0.001.
  - `st-deded789…` is a three-way strain junction. Its front (1.571)
    and back (4.712) sets share (±1.15 / 0, 0, 9.05). A tap set at
    3.142 sits at (0.7, ±1.15, 8.05).
- **Two possible causes, not yet told apart:**
  - the back and ahead sections at `str-216` carry different cable ids;
  - the continuing line uses the 3.142 tap set, which forms a
    perpendicular pair, not an opposite one.
- **To decide:** read `cable_id` and `attachment_point_id` of sections
  91, 215, 376 and 503 at `str-216`.
- **Regression test:** screen `str-216` and `str-6` on this package.
  - `str-6` keeps its four joints.
  - `str-216` pairs its opposite same-cable ends. Any side that remains
    unpaired is named in a row advisory, and its one-sided wind span is
    never reported as the structure's wind span.
- **Negative controls:**
  - a single-sided terminal dead-end stays one realization, with no
    advisory;
  - a genuine perpendicular tap keeps separate realizations;
  - whole-support uplift and the cable-modulus and capacity blockers are
    unchanged.
- **Out of scope:** no solver approval, and tolerances are unchanged.
