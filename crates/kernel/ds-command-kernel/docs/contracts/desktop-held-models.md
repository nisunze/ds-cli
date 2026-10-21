# Desktop held model inventory and artifact capture

`ds map profile view --include-held` consumes every existing worker in the
paired Desktop's live registry. Ordinary Profile view consumes no workers.
This inventory is separate from the native machine working-copy catalogue and
the saved browser-local catalogue: neither catalogue proves live RAM residency.

One `sessionPresentationJson` supplies each worker's Rust-authored manifest
identity, revision, package ordinal, CRS and complete native history. The host
transports its local model ID, name, persistence, ownership, project provenance
and retained activity facts. Account/project generation, registry membership,
worker identity and dispatched-write generation must remain current throughout
the read. Concurrent consumers share only pending observations; there is no
polling, model opening, acquisition, calculation, checkpoint or network read.
Missing methods or crossed contexts refuse the whole inventory. A partial
listing cannot be presented as complete.

The CLI admits the returned closed inventory through `grid_held_models` after
the existing Desktop bridge's account/project/window fences. Counts, unique
local IDs, native revision/history equality and nonmutation are mandatory.
The host may label a native head persisted only when its saved host revision
matches that head. The native Profile Runtime's derived view cache is not an
exhaustive list of browser-authored workers.

`ds dsgrid profile open --live --held --model <id> --expect-history <native-json>
--checkpoint-out <new-absolute.dsgrid>` consumes one existing worker directly.
`--held` requires `--live`; ordinary package opening and ordinary live Profile
capture retain their existing precedence. Capture uses the named worker's own
operation queue and `checkpointDsgridAtHistory`. It never takes occupancy,
opens a missing source, changes Profile focus, writes the original source,
performs Save or publishes a project version. The output must be new and is
never overwritten. The CLI admits exact model/history/output/digest/length and
nonmutation facts in the receipt.

The `.dsgrid` artifact preserves the current authored state and package assets.
Its receipt witnesses the captured full history; the artifact does **not**
restore the live Undo/Redo journal on cold reopening. Receipts explicitly carry
`source_saved:false`, `focus_changed:false` and `undo_redo_preserved:false`.
Capturing current state does not prove exact RAM-session restoration. Keep the
live pair until operational acceptance resolves that boundary.

The additive flags reuse the existing `map.profile.view` and
`dsgrid.profile.open` bridge operations. No new native allowlist operation,
MCP tool, WASM binding or model schema is introduced.
