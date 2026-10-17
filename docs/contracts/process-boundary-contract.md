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

Local `solar.project` and `solar.reference` controls link the engine-free
`ds-solar-project` owner directly. They do not discover or launch the retired
`ds-solar` application executable; native application calculation remains with
`solar.application` and its separate prepared-input engine.

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
cannot deadlock the caller. Governed Solar capture now calls the linked
engine-free intake owner instead of launching a child. Its snapshot contains
short-lived download authority and remains in bounded private memory until the
owner strips that authority and seals the intake; no intermediate file exists.

`External::capture` resolves an admitted executable to its canonical version
path without starting a process. The Reporter host captures it once per batch
or individual export and uses that handle for build identity, execution and
final identity verification. Switching an active symlink affects subsequent
runs only. Version directories must stay immutable and retained while in use;
capturing a path does not admit an in-place executable replacement or implement
the signed package updater. Existing identity fences still refuse changed identities.

Lane-aware Reporter captures retain their exact stable/canary lane, including
explicit development Reporter overrides. Every captured Reporter launch sets
the fixed child-only `DS_REPORTER_NETWORK_LANE`; raw and pinned captures clear
it so stale ambient state cannot choose a lane. Other owners retain their
existing launch environment. Reporter validates this handoff only when its
lazy report-scoped Network capture is consumed, then uses the existing
receipt-verified lane capture and retains it across report consumers and
updates. Invalid lanes refuse without fallback; schema/template-only paths
acquire no Network. Missing handoff preserves existing standalone/cloud capture.

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
catalog. An explicit development override remains available as a deliberate
route. Missing managed selection or a missing/corrupt selected executable refuses
before any sibling/PATH lookup or spawn; lane capture never substitutes an
unmanaged engine. The separately named development `capture` route retains its
established sibling/PATH lookup, and `capture_pinned` only reopens the retained
path. Standalone Reporter `engine`, `tasks`, `export`, `bundle`,
`voltage-drop-combined`, `mv-frontmatter` and `spatial workbook` now declare
`--lane` (Stable by default) and capture the verified managed selection without
account/project reads. `plan-profile-config` reuses its existing authorized
project lane and one handle for every variant. Identity/path and schema/task
pairs retain that handle across pointer changes; discovery performs no probe.
Missing selection returns `reporter_engine_missing`, and invalid receipts or
selected bytes return `module_receipt_invalid`; repair the selected lane without
fallback. Local layout delivery and non-preview plan/profile rendering pass one
managed handle into their fixed process call after existing local/project
validation. Retained A4 Refresh holds that same renderer through schema admission,
release build-info before/after and rendering; its auth fences, staging,
sealing/publication and exact cloud readback remain unchanged. Preview paths keep
their existing managed project owner. The design project's voltage-drop print
run passes its captured renderer through governed A4 admission and local export;
obsolete zero-argument preflight wrappers are removed.

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


## Retained Linux Profile creator lifetime

The fixed `network_profile_session` owner retains a private Linux launch thread
until its exact child/tree has been terminated and the child reaped. Linux
[`PR_SET_PDEATHSIG`](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html) follows the creating thread, so a temporary caller or Tokio
blocking-pool thread may return without cancelling the retained session. The
parent-death signal, unique process group, bounded pipes/deadlines and native
identity fences remain enforced. Constructor failure, abandoned handoff,
Close, cancellation and Drop reap before releasing that private creator. The
Windows suspended-spawn/Job Object path remains its existing owner and creates
no additional launch thread.

Previously a returned blocking-pool caller could kill the retained Linux
worker while Core remained alive, making a later valid request refuse. A
qualified Core artifact containing this process-owner repair is required;
Desktop and engine artifacts retain their independent versions and identities.
Abrupt whole-parent death still kills the direct engine. Group cleanup covers
its admitted descendants during normal owner termination; this does not claim
that Linux parent-death signals propagate to arbitrary fork descendants.
