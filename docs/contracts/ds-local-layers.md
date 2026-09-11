# Prepared local layers — one owner for the descriptor and its lifecycle

**Status:** kernel implemented on `fable/local-layers`, 2026-09-11. Parent:
[The application kernel](ds-application-kernel.md) §2 (the litmus test), §4a
(the renderer is never a source of truth). Sibling of
[the layer domain](ds-layer-state.md) (slice 2 of it).

## 1. The question this module owns

A **prepared local layer** is a bounded catalogue row a host keeps on its own
machine: a name, a source origin, a geometry type, a flat style, a column
schema, a feature count, and wherever the payload is parked. The browser keeps
them in IndexedDB and paints them with MapLibre; a native store keeps them in a
catalogue file beside its payloads. Both ask the same five questions and get
the same answers from `local_layers` (schema `ds.local-layers/v1`, WASM export
`localLayers`).

| Question | Op | Hosts do |
|---|---|---|
| What is in the catalogue | `list` | render rows |
| Admit this new layer — its id, every default, its normalised origin | `register` | persist the returned registry; draw the returned descriptor |
| Reopen the store after a restart: what is admissible, what was repaired, what may not persist | `restore` | read the raw rows; rewrite when `changed`; delete exactly what `release` names |
| Apply one admitted change (payload, origin, rename, source rename, style, visibility) | `update` | persist; release exactly what the receipt names |
| Remove one logical layer | `remove` | persist; delete exactly what `release` names |

Not in scope: file import and parsing, project assets, cloud publish, Server
remote files, styling documents (carried, never read), Sync Center.

The descriptor is the **same JSON ds-web already persists** (`sketchLayersMeta`
rows), because the rules were transcribed from `stores/sketches.ts`. An
existing browser profile reopens unchanged — that is the parity baseline, not
an aspiration.

## 2. Rules worth stating

- **Purity.** `now_ms`, `sequence` and `colorIndex` are inputs. The id is
  `sketch-<now_ms>-<sequence>`; the colour is `SKETCH_COLORS[colorIndex % 10]`
  from the transcribed palette. No clock, no randomness, no host choice.
- **A refusal changes nothing.** It answers `{"outcome":"refused","refusal":
  {"code","message"}}` — no registry, so there is nothing to write from it. The
  vocabulary is closed: `malformed_descriptor`, `duplicate_layer`,
  `unknown_layer`, `scope_mismatch`, `project_context_changed`,
  `unsupported_source_kind`. A malformed *request* is not a refusal but an
  `Err` from `evaluate`: the host built it wrong.
- **`revision` advances only on a change**, and `changed` is the host's signal
  to persist. A `list` is never a change.
- **Scope is where the catalogue lives**: one browser profile (`browser`, no
  lane or uid) or a native store keyed by lane and user (`native`). Every op
  names the scope it believes it is in; another store's catalogue is never
  rewritten.
- **Project Work is refused by name**, at registration, in a registry row and
  as a rename target, and retired on restore. It is an ephemeral projection of
  canonical PM entities; persisting one would imply its local copy is truth.
- **Origins normalise or drop whole.** A folder connection needs its project,
  handle, folder, path and file name — missing one, the metadata is dropped,
  never half-kept. Imported-file metadata belongs only to an `import` layer.
  The file name is derived from the path and the format from the file name.
- **The project fence.** A layer prepared from a connected folder belongs to
  that folder's project; an `update` or `remove` naming another project is
  `project_context_changed`. An op that names no project is not fenced — the
  browser's catalogue is not project-bound — and an imported file belongs to no
  project, so nothing fences it.
- **Release is the removal proof.** `remove` and `restore` name **exactly**
  what the host may delete: `{"kind":"features_payload","id"}` — the layer's own
  record — and `{"kind":"upload_chunks","workspaceKey","fileName","parsedLayer"}`
  — its own parked chunks. The kernel never names a source file path, a folder
  handle, an asset, a project layer or another layer's storage: chunks another
  layer of the catalogue still parks its payload in are that layer's storage and
  are not named, on `remove`, on a replaced payload and for a retired row alike.
  A replaced payload releases the chunks it left behind unless the replacement
  is those same chunks. On `restore`, retired and reopened rows share one id
  space, so a retired row can never name a kept layer's payload — the same id
  twice refuses the whole store. The host deletes only what the receipt names;
  that is why a removal provably cannot touch an original.
- **A rename is not a re-provenance.** A manual layer's name inside its source
  is its name, so a rename moves both; an imported layer keeps the name it had
  inside its file. A manual layer is never filed under its own name (the source
  label rule), and renaming a *source* moves every layer of that kind filed
  under that label — the label rule holding through it, so the admitted
  registry reopens without repair.
- **Bounds:** 2 000 layers per catalogue, 200 characters per name and id, 500
  per source text, 256 schema fields, 16 MiB per request; `lineWidth` 0.5–12,
  `pointRadius` 1–24, `fillOpacity` 0–1, `featureCount` ≥ 0, clamped on restore,
  refused elsewhere. Feature storage a host offers must locate its chunks:
  `null` is no storage, an unreadable object is refused (`restore` drops it).

## 3. Decisions to challenge

- **D1 — Restore repairs; every other op refuses.** A legacy row is trimmed,
  clamped, defaulted and reopened, with each changed key named under
  `repaired`; the same fault in a `register` or `update` (a style outside its
  range, a negative count, storage that cannot locate its chunks) is a refusal
  of the whole op — nothing is silently corrected or dropped. A store that
  refuses to reopen strands every layer in it, which is why the asymmetry
  exists. *Alternative: refuse a bad row on restore too.*
- **D2 — `repaired.fields` means "what the host must rewrite",** not "what was
  wrong": it is every descriptor key whose admitted value differs from what the
  row carried, an absent `schema` against `[]` included. Uniform, so it cannot
  go stale. *Alternative: report only genuine corrections, and accept that a
  host cannot tell from the receipt whether to persist.*
- **D3 — A name is trimmed and bounded 1–200.** The TypeScript trimmed nothing
  and bounded nothing. A whitespace-only name is now refused.
- **D4 — A minted chunk `idPrefix` is `feat-<layer id>`** where the TypeScript
  used `feat-<now>-<random>`; purity forbids randomness and the prefix only has
  to be unique inside one layer.
- **D5 — `previewLimit` stays in the descriptor** although the contract sketch
  omitted it: the panel writes it and the loader reads it back with the same
  clamp, so dropping it would silently change the preview window.
- **D6 — `update` takes an optional `now_ms`.** Without it, metadata carrying
  no readable `lastRefreshAt` is not admitted at all rather than given an
  invented time.
- **D7 — At restore the palette is `nextColor()` from a fresh boot**: colourless
  rows take entries 0, 1, 2 … in turn, counted over the rows that needed one.
  *Alternative: index by the row's place in the store.*
- **D8 — An admitted answer carries no `outcome` key** (`{registry, receipt,
  changed}`), per the lifecycle contract; hosts branch on `outcome ===
  "refused"`. An over-full catalogue refuses `malformed_descriptor` — there is
  no dedicated code for it.
- **D9 — Shared chunks are never released.** Two layers naming the same
  `upload_chunks` triple should not exist (the importer clears a triple before
  re-parsing into it), but the kernel does not rely on that: a release names a
  triple only when no other layer of the catalogue parks its payload there.
  *Alternative: trust the importer and always name the triple.*

## 4. Proof

`tests/fixtures/local-layers-cases.json` — 68 recorded cases, each saying why
it exists — replayed by `tests/local_layers_cases.rs`, byte-identical to
ds-web's `src/tests/fixtures/local-layers/cases.json` for the browser's WASM
replay and the native store's. Two boundary properties are asserted over the
recorded answers themselves: a refusal answers no registry, and every release
item names exactly one record the layer owns. 35 module tests hold each rule
and each refusal on its own, the lane/uid fence of a native catalogue and the
shared-id and shared-chunk release guards included.

## 5. What hosts keep

| Left in the host | Why it is I/O or presentation |
|---|---|
| IndexedDB reads and writes, the write queue, the acknowledged-persistence fence | storage, not a decision |
| `<root>/prepared/<lane>/<uid>/catalogue.json`, the payload directory, the lock discipline | the native store's file I/O |
| Copying an imported GeoJSON into the payload directory; deleting the files a `release` names | the only deletions, and only those |
| MapLibre sources and layers, the attribute table, the temporary-layer panel | rendering a descriptor |
| `Date.now()`, the monotonic sequence, the palette cursor | the clock and counters the kernel refuses to own |
| Legacy IndexedDB key migration, chunk storage itself | host storage the kernel only names |
