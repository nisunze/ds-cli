# Next coding session: differential compute after a Profile edit

Paste this whole file into the next session.

Work on the Profile's differential compute. Everything you need is in
`ds-command-kernel/backlog/OPEN.md`, §14, entry "Differential compute after an
edit (the item to watch, owner 2026-10-08)". Read that entry in full before
touching code: it holds the owner's steers in order and why, the code map, the
numbers measured on Nyamagabe MV Model 1, the traps already hit and the
slice-by-slice path. Do not start from zero and do not re-measure what it
already measured.

The goal in one line: after any edit the Profile lands whole (it already does)
and reconciles at most the edited structure's own alignment — its affected
tension sections re-solved, no terrain work unless terrain changed, never all
alignments — equal byte for byte to a full calculation; and the next action
costs only what it touches.

- Ruling: `ds-command-kernel/backlog/PRINCIPLES.md` §6 (an edit lands whole,
  2026-10-08).
- Contracts: `ds-network/docs/contracts/profile-transactional-database.md`
  (§0 leads) and `ds-network/docs/contracts/alignment-terrain-partitions.md`.
- Measurement: `ds-network/crates/ds-grid-wasm/tests/profile_edit_lifecycle_probe.rs`.
- Parked retype work: `ds-network/out/parked-numbering-retype-20261008-1119/`.

Work directly on canonical `run`. Prove each slice natively (parity sweep on
Nyamagabe MV Model 1) and in the Linux Desktop, then commit and push it. Delete
this file when the OPEN.md entry is gone.
