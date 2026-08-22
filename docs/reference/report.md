# `ds report` — reference

Tier-4 reference. `ds report <command> --help` is the contract.

## Why this domain calls a binary instead of linking a crate

`ds-network-reporter` publishes exactly one surface an agent host may call,
and wrote down why. From `src/bin/ds-report.rs`'s own header:

> one named subcommand per call — never a caller-supplied argv … a typed
> request file — not flags built from model output … a machine-readable
> result document — never parsed stdout prose.

That is a deliberate ownership boundary, not an accident of packaging. So
`ds report` builds a typed request, names one subcommand, and reads the
document that comes back. It links none of the reporter's library and
reimplements none of it.

Contrast `ds network`, which *links* `ds-grid-model` and `ds-grid-exchange`
directly — those are pure libraries with a clean boundary and no such
contract. Both routes are legitimate; which one applies is decided by the
owning workspace, not by convenience.

## Two engine rules that shape every command here

**The result file must not already exist, and there is no `--force`.** The
reporter refuses before doing any work. Its reason: a caller that finds a
stale document where its answer should be cannot tell the difference between
this run and the last one.

`ds` honours this rather than working around it. When you pass `--result`,
that path is yours — checked, used, and never removed by `ds`. When you do
not, `ds` writes to a scratch file it owns, reads it, and deletes it.

**A failed task still writes its document, then exits non-zero.** The blockers
are *in the file*; the exit status is only the coarse signal. This is right
for the engine — an exit code cannot carry a list of blockers — but it leaves
a direct caller holding a number and a path.

So `ds report export` reads the document in both outcomes:

| Engine outcome | `ds` result |
|---|---|
| exit 0, status `completed` | success; the document is `data` |
| exit 0, status `partial` | success; the document is `data`, blockers included |
| exit 1, document written | `export_blocked`, with `detail.blockers` |
| exit 1, no document | `engine_refused`, with `detail.engine` |

A caller never has to know the convention.

## Discovering the request contract

The engine publishes a full JSON Schema per task. That document is tens of
kilobytes, so `ds` tiers it the same way it tiers its own help:

```bash
ds report tasks                                    # the index
ds report tasks --task export_transformer_report   # one full schema
```

The schemas are never copied into this repository. They are read from the
engine installed on this machine, at the version actually installed, so they
cannot be stale.

## Flags versus `--request`

`ds report export` offers named flags for the common path *and* a
`--request <path>` passthrough for the engine's complete typed request. The
two are mutually exclusive — passing both is `conflicting_inputs`, because
silently ignoring one set would be worse than refusing.

The flag names are a hand copy of the engine's schema field names. A hand copy
nobody checks drifts silently, so `crates/ds/tests/engine_parity.rs` fetches
`ds-report task-schemas` from the installed engine and asserts:

- every **required** field of every task is reachable from a declared flag;
- every declared flag corresponds to a real engine request property.

The second direction matters as much as the first: a flag writing a field the
engine ignores looks like it worked.

One deliberate asymmetry: the engine's `transformer` (singular, one report)
and `transformers` (plural, combined) are both reached through a repeated
`--transformer`, so a caller does not have to know which task pluralizes.

## Finding the engine

| Order | Location | Why |
|---|---|---|
| 1 | `DS_REPORT_BIN` | explicit beats inferred, always |
| 2 | a sibling of the running `ds` | the deployed case — the desktop installs both into the same directory |
| 3 | `PATH` | for a developer who put one there |

`PATH` is last on purpose. If it were first, a stale binary earlier in
someone's `PATH` would outrank the one shipped alongside the application, and
the resulting wrong answer would look like a correct one.

Availability is resolved with filesystem metadata only — it never runs the
binary, because `ds doctor` and domain help both call it.

## Effect classification

`report.export` declares `local_file_write`, not `artifact_write`. This
matches `ds-mcp`'s existing classification of the same operation
(`network_report_export_transformer`), and is the reason it does **not**
require `--yes`: it writes into a directory the operator named, and publishes
nothing of record. `ds-mcp` reserves `artifact_write` for operations like
`network_report_detect_collisions`, which produce a durable artifact.

## Related

- `ds-network-reporter/src/bin/ds-report.rs` — the contract, in its own words
- [`../contracts/cli-output-contract.md`](../contracts/cli-output-contract.md)
