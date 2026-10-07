# Bug evidence and review requests

The coding Codex is the sole source writer. Claude or a reviewing Codex can
pick up these requests without an owner relay. Review read-only; write only
this file and [STABILIZATION.md](STABILIZATION.md). No feedback submissions,
project mutations, publication, source edits or competing builds. Codex
independently confirms each finding, implements it and owns proof and landing.
Request the next bounded review before finishing the current coding slice.
The coding agent re-reads both files between slices and incorporates new
evidence; reviewers mark a request claimed here to avoid duplicate work.

**First assignment: library identity survives CLI edits (STABILIZATION.md §5).** Trace the
shared persistence path in `ds-cli/crates/ds-cli-dsgrid/src/mutation.rs` and its
callers against the native package contract. Check exact library pins and
needs, retained assets and exchange bindings. Name every affected command;
distinguish commands that change library members from ordinary instance edits.
Find the smallest existing fixture and CLI invocation that can demonstrate a
lost pin, plus a dry-run/no-change negative control. If a live reproduction
needs a new fixture, give Codex the exact preparation instead of writing one.

Record at most three actionable findings in BUG-EVIDENCE.md: source/build
revision, exact request, expected versus observed behavior, path/line evidence,
negative control, affected callers, and the smallest owning fix/test surface.
Label unexecuted reproductions as hypotheses. Stop once those findings are
ready; avoid a broad review or repeated summaries. Codex fixed mutation.rs,
apply.rs and preview_receipt.rs in ds-cli `eb1fea75`, with CLI reproduction,
regression tests and Clippy. Review other write paths and negative controls
in parallel. Current proof: ds-cli/out/stabilization/pin-proof/.


**Claimed (Claude, 2026-10-07 13:28 UTC):** write paths other than mutation.rs/apply.rs/preview_receipt.rs, plus negative controls.

### Findings (Claude, 2026-10-07 13:45 UTC; review complete)

Sources: ds-network `d5f9d0c9` (clean); ds-cli `97f429e1` (dirty, Codex's
fix in progress); `ds` = `ds-cli/target/debug/ds` (built 13:26). The fixture is
a copy of `ds-cli/out/stabilization/pin-proof/adopted.dsgrid`: 1 pin
`pin-preservation-proof` r1, `managed_export_allowed: true`. Outputs went to
`~/.cache/claude-build-tmp/pin/` only.

**F1. Composition drops library pins (executed, reproduced).**

- **Request:**
  `ds dsgrid-exchange convert --source adopted.dsgrid --source labelled.dsgrid --mode compose --target dsgrid --out <dir> --output json`,
  then `ds library model show` on the composed package.
- **Expected:** the union of the inputs' pins (here, the one shared pin), or a
  refusal when pins conflict.
- **Observed:** 0 pins and `managed_export_allowed: false`; both inputs had
  1 pin and `true`.
- **Evidence:** `compose_execute` (`ds-grid-exchange/src/composition.rs:1589`)
  packs with `library_pins: Vec::new()`, `library_needs: Vec::new()` and
  `exchange_bindings: BTreeMap::new()` (`:1737-1748`).
  `require_managed_export` then returns `Ok` for a pinless model
  (`model_library.rs:495`), so a later PLS export runs unchecked.
- **Affected callers:** the model-menu multi-model export
  (`ds-web/src/lib/grid/export-model.ts:43`, `batchMode: 'compose_ds_grid'`)
  and `ds dsgrid-exchange plan/convert --mode compose`.
- **Classification:** this changes library membership (several models become
  one), so it needs a union with conflict refusal, not a plain copy.
- **Smallest fix:** in `compose_execute`, carry the union of the inputs' pins
  and needs together with their cached `ds-grid-library-*` assets, and refuse
  two releases of one library identity. Owning test: `ds-grid-exchange`
  composition tests with two pinned inputs (same pin → kept; conflicting pins
  → refused).

Codex independently reproduced F1 with two identical pinned inputs through the
current CLI. Exact requests/results: `ds-cli/out/stabilization/pin-proof/`
`composition-identical.json` and `composition-identical-pins.json`; output has
zero pins and managed export refused. Implement the union/conflict/verification
slice separately; do not silently detach.

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
