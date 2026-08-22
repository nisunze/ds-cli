# Migration matrix — `ds-mcp` → `ds`

**Status of this document:** evidence-based. Every claim below was read out of
the current tree, with the file and line recorded. Nothing here is a plan
asserted as a fact.

**Status of the migration:** Go is gone from the desktop package and from the
local development launcher. `ds` ships as a core bundled component of the
Linux `.deb`. Five domains and sixteen domain commands are real. The
hosted-`ds-mcp` release gate is removed. The old repository is retained only as
historical source material; no shipping, development, or hosted runtime path
depends on it.

## What is proved, and how

### The packaging cutover

| Claim | Evidence |
|---|---|
| Tiered discovery works and stays cheap | `context_budget.rs`, 10 tests; root help stays under its domain-scaled byte budget |
| Help cannot drift from behaviour | `command_help_matches_its_descriptor` compares both against one declaration |
| The output/exit-code contract holds | `cli.rs`, 19 tests, incl. `exit_code_and_envelope_class_always_agree` |
| `ds` reaches a real engine, not a mock | `ds network inspect` over `ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid`, asserting the engine's own fingerprint `fnv1a64:f091cb7021191169` |
| Refusals are typed and actionable | `desktop_ambiguous` fires on this machine's two live profiles and names both descriptor paths |
| Build identity is verifiable | `ds version --output json` → source SHA, dirty flag, target, profile |
| Release builds reproducibly | `cargo build --locked --release` → 3.6 MB stripped binary |

| Claim | Evidence |
|---|---|
| The `.deb` builds with `ds` and without Go | `./desktop-build-linux.sh canary` → exit 0, `DS GridDesign Canary_0.1.2_amd64.deb`, 84.9 MB |
| The package installs exactly three binaries | `dpkg-deb -c` → `usr/bin/ds-web-desktop-canary`, `usr/bin/ds-report`, `usr/bin/ds` |
| No MCP binary is in the package | `dpkg-deb -c … \| grep -i mcp` → empty |
| The installed `ds` finds its engine with no configuration | extracted `.deb`, `env -u DS_REPORT_BIN ./ds report engine` → reporter identity `67a72e12…` from the sibling `ds-report` |
| `ds doctor` is honest about what is missing | same tree → 8 available, 4 blocked, each solar command naming its remedy |
| The whole ds-web suite still passes | `npx vitest run` → 645 files, 5 192 tests, 0 failures |
| The component boundary holds in Rust | `cargo test --lib component_manager` → all pass, incl. `catalog_has_exact_core_and_optional_boundaries` |

**Not proved:** the Windows build (edited for parity, not executed here), a
real `apt install` smoke, any `ds report export` against real project data, and
any parity claim for a specific `ds-mcp` tool.

## The surface to migrate

Counts read from the current tree:

| Surface | Count | Where |
|---|---|---|
| Registered MCP tools | 58 | `ds-mcp/internal/tools/tools.go:718-724` |
| Planned-only catalog entries | 7 | `ds-mcp/internal/tools/catalog.go` `plannedCatalog` |
| Desktop bridge semantic operations | 42 | `ds-web/src-tauri/src/agent_bridge.rs:37-83` |
| `ds-grid` CLI subcommands | 45 | `ds-network/crates/ds-grid-cli/src/main.rs:213` |
| Shipping Go/MCP references | 94 | across `ds-web`, `ds-apis-tf` |

## Owners already in place

This is the most important architectural finding, and it changes how `ds`
should reach three of its domains.

**Three typed process boundaries already exist**, each deliberately designed —
not accidentally separate:

| Binary | Source | Contract |
|---|---|---|
| `ds-grid` | `ds-network/crates/ds-grid-cli` | one named subcommand per call |
| `ds-report` | `ds-network-reporter/src/bin/ds-report.rs` | typed request file; machine-readable result file; result must not already exist, and `--force` is deliberately absent |
| `ds-solar` | `ds-solar/apps/ds-solar-cli` | `prepare` may touch the network, `run` may not |

`ds-report`'s own header states the rule verbatim: *"one named subcommand per
call — never a caller-supplied argv… a typed request file — not flags built
from model output… a machine-readable result document — never parsed stdout
prose."*

So `ds` uses **two** routes, chosen by evidence rather than taste:

- **Link the crate** where it is a pure library with a clean boundary. This is
  what `ds network inspect` does today with `ds-grid-model` and
  `ds-grid-exchange` — the same crates `ds-web/src-tauri/Cargo.toml:32-34`
  links.
- **Call the typed process contract** where the owning workspace has chosen
  process separation and documented why. Reporter and Solar are in this
  category. Re-exposing their libraries would discard a boundary their owners
  built on purpose.

## Dispositions

Classification of the whole `ds-mcp` registry is in progress. What is settled:

| Class | Disposition | Examples |
|---|---|---|
| MCP transport plumbing | **delete** — it disappears with the protocol | stdio/HTTP servers, session handling, tool schemas, handshake, `/mcp`, `/healthz` |
| Model/chat orchestration | **delete from this path** — it does not belong in a deterministic CLI | the chat loop, provider routing, capability profiles, `ChatTools` |
| Planned-only catalog entries | **delete the descriptor; keep the gap recorded** | `solar_publish`, `apply_layer_style`, `network_report_transformer_errors` — none has a handler |
| Dead registrations | **delete** | `compare_don_assignment` is registered but `DonAssignmentTasks` is never wired in either `cmd` binary (`cmd/ds-mcp/main.go:257`), so it always answers `not_configured` |
| Dead bridge operations | **delete from the allowlist** | `gis.clip`, `style.preview`, `style.save_local`, `workspace.save`, `workspace.save_as` are in `LOCAL_OPERATIONS` but have no case in `executeAgentOperation`; the frontend throws "not available in this release" |
| Real capabilities | **migrate** to a `ds` command over the correct owner | the rest |

## What was actually changed

### `ds-cli`

Five domains, sixteen domain commands plus three root metadata commands.
`solar` and `report` reach their owners through the typed process contracts
those workspaces published; see
[`../contracts/process-boundary-contract.md`](../contracts/process-boundary-contract.md).

### `ds-web` — Go removed from the shipping path

| File | Change |
|---|---|
| `scripts/desktop/prepare-ds-mcp-sidecars.sh` | **deleted**; replaced by `prepare-ds-cli-sidecar.sh` |
| `scripts/desktop/test-prepare-ds-mcp-sidecars.sh` | **deleted**; replaced by `test-prepare-ds-cli-sidecar.sh` |
| `desktop-build-linux.sh` | Go toolchain check removed; `ds-mcp` sibling requirement → `ds-cli`; builds the `ds` sidecar |
| `src-tauri/tauri.desktop-runtime.conf.json` | `externalBin` → `["binaries/ds-report","binaries/ds"]` |
| `scripts/verify-desktop-bundle-policy.sh` | allowlist assertion retargeted; **and** the three package-content checks made exact — a substring test for `ds` would have matched every `ds-griddesign` path and proved nothing |
| `desktop-build-windows.sh` | the same allowlist assertion, for platform parity |
| `desktop-components.json` | component `ds-mcp` → `ds-cli`, still `core` / `bundled-native` |
| `src-tauri/src/component_manager.rs` | core-id set and its tests |
| `src/lib/desktop/components.ts` | the TypeScript mirror of that boundary |
| `scripts/build-desktop.mjs`, `desktop-release-lib.sh`, `scripts/desktop/build-cache.sh` | release metadata field `mcp_source_sha` → `cli_source_sha`, same position in the composite hash; **and** the Go version removed from the cache fingerprint |
| `scripts/release/edge-readiness-preflight.mjs` | `ds-mcp` removed from `LIVE_CANARY_SERVICES` — the hosted service is gone, so no release can block on it; the component gate retargeted to `ds-cli` |
| `scripts/run/services.sh`, `scripts/run/preflight.sh`, `run-web.sh` | the `mcp` service kind (`go run`), the `ds-grid-mcp` prebuild, and the `DS_MCP_*` environment shaping all removed |

Every test that encoded the old architecture was examined rather than
weakened. Most invariants survived a retarget; two did not and were replaced
with the general rule they were special cases of:

- *"explicitly named ds-mcp without ds-network fails loudly"* → *"an
  explicitly named service whose repo is absent fails loudly"*. `ds-mcp` was
  the only service with a cross-repo dependency, so the specific case became
  unreachable.
- *"buildExpectedEdgeIdentities maps ds-mcp onto a live edge service"* → an
  assertion that **no** such identity exists, so a hosted agent service
  reappearing fails there rather than quietly regrowing a release gate.

## Deletion targets

Nothing here is deleted yet. Each row names what must be proved first.

### `ds-web` packaging

| Target | Where | Proof required first |
|---|---|---|
| `ds-mcp` sidecar build (Go) | `scripts/desktop/prepare-ds-mcp-sidecars.sh:150` | `ds` staged and verified in its place |
| `ds-grid-mcp` sidecar build | same file, `:229` | `ds` covers its last proven consumer |
| `externalBin` allowlist | `src-tauri/tauri.desktop-runtime.conf.json` — currently `["binaries/ds-report","binaries/ds-mcp","binaries/ds-grid-mcp"]` | end state `["binaries/ds-report","binaries/ds"]` |
| Bundle-policy assertion | `scripts/verify-desktop-bundle-policy.sh:56` pins that exact triple | rewritten in the same commit as the allowlist |
| Windows builder assertion | `desktop-build-windows.sh:513` pins the same triple | same |
| `ds-mcp` component entry | `desktop-components.json` — `class: core`, `delivery: bundled-native` | replaced by a `ds-cli` component with its own pin |
| Sidecar contract test | `scripts/desktop/test-prepare-ds-mcp-sidecars.sh` | rewritten for `ds`; it currently asserts `go build -mod=readonly -trimpath -buildvcs=true` and a `ds-grid-mcp` MCP `initialize` handshake |

`ds-grid-mcp` itself (`ds-network/crates/ds-grid-mcp`) has **no dependants** —
`grep -rn "ds-grid-mcp" --include=Cargo.toml` in `ds-network` returns only its
own manifest. Its only consumer is the packaging script above.

### Cloud

Smaller than expected, and worth stating precisely:

- **No Terraform creates a hosted `ds-mcp` Cloud Run service** in
  `ds-apis-tf`, `ds-deploy`, `ds-sre` or `ds-system`.
- The only hosted-`ds-mcp` infrastructure is one `roles/run.invoker` grant:
  `ds-apis-tf/edge_runtime_permissions.tf:63`, inside
  `ds_brain_runtime_invoked_services`.
- **But** `ds-web/scripts/release/edge-readiness-preflight.mjs` blocks releases
  on `ds-mcp`: it requires a live `ds-mcp-canary` revision (`:34`) and raises
  `edge-component-unreleased` when a release neither bundles `ds-mcp` nor pins
  a ready Windows artifact (`:661`).

**This preflight is the real blocker.** A migration that removes `ds-mcp`
without rewriting it will fail every release. It has to change in the same
slice that removes the component entry — not later.

`ds-sre` and `ds-system` have zero `ds-mcp` references; `ds-deploy` has no
`ds-mcp` service configuration at all.

## Next slices

Each is a vertical: a real command, over a real owner, with the deletion it
unlocks.

### Slice 2 — `ds desktop` reaches the bridge for real

Add the commands that use the paired session rather than only reporting it:
`ds desktop projects` and `ds desktop open --project <id>`, over the existing
`project.list` and `project.open` bridge operations.

*Proves:* the `desktop_user` authority path end to end — descriptor discovery,
pairing, a real semantic invocation, credentials never leaving the app.

*Unlocks deletion of:* `desktop_list_projects`, `desktop_open_project`,
`desktop_get_app_state` in `ds-mcp/internal/tools/`, plus
`ds-mcp/internal/desktop/bridge.go` once no other tool uses it.

*Requires:* a running DS GridDesign session — an operator proof, not a CI one.

### Slice 3 — `ds report` over `ds-report`

Wire the first typed process contract: `ds report transformer export`, calling
`ds-report export-transformer-report` with a typed request file.

*Proves:* the process-boundary route, including the "result file must not
already exist" idempotency rule, and `local_file_write` effect handling.

*Unlocks deletion of:* `network_report_export_transformer`,
`network_report_export_combined`, `network_report_export_compounded` from
`ds-mcp`.

*Note:* `ds-report` stays in `externalBin`. It is not being replaced.

### Slice 4 — packaging cutover

Stage `ds` as a sidecar; remove `ds-mcp` and `ds-grid-mcp` from the build,
the allowlist, the two policy assertions, and `desktop-components.json`;
rewrite `edge-readiness-preflight.mjs`; rewrite the sidecar contract test for a
Rust build with no Go step.

*Proves:* no deployed path builds Go.

*Unlocks deletion of:* the Go toolchain requirement, `ds-network/crates/ds-grid-mcp`
entirely, and the `roles/run.invoker` grant at
`ds-apis-tf/edge_runtime_permissions.tf:63`.

*This is the slice that must not be attempted before the preflight rewrite is
understood* — see above.

## Open questions for the operator

1. **`ds-grid` in the end state.** It is a 5 253-line argv surface with 45
   subcommands. `ds` can call it as a typed process boundary, or its
   capabilities can migrate into `ds` domains over time. The second is more
   work and is the better end state; the first is available immediately. No
   decision is needed to continue — slices 2 and 3 touch neither.
2. **Hosted `ds-mcp`.** There is no Terraform for it, but the release preflight
   demands a live canary revision. Was the service created out of band, and is
   it still serving anything?
