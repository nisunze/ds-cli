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
| ds-network | `cargo test --locked --workspace --no-fail-fast` | Full settled-tip run: three known private-corpus failures in two targets; all other targets passed. Fixes remain in OPEN.md §11 |
| ds-solar | `cargo test --locked --workspace` | Not run. It links the kernel and the reporter's `ds-network-information`. |

- The three private-corpus tests are `conversion_planner::corpus_planner_bak_matches_direct_ingest_model`, `conversion_planner::corpus_planner_normalizes_characterized_19_01_source_to_16_81` and `ds-grid-wasm import_convert::corpus_huye_backup_imports_through_the_wasm_handle`.
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

## 7. Owner wording check

The MV plan key now prints the held legend titles (ds-network-reporter
`3748e48`). On the Gisagara capture that reads "New LV cables / Existing LV
lines / Existing MV lines" instead of "Context LV / Existing LV / Existing
MV". If the owner wants the old words, author them as governed legend titles
in the MV context template; the template-literal ratchet forbids Rust
literals.

## 11. One colour code for usage %

**Owner, 2026-10-07:** the same value must use the same colour code.

**Seen:** Nyamagabe MV Model 1 (oracle reference copy), structure 5
(`l-s-S800-stay.012`). The Profile drawing shows 3.4% in green; the toolbar
badge shows the same 3.4% on amber.

**Cause:**

- The badge picks its own colour in TypeScript from `engineering_status`
  (`fail` → rose, `pass` → green, `unknown` → amber), in
  `ds-web/src/lib/components/grid/GridProfileContextBar.svelte:27-29`.
- The drawing uses the engine's colour role for the label.
- So there are two colour authorities for one value.

**Needed:**

- The badge takes the same colour role as the Profile label from the
  engine/kernel; the UI does not decide colours (PRINCIPLES.md §5).
- The band thresholds themselves are still the owner's open choice:
  OPEN-X.md "Usage-% colour bands".

## Reference: landed on 7 October, don't redo

ds-network `720f4184` (one compact attachment box per structure), `516e29c0`;
ds-network-reporter `ade0aa2`, `d0db433`, `e24602e`, `3748e48`, `6147edc`,
`ac7e740`, `b28c844`; ds-cli `31e3c1b4`, `af9b7621`, `4e466494`; ds-web
`be5be7445`; kernel `3e23208`.
