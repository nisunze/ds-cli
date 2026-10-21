# Public development-feedback evidence

This reference belongs to shared agent guidance, separate from product cartography.
The adopted DS behavior is owned by [the collaboration agreement](../../collaboration.md#small-implementation-loop)
and reached through [AGENTS.md](../../../../AGENTS.md). Upstream snapshots below
provide evidence; they do not become another agent instruction file.

Question: how do GeoLibre and RustGIS developers encourage agents to be correct
and sustain useful coding work? The strongest visible mechanism is a short path
from a change to a specific failure, review finding or rendered result. Public
files do not establish private prompts, model choices or continuous-session limits.

## RustGIS: commands, executable boundaries and visible results

The pinned [AGENTS.md](https://github.com/opengeos/RustGIS/blob/e9d71948a975989629d83dee9ddce0a63eece204/AGENTS.md)
routes behavior through registered commands, keeps UI thin, requires browser
compatibility and calls for looking at an actual PNG for UI changes. It names a
single local `cargo xtask ci` entry point and hostile-parameter regression tests.

[The xtask implementation](https://github.com/opengeos/RustGIS/blob/e9d71948a975989629d83dee9ddce0a63eece204/xtask/src/main.rs)
runs formatting, warnings-as-errors, tests and a dependency-layer check. That
check reads crate manifests and rejects forbidden UI or same/higher-layer edges.
It is a specific enforcement mechanism, not proof that every possible dependency
alias or transitive path is covered.

The [headless UI harness](https://github.com/opengeos/RustGIS/blob/e9d71948a975989629d83dee9ddce0a63eece204/crates/ui-egui/examples/ui_shot.rs)
accepts scripted commands, labelled clicks, frames, dimensions and screenshot
targets, rendering the application offscreen. The result gives a coding agent a
repeatable way to inspect what its UI change produced. It still needs inspection;
generating a file alone does not prove layout quality.

[CI](https://github.com/opengeos/RustGIS/blob/e9d71948a975989629d83dee9ddce0a63eece204/.github/workflows/ci.yml)
includes Linux/macOS/Windows tests and browser compilation. The
[review workflow](https://github.com/opengeos/RustGIS/blob/e9d71948a975989629d83dee9ddce0a63eece204/.github/workflows/claude-code-review.yml)
has a 30-minute job timeout, staged PR context and a restricted read/comment
reviewer. That timeout is a review budget, not evidence of a continuously coding
agent. No coding-model override or force-continuation hook was found in the
examined workflow and tracked source snapshot.

## GeoLibre: quality ratchets and consumer-level feedback

The pinned [agent guide](https://github.com/opengeos/GeoLibre/blob/ab909bd634639def1038480753e32ba23c3542e0/CLAUDE.md)
gives focused commands and a broader local gate, describes one state owner, and
links detailed maintenance rules. It warns that missing optional backend engines
can cause important tests to skip, and that a broad pre-commit build can touch
unrelated state. Those warnings make the claimed verification more precise.

[Maintenance](https://github.com/opengeos/GeoLibre/blob/ab909bd634639def1038480753e32ba23c3542e0/docs/maintenance.md)
documents floors of 78% lines/branches and 63% functions for its frontend and 55%
for its backend. It also addresses modules omitted from coverage: the checker
tracks large untested files, refuses new entries over 500 lines and growth over
50 lines beyond a recorded entry, and prunes the baseline as obligations shrink.
These are upstream-specific baselines, not numbers to impose on DS without a
measured starting point.

The [built-app Playwright configuration](https://github.com/opengeos/GeoLibre/blob/ab909bd634639def1038480753e32ba23c3542e0/playwright.config.ts)
separates fast shared-surface checks from slower feature suites, with measured
worker limits and failure traces/screenshots. The inspected style-manager and
print-SVG specs are feature tests, not blanket per-PR gates. That tradeoff is
explicit; their existence does not prove they ran for a particular release.

Its [style-manager test](https://github.com/opengeos/GeoLibre/blob/ab909bd634639def1038480753e32ba23c3542e0/e2e/style-manager.spec.ts)
checks editing, cancellation, exported bytes and reopen/persistence. Its
[SVG-export test](https://github.com/opengeos/GeoLibre/blob/ab909bd634639def1038480753e32ba23c3542e0/e2e/print-layout-svg.spec.ts)
opens the composer, exports, parses and decodes the result, and checks dimensions,
editable furniture, embedded-image and unsafe-element properties. Neither test
qualifies DS engineering output or proves fully vector maps.

GeoLibre separates [a read/comment reviewer](https://github.com/opengeos/GeoLibre/blob/ab909bd634639def1038480753e32ba23c3542e0/.github/workflows/claude-code-review.yml)
from [an explicitly invoked editing agent](https://github.com/opengeos/GeoLibre/blob/ab909bd634639def1038480753e32ba23c3542e0/.github/workflows/claude.yml).
The latter permits a 60-minute job after checking the invoking person's write
authority. The inspected configuration does not set a coding model, reasoning
budget or maximum-turn override. A workflow definition is not a record that it
executed successfully or that it explains the project's release speed.

## What can be concluded about longer sessions

**Supported:** the repositories expose precise architecture constraints, named
verification commands, regression checks, visual feedback and code-review surfaces.
Those give an agent concrete next actions when a change fails.

**Inference:** reducing uncertainty about what to inspect, run and repair can make
a sustained coding session more productive. Public evidence does not establish
the causal contribution of any model, prompt, private stop hook or session length.

**Unknown:** private editor settings, prompt history, model/reasoning preference,
token budgets, retry/continuation policy, amount of human correction and actual
installed-release acceptance. In-app assistant model choices describe the product;
they do not identify the model used to develop it.

## Adopted behavior and remaining candidates

The shared agreement now makes continuation explicit: resolve actionable failures
within the authorized slice, verify its real consumer, and stop after the agreed
proof or report an exact blocker. Existing rules already require scoped tests,
UI/artifact inspection, ownership and selective landing; those are retained rather
than copied here. AGENTS.md remains the sole instruction filename.

The evidence does not justify copying a private model policy, forcing a longer
session, adding a new framework or imposing upstream coverage numbers on DS.
A new verification mechanism still needs a demonstrated gap and its owning proof.
Engine-specific consumer work follows the actual extraction unit; the general
continuation rule applies to already authorized coding work. No automation or
product checks were started by this documentation change.

## Candidate enforcement session: close one real feedback gap

Starting point: existing AGENTS.md routing, local verification commands and the
selected component's current checks. Choose a specific problem that was previously
missed: a forbidden import, a skipped required engine, a stale preview, wrong units,
lost pen override, unreadable export or a consumer that never exercised the binary.

Prefer an existing gate with a direct contract/control case. For a later UI slice,
retain an application-rendered view; for output, reopen and verify the actual
artifact. Keep repeated diagnostics and checks scoped to new failures or changes.
The agent can continue independently while a specific failure is actionable, and
must report the precise blocker when the scope cannot be completed.

Future evidence: demonstrate the problematic change fails the gate and the admitted
control passes. Verify the final consumer; a grep or fixture-only check is insufficient
when the original defect was in runtime behavior. Preserve the local-build and owned
`run`-path agreement; do not copy upstream remote deployment automation.

Stop after one demonstrated verification gap. No private model-policy file, global
coverage percentage, broad repository restructuring or autonomous forever-work loop.
