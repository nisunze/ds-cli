# The product surface order — the kernel is driven through CLI and MCP; the UI reads

**Status:** apex ruling, owner, 2026-09-13. It sits at the same tier as
[`ds-application-runtime.md`](ds-application-runtime.md) and above
[`ds-application-kernel.md`](ds-application-kernel.md). Those two say *where the
product is decided* (Rust) and *how it runs* (one headless runtime). This one
says **the kernel is built first and driven through CLI and MCP**, and what the
UI is for while that happens. The kernel is not one surface among several; it
is the product, and the CLI is the form in which it is defined and proven.
Where an older contract treats the CLI, the web UI and the desktop as
interchangeable peers of the kernel, that remains true of the *kernel*; it is
no longer true of the *work*.

## 1. The ruling (verbatim; spelling normalised)

> Honestly — I have this realisation after, of course, too much deliberation and
> this stack building: **UI-driven software is the past** — a
> 10,000,000,000-percent bet — **and it slows me down.** Now it is **CLI- and
> MCP-driven**, and then the **UI reads**, for a human to access data, and
> **later** for human interactions (the **write path**). That is the direction I
> am strongly betting on right now. And you better put that in contracts.

Stated minutes later, the reason:

> You will see how much money it will save — simply too much. Because thinking
> visually to design a UI *while* reasoning about the backend is too much for an
> LLM. But if I drive the CLI + kernel first and then, later, the frontend,
> frontend guessing will be much, much cheaper in time and implementation speed.
> And it will serve me much faster in terms of server deployment.

And on how it is applied, and why the bet is safe:

> And no grand refactoring — whenever I need to do the work, I work, and move
> code in that direction. Wow — the desktop has given me too much power that
> the web could not give me.

And on how a command is shaped:

> And like Cloud Run — we always, always, always port project context unless
> global, as if the desktop were stateless. So the desktop and the Server are
> stateless: never, ever hold the state.

The reason is recorded with the ruling because it is the test for everything
below. **Cost and speed, through separation of reasoning.** A session that
must hold the domain, the kernel seam, the transport and a screen's visual
design at once pays for all of them on every step and gets each one worse.
Driving the kernel first, through the CLI, settles the decision with no picture
in the loop; the frontend then *projects* a decision that already exists — a
cheap guess against a fixed answer, not a co-design. And a decision that lives
in the kernel runs on the Server the day it lands — the Server is the desktop's
core running the same kernel headlessly — with no screen in the way. The UI is
not the product and it is not where the product is defined; building through
it is what has been slow and what has been expensive.

## 2. What it settles

**The kernel is the product, and it is built first.** Every capability is a
kernel decision before it is anything else: one entry point that takes facts
and returns a decision, pure, corpus-tested, exported to WASM, native in `ds`
and in the desktop shell, headless on the Server (kernel contract §7.2). The
kernel is where the work goes — the domain rule, the state transition, the
validation, the view model, the refusal. Nothing outside it decides.

**`ds` is how the kernel is driven and defined.** The CLI is the kernel's first
surface and its specification: a kernel capability exists when `ds` exposes
it — a command id in `ds capabilities`, a tier-3 contract in its `--help`, a
DS envelope for its answers and refusals. "The CLI is a definition of kernel
capability" (2026-09-12) is the standing formulation; this ruling makes it
the order of work. Driving the kernel through the CLI is what keeps the
picture out of the loop while the decision is being made.

**MCP is the same door, opened for an agent.** `ds mcp serve` is a shim over
the same descriptors (chapter → describe → invoke, same ids, same arguments,
same answers). A kernel capability is agent-reachable the moment it is
CLI-reachable; there is no separate MCP feature list, and the skills in
`ds-cli-skills` are how a capability is taught, not a second definition of it.

**The UI reads.** Its present obligation is to let a human see what the kernel
answers: projections, status, evidence, maps. It renders a kernel projection;
it does not originate. A screen must show nothing the kernel cannot answer
through `ds`.

**The UI write path comes later, and it comes second.** Human interactions
that mutate arrive by binding a screen to a kernel command that already
exists on `ds`. A screen never introduces a semantic; it forwards intent to
one.

**UI-driven development stops.** No capability begins as a screen, a store, a
dialog or a Svelte effect. A screen with no kernel decision behind it is not a
feature; it is the debt
[`ds-application-kernel.md`](ds-application-kernel.md) §6 measures.

## 3. The order

| Rank | Surface | Obligation | Done when |
|---|---|---|---|
| 0 | **Kernel** (`ds-command-kernel`) | owns the decision | one pure entry point, `deny_unknown_fields` request, serialisable reply, corpus, WASM export; identical from WASM, `ds`, Tauri and the Server |
| 1 | `ds` CLI | drives, defines and proves the kernel capability | `ds <domain> <cmd>` succeeds headlessly; descriptor in `ds capabilities`; contract in tier-3 help; refusals typed |
| 1 | MCP (`ds mcp serve`) | opens the same descriptor to an agent | visible through catalog → describe → invoke with identical ids, arguments and answers; skill updated |
| 2 | UI read | renders the kernel's projection | the screen shows what rank 0 answers and holds no state `ds` cannot answer |
| 3 | UI write (later) | collects intent, forwards it to rank 1 | the screen calls the kernel command that already exists; no new semantics, no second validation |

Ranks 0 and 1 are one unit of work: the kernel decision and the `ds` command
that defines it land together, and the kernel decision is not done until the
CLI proves it. Ranks are skipped only from the bottom: a capability may ship
with no screen, and its write path may wait indefinitely. Ranks 0–1 may never
be skipped. The table orders *new* work; §4a says existing code moves only
when it is touched.

## 4. Consequences

- **Sequencing per capability:** kernel decision + `ds` command + MCP → skill
  → UI read → UI write. A brief names the kernel decision and its `ds` command
  id first; the screen, if there is one, is a later section of the same brief,
  never a brief of its own.
- **Acceptance evidence:** a capability is proven by the kernel's corpus, `ds`
  output and an MCP invocation. A screenshot proves a read surface; it does not
  prove a capability. UI evidence stays where it lives today, for UI work only.
- **Discovery is the specification.** `ds capabilities` and tier-3 help are the
  definition of the product surface
  ([`ds-cli/docs/contracts/discovery-contract.md`](../../../../../docs/contracts/discovery-contract.md)),
  which is the 2026-09-12 ruling that the CLI is a definition of kernel
  capability, restated as the ordering rule.
- **Host transparency is unchanged.** CLI/MCP → Desktop or Server is one
  surface with no difference in ids, arguments or answers (`--target`;
  map-bound operations answer `needs_paired_map`).
- **Existing UI write paths** are neither extended as UI work nor patched in
  place. When their domain is worked, the write path is re-based on the `ds`
  command by the authority-not-translation method (kernel §6a), and the
  deciding TypeScript goes into the removal ledger
  (`ds-web/docs/typescript-removal-ledger.md`).
- **One brief, one layer.** The cost argument in §1 is enforced at the brief:
  a kernel brief carries no screen, no visual design and no Svelte file — its
  deliverable is the decision and the `ds` command that drives it; a UI brief
  starts from a kernel command that already answers, and its job is
  projection. An agent asked to reason about both at once is the expense the
  ruling removes.
- **Contract tests follow the order.** The existing bridge-parity and
  refusal-coverage suites already pin that the browser selects what the CLI
  selects. A new capability adds its CLI/MCP proof first; a UI test that
  exercises a path `ds` cannot reach is a finding, not coverage.

## 4a. How it proceeds — no grand refactoring

The ruling is a **direction of travel, not a migration campaign.** There is no
sweep of `ds-web` into the kernel, no "surface-order slice N", no worktree
whose purpose is to move code for this document's sake. The kernel contract's
§6 already measures the distance (~240k lines of TypeScript); this ruling does
not schedule closing it.

What it does instead: **whenever a domain is worked, the work goes in this
direction.** A defect, a new capability, a change an operator needs — each is
done kernel-first, driven through `ds`, and the TypeScript that used to decide
it is removed on the way (kernel §6a, four steps, in order). Code moves when
it is touched, and only as far as the work in hand requires. The same rule
already governs ds-brain ("no rush is part of the ruling", kernel §1.2) and
now governs every surface.

Two things this forbids, because they are how a direction turns into a
campaign: opening a domain nobody needs in order to migrate it, and leaving a
touched domain half-moved with an adapter holding the old answer alive. Move
what the work touches, completely; leave the rest alone.

## 4b. Why the bet holds — the desktop

The owner's observation is recorded because it is the evidence for the ruling,
not a mood. The desktop — the Tauri shell, native `ds`, the Server as the
desktop's core — gave the product powers the web never could: a real
filesystem, native engines, long-lived processes, offline-first state, PLS-CADD
and QGIS on the same machine, a headless twin of every operation. Those powers
belong to the kernel running natively; none of them came from a screen. The
web was a ceiling; the kernel driven through `ds` is not. That is why the order
is kernel → CLI/MCP → UI, and why the UI can afford to be a reader for now.

## 4c. Project context is carried, never held — the Cloud Run rule

Every command carries its project context explicitly, the way a Cloud Run
request carries everything it needs, **unless the operation is global**
(identity, doctor, capabilities, workstation, feedback — anything that is not
about one project). **The desktop and the Server are stateless hosts: they
never hold the state.** No process-wide "current project", no selection
captured at start-up, no id a host remembers between two calls and supplies to
the second one. What a host does not hold it cannot leak, cannot get wrong
after a switch, and cannot make two concurrent projects fight over.

"State" here is **request context** — which project, which lane, which
instance a command is about. It is not the durable local data the desktop's
power comes from: the sync store, queues, artifacts and cached datasets stay
exactly where the runtime and sync contracts put them. Those are keyed *by*
the carried project; they are never the source of it.

This is the rule the Server already implements
([`ds-execution-context.md`](ds-execution-context.md) §1: sessions keyed by
(principal, project), every route names its project) — now stated for every
host and every surface:

| Surface | How the project arrives |
|---|---|
| `ds` CLI | required `--project <p>` on every project-scoped command; no saved selection supplies it. A sealed input may name its project instead. |
| MCP | the project is an argument of the invoked descriptor, every call |
| Kernel | takes the project as an input fact; owns no "current project"; `execution_context::admit` decides *under which project* from the carried bytes, never from a host |
| Server | keyed by (principal, project) per request; no project at `serve` start |
| Desktop shell (Tauri) | forwards the project the caller named; a window's **open** project is a rendering fact (rank 2), not a command's context — the kernel compares the carried project to it (`desktop_project_not_open`) and never substitutes it |
| UI | renders the project it was given; when its write path arrives, it names the project on every forwarded intent |

Consequences: a command that answers differently depending on what a host
"has open" is under-specified — add the argument. A refusal for a wrong or
missing project is typed and names the remedy (`--project`, or the switch
command), never a silent fallback to whatever the host last saw. And the
kernel corpus for a project-scoped decision includes the project in every
request fixture — there is no fixture where the project is implied.

## 5. What it does not change

- **Rust owns the product.** The CLI is a deployment form of the kernel, not a
  new home for logic; `crates/ds` still owns argument parsing, terminal
  rendering and exit codes only (kernel §1.1). This ruling does not promote
  the CLI above the kernel; it says the kernel is *driven through* the CLI and
  nothing else while it is being built.
- **Thin kernel, engines in their crates.** Decisions in the kernel; heavy
  compute stays in ds-network, ds-solar, the reporter and the Cloud Run
  services (`ds-kernel-network-boundary.md`, 2026-09-12). Kernel-first does
  not mean engine-into-kernel.
- **ds-brain reduces to grants; engines stay in their crates and Cloud Run
  services** (kernel §1.2a; `ds-kernel-network-boundary.md`).
- **The runtime charter** (`ds-application-runtime.md`) — web/PWA, Linux,
  Windows and server as targets, durable queues, verified handoff — is
  untouched. The Server is the desktop's core; the UI on top of it reads.
- **DS Field** (`simple_notes`, Flutter) is the field-capture application under
  the runtime charter's §0 and is not re-ranked by this document.

## 6. Flagged, not assumed

- **Existing UI write paths — frozen, or maintained?** Assumed: maintained for
  defects, not extended; re-based on `ds` when their domain is worked. If the
  owner means frozen outright, that is a stricter reading and should be ruled.
- **The map as a read surface.** Assumed yes: MapLibre renders a kernel
  projection (kernel §4a); gesture-originated writes are rank-3 work and wait
  for their `ds` command like any other.

## 7. The acceptance test, one sentence

A capability is done when the kernel decides it and an agent can drive it
through `ds` or MCP with no screen open; a screen is done when it shows nothing
the kernel cannot answer and does nothing the kernel cannot do.
