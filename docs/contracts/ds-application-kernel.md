# The application kernel — Rust owns the application, TypeScript operates the screen

**Status:** owner ruling, 2026-09-10. This is the destination the codebase is
being moved toward, not a description of where it is. §6 measures the distance
honestly. The transport work completed on 2026-09-10 is the first finished
instance of the pattern, not the whole of it.

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

**This document does not choose.** No campaign should act on the wide reading
without an explicit owner decision, because it would move a security boundary,
and §7 rule 5 says plainly that Rust owning the *application* means the client
application. Until that decision exists, treat ds-brain as unchanged and record
any place where a domain rule is genuinely implemented twice — once in Go and
once in a client — as a finding for that decision to consider.

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
5. **Do not move rendering, DOM, or browser IO into Rust,** and do not read this
   as permission to move ds-brain's server-side authorization, lifecycle,
   publication or data-authority checks into the client. Rust owning the
   application means the *client application*; the server still decides what it
   decided before.

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
