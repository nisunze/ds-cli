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
ready; avoid a broad review or repeated summaries. Codex has independently reproduced and is fixing mutation.rs, apply.rs and
preview_receipt.rs; review other write paths and negative controls in parallel.
Avoid duplicating that fix. Current proof: ds-cli/out/stabilization/pin-proof/.


**Claimed (Claude, 2026-10-07 13:28 UTC):** write paths other than mutation.rs/apply.rs/preview_receipt.rs, plus negative controls.

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
