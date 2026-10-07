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
