# Retained project model catalogue

Status: contract, 2026-10-19. Owner report (Rwanda Template, Design → MV):
"later created models are coming late … by arriving for the first time, they
are meant to be forever offline-first persisted just like other models."

## Owner and edges

`src/grid_project_catalog.rs` owns one SQLite schema, every transition, the
page walk over `list_models`, the source-basis comparison and the presentation
projection (`ds.grid-project-catalog/v1`). Hosts execute its bounded SQL in
one transaction per operation and the one gateway read it names; they decide
nothing.

| Edge | Storage adapter | Refresh transport |
| --- | --- | --- |
| Core (`ds design model catalog`) and the Desktop through it | native SQLite `<layer root>/grid-project-catalogue/catalog.sqlite` (`ds_layer_store::grid_project_catalogue`) | `ds_client_core::grid_models::catalogue` over the Kernel walk |
| Browser | SQLite-WASM on OPFS (the durable storage worker) | the Kernel flow's `fetch` step through the mechanical gateway port |

The Desktop keeps no catalogue of its own: its `design.model.catalog` door
attaches to installed Core and verifies the private receipt. Printing's native
MV picker (`dsgrid.project.list --mv-picker`) reads the same rows through
`grid_project_catalogue::consume`.

## Rows and transitions

`grid_project_catalog_heads(scope, model_id, head, head_sha256,
first_seen_at_ms, retained_version)` and
`grid_project_catalog_observations(scope, basis_sha256, observed_at_ms,
needs_refresh, last_error, revoked, revision)`. A scope is the complete
principal, lane, audience and project fence.

- **Commit** of a complete listing compares its basis (the digest of every
  head's canonical projection in model order) with the held basis through
  `freshness::compare`. Equal: only the observation time moves. Changed: heads
  seen for the first time are inserted with their first-seen time, changed
  heads are updated, heads the listing no longer names are retired. A moved
  head keeps its `retained_version`. A stale revision fence writes nothing.
- **RetainVersion** records the exact version whose verified bytes the host's
  bytes owner holds; it must name the held head's revision, digest and version.
- **MarkFailed** and **Invalidate** keep rows and label them; **Revoke** (a
  refused credential) stops serving them until a deliberate refresh succeeds.

## Reads

`held` never acquires: it paints every retained row immediately, offline
included, and distinguishes `unread`, `ready`, `held` (labelled) and
`revoked`. `refresh` performs one bounded authorized listing. Package bytes are
never acquired here: open, print and Project Control preparation fetch a head
on demand, by exact identity, and record it as retained.

## Migration

The catalogue is reconstructible metadata: there is no importer. The retired
browser record (`project-mv-model-cache/v1` in IndexedDB) and the native
`catalogue.json` files are left untouched as recovery evidence with no reader
or writer; the first online refresh rebuilds the rows, and verified package
bytes keep their exact-identity cache, so no unchanged model is downloaded
again. An older build keeps reading its own JSON file; the two stores never
exchange rows.

## Proof

Kernel transitions: `cargo test --lib grid_project_catalog`; native SQLite:
`cargo test -p ds-sync-store --test grid_project_catalog`; native adapter:
`ds-layer-store` `grid_project_catalogue` tests; browser binding:
`ds-web/src/lib/grid/project-catalog.test.ts` (vendored Kernel WASM over
SQLite-WASM); Desktop door: `core_attachment_grid_catalog` tests with a shell
fake Core.
