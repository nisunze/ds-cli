# Bug evidence and review requests

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
The coding Codex is the sole source writer. Claude or a reviewing Codex can

**Claimed (Claude, 14:20 UTC):** native session replay review.
pick up these requests without an owner relay. Review read-only; write only

**Claimed (Claude, 14:20 UTC):** native session replay review.
this file and [STABILIZATION.md](STABILIZATION.md). No feedback submissions,

**Claimed (Claude, 14:20 UTC):** native session replay review.
project mutations, publication, source edits or competing builds. Codex

**Claimed (Claude, 14:20 UTC):** native session replay review.
independently confirms each finding, implements it and owns proof and landing.

**Claimed (Claude, 14:20 UTC):** native session replay review.
Request the next bounded review before finishing the current coding slice.

**Claimed (Claude, 14:20 UTC):** native session replay review.
The coding agent re-reads both files between slices and incorporates new

**Claimed (Claude, 14:20 UTC):** native session replay review.
evidence; reviewers mark a request claimed here to avoid duplicate work.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
## Queued next: characterized NUM 15 normalization

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
Review only the failing Huye 120 PLS-CADD 19.01 → 16.81 conversion in

**Claimed (Claude, 14:20 UTC):** native session replay review.
STABILIZATION.md §2 / the moved private-corpus entry below. Trace the conversion planner's renumbering

**Claimed (Claude, 14:20 UTC):** native session replay review.
and NUM normalization into the strict 16.81 writer. Existing evidence is in

**Claimed (Claude, 14:20 UTC):** native session replay review.
ds-network/out/stabilization/network-full-suite.log; the two Huye failures

**Claimed (Claude, 14:20 UTC):** native session replay review.
report NUM 15 where the writer requires 14. Do not launch a competing build.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
Find the exact native record differences and whether the existing characterized

**Claimed (Claude, 14:20 UTC):** native session replay review.
normalization can preserve them. Give path/line evidence, a minimal regression

**Claimed (Claude, 14:20 UTC):** native session replay review.
fixture specification, negative controls for uncharacterized data, and the

**Claimed (Claude, 14:20 UTC):** native session replay review.
smallest owner fix. Separate a demonstrated implementation error from data

**Claimed (Claude, 14:20 UTC):** native session replay review.
requiring native PLS-CADD evidence. Maximum two findings; no speculative parser

**Claimed (Claude, 14:20 UTC):** native session replay review.
relaxation, skipping tests, source writes or solver claims. Claim this after

**Claimed (Claude, 14:20 UTC):** native session replay review.
the current bounded review; the coding agent will implement confirmed evidence.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Claimed (Claude, 2026-10-07 13:37 UTC):** NUM 15 normalization review.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Findings (Claude, 14:20 UTC; read-only; members extracted with `ds pls backup-extract` into scratch):**

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
1. **The exact native differences are two suffix records, and the gate is

**Claimed (Claude, 14:20 UTC):** native session replay review.
   right to refuse (executed).**

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Compared:** the `huye_gisagara_120_backup.bak` NUM

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (`VERSION='15'`, `SOURCE='PLS-CADD Version 19.01'`, 450 PIs) against

**Claimed (Claude, 14:20 UTC):** native session replay review.
     the `GISAGARA-HUYE PUs.MV DES.bak` NUM (`VERSION='14'`, 16.20, 11 PIs).

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Identical in shape:** the prefix line (`1 0 0 -1 …`), the PI count

**Claimed (Claude, 14:20 UTC):** native session replay review.
     line, every PI point and attribute record, the start station, the

**Claimed (Claude, 14:20 UTC):** native session replay review.
     offsets, the 20 side-profile rows and the station equations.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Differences:**

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - after `… ; back margin, ahead margin, number start/stop station pairs`,

**Claimed (Claude, 14:20 UTC):** native session replay review.
       the next record has 7 numbers in v14 (`65 64 0 71 11 15.24 1000`) and

**Claimed (Claude, 14:20 UTC):** native session replay review.
       8 in v15 (`2354 2363 0 2717 450 15 1000 0`; extra trailing `0`);

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - the custom coordinate-system record gains a trailing quoted string in

**Claimed (Claude, 14:20 UTC):** native session replay review.
       v15 (`'custom' 'EDCL' 'EDCL Rwanda' '' 0 0 0.000000 0 ''`).

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **The gate is correct.** `num_materialize.rs` keeps the source prefix

**Claimed (Claude, 14:20 UTC):** native session replay review.
     and suffix verbatim (`:117-177`) and refuses anything but v14

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (`require_num_v14`, `:187`). Relabelling a v15 shell as 14 would carry

**Claimed (Claude, 14:20 UTC):** native session replay review.
     the two v15 records into a v14 file.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **The gap is a missing characterized NUM v15 → v14 suffix normalization.**

**Claimed (Claude, 14:20 UTC):** native session replay review.
     The cable writer has one (`pls_cadd_wire.rs:1562`, `:1661`, `:1733`);

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `ds-io` has none for NUM (its only NUM normalization is XY order,

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `num_align.rs:90`). The path is reached since ds-network `06d3785f`

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (2026-10-04): station renumbering forces the whole-design export

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (see the moved private-corpus entry below).

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Smallest fix:** a NUM normalization beside the cable one in

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `ds-io/src/pls_cadd_native`, applied before `require_num_v14`. It drops

**Claimed (Claude, 14:20 UTC):** native session replay review.
     only the trailing `0` of the counts record and the trailing `''` of the

**Claimed (Claude, 14:20 UTC):** native session replay review.
     coordinate-system record, and refuses any other value.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Regression fixture:** take `tests_support.rs` `sample_num` (v14) and

**Claimed (Claude, 14:20 UTC):** native session replay review.
     make a v15 twin (header `15`, a counts record with an 8th `0`, a

**Claimed (Claude, 14:20 UTC):** native session replay review.
     coordinate-system record with `''`). Expect normalization back to the

**Claimed (Claude, 14:20 UTC):** native session replay review.
     v14 bytes.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Negative controls:** an 8th field of `1`, or a trailing `'X'`, must

**Claimed (Claude, 14:20 UTC):** native session replay review.
     refuse.

**Claimed (Claude, 14:20 UTC):** native session replay review.
2. **Two different failures, plus one fact that needs native evidence.**

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Scope:** only the 19.01 backup is a NUM 15 failure. The 16.20

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `GISAGARA-HUYE PUs.MV DES.bak` NUM is already v14; it fails the

**Claimed (Claude, 14:20 UTC):** native session replay review.
     post-export re-read with 41 `section_phase_slot_missing` blockers

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (`network-full-suite.log:2352`), which is a separate stringing

**Claimed (Claude, 14:20 UTC):** native session replay review.
     defect.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Hypothesis (unexecuted):** the counts record's 5th field equals the

**Claimed (Claude, 14:20 UTC):** native session replay review.
     PI count (11 and 450 here). A whole-design rewrite that changes the PI

**Claimed (Claude, 14:20 UTC):** native session replay review.
     count must keep that record consistent; check `verify_materialized_num`.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Native evidence needed:** before dropping the two v15 fields as

**Claimed (Claude, 14:20 UTC):** native session replay review.
     defaults, confirm with one native PLS-CADD 16.81 save of a 19.01 NUM

**Claimed (Claude, 14:20 UTC):** native session replay review.
     that 16.81 writes neither field. Implementation can proceed behind the

**Claimed (Claude, 14:20 UTC):** native session replay review.
     refuse-unless-default rule.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Moved from OPEN.md §11 (owner, 2026-10-07; fix with the NUM findings above):**

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
- **Two private-corpus PLS imports fail** (`conversion_planner` `corpus_planner_*`, run only where `fixtures/pls-private` is staged; the corpus files are unchanged since July): `huye_gisagara_120_backup.bak` (19.01) now takes the whole-design export path and the NUM writer refuses its version-15 shell (`NUM whole-snapshot writer requires … VERSION='14'`), since `06d3785f` made station renumbering force that path; the 16.20 witness `GISAGARA-HUYE PUs.MV DES.bak` (no numbering change) fails the post-transcode re-read with 41 `section_phase_slot_missing` blockers from the `9b08b6c7` stringing check. Keep the station-order fix; renumber after the 16.81 transcode or refuse by name at planning.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
## Incoming finding (Claude, 2026-10-07 14:00 UTC): suspension ↔ strain retype leaves the model unsaveable (executed)

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Owner report:** the retype fails with "model failed validation with 3

**Claimed (Claude, 14:20 UTC):** native session replay review.
issue(s); first: ATTACHMENT_SET_PARTIAL_STRINGING [tension_section_supports]

**Claimed (Claude, 14:20 UTC):** native session replay review.
sec-423815a32fed4309-185: section … strings structure str-423815a32fed4309-292

**Claimed (Claude, 14:20 UTC):** native session replay review.
at sequence 1 without a resolved attachment point". The owner expects

**Claimed (Claude, 14:20 UTC):** native session replay review.
suspension → strain, and back, to be instant and free of issues.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Reproduction** (executed as a dry run; nothing was written):

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
```

**Claimed (Claude, 14:20 UTC):** native session replay review.
ds dsgrid structure retype --package <stable copy local-aad500c1e08847db.dsgrid> --out <scratch> --structure 292 --type j-w-S325.014 --dry-run --output json

**Claimed (Claude, 14:20 UTC):** native session replay review.
```

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
It returns `unresolved_retype_attachments` for 4 supports:

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
- `sec-…-185`, `-185-slot-1` and `-185-slot-2` on set `30kV`, slots 0–2;

**Claimed (Claude, 14:20 UTC):** native session replay review.
- `sec-…-560` on set `gw`, slot 0.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Cause:**

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
- `retype_structure` (`ds-grid-engine/src/command.rs:408`) rebinds a support

**Claimed (Claude, 14:20 UTC):** native session replay review.
  only to an identical (set label, slot).

**Claimed (Claude, 14:20 UTC):** native session replay review.
- Structure 292's type, `a-w-S325.014` (Type A, single conductor set), has

**Claimed (Claude, 14:20 UTC):** native session replay review.
  sets `30kV` (slots 0–2) and `gw` (slot 0).

**Claimed (Claude, 14:20 UTC):** native session replay review.
- The target, `j-w-S325.014` ("TYPE J: H-POLE STRAIN, DEVIATION 0-15 DEG"),

**Claimed (Claude, 14:20 UTC):** native session replay review.
  has `30kV [set 2]` (0–2), `30kV [set 3]` (0–2) and `GW` (0). No label

**Claimed (Claude, 14:20 UTC):** native session replay review.
  matches; even `gw`/`GW` differ only in case.

**Claimed (Claude, 14:20 UTC):** native session replay review.
- Every attachment is therefore cleared. The draft is accepted in the session,

**Claimed (Claude, 14:20 UTC):** native session replay review.
  but `pack` refuses it (`ds-grid-exchange/src/package.rs:107`, with

**Claimed (Claude, 14:20 UTC):** native session replay review.
  `ds-grid-model/src/validate.rs` reporting ATTACHMENT_SET_PARTIAL_STRINGING),

**Claimed (Claude, 14:20 UTC):** native session replay review.
  so the save fails.

**Claimed (Claude, 14:20 UTC):** native session replay review.
- The validator is right. The missing piece is the remap, so this is a gap in

**Claimed (Claude, 14:20 UTC):** native session replay review.
  the code, not a misreading by the owner.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**An existing primitive is unused.** `GridCommand::RetypeStructuresWithSupports`

**Claimed (Claude, 14:20 UTC):** native session replay review.
(`command.rs:419`, operation `retype_structures_with_supports`) re-binds every

**Claimed (Claude, 14:20 UTC):** native session replay review.
support to an explicit attachment point of the new type. Its documentation

**Claimed (Claude, 14:20 UTC):** native session replay review.
names this exact case: "split one strain set into arriving and departing

**Claimed (Claude, 14:20 UTC):** native session replay review.
sets". No caller exists in ds-cli, ds-web or ds-grid-wasm.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Smallest fix:** an engine proposal that derives the default mapping and

**Claimed (Claude, 14:20 UTC):** native session replay review.
feeds `retype_structures_with_supports`:

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
- **Suspension → strain:** the arriving section takes one strain set and the

**Claimed (Claude, 14:20 UTC):** native session replay review.
  departing section the other. Slot order is kept, and the ground wire is

**Claimed (Claude, 14:20 UTC):** native session replay review.
  matched by normalized label (`gw`/`GW`).

**Claimed (Claude, 14:20 UTC):** native session replay review.
- **Strain → suspension:** both sides map to the single set. Refuse only when

**Claimed (Claude, 14:20 UTC):** native session replay review.
  the slot or phase counts differ.

**Claimed (Claude, 14:20 UTC):** native session replay review.
- **Which set is which:** whether `[set 2]` or `[set 3]` is arriving must come

**Claimed (Claude, 14:20 UTC):** native session replay review.
  from existing strung Type J structures in this model (for example 618) or

**Claimed (Claude, 14:20 UTC):** native session replay review.
  from native evidence. Never guess it.

**Claimed (Claude, 14:20 UTC):** native session replay review.
- **Callers:** CLI `dsgrid structure retype` and the Desktop retype picker

**Claimed (Claude, 14:20 UTC):** native session replay review.
  both use it. The preview shows the mapping, and the retype is one

**Claimed (Claude, 14:20 UTC):** native session replay review.
  transaction. Fall back to today's draft only when no safe mapping exists.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Negative control (executed earlier today on a scratch package):** retyping

**Claimed (Claude, 14:20 UTC):** native session replay review.
618 `j-w-S325.014` → `j-w-S255.012` (same set labels) kept every attachment

**Claimed (Claude, 14:20 UTC):** native session replay review.
with no warning. Retyping 618 → `a-w-S325.014` reproduced the unresolved

**Claimed (Claude, 14:20 UTC):** native session replay review.
attachments.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Regression fixture:** a single-set structure strung on both sides,

**Claimed (Claude, 14:20 UTC):** native session replay review.
retyped to a two-set strain type. Expect every support rebound and the package

**Claimed (Claude, 14:20 UTC):** native session replay review.
to emit. Then retype it back: expect the same.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Related:** STABILIZATION.md §4 (swift editing).

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Bounded follow-up requested:** prove the arriving/departing set convention

**Claimed (Claude, 14:20 UTC):** native session replay review.
from already-strung native Type J witnesses such as structure 618, and check

**Claimed (Claude, 14:20 UTC):** native session replay review.
whether structure 292 is an interior support in a continuous tension section

**Claimed (Claude, 14:20 UTC):** native session replay review.
or an endpoint of independent sections. Give exact support sequence, neighboring

**Claimed (Claude, 14:20 UTC):** native session replay review.
structures/stations, attachment labels/slots and native request evidence.

**Claimed (Claude, 14:20 UTC):** native session replay review.
If an interior section must split to introduce a strain, identify the existing

**Claimed (Claude, 14:20 UTC):** native session replay review.
atomic splitting command and its sag/criterion preservation contract. Do not

**Claimed (Claude, 14:20 UTC):** native session replay review.
guess phase ordering from labels, edit the working copy or write source.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
Codex independently read the exact immutable Nyamagabe reference through

**Claimed (Claude, 14:20 UTC):** native session replay review.
`ds dsgrid run --operation project_table` (structures, attachment points,

**Claimed (Claude, 14:20 UTC):** native session replay review.
tension-section supports; receipts in ds-cli/out/stabilization/retype-evidence).

**Claimed (Claude, 14:20 UTC):** native session replay review.
292 is interior sequence 1 of each three-support section: 291 → 292 → 293.

**Claimed (Claude, 14:20 UTC):** native session replay review.
618 is an endpoint on both sides: section 29 ends on `[set 2]`, section 361

**Claimed (Claude, 14:20 UTC):** native session replay review.
starts on `[set 3]`, all phase slots preserved. These are sequence conventions;

**Claimed (Claude, 14:20 UTC):** native session replay review.
618's arriving section crosses alignments, so station comparison would be

**Claimed (Claude, 14:20 UTC):** native session replay review.
wrong. A safe 292 retype must split its sections atomically, not only rebind

**Claimed (Claude, 14:20 UTC):** native session replay review.
four points. Please trace the existing split primitive and the reverse merge.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Claimed (Claude, 13:52 UTC):** split primitive and reverse merge trace.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Findings (Claude, 14:05 UTC; read-only, ds-network `d5f9d0c9`):**

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
1. **Split exists, but only for structure insertion.**

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - `split_at_anchor` (`ds-grid-engine/src/structure_placement.rs:867`,

**Claimed (Claude, 14:20 UTC):** native session replay review.
     private) replaces each member section with two anchor-to-anchor halves.

**Claimed (Claude, 14:20 UTC):** native session replay review.
     Each half clones the replaced section: cable, stringing intent, sag,

**Claimed (Claude, 14:20 UTC):** native session replay review.
     criteria and details are unchanged.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - Lineage is recorded as `TensionSectionSplitDelta`

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (`command.rs:834-881`, `effects.tension_section_splits`).

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - Back/ahead sets come from `resolve_directional_strain` (`:749`), which

**Claimed (Claude, 14:20 UTC):** native session replay review.
     uses authored attachment facings (`orientation_rad`), never labels, and

**Claimed (Claude, 14:20 UTC):** native session replay review.
     refuses uncharacterized facings. It is reached only via

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `apply_plan` / `resolve_action` (`:809-826`, `:579`, candidate sets

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `:514`) when a new structure is placed. Suspension insertion uses

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `splice_suspension` (`:833`) and does not split.

**Claimed (Claude, 14:20 UTC):** native session replay review.
2. **Reverse merge exists, but only for deletion.**

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - Deleting a strain structure heals the run (`command.rs:4103`, owner

**Claimed (Claude, 14:20 UTC):** native session replay review.
     2026-10-05).

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - Only the physical conductor must agree (`conductor_join_conflict`,

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `:4188-4206`: cable, nominal voltage, wires per phase, bundle). The

**Claimed (Claude, 14:20 UTC):** native session replay review.
     incoming section's sag, criteria and details carry the joined run.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - Branch points refuse (`:4087`).

**Claimed (Claude, 14:20 UTC):** native session replay review.
3. **Neither is reachable from a retype.** `RetypeStructuresWithSupports`

**Claimed (Claude, 14:20 UTC):** native session replay review.
   only rebinds supports; it never splits or merges.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Executed:** `project_structure_library` for `j-w-S325.014` shows

**Claimed (Claude, 14:20 UTC):** native session replay review.
     characterized facings: `30kV [set 2]` dead_end at 1.571 rad,

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `30kV [set 3]` dead_end at 4.712 rad, `GW` shield. So

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `resolve_directional_strain` can assign 292's back/ahead sets

**Claimed (Claude, 14:20 UTC):** native session replay review.
     deterministically, consistent with Codex's 618 sequence evidence.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Smallest owner change:** in `structure_placement`, plan a retype that

**Claimed (Claude, 14:20 UTC):** native session replay review.
     changes duty as one transaction, with the existing structure as the

**Claimed (Claude, 14:20 UTC):** native session replay review.
     anchor.

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - **Suspension → strain:** reuse `candidate_sets` +

**Claimed (Claude, 14:20 UTC):** native session replay review.
       `resolve_directional_strain` + `split_at_anchor` at 292's index in

**Claimed (Claude, 14:20 UTC):** native session replay review.
       each member path (both 30kV phase sections and the GW section, which

**Claimed (Claude, 14:20 UTC):** native session replay review.
       binds to `GW`).

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - **Strain → suspension:** reuse the deletion heal (path join +

**Claimed (Claude, 14:20 UTC):** native session replay review.
       `conductor_join_conflict`) while keeping the structure, then bind the

**Claimed (Claude, 14:20 UTC):** native session replay review.
       joined path to the single suspension set.

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - **Refusals:** the same as today's insertion and deletion.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Regression fixture:** the Nyamagabe reference, 292 → `j-w-S325.014`.

**Claimed (Claude, 14:20 UTC):** native session replay review.
     Expect 291→292 and 292→293 halves with inherited sag and criteria, and

**Claimed (Claude, 14:20 UTC):** native session replay review.
     no unresolved attachments. The reverse retype heals back to one

**Claimed (Claude, 14:20 UTC):** native session replay review.
     section per set.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
### Why even successful retypes are too slow for real-time use (measured, Claude, 2026-10-07)

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Owner:** retypes that do work take too long to use live. The edit itself is

**Claimed (Claude, 14:20 UTC):** native session replay review.
cheap; what follows it reprocesses the whole model several times. Full

**Claimed (Claude, 14:20 UTC):** native session replay review.
timings and trace are in STABILIZATION.md §4.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Measured** on ds-server, debug builds (the only `ds` that reads today's

**Claimed (Claude, 14:20 UTC):** native session replay review.
schema-5 packages); Nyamagabe M1, 1.4 MB, structure 618:

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
| Step | Time |

**Claimed (Claude, 14:20 UTC):** native session replay review.
| --- | --- |

**Claimed (Claude, 14:20 UTC):** native session replay review.
| CLI retype dry run, nothing written | 6.0–6.2 s |

**Claimed (Claude, 14:20 UTC):** native session replay review.
| CLI retype write | 3.9–7.3 s |

**Claimed (Claude, 14:20 UTC):** native session replay review.
| Open and validate only | 1.9 s (schema 5) / 5.5 s (schema 4) |

**Claimed (Claude, 14:20 UTC):** native session replay review.
| Whole-model `engineering_issue_layer` | 5.6 s |

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**What one Desktop retype triggers:**

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
1. **The edit.** `gridCommitStructureRetype` applies it in the worker

**Claimed (Claude, 14:20 UTC):** native session replay review.
   (`ds-web/src/lib/grid/session.ts:749`). This part is cheap.

**Claimed (Claude, 14:20 UTC):** native session replay review.
3. **Auto analysis.** The new revision re-offers it

**Claimed (Claude, 14:20 UTC):** native session replay review.
   (`GridProfilePanel.svelte:1210-1226`; on by default since 2026-10-05).

**Claimed (Claude, 14:20 UTC):** native session replay review.
   `executeNativeProfileCommand` (`profile-native.ts`) then:

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - runs `checkpointDsgrid()`, a full package emit;

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - sends `package_bytes: Array.from(bytes)`, a JSON array of 1.4 million

**Claimed (Claude, 14:20 UTC):** native session replay review.
     numbers over Tauri IPC;

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - has the native side decode and validate the whole package, rebuild

**Claimed (Claude, 14:20 UTC):** native session replay review.
     every Profile band, and run the whole-model analysis;

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - returns the full scene and up to 2,000 issues per kind, which are

**Claimed (Claude, 14:20 UTC):** native session replay review.
     re-installed in the worker.

**Claimed (Claude, 14:20 UTC):** native session replay review.
4. **CLI path** (`ds-cli-dsgrid/src/mutation.rs`, `structure/retype.rs`):

**Claimed (Claude, 14:20 UTC):** native session replay review.
   decode the whole package (schema-4 copies re-run the 4 → 5 migration on

**Claimed (Claude, 14:20 UTC):** native session replay review.
   every open), compute line angles for every structure, then re-emit and hash the whole

**Claimed (Claude, 14:20 UTC):** native session replay review.
   package on an explicit file write.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Remaining:** Profile analysis still costs a full package emit, a decode and

**Claimed (Claude, 14:20 UTC):** native session replay review.
validation, a whole-scene rebuild and a whole-model analysis.

**Claimed (Claude, 14:20 UTC):** native session replay review.
A retype that fails, as above, pays the same chain before the save refuses.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Owner ruling (2026-10-07):** a retype, a slide/move, or any other edit must

**Claimed (Claude, 14:20 UTC):** native session replay review.
not save the model. The model stays in memory in a dirty state for fast

**Claimed (Claude, 14:20 UTC):** native session replay review.
interaction; persistence happens only through the Save button. A retype plus

**Claimed (Claude, 14:20 UTC):** native session replay review.
its recalculation must take well under 0.5 s: high-performance editing, not a

**Claimed (Claude, 14:20 UTC):** native session replay review.
sluggish Profile. This means:

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
- no `checkpointDsgrid()`, working checkpoint or package `pack`/validation on

**Claimed (Claude, 14:20 UTC):** native session replay review.
  the edit path;

**Claimed (Claude, 14:20 UTC):** native session replay review.
- a draft that fails package validation must not block the edit; it shows as

**Claimed (Claude, 14:20 UTC):** native session replay review.
  pending issues until the user saves;

**Claimed (Claude, 14:20 UTC):** native session replay review.
- recalculation is scoped to the affected sections, bands and spans.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Fix direction:** send the edit, not the package (the queued replay review

**Claimed (Claude, 14:20 UTC):** native session replay review.
below), and recompute only the affected tension sections, bands and spans.

**Claimed (Claude, 14:20 UTC):** native session replay review.
Acceptance (STABILIZATION.md §4): retyping 618 shows the new type and usage in

**Claimed (Claude, 14:20 UTC):** native session replay review.
well under a second in a release build.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
## Queued next: native session replay for swift Profile edits

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
After the NUM review, trace only STABILIZATION.md §4's package round trip:

**Claimed (Claude, 14:20 UTC):** native session replay review.
`ds-web/src/lib/grid/profile-native.ts`, worker command history, and

**Claimed (Claude, 14:20 UTC):** native session replay review.
`ds-command-kernel/crates/ds-profile-runtime`. The runtime already holds a

**Claimed (Claude, 14:20 UTC):** native session replay review.
session per scope. Identify the smallest native request that can replay the

**Claimed (Claude, 14:20 UTC):** native session replay review.
worker's typed journal with exact revision fences instead of checkpointing and

**Claimed (Claude, 14:20 UTC):** native session replay review.
decoding the entire package after every edit. Cover undo/redo, a replacement

**Claimed (Claude, 14:20 UTC):** native session replay review.
model, interleaved scopes, cancellation and stale results. Find existing

**Claimed (Claude, 14:20 UTC):** native session replay review.
fixtures for parity/refusal tests; name authoritative APIs and any blocker.

**Claimed (Claude, 14:20 UTC):** native session replay review.
Do not write source or launch builds. Maximum two actionable findings; record

**Claimed (Claude, 14:20 UTC):** native session replay review.
unexecuted scenarios as hypotheses. Codex will implement while you review.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Dependency follow-up (owner, 2026-10-07):** scoped recalculation must reach

**Claimed (Claude, 14:20 UTC):** native session replay review.
the adjacent tension sections, not merely the edited section. Trace the native

**Claimed (Claude, 14:20 UTC):** native session replay review.
section-demand/structure-usage/Profile caches and existing dependency tokens.

**Claimed (Claude, 14:20 UTC):** native session replay review.
Give the exact smallest dirty closure for retype, station move, ground move,

**Claimed (Claude, 14:20 UTC):** native session replay review.
split and merge: incident sections, neighboring supports, those supports'

**Claimed (Claude, 14:20 UTC):** native session replay review.
other incident sections, adjacent spans and affected bands. Identify a fixture

**Claimed (Claude, 14:20 UTC):** native session replay review.
where updating only the directly edited section leaves a neighboring support's

**Claimed (Claude, 14:20 UTC):** native session replay review.
usage stale. Specify positive/negative cache controls and fences; no broad

**Claimed (Claude, 14:20 UTC):** native session replay review.
reimplementation or source writes. Edited models remain dirty in memory;

**Claimed (Claude, 14:20 UTC):** native session replay review.
recalculation must not save a package. Split and merge must share this closure.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Interactive boundary (owner, 2026-10-07):** stop at the affected tension-section

**Claimed (Claude, 14:20 UTC):** native session replay review.
ends for both CLI and user edits. Combine adjacent forces at shared end supports;

**Claimed (Claude, 14:20 UTC):** native session replay review.
do not recursively chase sway or negligible downstream effects through the

**Claimed (Claude, 14:20 UTC):** native session replay review.
alignment. Wider propagation is deferred to reconciliation. Distinguish local

**Claimed (Claude, 14:20 UTC):** native session replay review.
interactive work from pending reconciliation in the proposed cache controls.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Claimed (Claude, 14:00 UTC):** dirty-closure / adjacent-section recalculation trace.

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Findings (Claude, 14:25 UTC; read-only, scratch packages only; ds-network `d5f9d0c9`):**

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
1. **Caches are whole-revision; no entity-level dependency exists.**

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - `StructureUsageScreeningCache`

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (`ds-grid-engine/src/structure_usage_cache.rs:22-55`) clears every

**Claimed (Claude, 14:20 UTC):** native session replay review.
     report when the revision changes (`at_revision`) and stores whole

**Claimed (Claude, 14:20 UTC):** native session replay review.
     reports keyed by the exact request. So any edit forces a whole-model

**Claimed (Claude, 14:20 UTC):** native session replay review.
     rescreen.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - Revision tokens exist only per table

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (`RevisionManifest::dependency_tokens`, `revision.rs:266`), plus one

**Claimed (Claude, 14:20 UTC):** native session replay review.
     whole-snapshot `engineering_input_root` (`:286`).

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - An entity-level reference-graph closure exists only for combined models

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (`composite.rs:1430` `mark_dirty`, `:1461` `dependency_closure`). It is

**Claimed (Claude, 14:20 UTC):** native session replay review.
     a reusable shape.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - A scoped primitive already exists: `StructureUsageScreeningRequest.structure_ids`

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (`structure_usage.rs:112-121`) restricts demand assembly to incident

**Claimed (Claude, 14:20 UTC):** native session replay review.
     sections.

**Claimed (Claude, 14:20 UTC):** native session replay review.
2. **Staleness fixture (executed).**

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - Setup: the Nyamagabe reference (stable copy `local-aad500c1e08847db`).

**Claimed (Claude, 14:20 UTC):** native session replay review.
     Retype 292 `a-w-S325.014` → `a-w-S255.012` (same attachment sets; scratch

**Claimed (Claude, 14:20 UTC):** native session replay review.
     package). Run `screen_structure_usage` with

**Claimed (Claude, 14:20 UTC):** native session replay review.
     `criterion-set-b37880c65c5c362a`, all 8 cases, `conventional`, for 291–293.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - Result:

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
     | Structure | Max signed weight span, before → after | Max wind span, before → after |

**Claimed (Claude, 14:20 UTC):** native session replay review.
     | --- | --- | --- |

**Claimed (Claude, 14:20 UTC):** native session replay review.
     | 291 | 212.57 → 226.22 m | 88.214 → 88.201 m |

**Claimed (Claude, 14:20 UTC):** native session replay review.
     | 292 (edited) | 330.14 → 281.65 m | unchanged |

**Claimed (Claude, 14:20 UTC):** native session replay review.
     | 293 | 234.79 → 262.19 m | 32.272 → 32.259 m |

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
   - Recomputing only 292 leaves 291 and 293 stale.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - Timing: screening those 3 structures took 5.3 s on the schema-4 package

**Claimed (Claude, 14:20 UTC):** native session replay review.
     and 1.9 s on schema 5. Opening the package dominates, not the scoped

**Claimed (Claude, 14:20 UTC):** native session replay review.
     screen.

**Claimed (Claude, 14:20 UTC):** native session replay review.
3. **Smallest dirty closure** (one rule, shared by split and merge).

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Basis:** a structure's usage depends on all its incident sections

**Claimed (Claude, 14:20 UTC):** native session replay review.
     (`structure_usage.rs:341-362`). A section's demand depends on its whole

**Claimed (Claude, 14:20 UTC):** native session replay review.
     support path through the ruling span (`engineering_tables.rs:869`).

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Affected sections X per edit:**

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - **Retype or station move of S:** the incident sections of S, plus

**Claimed (Claude, 14:20 UTC):** native session replay review.
       S's route-neighbour line angles if S is a route node.

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - **Ground move of point T:** the sections spanning T's station

**Claimed (Claude, 14:20 UTC):** native session replay review.
       (clearance), plus the incident sections of structures whose ground

**Claimed (Claude, 14:20 UTC):** native session replay review.
       derives from T.

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - **Split:** the replaced section's two halves.

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - **Merge:** the joined section.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Recompute:**

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - section demand and stringing for X;

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - usage for every support on X, combining the cached demand of those

**Claimed (Claude, 14:20 UTC):** native session replay review.
       supports' other incident sections (which is not recomputed);

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - clearance for X's spans;

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - the Profile bands containing them.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Cache keys:**

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - per section: a digest of the section row, ordered supports, their

**Claimed (Claude, 14:20 UTC):** native session replay review.
       type and attachment rows, spans, terrain under the spans, and the

**Claimed (Claude, 14:20 UTC):** native session replay review.
       criterion and cases;

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - per structure: its incident section digests plus its capacity.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Controls:**

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - **Positive:** a distant section keeps its cached demand after 292 is

**Claimed (Claude, 14:20 UTC):** native session replay review.
       edited.

**Claimed (Claude, 14:20 UTC):** native session replay review.
     - **Negative:** 291 and 293 must recompute, as shown in item 2.

**Claimed (Claude, 14:20 UTC):** native session replay review.
   - **Fence:** every cached row carries its revision, and a result from an

**Claimed (Claude, 14:20 UTC):** native session replay review.
     older revision never installs (the existing `profile_selection_stale`

**Claimed (Claude, 14:20 UTC):** native session replay review.
     pattern). Nothing is saved during recalculation (owner ruling above).

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
**Moved from OPEN.md §12 (owner, 2026-10-07; fix with the closure findings above: usage must combine every incident side):**

**Claimed (Claude, 14:20 UTC):** native session replay review.


**Claimed (Claude, 14:20 UTC):** native session replay review.
- `4ba8ab15` (major): screen_structure_usage takes one side at section-boundary structures: wind span ~0.5x PLS, 115 spurious uplifts (Gisagara calibration).

**Claimed (Claude, 14:20 UTC):** native session replay review.
