# Process boundary contract — `ds-cli-exec/v1`

**Status:** active contract.

`ds` reaches a domain owner one of two ways. This document governs the second.

## When to link, when to call

| Route | When | Example |
|---|---|---|
| **Link the crate** | the owner is a pure library with a clean boundary | `ds dsgrid` links `ds-grid-model`, `ds-grid-exchange` — the same crates `ds-web/src-tauri` links |
| **Call the binary** | the owning workspace deliberately chose process separation and wrote down why | `ds report` → `ds-report`; `ds solar` → `ds-solar` |

The choice belongs to the owning workspace, not to convenience. Three typed
process contracts exist today, each designed as one:

| Binary | Owner | Shape |
|---|---|---|
| `ds-grid` | `ds-network/crates/ds-grid-cli` | one named subcommand per call |
| `ds-report` | `ds-network-reporter/src/bin/ds-report.rs` | typed request file; machine-readable result file; the result must not already exist |
| `ds-solar` | `ds-solar/apps/ds-solar-cli` | `prepare` may reach the network, `run` may not |

## There is no generic invoker

`ds-cli-exec` exposes `External::call(subcommand, args, timeout)` where
`subcommand` is `&'static str`. It comes from a `ds` command's own source and
can never come from a caller.

There is deliberately **no** `run(binary, argv)`. A subcommand that no `ds`
command names is not reachable from `ds`, and that unreachability is the
property the owners' contracts exist to preserve. `ds-report` states it
plainly: *"never a caller-supplied argv."*

Adding a generic argv route — or accepting a caller-supplied subcommand
string — is a rejectable change.

## Locating a sibling executable

```
1. $DS_<NAME>_BIN     explicit beats inferred, always
2. a sibling of the running `ds`
3. $PATH
```

**`PATH` is last on purpose.** If it were first, a stale binary earlier in
someone's `PATH` would outrank the one shipped alongside the application, and
the resulting wrong answer would look like a correct one.

**An override that does not resolve is a failure, not a fallback.** If
`DS_REPORT_BIN` names a path that is not a file, `ds` refuses rather than
quietly using a different binary than the operator named.

The sibling rule is what makes the deployed case configuration-free: the
Linux `.deb` installs `ds` and `ds-report` into the same directory, so an
installed `ds` finds an installed `ds-report` with no environment at all.

## Availability must not execute

`External::availability()` resolves with filesystem metadata only. `ds doctor`
and every domain's help call it, and a discovery call that spawns processes is
one nobody can afford to make.

## Bounds

| Bound | Value | Why |
|---|---|---|
| Output | 4 MiB per stream | beyond this the result belongs in a file the callee already wrote |
| Timeout | per command, declared | a `callee_timed_out` refusal names the bound it exceeded |

Both pipes are drained on their own threads. A callee that fills one while
`ds` waits on the other deadlocks — and `ds-report task-schemas` is already
tens of kilobytes.

Output past the bound is still **drained**, so the callee is never blocked on
a full pipe; the bytes are simply not kept, and `truncated` says so.

## Mapping a callee's failure

A non-zero exit becomes `engine_refused`, carrying the callee's own message —
bounded to six lines of at most 200 characters each — in `detail.engine`.

The engine's words go in `detail`, never in `message`. They are useful to a
human and are not something a caller should match on; `error.code` is the
stable thing.

**Where a callee writes a result document even on failure, read it.** That is
the whole value `ds` adds over a direct call: `ds report export` returns the
engine's typed blockers instead of an exit code and a path. See
[`../reference/report.md`](../reference/report.md).

## Hand-copied schemas must be checked

Where a `ds` command translates its own flags into an owner's typed request,
the field names are a hand copy. An unchecked hand copy drifts silently — the
flag keeps working, the field it writes stops being the one the engine reads.

So a contract test fetches the schema from the **installed** engine and
asserts both directions:

- every required field of every task is reachable from a declared flag;
- every declared flag corresponds to a real engine request property.

The second direction matters as much as the first: a flag writing a field the
engine ignores looks like it worked.

See `crates/ds/tests/engine_parity.rs`. This is the same discipline `ds-mcp`
applies to its own hand-authored schemas.

## Every refusal code must be documented

`crates/ds/tests/refusal_coverage.rs` reads the domain crates' source,
collects every literal error code they can construct, and requires each to
appear in some command of that domain's declared refusals.

It is source analysis because most of these codes are reached only in
situations a test cannot reliably produce — a full disk, a killed engine, a
corrupted package. A code that is genuinely unreachable by a caller goes in
`NOT_A_REFUSAL` with the reason; that list is the escape hatch and is
deliberately short.
