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
STABILIZATION.md §2 / OPEN.md §11. Trace the conversion planner's renumbering
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
2. **Session refresh:** read validation/history and transfer every headline
   table in full, even though the native readiness summary carries counts.
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

## Queued next: native session replay for swift Profile edits

After the NUM review, trace only STABILIZATION.md §4's package round trip:
`ds-web/src/lib/grid/profile-native.ts`, worker command history, and
`ds-command-kernel/crates/ds-profile-runtime`. The runtime already holds a
session per scope. Identify the smallest native request that can replay the
worker's typed journal with exact revision fences instead of checkpointing and
decoding the entire package after every edit. Cover undo/redo, a replacement
model, interleaved scopes, cancellation and stale results. Find existing
fixtures for parity/refusal tests; name authoritative APIs and any blocker.
Do not write source or launch builds. Maximum two actionable findings; record
unexecuted scenarios as hypotheses. Codex will implement while you review.

**Dependency follow-up (owner, 2026-10-07):** scoped recalculation must reach
the adjacent tension sections, not merely the edited section. Trace the native
section-demand/structure-usage/Profile caches and existing dependency tokens.
Give the exact smallest dirty closure for retype, station move, ground move,
split and merge: incident sections, neighboring supports, those supports'
other incident sections, adjacent spans and affected bands. Identify a fixture
where updating only the directly edited section leaves a neighboring support's
usage stale. Specify positive/negative cache controls and fences; no broad
reimplementation or source writes. Edited models remain dirty in memory;
recalculation must not save a package. Split and merge must share this closure.
