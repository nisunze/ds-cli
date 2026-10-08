# Continue Profile differential compute — fresh-window handover, 2026-10-08

Paste this whole file into the next window. Continue the existing work; do not
start from zero. The owner requested this handover because the prior
conversation had become corrupt.

Read `ds-command-kernel/backlog/OPEN.md` §14, "Differential compute after an edit
(the item to watch, owner 2026-10-08)", **in full before touching code**, including
"Current unfinished continuation, 2026-10-08". It holds the ordered owner
steers, original measurements, code map, exact current evidence and next work.
Read `backlog/PRINCIPLES.md` §6 and the two owning contracts:

- `ds-network/docs/contracts/profile-transactional-database.md` (§0 leads).
- `ds-network/docs/contracts/alignment-terrain-partitions.md`.

The goal: each Profile edit lands whole, reconciling at most its affected
alignment(s), solving only affected tension sections, retaining unchanged
terrain and every unrelated alignment. The result equals a full calculation
byte for byte in the held axis. The next edit costs only what it touches.

The server/kernel is the core shared by Web, Desktop and CLI. Much of the code
exists; prove its wiring. Use the local Linux Desktop dev build for **real
retypes, pole-handle drags, undo and another edit**, and watch the sections heal.
Keep per-phase timings and actual work counters so human/assistant co-design
is predictable and easy. After spotting an alignment, correcting a few poles
must not repeat the whole alignment's work. Optimum spotting/typing must not
repeat unchanged terrain, other alignments or their typing. Do not repeat the
recorded baseline measurements instead of fixing the boundary.

Current source is **unfinished and uncommitted**, directly in canonical
`ds-network/run` (HEAD `7ec4d9cb590f89957fc7d166ec1d1314f0aa217e` at handover):

1. `crates/ds-grid-engine/src/profile_edit_update.rs`: builder-order row merge,
   including interleaved slot sections and rows moving across unchanged anchors.
   Two release unit tests passed.
2. `crates/ds-grid-wasm/tests/profile_edit_lifecycle_probe.rs`: now asserts
   conductor byte parity. One Nyamagabe drag probe passed. Spans now match a
   full build; the remaining reconcile differs in 13 engineering markers and
   28 terrain clearance margins (115,528-byte delta). **Whole-scene parity is
   still false; no sweep or installed-source acceptance has been completed.**
3. `crates/ds-grid-engine/src/profile_atlas.rs`: last, **uncompiled** scaffold
   `layout_scene_in(..., held_axis)` can place supplied bands at retained chain
   positions. No scoped caller exists yet; ordinary layout passes `None`.
   Complete it through retained surface/reference/case inputs and exact
   per-band assembly/folds; never claim this already localizes reconciliation.

Preserve these edits. Backup:
`ds-network/out/profile-differential/in-flight.patch`
(SHA-256 `93fd05b1a966959e789294688e8502d417d9edcac4084d42933e9b548b93cbfa`).
All new receipts/screenshots are in that gitignored output folder.
The older parked retype work remains in
`ds-network/out/parked-numbering-retype-20261008-1119/`; do not delete it.

The **older installed** Linux Desktop dev build is running on display `:10`.
Its model is `local-8e19c052cf574797`, Nyamagabe MV Model 1 test copy. A real
drag and same-family retype of support 127 (`str-423815a32fed4309-274`) healed
their wires/markers, with unchanged camera values. The retype receipt reported
`profile_publication_failed` during observation while a later view showed the
head fully healed: the duplicate-observation/publication race is unresolved.
Preview/commit/observe/total were 30/731/4315/5076 ms. See the receipts before
changing fences or classifying this as a solver failure.

At handover the RAM and computed head were
`rev:9287c4a95405b6c3159933756e2b41d2103bb0337d6c4cf1dadc5f9f1c999502`,
undo depth 4: **two owner edits plus this session's drag and retype remain
unsaved**. Preserve RAM work before restarting. Re-read the live head and
history; do not use these pins for another write. `desktop-handover.json`
captures the exact state. No matching Desktop rebuild has installed our source.

`ds` is not on PATH. Read the ds skill, then use installed discovery through:

```bash
ds-web/run-linux-desktop.sh --cli capabilities --search 'profile' --output json
ds-web/run-linux-desktop.sh --cli map profile view --output json
```

`DISPLAY=:10 ~/.cache/ds-xdrive/xd` captures/clicks/drags the real Desktop.
The successful drag was `(1115,898)` → `(1135,898)` after selecting support 127;
positions may change, so inspect first. The retype target was
`j-w-S325.014` / `st-02148991a07337d1` from `j-w-S190.012`.

Native proof uses `CARGO_BUILD_JOBS=2`, release cache
`/home/magese/.cache/ds-network-probe-target`, and disk scratch
`ds-network/out/profile-differential/tmp`. The existing lifecycle probe reads
the package named in OPEN.md through `DS_LIFECYCLE_PACKAGE`; run with
`--release --ignored --nocapture`. The passed logs are `ordering-unit.log` and
`ordering-nyamagabe.log`; they predate the layout scaffold.

Claude/Codex are concurrently finishing Solar sync on canonical `run`, including
`ds-command-kernel/crates/ds-client-core/`; preserve their changes, commits and
processes. The owner specifically expects them. The Solar continuation tree
was safely deleted after all seven heads were proved on `run` and `origin/run`.
Registry: 0 live trees against cap 4; zero unlanded commits. Cleanup left at
least 25% disk free, no process was killed, and this session has no remaining
heavy job. Check live processes/RAM before each heavy job; preserve warm caches.

Work on canonical `run`, no new worktrees or official goal tool. Keep each
slice coherent; prove natively with Nyamagabe parity/work counts and then in
the matching Linux Desktop, commit and push before the next slice. Preserve
native history/publication/signed-zero fences and never undo the whole-landing
lifecycle. Keep unfinished work only in the owning backlogs, delete fulfilled
obligations, and remove this prompt once its OPEN.md entry is gone. Build
locally; never submit a remote build or deploy `ds-system`.
