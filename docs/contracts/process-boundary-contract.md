# Process boundary contract — `ds-cli-exec/v1`

**Status:** active contract.

`ds` reaches a domain owner one of two ways. This document governs the second.

## When to link, when to call

| Route | When | Example |
|---|---|---|
| **Link the crate** | the owner is a pure library with a clean boundary | `ds dsgrid` links `ds-grid-model`, `ds-grid-exchange` — the same crates `ds-web/src-tauri` links |
| **Call the binary** | the owning workspace deliberately chose process separation and wrote down why | `ds report` → `reporter`; prepared Solar calculation → `solar` |

The choice belongs to the owning workspace, not to convenience. Five typed
process contracts exist today, each designed as one:

| Binary | Owner | Shape |
|---|---|---|
| `ds-network` | `ds-network/crates/ds-network-engine` | fixed identity probe and closed LV processing request; immutable complete result plus typed identity/digest receipt |
| `ds-grid` | `ds-network/crates/ds-grid-cli` | one named subcommand per call |
| `reporter` | `ds-network-reporter/src/bin/reporter.rs` | typed request file; machine-readable result file; the result must not already exist |
| `solar` | `ds-solar/apps/solar` | fixed build identity and sealed prepared-calculation file task; no acquisition |
| `ds-solar` | `ds-solar/apps/ds-solar-cli` | `prepare` may reach the network, `run` may not |

`solar` is the independent engine addon. `DS_SOLAR_ENGINE_BIN` selects it for
development. The application tool `ds-solar` and its `DS_SOLAR_BIN` override
remain separate while native batch and portfolio orchestration are extracted;
the addon inventory does not claim those workflows are already detached.

## There is no generic invoker

The rule, stated as what is actually enforced:

**No caller-supplied argv reaches any owner. Every spawn site passes a
statically known argument list, and the set of files permitted to spawn a
process at all is pinned.**

`ds-cli-exec` is the boundary for the typed sibling contracts. It exposes
`External::call(subcommand, args, timeout)` where `subcommand` is
`&'static str`: it comes from a `ds` command's own source and can never come
from a caller. A subcommand that no `ds` command names is therefore not
reachable from `ds`, and that unreachability is the property the owners'
contracts exist to preserve. `ds-report` states it plainly: *"never a
caller-supplied argv."*

For an owner contract whose typed document must not be persisted before the
owner seals it, `External::call_with_stdin` keeps the same static subcommand and
closed argv rules and accepts one caller-owned byte slice bounded to 32 MiB.
It drains output and writes stdin concurrently, so a child that stops reading
cannot deadlock the caller. The one current use is the fixed `ds-solar intake
--snapshot - --out <fresh-file>` handoff: its snapshot contains short-lived
download authority and must never cross a temporary-file boundary.

`External::capture` resolves an admitted executable to its canonical version
path without starting a process. The Reporter host captures it once per batch
or individual export and uses that handle for build identity, execution and
final identity verification. Switching an active symlink affects subsequent
runs only. Version directories must stay immutable and retained while in use;
capturing a path does not admit an in-place executable replacement or implement
the signed package updater. Existing identity fences still refuse changed identities.

Native-account Reporter project exports, captured layout/voltage-drop previews
and MV previews resolve the lane of their already captured native identity with
`capture_for_lane` before entering the report host. One captured handle serves
reuse identity checks, rendering and completion identity checks. Discovery does
not probe an addon or assume an input lane. Desktop Reporter task construction
uses the compiled shell lane; its staged combined sessions and before/after
identity checks remain separate lifecycle guards. Standalone `solar engine` and
`solar calculate-prepared` declare the native `--lane` (stable by default),
capture its verified managed selection once and retain that handle through the
identity/task pair. These local commands read no account/project or release
catalog. Explicit development override and the existing sibling/PATH fallback
remain available when no managed selection exists; a corrupt managed receipt
refuses before spawning. Raw local-file Reporter commands retain their explicit
unmanaged boundary until separately migrated; inventory/selection alone does not
prove those consumers use a managed receipt.

`ds-cli-exec` is not, however, the only place a process is created — there are
named owner classes, and each is audited where it lives:

| Owner class | Where | What it may spawn |
|---|---|---|
| Native Assets PDF text decoder | `ds-command-kernel/crates/ds-assets-pdf-host` (linked by `ds-cli-assets`) | fixed `/usr/bin/prlimit` executing only `/usr/bin/pdfinfo` or `/usr/bin/pdftotext`, over a private local copy of governed signed-read bytes; validated numeric pages, fixed resource limits, no executable/argv/environment/URL/password input |
| A sibling DS executable | `crates/ds-cli-exec` | `reporter`, `solar`, `ds-solar`, under one named subcommand |
| Detached Network host client | `ds-command-kernel/crates/ds-network-client` through `ds-cli-exec` supervision | fixed `build-info` and `lv-process` operations; typed local request and validated artifacts, without algorithms linked into this consumer |
| The platform package manager | `crates/ds-cli-workstation/src/install.rs` | one fixed manager with a `const` package identity |
| An executable already on this machine, probed | `crates/ds-cli-workstation/src/detect.rs`, `verify.rs` | bounded version probes and the harmless verification smoke test |
| This same `ds`, and the installed desktop | `crates/ds-cli-mcp/src/tools.rs` | one `ds <path> … --output json` per `tools/call`; for a live-descriptor command that requires desktop authority and has no named descriptor, one fixed no-argument DS GridDesign launch |
| Installed document tools | `crates/ds-cli-server/src/solar_documents.rs` | fixed Pandoc Markdown-to-DOCX and LibreOffice DOCX-to-PDF conversions, using private resources verified by the captured Solar owner; no caller-supplied executable or argv |
| The PLS-CADD desktop drivers | `crates/ds-cli-pls/src/desktop/run.rs` | Windows PowerShell 5.1, found under `%SystemRoot%` and never on `PATH`, on one `ds-desktop-*.ps1` entry of the embedded driver bundle; the bundle is extracted into a private folder and every file checked against its pinned sha256 before the run; literal switches, a closed `Entry` enum, static parameter names, and values the verb validated (paths, a digest, a label, numbers), each quoted by one tested function |

Every one of those sites builds its arguments from a literal array in its own
source. None accepts an argv, a subcommand string, or a shell fragment from a
caller, and the MCP adapter refuses any argument key the live descriptor does
not declare before it maps a single element. Its desktop launch has no
arguments at all and is available only to an MCP *invoke* whose live descriptor
authority is `desktop_pairing`, `desktop_user`, or legacy `project`; catalogue,
describe paths, every `authority: none` command, and the `headless_user` and
`headless_project` native auth authorities never observe or launch a
desktop.

`crates/ds/tests/process_boundary.rs` pins that inventory: it asserts the exact
set of non-test files permitted to construct a process. A fifth spawn site
fails the suite until it is added deliberately, with its owner class named.

A generic `run(binary, argv)` reachable from a command — or a route that
accepts a caller-supplied subcommand string — remains a rejectable change. The
list above is a closed set of audited owners, not permission to grow one.

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

The sibling rule makes a colocated addon configuration-free: `ds` finds
`reporter` or `solar` beside itself without an environment override. Missing
addons return a named installation remedy; they do not require rebuilding the
shell.

Addon interface qualification can run against an existing Linux debug build
without recompilation:

```sh
python3 scripts/test-module-interface.py --ds target/debug/ds --scratch out/<run>/tmp
```

The scratch directory must already exist on disk. This checks actual CLI/MCP
dispatch, zero discovery spawns, one explicit identity probe per addon, missing
override refusal, and Kernel catalog admission without engine execution.

Qualify the actual detached LV processing consumer with captured native artifacts:

```sh
python3 scripts/test-network-lv-process.py --ds /absolute/path/to/ds \
  --engine /absolute/path/to/captured-network-insertion \
  --fixtures ../ds-network/crates/ds-network/tests/fixtures/native-engine-boundary/v1 \
  --trace-helper ../ds-command-kernel/scripts/native_host_network_trace.py \
  --evidence /absolute/disk/path/to/new-evidence
```

This executes CLI/MCP parity, both historical byte goldens, the canonical schema,
missing/incompatible/corrupt/cancelled candidates and zero-network checks including
a detector control. Native process unit/domain tests marked as requiring an
explicit engine must also run with `DS_NETWORK_ENGINE_BIN` and `--ignored` during
this qualification; an ordinary host-only test run does not prove execution.

## Availability must not execute

`External::availability()` resolves with filesystem metadata only. `ds doctor`
and every domain's help call it, and a discovery call that spawns processes is
one nobody can afford to make.

## Bounds

| Bound | Value | Why |
|---|---|---|
| Output | 4 MiB per stream | beyond this the result belongs in a file the callee already wrote |
| Typed stdin | 32 MiB | the largest governed owner intake envelope; larger input refuses before spawn |
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

See `crates/ds/tests/engine_parity.rs`. The retired `ds-mcp` applied the same
discipline to its own hand-authored schemas, and that is where the practice
came from; the enforcement a change is measured against now is the suite in
this repository.

## Every refusal code must be documented

`crates/ds/tests/refusal_coverage.rs` reads the domain crates' source,
collects every literal error code they can construct, and requires each to
appear in some command of that domain's declared refusals.

It is source analysis because most of these codes are reached only in
situations a test cannot reliably produce — a full disk, a killed engine, a
corrupted package. A code that is genuinely unreachable by a caller goes in
`NOT_A_REFUSAL` with the reason; that list is the escape hatch and is
deliberately short.

The fixed Solar background sender may re-invoke the current `ds` executable
only as `solar project sync --workspace <absolute path> --project <validated project id> --lane <stable|canary>
--watch --yes --output json`. `ds-cli-exec::start_solar_project_sync` owns this
closed process boundary after the initiating command's governed-write gate.
It accepts no arbitrary executable, subcommand, URL or argument vector.
