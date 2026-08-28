# Cutover history — `ds-mcp` → `ds`

**This is history, not a contract.** It is a closed ledger of what the Go
`ds-mcp` → `ds` cutover actually changed, kept because the reasoning behind two
replaced test invariants is worth more than the commits that carried them.
Nothing here is pending, nothing here describes today's surface, and no rule in
it governs a change you are making now. For that, read
[`../contracts/process-boundary-contract.md`](../contracts/process-boundary-contract.md),
the reference document for the domain you are touching, and the live
`ds <domain> <command> --help`.

The counts below are the counts **at cutover**. They were never meant to track
the CLI, and they do not.

## `ds-cli` at cutover

Six domains and nineteen domain commands, plus three root metadata commands.
`solar` and `report` reached their owners through the typed process contracts
those workspaces had published, which is still how they reach them.

## `ds-web` — Go removed from the shipping path

| File | Change |
|---|---|
| `scripts/desktop/prepare-ds-mcp-sidecars.sh` | deleted; replaced by `prepare-ds-cli-sidecar.sh` |
| `scripts/desktop/test-prepare-ds-mcp-sidecars.sh` | deleted; replaced by `test-prepare-ds-cli-sidecar.sh` |
| `desktop-build-linux.sh` | Go toolchain check removed; `ds-mcp` sibling requirement → `ds-cli`; builds the `ds` sidecar |
| `src-tauri/tauri.desktop-runtime.conf.json` | `externalBin` → `["binaries/ds-report","binaries/ds"]` |
| `scripts/verify-desktop-bundle-policy.sh` | allowlist assertion retargeted; **and** the three package-content checks made exact — a substring test for `ds` would have matched every `ds-griddesign` path and proved nothing |
| `desktop-build-windows.sh` | the same allowlist assertion, for platform parity |
| `desktop-components.json` | component `ds-mcp` → `ds-cli`, still `core` / `bundled-native` |
| `src-tauri/src/component_manager.rs` | core-id set and its tests |
| `src/lib/desktop/components.ts` | the TypeScript mirror of that boundary |
| `scripts/build-desktop.mjs`, `desktop-release-lib.sh`, `scripts/desktop/build-cache.sh` | release metadata field `mcp_source_sha` → `cli_source_sha`, same position in the composite hash; **and** the Go version removed from the cache fingerprint |
| `scripts/release/edge-readiness-preflight.mjs` | `ds-mcp` removed from `LIVE_CANARY_SERVICES` — the hosted service was gone, so no release could block on it; the component gate retargeted to `ds-cli` |
| `scripts/run/services.sh`, `scripts/run/preflight.sh`, `run-web.sh` | the `mcp` service kind (`go run`), the `ds-grid-mcp` prebuild, and the `DS_MCP_*` environment shaping all removed |

Every test that encoded the old architecture was examined rather than weakened.
Most invariants survived a retarget; two did not, and were replaced with the
general rule they had been special cases of:

- *"explicitly named ds-mcp without ds-network fails loudly"* → *"an explicitly
  named service whose repo is absent fails loudly"*. `ds-mcp` was the only
  service with a cross-repo dependency, so the specific case became
  unreachable.
- *"buildExpectedEdgeIdentities maps ds-mcp onto a live edge service"* → an
  assertion that **no** such identity exists, so a hosted agent service
  reappearing fails there rather than quietly regrowing a release gate.
