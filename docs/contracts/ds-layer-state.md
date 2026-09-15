# The layer domain — one owner for the drawer's truth and its wire

**Status:** implemented and landed on the layer-domain branches, 2026-09-11.
Parent: [The application kernel](ds-application-kernel.md) §2 (the litmus
test), §4a (the renderer is never a source of truth), §6a (the method).
Refines [Headless application runtime](ds-headless-application-runtime.md) §2
for the layer drawer.

## 1. The decisions this settles

Everything the layer drawer used to decide in TypeScript is now decided by
two kernel modules and answered identically by `ds`, the desktop shell and
the browser WASM:

| Question | Owner | Hosts do |
|---|---|---|
| Which runtime layers form one canonical layer's family, and which role each plays (`primary`, `label`, `boundary`, `d3`, `companion`) | `layer_state` op `classify` / `families` | render; pass the ids they hold |
| What mounts, each layer's desired and runtime visibility and why, which preferences are recorded; session toggles vs persisted preferences on a project switch | `layer_state` op `plan` | add/reconcile MapLibre layers with the returned `runtime`, persist the returned `preferences` |
| How a set of layers reads (count / any / all) and what a parent toggle writes | `layer_state` ops `scope`, `fold`, `merge` | paint the checkbox |
| The preferences after a toggle, an exact set, "hide others" and its restore, and the layout writes each implies; whether a snapshot still applies | `layer_state` ops `toggle`, `set`, `isolate`, `restore`, `isolation` | set the store, persist, `setLayoutProperty` |
| The layout word each named runtime layer gets right now, forced on when loaded explicitly | `layer_state` op `runtime` | `setLayoutProperty` on repopulate |
| The canonical catalogue (families, roles, folded visibility, source state, zoom range) and the matrix (by layer, by source, provenance) | `layer_state` ops `catalog`, `matrix` | render rows |
| Whether an order request is acceptable: unknown, duplicate or unbounded ids, and what a partial order leaves unlisted | `layer_state` op `reorder` | send only an accepted request |
| Starting visibility below each user's remembered choice | `metadata.default_visible`, folded by `layer_state` | store the project default through `set_default_visibility`; never rewrite user preferences |
| The request body per layer action, the response bound, and every rule an answer must satisfy (envelope, shape, sources, parents, matrix rows, sidebar groups, dedicated sections) | `layer_document` ops `request`, `admit`; `ds-client-core::layers` delegates | perform the declared operation `domains.layers.action` from the generated manifest; hand the parsed answer back |

Schemas: `ds.layer-state/v1`, `ds.layer-document/v1`. WASM exports
`layerState`, `layerDocument`. Native: `ds map layer list|default|show|hide|reorder`.

## 2. Rules worth stating

- **Preferences** map a runtime layer id to the user's choice. A layer
  without an entry inherits its family's remembered choice (the last recorded
  member in document order), then the governed project default, then the
  authored default. After a mount plan
  every primary is recorded; a companion is recorded once its family has a
  remembered choice. Foreign ids (temporary and analysis layers toggled
  through the same store) keep their own entry and read as visible when
  absent.
- **Authored `visibility: none` is a hard hide**; a remembered `true` still
  yields runtime `none`, with `authored_hidden` named. A `__d3` companion
  renders only in the 3D renderer with shapes enabled (`needs_3d_shapes`).
- **Zoom is reported, never applied.** MapLibre clips by min/max zoom;
  `in_zoom_range` is an answer for the drawer and for `ds --zoom`.
- **A missing source is a fact**, `source_state: missing`: the layer is not
  mounted and not recorded; the document itself is refused at admission if
  the backend emitted it that way.
- **The project fence.** Every question names the project it is about; a
  document for another project is refused with `project_context_changed`,
  so a stale response after a project switch cannot be applied.
- **Isolation** snapshots the managed set; restore applies only when the
  document's layer set is unchanged, otherwise the snapshot is discarded
  without writes (`stale_snapshot`).
- **Order admission** is a decision, not a transport error: `unknown_layer`
  (runtime ids are never order keys), `duplicate_layer`, `invalid_order`,
  or `accepted` with `complete` and `unlisted`.
- **Admission refusals are typed**: `backend_refused` carries the backend's
  words and status, `contract_violation` names the issues (first six, then a
  count), `unreadable_response` is the shape rule.

## 3. Parity proof

`tests/fixtures/layer-state-corpus.json` (byte-identical to ds-web's
`src/tests/fixtures/layer-state/corpus.json`): 28 recorded cases over one
assembler-shaped document — what the ds-web TypeScript answered
(`expected_ts`, recorded by a differential recorder over the real modules
before their removal) and what the kernel answers (`expected`).
`tests/layer_state_corpus.rs` holds the kernel to the TypeScript at every
field except three declared, exercised divergences; ds-web's
`layer-state-parity.test.ts` replays every request through the vendored
WASM. The divergences, decided for the kernel:

- **D1** an unrecorded companion follows its family's memory then its
  authored visibility (the TypeScript read any unrecorded id as visible, so a
  hidden family with an authored-hidden label reported "some visible");
- **D2** the first toggle of an unrecorded layer hides it (the TypeScript
  flipped the raw entry and "toggled" a visible layer to visible);
- **D3** a renderer failure does not drop a recorded preference.

## 4. What was removed from TypeScript

`src/lib/layers/{visibility,metadata,matrix}.ts`; in `stores/map.ts` the
mount plan, the family memory, `runtimeLayerVisibilityForDesired`,
`styleAuthoredLayerVisibility`, `toggleLayerVisibility`; the deciding halves
of `helpers.ts` and `isolation.ts`; the desktop bridge's own catalogue and
order checks; in `api/layers.ts` the route literal, the request bodies,
`layerConfigError` and the contract checks (`assertLayerConfigContract` and
its helpers). Tests that only pinned them went with them.

## 5. What TypeScript still holds, and why

| Left in TypeScript | Why it is presentation or a binding |
|---|---|
| `stores/map.ts` `applyLayerTransition`, `applyLayerWrites` | MapLibre `setLayoutProperty`, store set, IndexedDB persist — effects of a kernel answer |
| `stores/layerVisibilityPrefs.ts` | IndexedDB persistence of the kernel's preferences |
| `stores/layerIsolation.ts` | holds the kernel's snapshot |
| `layers/kernel.ts` | marshalling: reduces the document to what the kernel reads, shapes questions, types answers |
| `layers/helpers.ts` `formatLabel`, `layerLabel`; `layers/group-menu.ts` | label formatting and the menu's item inventory over kernel-provided flags |
| `layers/loader.ts`, `layers/runtime.ts` | fetch orchestration, IndexedDB cache, stale-project race guards, MapLibre render-signature (does a fresh document need a remount) |
| `layers/workspace.ts`, `stores/layerGroupSettings.ts` | which drawer tabs are shown — browser chrome preferences |
| `api/layers.ts` `layerAction` | performs the declared operation through the generated manifest and hands the answer to the kernel |

## 6. Not yet done in this domain

Local layer lifecycle (slice 2), styling and canonical field domains
(slice 3), and the remote host boundary (slice 4) are separate slices; see
`ds-web/docs/layer-domain-record.md` for the landing record and gaps.
