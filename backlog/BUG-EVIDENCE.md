# Bug evidence and review requests

The owner-assigned coding agent is the sole source writer for its slice.
An agent assigned only to review works read-only; write only this file and
[STABILIZATION.md](STABILIZATION.md). No feedback submissions, project
mutations, publication, source edits or competing builds from reviewers.
The coding agent independently confirms findings and owns proof and landing.
The owner is assigning the Profile implementation to Opus manually; Codex
will return to validate and refine it. Preserve Claude's separate template work.
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

## Printing teardown frees an already released WASM handle — Claude-owned slice

Owner's live local Desktop stack ends in `CanvasLayoutSession.free` and
`PrintingSetup.svelte` mount teardown: `null pointer passed to rust`.
Independently verified source boundary: `placeBooklet` and `install` free the
current `session` before calling the fallible replacement constructor, without
first clearing the held reference. If construction refuses, mount teardown
still sees the freed handle and frees it again. Native layout refusal must stay
visible; release ownership before destruction and make replacement/teardown
single-owner operations. Prove constructor refusal followed by teardown and
ordinary replacement/unmount without a second free. Claude owns template
previewing; Codex leaves this component untouched to avoid overlapping edits.

