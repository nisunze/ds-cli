# Foundation Datasets Contract — cloud-resident reference data

**Owner:** ds-brain (catalogue authority and the bounded reads). Command surface: ds-command-kernel descriptors reached **CLI first** (`ds data …`), then MCP through the same descriptors; the Desktop datasets panel renders the same catalogue rows.

**Status:** owner ruling 2026-09-20 (verbatim, read generously): *"customers, UPI and future big data will NOT be downloaded locally, we will query the cloud and we will implement governance and rate limiting later so that one cannot abuse our BigQuery. After all that is why we have BigQuery — we cannot turn this into a Frankenstein monster."* Lands in the current delivery. This is a refactoring of the existing geographic data distribution (catalogue + bounded BigQuery reads), not a new pipeline.

**Related:** [project-dataset-cache.md](project-dataset-cache.md) (rooms, coverage, `ready`), [reference-query-cache.md](reference-query-cache.md) (national bundles on this machine), ds-brain `docs/contracts/global-tiles.md` (publications the catalogue is derived from), ds-brain `CLAUDE.md` §"ds-brain-bulk allow-listed direct surface" (`admin_rwanda` — the precedent for bounded, capped, single-query BigQuery reads).

## Observed today (2026-09-20)

Feedback `27c10f62`, `de978fdf`, `dbe4d17e` and the owner's Desktop screenshot, checked against the live catalogue (`POST /api/v1/data-distribution list_datasets`, Firestore `eds_data_catalog`, BigQuery `engineering-data-solutions`):

1. The Firestore catalogue holds 45 rows; 40 are visible to every Rwanda project. The five hidden rows are hidden only because their global tile publication was retired on 2026-09-18 (`rwanda_foundation_20260608`, `rwanda_data_elementary_schools`, `rwanda_data_power_lines`, `is_active:false`): `rwanda_upi_parcels` (`global_tiles.rwanda_upi_parcels_clean`, 11,521,418 rows, 7.6 GB), `edcl_customers` (`global_tiles.edcl_customers_anonymized`, 1,052,250 rows, 148 MB), `rwanda_villages` (`global_tiles.rwanda_villages_clean`, 14,816 rows), `elementary_school`, `powerlines`. Until that retirement they were listed as `state: missing` with `download_bytes: 0` — an unpublished 7.6 GB bundle offered as a download that never came.
2. Nothing in the stack can answer "which parcel is UPI `20506012183`" or "which customers sit in this village"; a corridor-houses list of 10 UPIs was mapped at village level.
3. The catalogue has no notion of residency: every row is a bundle candidate, so the Desktop panel counts an unpublishable row as *incomplete*, offers it for download, and never settles (`complete=false` for ever).
4. Three village datasets exist for one real-world layer: `village_boundaries` (`rwanda_geo_data.village_boundaries` — the table `/api/v1/admin/rwanda` and the print reference read), `rwanda_villages` (the Foundation copy), and the Desktop's `rw-admin-villages` calculation index, which is assembled from `admin/rwanda descendants` — i.e. from `village_boundaries` again. `powerlines` sat beside `hv_line`/`mv_line`/`lv_line`; `elementary_school` beside `primary_schools`/`secondary_schools`.
5. `ready` on a project room is computed in the store (`ds-layer-store::project_dataset_cache::status_value`) as *index answers ∧ nothing pending ∧ completed non-empty ∧ completed covers requested*, but only the boolean is reported. A room that is held, indexed and fresh yet `ready:false` (feedback: `google_open_buildings`, 29,124 features, `index_state:ready`, `fresh:true`) gives the operator nothing to act on; printing and clearance treat `ready:false` as absent.
6. The Desktop panel hand-writes the `rw-admin-villages` row (label, group, "Size not published", "Download administrative boundaries") instead of describing it from the catalogue row it is built from, so it reads as a second, differently-classified dataset beside `cell boundaries`/`village boundaries`.
7. Cost of the reads, measured by dry run: one UPI lookup scans ≤ 361 MB (about USD 0.002); one customers query scans ≤ 148 MB (the whole anonymized table, about USD 0.001). Both tables are clustered by `district, sector`.

## Definitions

| Term | Meaning in this contract |
|---|---|
| **Catalogue row** | One entry of `list_datasets`: one real-world layer of one country, identified by `id = sha256(lower(country) ‖ NUL ‖ layer ‖ NUL ‖ project.dataset.table)`. Unchanged. |
| **Residency** | Where a dataset's bytes live for a project: `bundle` (published once as a verified bundle; installed on the machine, subset per project into a room) or `cloud` (BigQuery is the only holding; `ds` reaches it through bounded reads). Every row carries `residency`. |
| **Foundation dataset** | A cloud-resident row declared by name in ds-brain (`rwanda_upi_parcels`, `edcl_customers`) — declared, not inferred, because a personal-data table is cloud-resident whatever its size. |
| **Residency threshold** | `residency_threshold_bytes` — a catalogue attribute published on every row (today 524,288,000 = the Desktop's existing "Large datasets · over 500 MiB" line). A row whose source table exceeds it is `residency: cloud` by size; a future big dataset needs no code to be cloud-resident. |
| **Authority (villages)** | `village_boundaries` (`engineering-data-solutions.rwanda_geo_data.village_boundaries`, id `c74c2626…`): the table `admin/rwanda` reads, the print reference, and the source of the Desktop calculation index. `rwanda_villages` is a retired alias of it. |
| **Alias** | A retired dataset id or layer name an authority row answers for: `aliases: ["rwanda_villages", "<its digest>"]` on the authority row. A request naming an alias is answered as the authority, and the receipt says so. |
| **Bounded read** | One BigQuery query with a declared bound (one exact key, one administrative unit, or one rectangle under an area cap), a hard row cap, and a receipt that names the dataset, the query, the bound, rows returned and rows total. Never a full-table pull, never paged into a local copy. |
| **Receipt** | `{dataset: {id, layer, source, residency, version}, query: {action, bound}, rows_cap, rows_returned, rows_total, truncated, …rows}`. |
| **`ready` / `ready_reason`** | `ready` is derived once, in the kernel (`project_dataset_cache::readiness`), from holding + index + pending + coverage; when false, `ready_reason` names the first failing condition. |

## Required

### R1 — Residency is a catalogue attribute
- Every `list_datasets` row carries `residency` (`bundle` | `cloud`), `residency_threshold_bytes`, and, for a cloud row, `residency_reason` (`declared foundation dataset` or `source <bytes> exceeds the residency threshold`).
- A cloud row is published with `download_url` empty and no bundle fields — it is **not unpublished**, it is cloud-resident; `unavailable` stays empty. ds-brain never builds a bundle for it (`publish_dataset` refuses `dataset_cloud_only` by name).
- Cloud rows are admitted to the catalogue by declaration and project country, not by a tile publication being visible: retiring a tile never hides a foundation dataset. Reconcile refreshes their BigQuery metadata (rows, bytes, version) like any other row.
- The kernel's catalogue reader (`ds-project-data::catalog::ReferenceResource`) carries `residency` and `aliases`; `facts()` reports a cloud row as neither `ready` nor `downloadable` so printing's `catalog()` never selects it (`refusal: printing_context_no_dataset` stays the answer; a cloud layer is not a print context source in this slice).

### R2 — Cloud-resident datasets are never seeded, installed or downloaded
- `data.project-cache.seed --dataset <cloud id|layer>` and `desktop.data.rwanda.install --resources <cloud id>` refuse **by name**: `dataset_cloud_only` — *"`rwanda_upi_parcels` is cloud-resident; query it with `ds data upi lookup`"* (customers: `ds data customers query`). Nothing is written locally; no room is created.
- `data.project-cache.status` and `desktop.data.rwanda.status` rows carry `residency`; a cloud row reports `seeded:false, ready:false, ready_reason: cloud_resident` and is excluded from the "incomplete" count.
- The Desktop datasets panel shows a cloud row as **"In the cloud — queried on demand"** with no checkbox, no Download/Add button, no size, and does not count it toward *installed of available* or toward `complete`.

### R3 — Bounded reads on the existing surface
Two actions on `POST /api/v1/data-distribution` (ds-brain-bulk lane; `requireProjectAccess` membership gate; no lifecycle gate; `DisallowUnknownFields`), beside `list_datasets` / `query_dataset` / `query_print_context`. Query shaping is a pure function per action, unit-tested; one BigQuery query per call; parameters, never string interpolation.

| Action | Body | Bound | Answer |
|---|---|---|---|
| `upi_lookup` | `{action, project_id, upi}` — `upi` is 8–20 ASCII digits or the slashed form `P/DD/SS/CC/NNNN…` (slashes stripped, then the same rule) | exactly one parcel (`LIMIT 2` proves uniqueness) | receipt + `feature` (GeoJSON Feature: polygon + `upi, parcel_key, province, district, sector, cell, village, cell_code, status, source_area, start_date, end_date`); `upi_not_found` (404) when none; `dataset_ambiguous` (409) when two |
| `customers_query` | `{action, project_id, village_code?, cell_code?, bbox?, limit?}` — exactly one of `village_code` (8 digits) / `cell_code` (6 digits) / `bbox` `[w,s,e,n]`; `limit` ≤ 5,000, default 5,000 | a unit is answered by `ST_WITHIN(customer, unit polygon)` from the villages authority (village or cell boundary) in the same query; a rectangle is answered by `ST_INTERSECTSBOX` and refused `bound_exceeded` above 25 km² (the kernel's `MAX_ENVELOPE_M2`) | receipt + `features` (Point Features: `customer_segmentation, meter_type, meter_category, payment_method, connection_year, province, district, sector, cell, village`); `rows_total` from `COUNT(*) OVER ()`; `truncated: rows_total > rows_returned` |

Both answer with the standard envelope; refusals as the route already does (`409` + message) plus `404 upi_not_found`. Every answer names `dataset.source` (the exact `project.dataset.table`) and `dataset.version` (the catalogue's source digest at the time of the read).

### R4 — The `ds data` surface (kernel descriptors, CLI first, MCP parity)
| Command | Inputs | Authority / requires | Refusals (own) |
|---|---|---|---|
| `data.upi.lookup` | `--upi <id> [--geometry-out <path.geojson>] [--lane]` | `headless_project`, `server` | `upi_invalid`, `upi_not_found`, `dataset_ambiguous`, `bound_exceeded`, `data_distribution_unavailable` + the native-user refusals |
| `data.customers.query` | exactly one of `--village <8-digit code>` / `--cell <6-digit code>` / `--bbox w,s,e,n` / `--transformer <name>`; `[--limit N ≤ 5000]` `[--geometry-out <path.geojson>]` `[--lane]` | `headless_project`, `server` | `invalid_admin_scope`, `bound_exceeded`, `transformer_not_found`, `project_has_no_extent`, `data_distribution_unavailable` + the native-user refusals |
| `data.project-cache.status` / `.seed` | existing; `--dataset` also accepts a layer name or an alias | existing | `+ dataset_cloud_only`, `dataset_retired` |
| `desktop.data.rwanda.status` / `.install` | existing | existing | `+ dataset_cloud_only` |

- `--transformer` is the project scope: the CLI takes that transformer's design extent (the same `extents::extent_of` the seed uses), buffers it by the project's `design_buffer_m` (buffer policy), and sends the resulting rectangle as `bbox`. The receipt's `query.bound` names the transformer and the rectangle.
- The terminal receipt prints identity and evidence, not coordinates (as `data admin-bounds read`): one parcel is one line plus bounds and vertex count; customers are counted per village/cell and per segmentation. `--geometry-out` writes the exact GeoJSON (`create_new`, never overwrites) — the file `ds map local register` and `ds map draw` take as it stands.
- `--dataset rwanda_villages` / `--dataset cb1341c2…` on the project-cache commands answers as `village_boundaries` with `answered_as: {requested, authority}` in the receipt; `--dataset powerlines` / `elementary_school` refuses `dataset_retired` naming the detailed alternatives the kernel already knows (`hv_line, mv_line, lv_line` / `primary_schools, secondary_schools`).
- WINDOW ratchet and context budgets are not raised; no `CLIENT_PROFILE_SCHEMA` bump; both new commands are `Requires::Server`.

### R5 — `ready` derived once, with its reason
- `project_dataset_cache::readiness(index_state, pending, completed, requested, last_error) -> {ready, ready_reason}`; the store calls it, the overview echoes it, the CLI and the Desktop card print it. Reasons, in the order they are tested: `index_absent`, `index_incomplete`, `acquisition_pending`, `nothing_completed`, `coverage_gap` (with the gap count), `acquisition_failed` (last error named). A ready room has `ready_reason: null`. Stale coverage stays a report, never a gate (unchanged).
- Rows for cloud datasets: `ready_reason: cloud_resident`; rows the catalogue lists but nobody seeded: `ready_reason: not_seeded`.

### R6 — Catalogue hygiene (one row per real-world layer)
- `village_boundaries` carries `aliases: ["rwanda_villages", "cb1341c2ffe0…"]`; the `rwanda_villages` row stays inactive in the catalogue (its tile is retired) and is never listed again.
- `powerlines` and `elementary_school` stay retired (tiles removed 2026-09-18); their names answer through the kernel's detailed-alternatives table (R4).
- The Desktop's `rw-admin-villages` row is described **from the `village_boundaries` catalogue row** (group, label suffix "· calculation index", source rows, source-updated date, "about &lt;expanded_bytes&gt;" as its assembled size) — never hand-written; it remains a separate install because it is a different artefact (the reporter's DSAB index) of the same authority.
- An unpublished bundle row (bundle residency, no `download_url`) is reported as **unpublished** (`ready_reason: bundle_unpublished`) — not as an incomplete download — and does not count toward `complete`. A downloaded bundle that cannot be indexed names the failing step in its status `message` (the store's existing message, surfaced unchanged).

## Refusals (named)
| Code | When | Remedy |
|---|---|---|
| `dataset_cloud_only` | a seed, install or publish names a cloud-resident dataset | query it: `ds data upi lookup` / `ds data customers query` |
| `dataset_retired` | a request names a retired layer with no single authority | use the detailed alternatives named in the message |
| `upi_invalid` | the UPI is not 8–20 digits (slashes allowed) | pass the parcel's UPI as printed on the title |
| `upi_not_found` | the authority holds no parcel with that UPI | check the UPI; parcels are as published by the land registry snapshot |
| `dataset_ambiguous` | more than one parcel carries the UPI | report it as a data defect; nothing is chosen for you |
| `bound_exceeded` | `--limit` above 5,000, or a rectangle above 25 km² | narrow the area or name a village/cell |
| `invalid_admin_scope` | the village/cell code is not 8/6 digits | use the exact code from `ds data admin-bounds list` |

## Open question — governance and rate limiting (designed later, not built)
Per-account quotas, a daily bytes-scanned budget per project, and an audit row per read (who, project, dataset, bound, bytes billed) are the governance the owner named. Nothing here pre-builds them: the receipt already carries every field an audit would record, the SRE request-event store already records BigQuery failures for `admin/rwanda`, and the bounds above cap the worst single read at ~361 MB scanned. When designed, they attach at ds-brain (the one gate every read passes) without changing this surface.

## Acceptance (CLI, canary, `agct38sq_sample`)
1. `ds data upi lookup --upi 20506012183 --lane canary --output json` → one Polygon feature, `dataset.layer: rwanda_upi_parcels`, `dataset.residency: cloud`, `rows_returned: 1`, `rows_total: 1`, `truncated: false`.
2. `ds data upi lookup --upi 99999999999 --lane canary` → `upi_not_found`.
3. `ds data customers query --cell 205060 --lane canary --output json` (cell code from `ds data admin-bounds list --level cell --parent-code 20506`) → Point features, `rows_cap: 5000`, `rows_total` ≥ `rows_returned`, `truncated` stated; `--limit 10` → `rows_returned: 10`, `truncated: true`.
4. `ds data customers query --bbox 29.5,-2.6,29.9,-2.3 --lane canary` → `bound_exceeded` (≈ 1,480 km²).
5. `ds data project-cache status --lane canary --output json` → the `rwanda_upi_parcels` and `edcl_customers` rows carry `residency: cloud`, `ready_reason: cloud_resident`; every other not-ready row names its `ready_reason`.
6. `ds data project-cache seed --dataset rwanda_upi_parcels --lane canary --yes` → `dataset_cloud_only`, by name, nothing written.
7. `ds data project-cache status --dataset rwanda_villages --lane canary` → answered as `village_boundaries` (`answered_as`); `--dataset powerlines` → `dataset_retired` naming `hv_line, mv_line, lv_line`.
8. Desktop panel (local dev instance): cloud rows read "In the cloud — queried on demand" with no action; `rw-admin-villages` is described from the `village_boundaries` row; `complete` counts publishable rows only.

## Decisions written here (owner may veto)
- D1 Villages authority = `village_boundaries`; `rwanda_villages` retired as its alias; `rw-admin-villages` kept as the reporter's index of the same authority, described from it.
- D2 The reads live on `/api/v1/data-distribution` (the catalogue's own surface, already on the bulk lane and membership-gated) rather than `/api/v1/data`, because the catalogue row is what a read is scoped and receipted by.
- D3 A customers unit query is spatial (`ST_WITHIN` against the villages authority polygon), not by name: the anonymized customers table carries names, not codes, and names are not unique across Rwanda.
- D4 `--transformer` scoping is derived in the CLI from the existing design extent + buffer policy and sent as a rectangle; ds-brain learns nothing about transformers on this route.
- D5 Threshold 524,288,000 bytes (the Desktop's existing 500 MiB line); customers (148 MB) is cloud by declaration, not by size.
- D6 Cost per read is documented (Observed 7) instead of throttled; governance is the open question above.
