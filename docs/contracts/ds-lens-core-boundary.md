# Lens and core — the boundary between `map.*` and every other command

Status: contract drafted 2026-09-18. Awaiting owner ratification; implementation
plan in §5. Extends `ds-surface-order.md` (§4c: project context is carried,
never held) and the 09-11 host-transparency ruling.

**Scope fence.** This contract is about the *surface separation* — which commands
exist on the Server, which exist only against a window, and how a caller tells.
Transformer versioning (the audit under
`outputs/transformer-versioning-audit-20260918/`) is a **separate lane, already
in Codex's hands**; it is cited below only as the example of a crossing. Nothing
in this plan edits version code, and nothing in the versioning lane is gated on
this plan.

## 1. The problem, stated plainly

The desktop is the Server plus a window. So `ds` has two kinds of command:

- **Core commands** (`design.*`, `dsgrid.*`, `assets.*`, `survey.*`, …) are kernel
  decisions. They name their project, act on governed state, and answer the same
  on the Server, the desktop, the CLI and MCP. The Server is stateless: no open
  project, no selection, no window.
- **Lens commands** (`map.*`) act on a *paired window*. The window is stateful by
  nature — open project, camera, selection, panels. A lens command changes what
  that window shows.

The boundary between them is wide (61 `map.*` commands; 171 desktop-bound
non-`map.*` ids at 09-12) and nothing enforces it. Example of a crossing, from
the versioning audit: `map.design.version.begin` — a lens command — mints a
`local-<digest>` identity that no core surface holds. One crossing, and the
word *version* means two things. (Its fix belongs to the versioning lane.)

## 2. Where the confusion comes from

1. **Same vocabulary, two lanes.** `version`, `open`, `save`, `discard` exist in
   both `design.*` and `map.design.*`. Nothing in the id says which one holds
   truth.
2. **Same file hosts both.** The desktop dispatchers (`ds-web/src/lib/desktop/
   cli-map.ts`, `cli-design.ts`, `cli-assets.ts`, `cli-dsgrid.ts`) call
   presentation stores *and* `$lib/api/*`. A crossing is one import line.
3. **Whose project?** A core command carries `--project`. A lens command does not;
   it borrows the window's open project. The same instruction is context-free in
   one lane and context-bound in the other.
4. **Two catalogues.** CLI/MCP invoke by descriptor (`ds capabilities`). The UI
   invokes the gateway by `brainOperation(operation_id, payload)` — a second,
   hand-rolled registry with its own error shapes. "What can the UI do?" and
   "what can `ds` do?" have different answers.
5. **A third set that should not exist.** Commands that are desktop-only but not
   `map.*` — core work that has not reached the Server. Today they sit in the
   same catalogue with no marker but `paired_availability`.

## 3. Definitions

- **Governed state** — anything with an identity or a fence: a save
  (`metadata.version`), a version (`vN`), a model head/revision, an attachment,
  a tag, a comment, a selection, a lease. It lives in ds-brain (or a core
  offline workspace whose only exit is a core command).
- **Presentation state** — what a window shows: open project (as a rendering
  fact), camera, renderer, open panel, current selection, layer visibility and
  order, scratch/local layers, captured PNGs. Disposable: closing the window may
  lose it and nothing is lost.
- **Lens command** — any `map.*` command. Reads or changes presentation state of
  one paired window.
- **Core command** — every other command. Decides over governed state and names
  its project.
- **Foreign application control** — launching a third-party application and
  driving edits inside it. Not a `ds` command at all: it belongs to that
  application's own MCP surface. The clearest case is PLS-CADD (owner,
  2026-09-18): *running* PLS-CADD and editing in it is a third-party MCP's job,
  while `ds pls …` — which reads and writes PLS-CADD **files** through typed
  tasks, `Authority::None`, no window and no network — is ordinary core.

  This is not the forbidden third set. That one is core work stranded behind
  our own window. This is the boundary of `ds` itself: **`ds` acts on data and
  on our own surfaces; controlling somebody else's application is a foreign
  surface's job.** So a future "drive PLS-CADD from an agent" is neither a
  `map.*` command nor a bridge operation — the lens layer is our window's
  crate, never a general remote-control layer.

## 4. The contract (each rule is testable)

**L0 — Desktop = Server + window. Two sets, nested, nothing else.**
Every command is in exactly one set:
- **core** — available on the Server, and on the desktop *identically* (same id,
  inputs, answers, refusals);
- **lens** — available on the desktop only, because it needs the window.

There is no third set. A desktop-only command that is not `map.*` is a boundary
violation: core work that has not reached the Server (the host-transparency
backlog). The boundary is therefore an **availability** test, not an id-prefix
test: `paired_availability` ⇔ `map.*`; everything else ⇒ `headless_project` or
`none`.

**L0a — The boundary is physical: two crate layers, and the bridge is the fence.**
A rule a test checks can be broken and unfrozen. A rule cargo checks cannot be
broken at all, because the code that would break it does not compile. So the
`ds` workspace has two layers:

| Layer | Crates | May depend on `ds-cli-desktop` (the pairing bridge) |
|---|---|---|
| **core** | every domain crate (`ds-cli-design`, `ds-cli-assets`, `ds-cli-dsgrid`, `ds-cli-work`, `ds-cli-style`, `ds-cli-survey`, `ds-cli-solar`, `ds-cli-tile`, `ds-cli-data`, `ds-cli-library`, `ds-cli-sre`, `ds-cli-feedback`, `ds-cli-auth`, …) | **no** |
| **lens** | `ds-cli-map` (and any future lens crate) | yes — it *is* the window's crate |
| **host** | `ds` (the one binary), `ds-cli-mcp` | both, because the desktop is Server + window |

`ds-cli-desktop` is the only door to the paired window: the descriptor, the
pairing secret, `paired_availability`, every `desktop_*` refusal. A core crate
that cannot name that crate cannot open a window, cannot invent a
`desktop_*` refusal, and cannot grow a second host-shaped code path — not
because a reviewer noticed, but because the symbol does not exist.

**Measured 2026-09-18, before any of this lands:** the bridge is a dependency of
**15 crates** and is used in **~39 source files** across `ds-cli-assets` (10),
`ds-cli-map` (4), `ds-cli-data` (4), `ds-cli-feedback` (4), `ds-cli-survey` (4),
`ds-cli-tile` (3), `ds-cli-style` (2), `ds-cli-solar` (2) and seven more with one
each. `paired_availability` appears in 12 crates — `ds-cli-map` (38 files) but
also `ds-cli-design` (24), `ds-cli-work` (10), `ds-cli-assets` (10),
`ds-cli-style` (9). That is the entanglement: today any domain can reach the
window, and nothing but attention stops it.

The target is **2 dependents** (`ds-cli-map`, `ds`). Each crate that leaves the
list is one host-transparency batch finished, and the list is the campaign's
progress bar.

**L0b — The end state is two CLIs, installable apart, on different machines.**
The crate layers are not a tidy graph for its own sake; they are what makes two
distributables possible, and that is the destination (owner, 2026-09-18):

| | **core CLI** | **lens CLI** |
|---|---|---|
| What it is | the surface that *receives* a command and decides it | the surface that *calls*, and drives one window |
| State | **none.** No open project, no selection, no window. Every project-scoped command names its project | the window's open project, camera, selection — a rendering fact it may assume |
| Installed | server, container, CI, laptop, anywhere | beside a desktop, which may be a different machine from the server |
| Catalogue | core commands. `map.*` does not exist here | `map.*`. It reaches core commands by *calling*, not by containing them |
| Links | contract + domain crates + the Server host | the door, the map crate, and the core surface as a client |

One calls, the other receives. The seam already exists as `--target
server|desktop` and as `ds-cli-server`'s routes — today both ends are linked
into one binary, and the split is what makes the near end optional. The
dependency direction is already right: `ds-cli-map` depends on `ds-cli-server`
(the lens calls the core), and no core crate depends on the map. The one
inversion is the door, which is what the fence is emptying.

Two consequences worth stating, because they are what an operator and an agent
will feel:

- **`ds capabilities` answers for the install.** On a server it lists core
  commands and never a `map.*` the machine cannot run. On a lens install it
  lists `map.*` and whatever core surface it can reach. An agent's question
  "what can I do here" stops needing a footnote.
- **`ds mcp serve` exists on both**, exposing that install's catalogue. The MCP
  surface stays a projection of the live CLI, so it splits for free.

Sequencing: the cargo feature (`--no-default-features` → a core-only `ds`) is
the cheap first form and proves the graph is really severed; the second binary
is the delivered form. Neither can happen while thirteen core crates link the
door, so the fence's inventory is the gate on both.

**L1 — A lens command has a presentation effect only.**
`effect ∈ {read_only, local_ui, artifact_write}`, and `artifact_write` only for
disposable local artefacts (PNG evidence, scratch layer payloads). Never
`global_write`, never `local_auth_state`, never a write to an offline workspace
or the sync store.

**L2 — A lens command never mints or returns a governed identity.**
No `version_id`, `vN`, `revision`, `attachment`, `selection`, `tag`, `comment`,
`lease` in a lens output unless it echoes an identity the caller passed in.

**L3 — A lens command carries its project.**
Every `map.*` descriptor takes `project`. The application compares it with the
window's open project and refuses `project_context_mismatch` on disagreement.
The open project is evidence, never a default (`ds-surface-order.md` §4c).

**L4 — Dispatchers import presentation only.**
`ds-web/src/lib/desktop/cli-*.ts` may import map/UI stores and kernel bindings;
never `$lib/api/*`, `$lib/reconciliation/*`, `$lib/sync/*`.

**L5 — A gesture needing a governed effect composes core-then-lens.**
The core command runs first, headless, with `--project`; the lens then re-reads
and renders. The lens never holds the outcome.

**L6 — One catalogue.**
A UI write invokes `command id + inputs` through the kernel (WASM on web, native
under Tauri) and receives the DS envelope and refusal codes the CLI receives. No
second registry. Migration is per domain when the domain is worked (§4a of the
surface-order ruling — no sweep).

**L7 — No intent without a carrier.**
A sync/reconciliation intent kind may exist only if a runtime route delivers it.
(The `DesignVersion` instance is the versioning lane's to remove; the rule is
this contract's.)

## 5. Implementation plan — one brief = one layer, land as you prove

Gates on every slice: `cargo test --locked` (kernel, ds-cli), `npm run check` +
boundary tests (ds-web), catalogue test (ds-cli), `[skip ci]`, pins re-pointed
after each kernel/ds-web merge. Every allowlist below is a ratchet: committed
with the exact ids found, and only ever shrinks.

| Slice | Layer | Work | Proof |
|---|---|---|---|
| **0 — Measure the boundary** | ds-cli | One test over the whole catalogue: (a) **L0** — every `paired_availability` id is `map.*` and every `map.*` is `paired_availability`; desktop-only non-`map.*` ids frozen as the host-transparency backlog; (b) **L1/L2** — effect and output vocabulary over `map.*`; (c) **L3** — lens commands lacking a `project` input. Three allowlists. | Test red on any new violation. The three lists are the first honest count of the boundary (expected: (a) ≈ the 171 minus what has moved; (b) the version twin, to be classified with `map.design.open` (lease?) and `map.design.discard`). |
| **0a — The crate fence** | ds-cli | **L0a**: one test in `ds-cli-contract` parses every `crates/*/Cargo.toml` and fails when a crate outside the lens+host layer names `ds-cli-desktop`. Seeded with today's 15 dependents; the list only shrinks. No code moves in this slice — it makes the fence visible and irreversible first. | The allowlist, committed, is the entanglement's exact size; a new core crate reaching for the bridge is red on the first `cargo test`. |
| **0b — First crates out of the fence** | ds-cli | Take the cheapest dependents (one bridge file each: `ds-cli-auth`, `ds-cli-library`, `ds-cli-sre`, `ds-cli-work`, `ds-cli-design`, `ds-cli-dsgrid`) and, per crate, either move the desktop-bound command to `ds-cli-map` as a lens command or give it its headless core implementation. Then **delete the `ds-cli-desktop` dependency line** so the crate can never grow another. | Allowlist shrinks by one per crate; `cargo build -p <crate>` proves the bridge is unreachable from it. `ds capabilities` shows the moved ids where L0 says they belong. |
| **1 — Dispatcher import boundary** | ds-web | Add **L4** to `src/tests/frontend-boundary.test.ts` (existing ratchet pattern). Freeze current violations per dispatcher file. The ds-web mirror of 0a: TypeScript has no crate graph, so the test is the fence. | Test red on a new `$lib/api` import in a dispatcher; frozen count only shrinks. |
| **2 — Project carried in the lens** | ds-cli, ds-web | Add `project` to every `map.*` descriptor (default = saved selection, sent explicitly, as `ds-cli-server` already does). Bridge compares with the open project; new refusal `project_context_mismatch` with remedy. | Corpus test: open project A, call with `--project B` → refusal; with A → proceeds. Slice-0 list (c) reaches zero. |
| **3 — One catalogue for UI writes** | kernel, ds-web | Kernel exposes `invoke(command_id, inputs)` for web/Tauri returning the DS envelope. `brainOperation` callers migrate domain by domain *when that domain is worked* — never a sweep. Removals ledgered in `ds-web/docs/typescript-removal-ledger.md`. | Per migrated domain: `grep brainOperation` count drops; a UI write refused by the kernel shows the CLI's refusal code. |
| **4 — Third set to zero** | ds-cli, kernel | The host-transparency campaign (`~/ds-fable/isolation-prep`, A 12 / B 118 / C 41) becomes the consumer of slice-0 list (a) and the 0a allowlist: each moved command shrinks one, each emptied crate shrinks the other. Ends with **2 bridge dependents** (`ds-cli-map`, `ds`). Optionally then: put the lens layer behind a cargo feature so the headless package does not link it. | List (a) empty and the 0a allowlist at 2 ⇒ L0 and L0a hold by construction, not by review. |
| **5 — Intents with carriers only** | kernel, ds-web | Reconciliation/sync intent kinds reduced to those a runtime route delivers; Sync Center counts only drainable rows as active. | Corpus test: an intent kind without a route is refused at `enqueue`. |

| **6 — Two CLIs** | ds-cli | **L0b.** First the cargo feature: `ds` builds with `--no-default-features` and the lens layer is absent — proof the graph is severed, and the headless/Server package stops shipping window code. Then the second binary: a lens distributable installable beside a desktop, on a different machine from the server, calling the core surface through the seam `--target` already uses. | A core-only `ds` builds, runs `ds capabilities` with no `map.*` in it, and passes the workspace suite; `ds mcp serve` on each install projects that install's catalogue. |

Slices 0, 0a and 1 are small, independent, and can start now — none of them
moves a command, they only make the boundary visible and irreversible. 0b is
the first real motion and is sized per crate, so it can be one short brief at a
time. Slice 3 is direction (per worked domain). Slice 4 is the existing
campaign, re-measured against the two lists. Slice 5 waits for the versioning
lane to remove its one undeliverable kind, then closes the door behind it.
Slice 6 is the destination and is gated on the fence reaching two dependents;
nothing else in this plan is gated on it.

### Landed so far

| Date | Slice | What |
|---|---|---|
| 2026-09-18 | 0a | `ds-cli/crates/ds/tests/lens_core_boundary.rs` — the fence, seeded at 15 dependents (ds-cli `6c8078a`) |
| 2026-09-18 | 0b | `integer`/`INVALID_NUMBER`/`plural` moved to `ds-cli-contract::args`, so the door dependency means only "has window-bound commands"; **`ds-cli-survey` freed** (its `BRIDGE_OPS` had been empty since its control plane moved to the kernel); `ds-cli-tile`'s ceiling 3 → 2. Thirteen core crates left (ds-cli `1e5d195`) |
| 2026-09-18 | 0b | **`ds-cli-library` freed.** Its thirteen governed global-catalog operations gained the headless owner they always should have had: `ds-client-core::grid_catalog` (closed action enum, per-action body keys, bounded governed objects) plus `grid_models::ForkExample`. The artifact upload and the prepared-directory publication the desktop used to compose are performed in the CLI. The desktop's thirteen bridge operations and its adapter are deleted. Twelve left (kernel `6883594`, ds-cli `a4430f4`, ds-web `6ca38d96`) |
| 2026-09-18 | 0b | **`ds-cli-feedback` freed.** It had two routes to one backlog; the native one was already the default, so the paired one and its prose-marker refusal recovery are gone, with `--target`, `--desktop-descriptor`, the three BridgeOps and the desktop adapter. Eleven left (ds-cli `c0d712c`, ds-web `7c9817c2`) |

## 6. Owner decisions needed

1. Refusal code for L3: `project_context_mismatch` (sibling of the existing
   `auth_context_mismatch`) — or reuse `project_context_stale`?
2. Slice 3's first domain (proposal: whichever domain the next brief touches;
   never chosen for this contract's sake).
3. Whether `map.design.open` (opens an edit context — does it take a server
   lease?) is a lens command with an L5 composition inside, or a core command
   misfiled under `map.*`. Slice 0 will surface the fact; the ruling is yours.
4. Slice 6's names and install layout: what the lens binary is called, whether
   the desktop installs two binaries or one that links both, and what a lens
   install without a reachable core answers.

## 7. Non-goals

- No renaming of the `map.*` domain; the id prefix *is* the lens marker.
- No web bridge: lens commands stay `paired_availability`; the web UI reaches the
  core through L6, not through a browser driver.
- No sweep of `brainOperation` callers; L6 migrates per worked domain.
- **No version work.** The version twin, offline capture, `DesignVersion`
  intent and the audit's P1–P3 items are the versioning lane's (Codex).
