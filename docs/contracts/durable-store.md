# Durable store

Status: contract, 2026-10-20. Owner direction: the Kernel owns every local
store's schema, SQL and transitions; IndexedDB is removed from ds-web domain by
domain, except two owner-approved platform exceptions (below). Architecture:
[development philosophy](https://github.com/nisunze/ds-architecture/blob/master/architecture/development-philosophy.md)
sentences 2, 4 and 9; migration rules:
[Rust, Svelte and SQLite contract](https://github.com/nisunze/ds-architecture/blob/master/architecture/rust-transport-sqlite-migration.md#storage-migration-and-indexeddb-retirement).

Current state: the Kernel API, the shared statement executor and
`ui_preferences` run natively (feature `durable-store`). Landing next, in this
order: the rusqlite 0.40.2 upgrade on every native consumer; the browser host
(the `wasm` feature implies `durable-store`; rusqlite over sqlite-wasm-rs on
OPFS); `core.store` and its Desktop door; the TypeScript platform adapter and
StoreHost. Until a store's slice lands, its IndexedDB code stays in service.

## Owner and edges

`src/durable_store.rs` owns the call vocabulary, the one transaction, the
identity binding, the owner fence, store generations, shared import receipts,
the effect vocabulary and the byte channel. Each store owns its tables in its
own module (`src/<store>.rs`) and runs its SQL directly on the transaction.

| Edge | Executor | Storage |
| --- | --- | --- |
| Desktop and headless | installed Core, `core.store` (contract 1), reached from the Desktop through its local Core door | Core's protected Store (`store.sqlite`, `ds-sync-store`) |
| Hosted Web | the storage worker calls the Kernel WASM export that runs `durable_store::execute` | rusqlite over sqlite-wasm-rs on OPFS: sahpool VFS `ds-kernel-store-v1`, directory `.ds-kernel-store-v1`, database `/kernel.sqlite`, Web Lock `ds-kernel-store-v1` |

One Kernel WASM binary serves the page and the storage worker. Which edge
executes a call is the platform adapter's choice; there is one implementation.

## Wire shapes

A call is `Call` JSON: `{"request": {"store": <name>, "operation": {"action":
<action>, …}}, "expect_owner"?: <owner>}`. Payload bytes travel beside it,
never inside the JSON. An answer is `Answer` JSON: the store's fields, then
`generation` and `owner` for a Core-homed store, then `effects` when there are
after-effects; answer bytes travel beside it. A refusal is the string
`<code>: <message>`, with codes from the Kernel (`durable_store_invalid`,
`durable_store_not_core`, `durable_store_owner_changed`, `<store>_invalid`,
`durable_store_storage_unavailable`).

- **TypeScript binding:** `durableStore<T>(store, operation, options?:
  {expectOwner?: string; bytes?: Uint8Array}): Promise<T & {generation: number;
  owner: string}>` in `$lib/storage/kernel-store.ts`, with
  `durableStoreBytes(...)` answering `{answer, bytes}`. It rejects with an
  `Error` whose message is the Kernel refusal. Per-store typed bindings live in
  `$lib/storage/<store>.ts` and call nothing else.
- **StoreHost (page WASM port):** Kernel flows that run in the page (gateway
  exchanges, drains) reach storage through a host object passed beside
  GatewayHost: `transaction(call: Uint8Array, bytes: Uint8Array | null):
  Promise<{answer: Uint8Array; bytes: Uint8Array | null}>`, where `call` and
  `answer` are the UTF-8 JSON above. `$lib/storage/kernel-store.ts` implements
  it: the storage worker on the Web, `core.store` on the Desktop. A flow that
  reads, fetches and writes carries the first answer's `owner` as
  `expect_owner` on its writes.

## Identity

Rows of a store homed in Core's protected Store are fenced by the host's
identity and, for project-scoped data, the exact project. A caller names at
most the project; a call that states an identity is refused.

| Host | `principal` | `lane` | `audience` |
| --- | --- | --- | --- |
| Core (`core.store`) | `{"account": <protected account uid>}` | the command's lane | the canonical (account, lane, credential audience) owner digest |
| Browser worker, signed in | `{"account": <Firebase uid>}` | the deployment lane | the observed authority identity (Firebase project and gateway) |
| Browser worker, signed out | `"anonymous"` (`Principal::Anonymous`, never a uid) | the deployment lane | the observed authority identity |

`Identity::owner()` is the SHA-256 of the canonical (principal, lane,
audience); every Core-homed answer carries it as `owner`, rows and import
receipts are fenced by it, and `expect_owner` refuses
`durable_store_owner_changed` before anything runs when the bound identity
differs. Core requires a signed-in principal; the anonymous principal exists
for the signed-out browser profile (map, sketches, local models, push
tombstones).

The two older statement-protocol stores, transformer status (`status`) and the
project model catalogue (`grid_project_catalog`), keep their caller-built
scopes and their own Core commands (`design.status`, `design.model.catalog`);
`core.store` refuses them (`durable_store_not_core`). Their statements run
through the one shared executor, `sql_effect::execute`; they take no bytes and
no owner fence.

## One transaction

`execute(connection, identity, call, bytes)` opens one `IMMEDIATE`
transaction, runs the store's schema step (`CREATE TABLE IF NOT EXISTS`) and
the operation, and commits only on success. A refused operation leaves
nothing behind, not even its schema step.

## Generations

Every write to a Core-homed store advances that store's generation
(`durable_store_generations`) in the same transaction, and every answer
carries the current `generation`. A store with independent kinds sets
`Outcome.generation_key` to `<store>.<kind>`, so a write to one kind never
moves another kind's generation and the answer carries the kind's own. A reactive layer observes change through the
generation without reading rows it does not need; the store never depends on
that layer.

## Imports

One table serves every store: `durable_store_imports(owner, store, source,
count, skipped, sha256, at_ms)`. `durable_store::import_once(connection,
identity, store, source, sha256, now, import)` answers the held receipt when
one exists; otherwise it runs `import` and records the receipt in the same
transaction. The Kernel declares each source's legacy keys and prefixes
(TypeScript holds none); the host reads them read-only and sends the entries;
the store maps each entry through that source's reader and inserts with `ON
CONFLICT DO NOTHING`, so a value written since the upgrade is never replaced.
An entry no reader owns is skipped and counted. Importers run once per
identity, the anonymous one included.

A source too large for one call imports in batches (`src/import_batches.rs`):
each batch writes its entries with `ON CONFLICT DO NOTHING` and adds them to
the identity's running tally (counts and a SHA-256 chain over the batches as
handed over); the finishing call records the one receipt through `import_once`
and clears the tally, and a batch after the receipt refuses.
`durable_store::held_receipt` answers whether an import still has to run, so a
host reads a legacy source only when it does.

## Effects

`Effect` is a closed vocabulary: `write_file {file, contents}` and
`delete_file {file}`, where `file` is a Kernel-named `EffectFile` with a fixed
OPFS path (`push_projection` → `ds-kernel-effects/push-projection.json`).
`effects_before(&call)` names what must complete before the transaction;
`Answer.effects` names what runs after the commit. The browser host runs both
on OPFS under the same Web Lock, so a crash between them fails closed. Core
treats them as no-ops: the Desktop and headless hosts have no service worker or
OPFS reader.

## Bytes

A call carries at most 64 MiB of payload bytes, and an answer at most 64 MiB.
Only a store whose operation takes bytes accepts them; others refuse. The
browser worker transfers them as `ArrayBuffer`s; `core.store` reads and writes
them as private files beside the request and answer.

A payload over one part (`src/byte_parts.rs`, 16 MiB) arrives as consecutive
calls of the same operation carrying `payload: {byte_length, part,
staging}`: the first part's answer names its `staging`, every later part
carries it, and only the last call takes effect, once the store verified the
whole payload. A staging untouched for an hour is swept. Stored bytes keep a
SHA-256 per part, so a read verifies only the parts it returns.

## Stores

| Store | Module | Scope | Keys or rows |
| --- | --- | --- | --- |
| `ui_preferences` | `src/ui_preferences.rs`; the key table (name, home, bound, contract) in `src/ui_preferences/values/catalog.rs`, value contracts in `src/ui_preferences/values.rs` and its domain modules `values/{map,workbench,design}.rs`, the `persistent_cache` legacy table and readers in `src/ui_preferences/legacy.rs` | account, or account + project per key | closed key list with per-key `max_bytes` (default 64 KiB) and canonicalising `admit`. Account: `active_project`, `layer_group_collapse`, `map_mode`, `map_defaults`, `tool_layout`, `tile_layers`, `application_settings`, `edit_projection_choice`, `edit_projection_custom`, `pinned_group_visible`, `profile_occupancy`, `create_transformer_map_sources`, `upload_dialog_layout`, `properties_dock_width`, `style_center_layout`. Project: `layer_visibility`, `popup_layers_disabled`, `earth_engine_explorer`, `working_area`, `process_preset`, `active_lv_transformer`, `status_warning_types`, `status_sort`, `map_viewport`, `pinned_transformers`, `fast_mode_customer_source`, `fast_mode_pole_source`. Legacy source `persistent_cache`: 15 exact `persist:` names and 11 project prefixes; `design:pinned_transformers:v1` fans out to one row per project |
| `retained_reads` | `src/retained_reads.rs`; flow helpers and the `Edge` port in `src/gateway_exchange/retained.rs` | account, or account + project per kind | closed `Kind` list; per kind a fence, key grammar, value byte bound and per-owner row bound with least-recently-used eviction; read, list, write (optionally conditional on the kind's generation), replace_set, invalidate, forget; per-kind generations `retained_reads.<kind>`; JSON only |
| `push_state` | `src/push_state.rs` | browser profile: the (lane, audience) digest and the Firebase app id; the session owner is the bound principal, so a sign-out reaches the record | one record per app id: generation, session, opt-in and registered owner, fid, installation hash, epoch. `effects_before` deletes the push projection before every write but activation; every write's after-effect re-states it from the committed record (written when active, deleted otherwise). Legacy source `ds-messaging-push/state`: the account's opt-in only; an opt-in or disable here supersedes it |
| `folder_connections` | `src/folder_connections.rs` | identity; the project per row, matched exactly | logical key, project, folder name, kind (`gis_folder`, `pls_folder`, `export_workspace`, `other`), a unique 128-bit token, times. `drop` names the tokens the web adapter deletes. Legacy source `ds_web_store/folder_connections` |
| `local_layer_store` | `src/local_layer_store.rs`; decisions stay in `src/local_layers.rs` | identity | the held catalogue records verbatim (at most 2000, 16 MiB) and one payload per record id (one JSON array of features, at most 256 MiB, counted). The sweep prunes payloads no record names and drops only unreferenced `gisFolder:` connections. Generations `catalogue` and `payloads`. Batched legacy source `ds-web/app` |
| `grid_model_store` | `src/grid_model_store.rs` | identity; a session per project or none | retained check results, temporary groups (id derived from the source digest) and remembered sessions; `remove_model` removes a model's groups, results and sessions in one transaction. Generations `results`, `groups`, `sessions`. Batched legacy source `ds-grid-catalog` (`results`, `temp_groups`); the `.dsgrid` working copies join with the byte store |
| `byte_cache` | `src/byte_cache.rs`; parts in `src/byte_parts.rs` | identity | content-addressed: content is unique per owner and SHA-256 and never shared across owners. Entries are `(kind, key)` of the closed kinds `photo`, `compute_artifact`, `report_preview`, `grid_remote_version`, each naming content with a media type and a small JSON `meta`. `has` compares stored lengths and reads no bytes; `get` verifies the part it answers, and a corrupt part drops its content with every entry naming it and answers a miss; `put` arrives part by part and a declared SHA-256 must match; `delete`. One retention rule: past its kind's byte budget, the owner's oldest entries of that kind leave first. A photo read serves a pending upload's bytes first and a photo put never replaces them. Generations `byte_cache.<kind>`; no importer |
| `media_outbox` | `src/media_outbox.rs`; transitions in `src/survey_photo_outbox.rs` | identity; a signed-in account queues | one row per pending upload and its bytes (the original, then its thumbnail companion) in its own work tables, which no retention reaches: `admit`, `claim` (recover abandoned leases, drive, dispatch each target with its SHA-256), `read` (a target's bytes for its transfer), `settle` (a verified upload moves both into `byte_cache` as photos and removes the row), `retry_failed`, `counts`, `failed`. Batched legacy source `ds_web_store` `blob_outbox` with its pinned `photos` bytes: this account's rows only |
| `upload_chunks` | `src/upload_chunks.rs` | identity, the signed-out one included | Arrow IPC chunks by (workspace, file, layer, chunk), the keys sketch layers keep: `put` (replaces), `get`, `list` (no bytes), `clear_layer`, `clear_workspace`; damaged bytes are reported, never dropped. Batched legacy source `ds_web_store` `upload_chunks` (inline bytes or OPFS spill files) |

### Plain caches reuse `retained_reads`

Every reconstructible JSON cache of a Brain read is a `retained_reads` kind:
add one line to the `kinds!` table in `src/retained_reads.rs` (its fence, key
grammar, value bound and row bound) and read and write it from the domain's
Kernel flow through `gateway_exchange::retained`. Do not add a store for a
plain cache. A cache of bytes belongs to the content-addressed byte store; a
person's work (drafts, queues, idempotency keys) belongs to a work store with
an importer.

## Adding a store

1. Write `src/<store>.rs`: a typed `Operation` (`#[serde(tag = "action",
   rename_all = "snake_case", deny_unknown_fields)]`) and `execute(&Connection,
   Input, Operation) -> Result<Outcome, String>` that creates its tables first,
   fences every row by `identity.owner()` (and project when scoped), bounds and
   canonicalises every value and refuses with `<store>_invalid: …`. Return
   `Outcome::read` or `Outcome::written` (writes advance the generation), with
   `bytes` or `after` effects when the store needs them. Tests go in
   `src/<store>_tests.rs` on in-memory rusqlite: round trip, account isolation,
   refusals and rollback.
2. Add one `Request` variant and one arm in `execute`; `core_homed()` answers
   `Ok`. Declare the module in `src/lib.rs` behind `durable-store`. These two
   files are shared: edit right after a fresh read and commit only your hunks.
3. A cache of a Brain read is one Kernel flow in the page: storage through
   StoreHost plus the gateway exchange; no TypeScript fetch remains.
4. In ds-web, add `src/lib/storage/<store>.ts`, rewire every caller, and delete
   the IndexedDB code, its TypeScript reader/writer and its drain loop in the
   same commit. Never bump the IndexedDB version or delete an object store: a
   retired domain stops reading and writing, and one final cleanup deletes
   `ds_web_store` after the last domain's recovery window.
5. Migration: a reconstructible cache has no importer and its old rows stay
   untouched. A store holding a person's work declares a legacy source and
   imports once per identity through `import_once`; the source stays for the
   recovery window, and the importer's deletion is recorded in
   `backlog/OPEN.md`.
6. Refresh the vendored Kernel WASM after Kernel changes, and commit per domain
   and per repository, Kernel first.

## IndexedDB inventory and durable-data model

Classes: **work** holds a person's unsynced or authored data and migrates with
a receipt; **cache** is reconstructible from its source and needs no importer;
**exception** is an owner-approved platform constraint.

| # | Database / object store | Domain (ds-web owners) | Class | Target |
| --- | --- | --- | --- | --- |
| 1 | `ds_web_store` / `tabular` | SRE activity (`routes/sre`) | cache | fetch flow |
| 2 | `ds_web_store` / `active_project` | navigation (`stores/app.ts`) | cache | `ui_preferences.active_project` |
| 3 | `ds_web_store` / `map_state` | map state (`layers/loader`, `survey/loader`, `api/pipeline`) | cache | fetch flow |
| 4 | `ds_web_store` / `cache` | session cache and `persist:` keys: preferences, drafts, Solar workspace, PM retry, reconciliation, help, tutorials, maps3d | per key | preferences → `ui_preferences`; drafts and queues → work stores; response caches → fetch flows |
| 5 | `ds_web_store` / `photos` | survey and Solar media bytes | cache, except pinned bytes of pending uploads (work) | byte store |
| 6 | `ds_web_store` / `blob_outbox` | pending media uploads (`api/blob-sync.ts`) | work | byte outbox |
| 7 | `ds_web_store` / `upload_chunks` | upload workspace Arrow chunks | work (in progress) | byte store |
| 8 | `ds_web_store` / `folder_connections` | GIS folder handles | exception | opaque `{token → FileSystemDirectoryHandle}` table in the web platform adapter; metadata and lifecycle in a Kernel store |
| 9 | `ds_web_store` / `outbox` | survey, messaging and Project Work commands | work | Kernel outboxes (survey: `ds-sync-store` survey owner) |
| 10 | `ds_web_store` / `messaging_conversations` | messaging read cache | cache | fetch flow |
| 11 | `ds_web_store` / `messaging_timelines` | messaging read cache | cache | fetch flow |
| 12 | `ds_web_store` / `project_work_graphs` | Project Work read cache | cache | fetch flow |
| 13 | `ds_web_store` / `solar_project_results` | Solar result receipts | work | Solar store |
| 14 | `ds_web_store` / `solar_project_result_bytes` | Solar result bytes | work | Solar byte store |
| 15 | `ds_web_store` / `solar_project_documents` | Solar documents | work | Solar store |
| 16 | `ds_web_store` / `design_transformers` | LV design rooms, dirty edits | work | design room store |
| 17 | `ds_web_store` / `design_transformer_headers` | room headers | work (derived from 16) | design room store |
| 18 | `ds_web_store` / `compute_artifact_cache_inventory` | artifact bytes inventory | cache | byte store |
| 19 | `ds_web_store` / `compute_artifact_cache_bytes` | artifact bytes | cache | byte store |
| 20 | `ds-grid-catalog` (6 stores) | local `.dsgrid` models, results, temp groups, links, session, remote versions | work (models, results, links, groups); cache (remote versions, session) | grid model store |
| 21 | `ds-assets-index` / `pages` | Assets timeline pages | cache | fetch flow |
| 22 | `ds-report-previews` / `documents`, `catalogue` | saved report previews | cache | byte store |
| 23 | `ds-messaging-push` / `state` | push registration | exception | Kernel record; after-effects keep the fail-closed OPFS projection `{owner, epoch, generation}` the service worker reads |
| 24 | `ds-web` / `app` | sketches (local layers) | work | local layer store (`local_layers` decisions) |

Firebase Messaging's own IndexedDB is third-party implementation detail and
stays.

The folder-handle table (row 8) is the single IndexedDB use DS keeps: a
`FileSystemDirectoryHandle` persists only by structured clone into IndexedDB,
a browser platform constraint. One standalone web platform adapter
(`ds-web/src/lib/storage/folder-handle-tokens.ts`) opens its own database,
`ds-folder-handles`, with one object store holding `{random 128-bit token →
handle}` and nothing else; every fact and lifecycle decision is
`folder_connections`'. `ds-grid-catalog/model_links` had no reader or writer
left and retires without an importer.

## Proof

Kernel: `cargo test --lib --features durable-store -- durable_store
ui_preferences push_state folder_connections local_layer_store
grid_model_store`. Core: `ds-sync-store` and `core.store` tests. Browser: the
real-Chromium OPFS acceptance of the storage worker (reopen, worker loss
mid-transaction, two tabs).
