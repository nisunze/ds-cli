# Worktree lifecycle

The owner's registry is `/home/magese/data-solutions/git_trees/gittrees-index.json`.
It tracks physical trees, including recovery work, rather than presenting local
commits as delivered features. The normal cap is three workers plus one main
integration tree. Exceeding the cap is reported explicitly.

Use `python3 scripts/worktree-index.py`, whose updates hold one file lock and
replace the JSON atomically. Never overwrite the index from a stale copy.

- Main runs `scan --main` at turn boundaries and after every landing. Unknown
  ownership remains visible until assigned. The protected `github-exit` mirror
  is recorded without inspection.
- At start, each worker uses `record --lane NAME --owner /root/WORKER --purpose
  TEXT --state working --next-action TEXT`. During gates use `gating`. Record
  exact commits as repeated `--commit REPO=SHA`, and receipts with `--receipt`.
- Before ending, a worker records `ready_for_integration` with exact committed
  source and repeated passing `--gate RECEIPT` evidence. Main independently
  checks the receipts and source vector. Ready means a handoff, not delivery.
- Main consumes every ready handoff: record `integrating`, gate the final
  source, push only through `push-train.sh`, prove every lane commit is on
  `origin/run`, preserve evidence, and immediately remove the consumed tree.
  Then record `consumed --main --receipt PROOF` with the landed commits. The
  helper refuses consumption while the path still exists or commits are absent
  from `run` and `origin/run`. The immutable proof includes refreshed remote
  heads, source preservation and host process checks. This removes the index
  entry immediately. Deleted trees have no registry history; commits retain
  source history.
- If a cache or tree is needed for more work, main records `followup --main`
  with the named next owner and concrete next action. It is still live and
  counts against the cap. A blocked or unfinished tree must have its reason
  and named next action; it never disappears silently.

`report` (also the default) gives active work, recovery count, physical count
and cap. `summary` returns the same groups as JSON. The index has one compact
line per live tree, only the latest handoff, and no event log, consumed entries
or file inventories. Evidence directories and the protected mirror are counted
separately. The registry does not
authorize cleaning another worker's tree, discarding unlanded commits, bypassing
the security guard, or deleting evidence.
