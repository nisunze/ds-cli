# The application kernel — Rust owns the application, TypeScript operates the screen

**Status:** owner ruling, 2026-09-10. This is the destination the codebase is
being moved toward, not a description of where it is. §6 measures the distance
honestly. The transport work completed on 2026-09-10 is the first finished
instance of the pattern, not the whole of it.

**Runtime and platform refinement, 2026-09-11:**
[Headless application runtime for web, desktop and server](ds-headless-application-runtime.md)
is the normative continuation for web/PWA, Linux, Windows and server. Native
Android/iOS ports are excluded from this product; Flutter remains the mobile
application. Survey, ds-cli and MCP remain supported.
It governs background execution, durable ownership, platform adapters and the
removal of frontend backend-client bypasses. Host effects do not imply permanent
TypeScript ownership of networking or persistence orchestration.

## 1. The ruling

> **Rust owns the product. WASM, CLI, server and Tauri are deployment forms.
> Svelte is a presentation client.**

Stated earlier the same day, and still the working formulation:

> Move application state, business logic, and domain behavior behind Rust/WASM
> so the product is headless by default. Svelte/TypeScript should mainly collect
> user intent, render state, and handle browser/map-specific effects.
>
> The intended end state is simple: **Rust owns the application; TypeScript
> operates the screen.**

### 1.1 What "deployment form" means, and what it forbids

A deployment form is a way the same product is shipped and reached. It is not a
place where the product is decided.

| Form | What it legitimately owns | What it must not own |
|---|---|---|
| **WASM** (browser, Web Worker) | loading the module, marshalling calls, browser IO adapters | any decision the CLI would answer differently |
| **CLI** (`ds`) | argument parsing, terminal rendering, exit codes, headless IO | the semantics of the command it parses |
| **Server** | request admission, transport, deployment topology | a second implementation of a domain rule the client also implements |
| **Tauri** | window, filesystem, native IO, process lifetime | product logic — the existing rule that the shell is an IO host, unchanged |
| **Svelte** | collecting intent, rendering state, DOM/CSS/MapLibre effects | truth of any kind |

The practical test for a deployment form: **removing it must remove a way to
reach the product, never a capability of the product.** If deleting the browser
would delete a rule, the rule was in the wrong place.

"Svelte is *a* presentation client" is deliberate. It is not the presentation
client, and nothing may be reachable only through it. That is the same property
§5 states for peer clients, from the other direction.

### 1.2 One thing this leaves open — flag, do not assume

"Server" as a deployment form sits uneasily with ds-brain, which is **Go** and
which today owns server-side authorization, lifecycle, publication and
data-authority checks. Two readings are available and they are very different:

- **Narrow:** "server" means the Rust services already deployed as servers
  (ds-network-reporter, ds-solar, ds-vector-tiler), and ds-brain remains the Go
  authorization boundary it is. Nothing in the current campaigns changes.
- **Wide:** ds-brain's domain decisions are also product, and should migrate
  behind the same Rust boundary over time, leaving Go as transport and admission.

**Answered by the owner, 2026-09-10:**

> "there are also logic that are in ds brain that can move to rust, absolutely.
> but no rush"

So the wide reading is accepted **in principle**: ds-brain holds domain logic
that belongs behind the same Rust boundary, and moving it is legitimate work.
Two qualifications keep it from becoming a licence:

- **"No rush" is part of the ruling.** ds-brain is one of the two components the
  owner calls relatively stable (with desktop compute), and Cloud Run is
  updated rarely. This is not a queue-jumping campaign; it happens when a domain
  is being worked anyway.
- **Domain logic is not the authorization boundary.** "Logic can move" was
  answered about domain rules. ds-brain's server-side authorization, lifecycle,
  publication and data-authority checks are a **security** boundary, and moving
  one of those still needs its own explicit call — the server must keep deciding
  what it decides today even when a client can compute the same answer.

Keep recording places where a domain rule is genuinely implemented twice, once
in Go and once in a client. Those are now a work list rather than evidence for a
pending decision, ordered by how much a disagreement would cost.

### 1.2a Ruled later the same day — the wide reading is the ruling

Owner, 2026-09-10, during the Design + Printing campaign:

> "One of the things that needed to be in ds-brain was simply the scary part
> of putting logic in TypeScript — so we can consider migration of
> ds-brain-bulk and other heavy hitters to the frontend WASM, so that ds-brain
> only grants permissions. This should be in this transitional state, slice by
> slice."
>
> "The WASM itself is almost like a container — I am confident in Rust
> scalability with coherence."

**What this settles.** ds-brain converges on **authentication, authorization
and entitlement** — who may do what, to which project, on which lane and
audience — and on fronting the storage it guards. Every other decision it
makes today is a migration candidate: domain rules, lifecycle and freshness
computations, publication composition, and the heavy compute that lives in
ds-brain-bulk. Those are decided in Rust and deployed as WASM in the browser,
as native code in `ds` and in the desktop shell, with ds-brain **granting the
permission and admitting the result**. The reason a rule sat in Go was never
that Go should decide it; it was that TypeScript could not be trusted to.
Rust can, and the WASM module is the container that makes one client-side
decision coherent across every deployment form.

**How it proceeds.** Inside the same transitional state, by the same method
(§6a), ordered by the cost of a disagreement between the Go answer and the
client answer, one slice at a time, each landing complete. A brain change
lands when its domain is being worked, never as a queue-jumper — Cloud Run is
still updated rarely, and "no rush" still holds for the schedule, not for the
direction.

**What it does not change.** ds-brain still decides whether a caller may read
or write. A client answer never bypasses the grant; a write is admitted under
the grant. Transport execution, storage and credentials remain host and server
effects (§3). Where a rule *is* the grant — a lock, a write block, an
entitlement — it stays in Go; where a rule merely computes what the grant is
then applied to, it moves.

## 2. The litmus test

> **If something should still make sense or work with the map closed, it
> probably belongs in Rust.**

Apply it to the thing itself, not to how it is currently reached. A validation
rule invoked from a dialog is still a validation rule; a selection expressed by
clicking is still a selection. Neither stops meaning anything when the map is
closed, so neither belongs to the screen.

## 3. The two lists

**Behind Rust/WASM:**

- canonical project/application state
- current selection and selected entity IDs
- active layer and filters
- visible/hidden domain objects
- workflow/editing state
- validation and domain rules
- commands and state transitions
- undo/redo semantics
- transformations and engineering calculations
- import/export behavior
- derived state and view models
- which features should be highlighted, warned about, or presented to the user

**Staying in TypeScript/Svelte:**

- pointer coordinates and mouse/keyboard events
- DOM focus
- dialogs and component composition
- CSS and animations
- MapLibre/WebGL rendering
- `setData()` and other synchronisation of rendered sources
- browser APIs and other presentation effects

The dividing line is not "UI vs backend". It is **intent and presentation vs
truth**. TypeScript collects intent and paints; Rust decides.

## 4. The anti-pattern: application truth recovered from the renderer

> Avoid patterns where application truth is recovered from the renderer, for
> example relying on `map.queryRenderedFeatures()` to determine what is
> selected. Rust should own the selection; MapLibre should only render its
> projection.

Rust must be able to answer **"what should currently be displayed?"** through
explicit view state. Svelte decides only *how* to display it. A renderer query
answers "what is painted at this pixel right now, at this zoom, after tile
simplification and label collision" — which is a different question, and a
lossy one.

### Measured today (2026-09-10, `ds-web` on `run`)

`map.queryRenderedFeatures` appears at **29 call sites across 13 files**. Not
all are wrong — asking "what is under this pointer?" is genuinely a renderer
question. Two are the anti-pattern exactly:

| Site | What it recovers from the renderer | Why it is wrong |
|---|---|---|
| `src/lib/styles/map-field-scan.ts` | a layer's **fields, their distinct values, their scalar types, and per-value counts** | These are properties of the data, not of the picture. Its own docstring explains the motive — the backend publishes fields without values and without the property *type*, and a MapLibre `match` silently falls through when a `"1"` label meets a numeric `1`. The answer is for the field domain to be owned and published, not scanned off the screen. What is scanned is also only what is *currently rendered*: a value outside the viewport, below minzoom, or dropped by tile simplification does not exist. |
| `src/lib/selection/lasso-run.ts` | **which features a lasso selected**, via `queryRenderedFeatures(bounds)` | Selection is application truth. This repository already has a Rust selection kernel and a mutable spatial index (`EditSessionIndex`) used by the edit session; the lasso path reaches for the renderer instead. Same viewport/simplification loss, and it cannot be reproduced headlessly. |

Neither can be answered by `ds` with no map open, which is the test.

## 4a. The invariant, and the authority to rewrite

Owner, 2026-09-10, after the anti-pattern sites were measured:

> **MapLibre is a renderer, never a source of application truth.** Anything that
> must be answerable by `ds` with no map open must be owned by the Rust kernel.
>
> For these affected subsystems, I would rather accept a deliberate rewrite from
> the proper Rust boundary than accumulate adapters around the old frontend
> architecture.

This is non-negotiable and it is not a diff-size trade. Explicitly authorised,
and explicitly forbidden:

| Do | Do not |
|---|---|
| Identify the correct canonical Rust ownership and API **first**, then implement it | Patch the anti-pattern in place |
| Delete the obsolete TypeScript authority | Keep it behind an adapter |
| Rewrite the subsystem from the proper boundary | Preserve the current architecture to minimise the diff |
| Preserve *behaviour* with tests | Preserve a bad internal design because the tests encode it |
| One Rust command/query, identical from WASM, CLI, Tauri and a headless server | A TypeScript mirror of Rust state, a compatibility patch, or a second source of truth |

**The acceptance test is one sentence:** can the same operation be performed and
answered correctly by `ds` with no browser, DOM, or map instance running?

### The audit, completed 2026-09-10

All 29 `queryRenderedFeatures` occurrences in `ds-web` classified by reading each
one. Several are comments about the API's behaviour rather than calls.

**Legitimately about the picture — kept:**

| Site | Question it asks |
|---|---|
| `map/draw.ts` (×5) | what is under this pointer / in this drag box, for vertex and move handles |
| `map/session-pick.ts` | its own docstring: "what's rendered under…" |
| `map/context-menu.ts` | what was right-clicked |
| `components/map/MapView.svelte` (×6) | every one a bbox around `e.point` or a hover point with a pixel tolerance — hover candidate, edit-session layer name, session-pick blocking, pin-polygon pick, grid-plan pick |
| `stores/map.ts` | a render **diagnostic**: "is this layer drawing anything at the current viewport?" — a question about the picture by construction |
| `components/map/AttributeTable.svelte` (×3) | the explicit **viewport-only** row filter, where "what is on screen" is exactly what the operator asked for |

MapView is worth a note because a survey suggested two of its sites were
misplaced: reading each one, they are hit tests. Its line-463 comment also shows
the correct pattern already in use — the renderer supplies the pick, and the
tile-clipped geometry is then *upgraded* from the stored geometry before
anything is decided on it.

**Application truth recovered from the renderer — rewritten:**

| Site | What it wrongly owns |
|---|---|
| `styles/map-field-scan.ts` | a layer's fields, distinct values, scalar types, per-value counts |
| `components/styles/StyleCenterDialog.svelte` | the **caller** that makes that scan a product behaviour — which values the Style Center offers an author depends on where they had panned |
| `selection/lasso-run.ts` | which features a lasso selected |

Three, not two: fixing the scan while leaving its call site would leave the
authority in place with a new implementation behind it, which §4a forbids.

### Auditing the renderer

Every `queryRenderedFeatures` site must be classified, not counted. Keep it only
where the question is *genuinely about the picture*: hit testing, pointer
interaction, what is currently visible on screen. Any use that determines domain
state, selection, metadata, fields, values, filtering semantics, or project
truth is redesigned around Rust.

The test that separates them: **off-screen features must exist exactly as
on-screen features do.** A renderer query cannot satisfy that — it cannot see
past the viewport, below minzoom, or through tile simplification — so any
question whose correct answer includes off-screen data is not a renderer
question, however it is currently implemented.

## 5. Peer clients, one kernel

The CLI, the Svelte web UI and the Tauri desktop are **peer clients of the same
Rust application kernel**. Commands are canonical: selecting structures,
validating assets, optimising routes, modifying a project, generating a report —
each must work identically without a browser, a map instance, a DOM, or a
running TypeScript application.

This is already the shape of the transport work: one kernel state machine, three
hosts that report facts and execute one returned action, and a contract test
proving the CLI selects the same operations the browser does. Extend that shape;
do not invent a second one.

Consequences worth stating, because they are the parts that get skipped:

- A command must be expressible without a pointer. If its inputs can only be
  produced by a gesture, the command is under-specified, not "map-only".
- A view model is a kernel output. "Which features should be highlighted,
  warned about, or presented" is a question the kernel answers; the component
  renders the answer.
- Undo/redo is state-transition semantics, so it is kernel-owned. A per-surface
  undo stack is two implementations of one idea.

## 6. Where the code actually is — measured, not estimated

`ds-web` on `run`, non-test TypeScript under `src/lib`, largest first:

| Directory | Files | Non-test lines | Test lines |
|---|---|---|---|
| `src/lib/design` | 122 | 26,421 | 660 |
| `src/lib/grid` | 94 | 22,806 | 7,263 |
| `src/lib/stores` | 56 | 21,805 | 83 |
| `src/lib/map` | 88 | 18,623 | 543 |
| `src/lib/project-work` | 60 | 13,413 | 5,812 |
| `src/lib/solar` | 47 | 10,452 | 5,034 |
| `src/lib/editing` | 11 | 2,686 | — |
| `src/lib/survey` | 16 | 2,419 | — |
| `src/lib/analysis` | 12 | 2,219 | — |
| `src/lib/layers` | 17 | 2,216 | 276 |
| `src/lib/selection` | 11 | 1,856 | — |

**Totals: 241,675 non-test lines in `src/lib`, plus 46,692 test lines there and
101,798 in `src/tests`.**

Not all of that is application logic — `src/lib/map` is largely legitimate
renderer glue, and `src/lib/components` is presentation. But `stores`,
`editing`, `selection`, `design`, `survey`, `analysis` and `layers` are
predominantly the truth the ruling assigns to Rust, and they alone are ~60k
lines. This is a programme across multiple campaigns, not a slice.

## 6a. The method — authority, not translation

> Do not translate TypeScript mechanically into Rust. Identify the application
> authority currently hiding in TypeScript, move that authority behind the
> Rust/WASM boundary, prove parity, then remove the duplicate path.

Four steps, in that order, and the order is the point.

**1. Identify the authority.** The unit is a *decision*, not a module. Ask "who
decides this, and where is that decision actually made?" — not "what would this
file look like in Rust". Authority is often nowhere near where the code lives:
in a store's derived value, in a Svelte `$effect`, in an argument-shaping
helper, or implicitly in the order two functions happen to be called. Those are
the ones worth finding, because reading file by file misses them.

**2. Move the authority — not the shape.** A Rust module that mirrors a
TypeScript file's decomposition has carried that file's accidental structure
across the boundary, and the accident is now the contract. Describe the Rust
seam on its own terms. Where the existing TypeScript decomposition is wrong,
fixing it is part of moving the authority, not scope creep.

**3. Prove parity, before deleting anything.** Name the proof: a golden-corpus
comparison, a differential test against the TypeScript on real inputs for one
release, a fixture set covering the edge cases the TypeScript handles. "Add
tests" is not a parity proof. This step is what makes step 4 safe, and it is the
step that gets skipped.

**4. Remove the duplicate path.** Then, and only then. Two live answers to one
question is the defect this whole document exists to remove — and a parity proof
that is kept alongside both implementations forever is just a slower version of
the same defect. §7a of the transport contract governs what goes: the module,
and the tests that only pinned it.

Rank work by **authority density, not line count**. A 200-line module that
decides what the user may do outranks a 4,000-line module that formats things.

## 7. How to migrate, and how not to

1. **Move the decision, delete the TypeScript.** Contract
   [`ds-transport-authority.md`](ds-transport-authority.md) §7a is the standing
   rule: a TypeScript module still holding domain logic is not a tracked
   exception, it is the work, and its tests are deleted with it. Do not preserve
   TypeScript business logic for convenience.
2. **Kernel modules take facts and return decisions.** One
   `evaluate(&[u8]) -> Result<String, String>` entry point, `deny_unknown_fields`
   request, serialisable reply, `wasm_bindgen` export — the shape every existing
   kernel module already uses. Pure: no IO, no clock of its own, no randomness;
   take `now` as an input.
3. **Prove it headlessly.** A migrated capability is finished when `ds` can
   exercise it with no map, and a contract test pins that the browser and the
   CLI select the same operation with the same inputs, outputs, errors and side
   effects.
4. **Never let a failure change the answer.** Placement is resolved once, before
   the work, from durable facts. A native failure does not fall back to a
   browser implementation of the same decision — that is two answers again.
5. **Do not move rendering, DOM, or browser IO into Rust,** and do not move
   ds-brain's authorization — the grant itself — into the client. Under §1.2a
   everything ds-brain decides *beyond* the grant (lifecycle and freshness
   computation, publication composition, domain validation, heavy compute) is
   a candidate, moved slice by slice; the server keeps deciding who may, and
   admits what the client decided.

## 8. Suggested order

Ordered by (truth density × how badly the current home hurts), not by size:

1. **Selection and view state.** Closes the named anti-pattern, and the
   selection kernel and spatial index already exist — this is consolidation, not
   invention.
2. **Field domains and derived layer state** (`map-field-scan.ts` and the
   layer/filter stores). Removes the second anti-pattern and stops the renderer
   being consulted about what data exists.
3. **Editing/workflow state and undo/redo** (`src/lib/editing`, the edit
   session). High truth density, already partly Rust-backed.
4. **Validation and domain rules** wherever they sit today, so the same refusal
   text reaches the GUI and `ds`.
5. **Derived state and view models** — the "what should be displayed" answer
   §4 requires.

The transport slice is done; treat it as the worked example of steps 1–4 of §7.
