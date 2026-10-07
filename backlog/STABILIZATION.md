# Stabilization

For Codex, on canonical `run`. Opened 2026-10-07 by the owner after the 7
October test sweep. There is no deployment pressure.

**Scope:** this work may touch any other backlog surface it intersects:
[OPEN.md](OPEN.md) §2 formatting and release gates, §11 PLS exchange, §12
structure usage, §18 libraries, §23 JSON UI, §25 CLI reliability; and
[SEEDING.md](SEEDING.md) §18. When a slice fulfils part of one of those
entries, update or delete that entry instead of duplicating it here. Delete
each item below once it is done, as for every backlog file.

**How to work it:** one small slice at a time. Prove it with the owning tests,
then commit to `run` in its owning repository. Never use a worktree.

## Owner rulings that apply

- **No more breaking schema bumps:** follow [PRINCIPLES.md §2](PRINCIPLES.md#2-how-to-work-an-entry).
- **Never work in worktrees.** Before any release, run the owning full suites
  on canonical `run`.
- **ds-server keeps only the canary package.** The stable `ds-cli-headless`
  package was uninstalled; only canary 0.1.6 and its service remain.
- **National ID is not urgent and gets no code now.** See the
  [OPEN.md §2](OPEN.md#2-green-run-and-release-gates) note: DS handles national
  IDs, and the `a69402e` restriction came from the datasolutions.rw product
  context.

## 2. Suites to prove on settled tips

| Suite | Command | Last result (7 October) |
| --- | --- | --- |
| ds-network-reporter | `cargo test --locked --no-fail-fast` | All green |
| ds-cli | `DS_REPORT_BIN=<reporter target>/debug/ds-report DS_SOLAR_BIN=<ds-solar target>/debug/ds-solar cargo test --locked --workspace --all-features --no-fail-fast` | 107 suites green; the MCP skill check was fixed in `4e466494` |
| ds-network | `cargo test --locked --workspace --no-fail-fast` | Pending on settled tips; the three private-corpus PLS imports are already in OPEN.md §11 |
| ds-solar | `cargo test --locked --workspace` | Not run. It links the kernel and the reporter's `ds-network-information`. |
| ds-command-kernel | `cargo test --locked --workspace --all-features --no-fail-fast` | `template_literal_ratchet` rejects three new Profile style-target literals |

- The three private-corpus tests are `conversion_planner::corpus_planner_bak_matches_direct_ingest_model`, `conversion_planner::corpus_planner_normalizes_characterized_19_01_source_to_16_81` and `ds-grid-wasm import_convert::corpus_huye_backup_imports_through_the_wasm_handle`.
- `c28ee1a5` landed during the ds-network run: rerun `ds-geo` and
  `ds-grid-engine`.
- The stable `/usr/bin/ds-report` is uninstalled on ds-server, so point
  `DS_REPORT_BIN` and `DS_SOLAR_BIN` at freshly built engines.
- All five lockfiles are current (`cargo metadata --locked`).

## 3. Release pins, last

Once the slices above have landed:

- set `ds-cli/pins/ds-command-kernel.rev` to the kernel `HEAD`, exactly;
- point `ds-cli/pins/ds-client-core.rev` at a commit whose native crates
  (`ds-client-core`, `ds-report-host`, `ds-sync-runtime`, `ds-layer-store`,
  `ds-project-data`, `ds-edge-authority`) are tree-identical to `HEAD`;
- set `ds-cli/pins/ds-network.rev` to the ds-network `HEAD`;
- in `ds-network-reporter/pins/native-build-sources.json`, set the kernel and
  ds-network entries to the same tips (ds-solar stays `bdae6aa`);
- prove with `cd ds-cli && scripts/with-network-deps.sh true`;
- then run `cd ds-web && bash -l -c './linux-server-deploy.sh prod --check'`.
  It builds and publishes nothing; the version gate passes (source 0.1.7,
  production `0.1.7-r86`). Publishing is the owner's decision.

## 4. PLS-CADD `.bak` export holes

Ten holes are diagnosed, with file references and the filed items they
advance (`e258b287`, `8f6e3ff1`, `b1a2b25c`, `4af28da3`, the §11 Restore
items, §18, §23, §25), in seven slices:
`outputs/pls-bak-export-plan-20261007/PLS-BAK-EXPORT-PLAN.md` (in the
workspace, outside git).

## 5. Uneven structure results across models

**What the owner sees:**

- Nyamagabe MV Model 1, "2026-10-02 oracle reference copy", shows complete
  structure computation in Profile: usage % per structure, the counted section
  boxes, and "Analysis complete" in the Engineering issues panel.
- Other models stop at `structure_usage` issues with an unresolved
  `capacity_case_binding` / unresolved capacity, or at "Engineering issue
  unsupported".

**Ask:**

1. For each project model, list through `ds` which structures lack a usage
   result and the exact reason. Possible reasons: missing or unqualified
   capacity artifact, unresolved capacity case binding, analytical-scaffold
   provenance, unverified Method-2 interaction artifact.
2. Test whether attaching the common cloud interoperability library gives
   every model the oracle copy's completeness. The candidate is the
   157-member `pls-interoperability-canonical-candidate` (2026-10-07-v2).
3. Report which gaps the library closes and which need source evidence
   instead. Never invent capacities.

**Intersects:**

- OPEN.md §12: "1. Loads and strength (structure usage)", "Qualified
  structure usage contracts", `df1f53ae`, `908c07c1`, `4ba8ab15`.
- OPEN.md §14: "Show loading, uplift and failures in Profile".
- SEEDING.md: "Saved scaffold trust states", "Shared-library capacity tables"
  (`3acfd343`), "Four-donor composite native qualification".
- OPEN-X.md: "Material and capacity source conflicts", "Uncharacterized .012
  policy" (`5d0fd02b`).

## 6. Owner direction: externalize DS Grid libraries

In the owner's words: libraries will be externalized "violently". Many models
refer to the same external libraries, such as criteria (CRI) and structures,
so that updating one library updates every model that uses it. The cloud
interoperability library sits beside DS Grid's own library, with third
parties starting with PLS-CADD and possibly later Camelia or Jove.

**What exists today:**

- A model carries exact, immutable library pins and a cached copy of each
  pinned release inside its package (`library.model.create`, `attach`,
  `detach`, `clone` and `show`; ds-network `c2a17b70`, `b442dd6c`).
- Governed global libraries are reached through `library.global.*`.
- Kernel publication is bound to exact package bytes (`3bda1f1`); exact
  indexed member downloads exist (`9c44615`).
- OPEN.md §18 forbids any head, basename or scaffold fallback.

**Before any code, bring the owner a design that answers:**

1. **Propagation versus reproducibility.** How does one library update reach
   every referencing model while each model revision still records the exact
   release it was computed with? For example, an auditable re-pin that
   produces new model revisions, with an impact report.
2. **Changed results.** What happens to a model whose results change after a
   library update: flagged stale, recomputed, or reviewed?
3. **Offline.** How do the Desktop and Server behave against a referenced,
   not embedded, library?
4. **Third-party adapters.** How do PLS-CADD first, and Camelia or Jove later,
   map onto one interoperability library without becoming the authority?
5. **Schema.** References must not force another breaking `.dsgrid` schema
   bump; carry them compatibly.

## 7. Owner wording check

The MV plan key now prints the held legend titles (ds-network-reporter
`3748e48`). On the Gisagara capture that reads "New LV cables / Existing LV
lines / Existing MV lines" instead of "Context LV / Existing LV / Existing
MV". If the owner wants the old words, author them as governed legend titles
in the MV context template; the template-literal ratchet forbids Rust
literals.

## 8. The right dock: Issues opens uninvited and keeps the focus

**Owner, 2026-10-07:**

- The Engineering issues drawer "is coming uninvited too much". Seen on
  Desktop with Nyamagabe MV Model 1, while working on the map and Profile.
- Opening Properties docks it, but Issues stays the active tab. The panel the
  user opened last should be the active one.

**Cause:**

- Every native Profile run with analysis publishes an issue target
  (`ds-web/src/lib/desktop/cli-profile-native.ts:57`,
  `engineeringIssueDock.set(...)`).
- `ds-web/src/lib/components/map/MapRightDock.svelte:74` then switches the
  right dock to Issues whenever issues become available
  (`if (showIssues && !previousIssues) active = 'issues'`).
- So each compute or refresh pops the drawer open, even when nobody asked.
- The invited path is the Profile toolbar's Issues button
  (`GridProfilePanel.svelte:726`).
- The dock's tab rule reacts only to Issues and Work appearing
  (`previousIssues`, `previousWork` in `MapRightDock.svelte`). It never reacts
  to Properties, so opening Properties leaves Issues active.

**Needed:**

- Keep the analysis result current without opening the drawer or switching
  the tab.
- Open it only on the user's action (the Issues button or a finding link).
- Respect a closed drawer until the user reopens it.
- Whatever the user opened last (Properties, Issues or Work) becomes the
  active tab.

This intersects Codex's in-progress Profile UI work: fold it into that slice.

## 9. One selection-aware context menu in every editing context

**Owner, 2026-10-07:** LV design shows a contextual menu when something is
selected, with the selection filter and the other selection actions. The
other editing contexts do not offer the same.

**Seen:**

- In the DS Grid model edit context (Nyamagabe MV Model 1, oracle reference
  copy), right-click shows only "Measure distance", "Exit edit context",
  "New…" and "Fix Popup".
- It has no selection filter and no selection actions. General geographic
  editing has the same gap.

**Needed:**

- The same selection-aware context menu as LV design (selection filter and
  selection actions) in the DS Grid model edit context and in general
  geographic editing.
- The active editing context decides which actions apply.
- Per [PRINCIPLES.md §5](PRINCIPLES.md), the actions come from the
  kernel/CLI contracts and the UI renders them.
- Starting point for the filter:
  `ds-web/src/lib/components/map/SelectionFilterDialog.svelte`.

## Reference: landed on 7 October, don't redo

ds-network `720f4184` (one compact attachment box per structure), `516e29c0`;
ds-network-reporter `ade0aa2`, `d0db433`, `e24602e`, `3748e48`, `6147edc`,
`ac7e740`, `b28c844`; ds-cli `31e3c1b4`, `af9b7621`, `4e466494`; ds-web
`be5be7445`; kernel `3e23208`.
