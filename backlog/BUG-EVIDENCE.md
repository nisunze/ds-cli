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

**F2. The Desktop/web WASM session drops pins on open and on every checkpoint
(hypothesis: code-proven, not executed).**

- **Evidence:**
  - `GridSessionHandle::open_dsgrid` (`ds-grid-wasm/src/session.rs:545`)
    keeps the model id, revision, presentation, CRS, assets and exchange
    bindings, but has no field for `library_pins` / `library_needs`.
  - `pack_options` (`:3170-3182`) hard-codes `library_pins: Vec::new()` and
    `library_needs: Vec::new()`.
  - `checkpoint_dsgrid` (`:933`) emits with those options.
  - `package_fingerprint` (`:3145`) also ignores pins, so a pin-only change
    never counts as a change.
  - The cached release bytes survive in `assets`, but the pins pointing at
    them are gone.
- **Affected callers:** all ordinary instance edits, none of which change
  library members, so all must carry pins unchanged:
  - every Desktop/web save and working checkpoint
    (`ds-web/src/lib/grid/session.ts:371`, `:1644`, `:1733`);
  - the native Profile/analysis handover
    (`ds-web/src/lib/grid/profile-native.ts:23`);
  - the model-menu export prepare (`GridExportModelDialog.svelte:213`).
- **Reproduction for Codex** (not written by Claude):
  1. In a `ds-grid-wasm` native test, load the bytes of `adopted.dsgrid`, or
     build them as `ds-grid-exchange/tests/model_library.rs` does
     (`create_model_library` + `attach_model_library`).
  2. Run `GridSessionHandle::open_dsgrid(&bytes)?.checkpoint_dsgrid()?`.
  3. Unpack the result and assert that `manifest.model.library_pins` and
     `library_needs` equal the input's. The expected failure is an empty
     vector.
  4. **Negative control:** an unpinned package checkpoints with empty pins
     unchanged.
- **Smallest fix:** store pins and needs on the handle at `open_dsgrid`, use
  them in `pack_options`, and include them in `package_fingerprint`.

**F3. Negative controls and paths that need no fix.**

- **Executed:** `ds dsgrid profile labels set --model adopted.dsgrid --fields number,type --out labelled.dsgrid`
  keeps the pin (`library model show`: 1 pin, managed `true`), via
  `profile/labels.rs:187-201`.
- **Correct by code:** `validate.rs:304-318` and `apply_batch.rs:818-832`
  copy pins and needs from the source manifest.
- **Legitimately pinless (new models):**
  - `pack_imported_model` (`conversion.rs:8940`);
  - `pack_lv_transformer` (`lv_transformer.rs:89`);
  - `blank_model.rs:251`;
  - `ds-grid-cli` `pack_command` / `pack_element_snapshot*` (`main.rs:2642`,
    `:2985`, `:4738`).
- **Test-only empty-pin sites:**
  - `lineage_transfer.rs` tests;
  - `ds-profile-runtime/src/lib.rs:461`;
  - `conversion.rs:9480`;
  - `retype_tests.rs`.

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
