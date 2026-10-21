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
| Reopen the store after a restart: what is admissible, what was repaired, what may not persist, what is carried | `restore` | read the raw rows; rewrite when `changed`; keep each carried row verbatim; delete exactly what `release` names |
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
  That kind rule is the descriptor's, not one op's: `register` and `restore`
  drop metadata that does not belong, and the whole-registry validation every
  other op runs refuses a row carrying it, so a catalogue a host hands in
  cannot smuggle in a row no path here could have minted.
- **The project fence.** A layer prepared from a connected folder belongs to
  that folder's project; an `update` or `remove` naming another project is
  `project_context_changed`. An op that names no project is not fenced — the
  browser's catalogue is not project-bound — and an imported file belongs to no
  project, so nothing fences it.
  A **source rename** moves every layer filed under that label, so the fence is
  asked of every one of them and not only of the layer the op named; it is
  asked before anything moves, so a refusal leaves the whole catalogue as it
  was.
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
- **An id is one path segment.** It keys a record on a host that keeps records
  in a file system — an IndexedDB key, `<id>.geojson` under the native store's
  payload directory — so it carries no `/`, `\` or `:` and is never `.` or
  `..`. Refused as `malformed_descriptor` at `register` and on every registry a
  host hands in.
- **An id is never repaired, and a row is never lost for one.** Trimming an id,
  stripping a control character from it or truncating it to 200 re-keys the row
  and orphans the payload the host parked under the id the row actually
  carries. So `restore` — which repairs everything else — asks the id question
  first and **carries** a row whose id it cannot admit (control character, over
  200 characters, untrimmed, not one path segment): the row is omitted from the
  registry, nothing of it is retired or released, and the receipt answers it
  under `carried: [{"raw": <the row verbatim>, "reason"}]`. `changed` speaks
  only for the admitted rows, because the host's bytes are still its bytes.
  **Hosts persist carried rows verbatim beside the admitted registry** (the
  browser writes `sketchLayersMeta` as the admitted rows followed by the
  carried raw rows, and its orphan sweep treats a carried row's payload key as
  known), so a later kernel can still make sense of one. The next reopen
  carries it again — stable, never lossy. A row that carries **no** id at all
  is a different thing: nothing was ever keyed under one, so it is minted
  `sketch-<now_ms>-<index>` as before.
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

### `pinned` — how a transformer's room is named

A pinned-transformer row is **not** named by its host. The browser used to mint
`pinned-<canonical transformer>__<canonical class>` itself, where the canonical
name lower-cased and folded everything outside `[a-z0-9_]` into `_`: `Poles A`
and `Poles-A` were ONE layer and the second overwrote the first. What a host
offers now is the pair; the identity is the kernel's.

**The draft** — `register`, `draft.pinned`, only for `sourceKind: "pinned"`:

```json
{"kind": "register", "registry": {...}, "scope": {"host": "browser"},
 "now_ms": 1758000000000, "sequence": 3, "colorIndex": 0,
 "draft": {"name": "MV line", "geometryType": "LineString", "sourceKind": "pinned",
           "sourceName": "Poles A", "sourceLayerName": "mv_line", "featureCount": 12,
           "pinned": {"transformer": "Poles A", "className": "mv_line",
                      "fields": ["phase", "doc_id", "phase"]}}}
```

- `transformer` and `className` are the **raw** pair the row was materialised
  from — not a canonical, slugged or humanised form of either. **The pair
  itself is required of `sourceKind: "pinned"`**: no `pinned` block, either
  half blank, or a `pinned` block on a layer of any other source kind is
  `malformed_descriptor`. Without the pair the id would fall back to the clock
  and the counter, and the same room materialised again would be a second row
  instead of the one already held — the collision the pair exists to close.
- `fields` (optional) are the property keys the pinned features carry, in any
  order and with repeats. The columns are the kernel's answer: each name
  sanitised like every schema field, ordered, de-duplicated, bounded at 256,
  all of them `text`. A draft that carries `fields` carries no `schema` of its
  own (`malformed_descriptor`) — one of them decides, and it is not the host.
- `name`, `sourceName`, `sourceLayerName`, `geometryType` and `featureCount`
  stay the host's: they are what it read, not what it decided.

**The answer** is an ordinary register answer, `{registry, receipt, changed}`,
the receipt being the admitted descriptor:

```json
{"id": "pinned-poles-a__mv-line-a1308e67", "name": "MV line", "sourceName": "Poles A",
 "sourceLayerName": "mv_line", "sourceKind": "pinned", "geometryType": "LineString",
 "color": "#64748b", "lineWidth": 2.5, "pointRadius": 5.0, "fillOpacity": 0.15,
 "schema": [{"name": "doc_id", "type": "text"}, {"name": "phase", "type": "text"}],
 "featureCount": 12, "visible": true, "createdAt": 1758000000000}
```

- **The id** is `pinned-<transformer slug>__<class slug>-<fingerprint>`. Each
  slug is lowercase ASCII letters and digits with every other character one
  `-`, bounded at 64 characters (`unnamed` when nothing is left); the
  fingerprint is the first four bytes of the SHA-256 of the pair itself
  (`<transformer>\u001f<class>`, each sanitised to 500 characters first),
  in hex. Readable, one path segment, and two distinct pairs are never one id.
- **The colour is the class colour** `#64748b` for every layer of kind
  `pinned`, with the flat style's defaults (2.5 / 5 / 0.15) beside it: a pinned
  row is one visual class, so it never takes a palette entry and `colorIndex`
  does not reach it.
- **Materialising the same pair again is not a second layer.** When the derived
  id is already in the registry the answer is `changed: false`, the registry
  the caller passed, and the descriptor **already held** as the receipt — with
  the visibility and the style the user gave it. The host draws that descriptor
  and writes nothing. A payload that actually changed is an `update`
  (`{"kind":"payload",...}`) afterwards; `register` never overwrites what is
  held.

So a host materialises by calling `register` once per (transformer, class) and
appends nothing to its own store: one un-admitted row makes every later op
refuse the whole registry, for every layer in it.

## 3. Decisions to challenge

- **D1 — Restore repairs; every other op refuses.** A legacy row is trimmed,
  clamped, defaulted and reopened, with each changed key named under
  `repaired` — every text it carries, its source label included, and metadata
  or storage it cannot admit dropped rather than refused. One unbounded string
  in one row must never hide a whole catalogue from its owner; the same fault in a `register` or `update` (a style outside its
  range, a negative count, storage that cannot locate its chunks) is a refusal
  of the whole op — nothing is silently corrected or dropped. A store that
  refuses to reopen strands every layer in it, which is why the asymmetry
  exists. The **id** is the one field restore neither repairs nor refuses the
  store over: it carries that row instead (the id rule above). *Alternative:
  refuse a bad row on restore too.*
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
  triple only when no other layer of the catalogue parks its payload there — a
  carried row included, since it is still a row and still parks its payload
  where it says it does.
  *Alternative: trust the importer and always name the triple.*

- **D10 — A pinned pair is an identity, so registering it twice is idempotent.**
  `register` answers the descriptor already held — visibility and style
  included — instead of `duplicate_layer`, because materialising a room again
  is the same room. A payload that has actually changed is an `update`.
  *Alternative: refuse the second register and make every host remove the row
  first, which is what the browser did and why a re-materialise lost the
  user's visibility.*
- **D11 — A pinned id spells the pair out and fingerprints it.** A slug alone
  cannot identify (`Poles A` and `Poles-A` slug the same, and that is exactly
  the collision this replaces); a digest alone cannot be read in a receipt. So
  the id is both, and the digest is over the raw pair. *Alternative: a pure
  digest, or a host-supplied id — neither of which a receipt can be read
  against.*

## 4. Proof

`tests/fixtures/local-layers-cases.json` — 79 recorded cases, each saying why
it exists — replayed by `tests/local_layers_cases.rs`, byte-identical to
ds-web's `src/tests/fixtures/local-layers/cases.json` for the browser's WASM
replay and the native store's. Two boundary properties are asserted over the
recorded answers themselves: a refusal answers no registry, and every release
item names exactly one record the layer owns. 45 module tests hold each rule
and each refusal on its own, the lane/uid fence of a native catalogue, the
shared-id and shared-chunk release guards, the pinned pair's derived identity
and the five fences a fixing review found missing included — a carried row's
stability across reopens and the pinned pair's requiredness among them.

## 5. What hosts keep

| Left in the host | Why it is I/O or presentation |
|---|---|
| IndexedDB reads and writes, the write queue, the acknowledged-persistence fence | storage, not a decision |
| `<root>/prepared/<lane>/<uid>/catalogue.json`, the payload directory, the lock discipline | the native store's file I/O |
| Copying an imported GeoJSON into the payload directory; deleting the files a `release` names | the only deletions, and only those |
| MapLibre sources and layers, the attribute table, the temporary-layer panel | rendering a descriptor |
| `Date.now()`, the monotonic sequence, the palette cursor | the clock and counters the kernel refuses to own |
| Legacy IndexedDB key migration, chunk storage itself | host storage the kernel only names |
