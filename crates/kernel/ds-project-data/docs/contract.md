# ds-project-data — the project's geographic context, held and read in Rust

Extends `ds-command-kernel/docs/contracts/project-dataset-cache.md` (the
decisions) with the host seam every DS host shares: how a project's rooms are
filled, how a national bundle is installed, and how one transformer's
`ds.print-context/v1` document is assembled. The desktop did all of this in
TypeScript (`project-data/*.ts`, `reports/transformer-print-context.ts`,
`reference-data/resources.ts`); this crate is that orchestration once, so the
headless `ds` and the desktop shell hold the same rooms and print the same
bytes.

## Owners, unchanged

| Decision or effect | Owner |
|---|---|
| Buffer policy, coverage plan, gap assessment, identity digests, seed order, decode of a provider page, the catalogue's default-on rows, the print-context document and its bounds | `ds-command-kernel` |
| Rooms (`project-dataset-cache/v1/<key>/room.sqlite`), national bundle indexes (`reference-query-cache/…`), locks, fencing, staging | `ds-layer-store` |
| Shared revision chain: head identity, manifest schema, base fence, delta and freshness rules, Google Open Buildings tile selection (S2 level 6), the engine task contracts | `ds-command-kernel::project_dataset_revisions` |
| Replaying revisions into a room, preparing the source pages `ensure` commits, building and publishing the next revision, the stale re-plan | this crate, [`revisions`] |
| Footprint normalization and contour derivation (offline, by request/response) | the Reporter engine (`prepare-publisher-footprints`, `plan-contour-lattice`, `derive-contours`) |
| The provider doors (cloud-resident bounded reads, the compute artifact door for revisions, public publisher objects, the Rwanda DEM), bundle bytes, credentials | the host (`ds-cli-data`), through [`Provider`], [`revisions::RevisionHost`], [`revisions::SourceHost`] and the fetch it lends |
| Authorizing and storing revisions (`design.report`, one head per project dataset, retained deltas, stale verdict) | ds-brain compute artifacts, engine `project_dataset`; it computes nothing |

This crate decides nothing about geometry or document shape and never opens a
socket.

## Explicit local analytical project sources

`local_source::Source` (`ds.local-analytical-source/v1`) names one exact project,
asset revision, actual source origin, footprint/contour kind, local native
GeoParquet artifact SHA-256, producer-source revision/digest and producer-completed coverage. The caller holds
and authorizes the project context; a project string is not authorization.
Google/Microsoft origins admit footprints; Rwanda DEM/Terrarium origins admit
contours at explicitly declared intervals. Custom origin is explicit. No source
is inferred or substituted, and feature bounds never imply complete coverage.
The producer source pin is separate from the generated artifact revision: it
records the elevation input for contours, with an exact SHA-256 or CRC32C
(the Rwanda COG publishes CRC32C). It participates in room/index identity and
the retained source receipt.

`DeclaredSource::LocalAnalytical` validates that binding before acquisition.
For genuinely missing cells it verifies and retains the analytical artifact,
reads bounded Arrow batches once, admits explicit canonical CRS84/WKB footer
metadata and geometry/stable identities, and builds
the existing native reference RTree. Identical stable rows deduplicate;
conflicting rows with the same identity refuse. Project subset queries use that
exact index and the existing room ticket/commit fence. A cell outside the
producer's complete coverage refuses. A completely empty analytical artifact
and an empty subset inside complete coverage can both complete and reuse warm.
The native columnar writer owns canonical CRS84 metadata
(`ds-columnar::geoparquet::geo_metadata`); this seam admits the actual reader
schema metadata against that owner, so a digest-valid file with altered CRS
refuses before its index or any project coverage is published.

Resource identity includes project/asset, actual origin and content parameters;
revision identity includes the exact artifact and completed source coverage.
Each selected revision has its own room. Old pinned revisions remain usable
offline; selecting a new revision cannot inherit the old revision's completion.
A warm seed and `Mode::Read` inspect no source file or national index, contact
no provider and perform no download. Expansion reads/patches only missing cells;
explicit `refresh` repeats local indexed subsets through the same ticket fence.
The retained analytical source survives removal of the original input file.

The CLI binds this seam through `data.project-cache.{seed,status,query}` and
an explicit `--local-source` manifest. Shared publication, contour production,
publisher download and contiguous city authority remain separate consumers.

## Explicit pinned publisher footprint inputs

`publisher_footprints::Input` (`ds.publisher-footprints/v1`) admits Google
Open Buildings CSV.gz or Microsoft GeoJSONL.gz objects only. It names the exact
project/asset revision, actual source, publisher revision, extraction coverage
and 1–16 publisher object identities, absolute local paths, SHA-256 pins and
publisher object coverage. The complete object coverage must cover every
selected extraction cell before any object is opened. Neither geometry bounds
nor a download request establishes completeness. Paths do not enter identity;
source/version, stable object identities, byte pins, coverage and conversion
revision do. Explicit source changes never substitute Microsoft for Google.

The producer verifies the compressed bytes against their pins (128 MiB/object)
and stages them; the Reporter engine's `prepare-publisher-footprints` task
(the same normalization the shared seed uses) streams bounded records
(2 MiB/record) and answers one page per extraction cell. Google geometry is strict
2D Polygon/MultiPolygon WKT, including holes; unsupported types, dimensional or
CRS modifiers, malformed/unclosed rings and nonfinite/non-WGS84 coordinates
refuse. Microsoft rows require canonical WGS84 Polygon/MultiPolygon Features.
The extraction is limited to 500000 selected footprints. Publisher ids are
preserved under their source namespace; an absent publisher id uses a stable
geometry digest. Identical ids deduplicate and conflicting rows refuse.
Publisher properties are preserved losslessly in the declared
`publisher_properties` JSON string column, avoiding schema drift between
streaming Arrow batches.

`ds-columnar::ConversionStream` owns canonical GeoParquet, CRS84/WKB metadata
and bounded Arrow batches. Output is published under the immutable conversion
identity with a source manifest and completed receipt published last; failed
conversion publishes no source. The source carries the exact publisher
revision and SHA-256 of the canonical pinned object/coverage/conversion
description, separately from the generated artifact SHA-256. The completed
receipt retains each publisher object pin. Empty verified extracts are valid.
Unchanged explicit acquisition reads only this retained manifest and receipt;
it never opens the original gzip objects or checks remote freshness. The
existing local analytical owner builds/uses the spatial index and commits
project coverage through the same ticket fence. There is no transport port,
ADC, Earth Engine or Brain compute call in this producer.

This is the pinned-input path (`--publisher-input`, e.g. Microsoft tiles an
operator chose); it publishes its own analytical source and room. Online
Google Open Buildings acquisition and contour derivation are the shared seed
([below](#seed-once-per-project-shared-revisions)).

## The registry the kernel echoes

`datasets::{buildings_dataset, contours_dataset, catalog_dataset}` produce the
`Dataset` documents — byte-identical to the desktop's — whose digest is the
room address. The Buildings/Contours provider token
`datasets::PRINT_CONTEXT_ROOM_PROVIDER` (`ds-brain:query_print_context`)
remains in the identity only so rooms seeded before the shared revisions keep
their address; it names no door and grants nothing (the Brain action answers
`NATIVE_COMPUTE_REQUIRED`). The shared head identity leaves it out. A print
lends only the cloud-read provider: for buildings and contours it refuses
`project_dataset_local_source_required` naming `ds data project-cache seed`,
and warm holdings read without any acquisition. A catalogue layer is a project subset of the
machine's installed national bundle (`desktop:reference_resource_cache`).
Contours are one room dataset carrying both kinds; the assembler splits them
back into `contours_index` / `contours_intermediate`.

## Seed once per project: shared revisions

Owner direction (22 October 2026): building footprints and contours are
fetched once per project; individuals drive differential updates on true
changes only. Engines are offline, acquisition is Core's, Brain authorizes and
stores. `revisions::seed` is that rule for `DeclaredSource::{Buildings,Contours}`.

| Responsibility | Where | Why there |
|---|---|---|
| Head identity, manifest, base fence, delta, freshness, S2 tile choice, task contracts | `ds-command-kernel::project_dataset_revisions` | decisions and schemas are the Kernel's; the Reporter and Core read one typed contract |
| Room replay, prepared pages, delta build, publish/re-plan loop | `ds-project-data::revisions` | the application owns orchestration and the room fence; it opens no socket |
| Revision head, verified download, open → transfer → finalize | `ds-cli-data` host over the native Sync Center session (`compute_artifacts`, engine `project_dataset`) | the existing governed edge publication door: one JWT, project membership, `design.report`, no new gateway route |
| Google Open Buildings v3 level-6 tiles (generation-pinned, size + CRC32C, SHA-256 receipt) and Rwanda DEM samples | `ds-cli-data` host (`ureq`, `ds-elevation-host`) | API-capable acquisition belongs to the application; the Kernel names which tiles and versions |
| Footprint normalization (strict WKT, centroid prefilter, room rule, dedupe) and contour derivation (lattice, marching squares, fixed-tile clipping) | Reporter engine tasks, by request/response | data transformation is engine compute; one implementation serves the shared seed and `--publisher-input`; the Kernel never links the engine |
| One head per project dataset, revision admission against the planned manifest, retained deltas | ds-brain `project_dataset_publication.go` | governance only: digest verification of stored bytes, no compute |

A seed, in order:

1. **Read** the project head (`compute_artifacts read`); a missing head is an
   empty chain. The manifest (`ds.project-dataset-revisions/v1`) is digest
   checked and decoded; every revision output must match its head declaration.
2. **Replay** revisions this room has not applied (the room records them in
   `applied_revisions`): each delta is downloaded once, verified, split by its
   cells under the dataset's acquisition rule and committed through the store's
   ticket fence as a replacement. A second member therefore needs no publisher
   byte. A refresh skips replay.
3. **Prepare** only the cells `ensure` will still ask for (`planned_cells`):
   Core fetches the Google Open Buildings S2 level-6 tiles those cells (plus
   the centroid margin) reach, or samples the Rwanda DEM on the Reporter's
   lattice, and the Reporter answers one complete page per cell.
4. **Ensure** (or `refresh`) commits those pages exactly as before.
5. **Publish** the delta: wanted ∩ held − published (a refresh republishes all
   it completed). The delta is the room's own rows for those cells under the
   acquisition rule (`query selection: acquisition`), gzip GeoJSON lines in
   feature-key order, published with the next manifest and the base
   fingerprint of the manifest it was planned on (`sha256("")` for the first).
   Brain answers `stored_stale` when another revision landed first; the seed
   re-reads, replays what landed and publishes only what is still missing
   (`MAX_STALE_RETRIES`, then `project_dataset_revision_stale`; held rows stay).

Differential updates: extent growth acquires and publishes only the new cells.
A publisher (`google-open-buildings/v3/polygons_s2_level_6`) or surface
(`rwanda_dem_10m/…`) version change is reported — the room's available version
and the chain freshness mark the old coverage stale — and only an explicit
`--refresh` re-acquires it; nothing polls. Others pick new revisions up on their
next seed. Reads (`Mode::Read`, prints, `status`, `query`) stay local and
offline. Contours are cut on fixed 1 km national-grid tiles so a piece is the
same bytes whichever cell asked, which keeps a cell's replacement exact at
its edges. Web reads a published head through the Kernel's existing
`computeArtifactPublishedHead`/`computeArtifactBytes` exports; it never seeds.

Bounds: 99 revisions per head (the door holds 100 outputs; compaction is
backlog), 64 MiB and 500000 features per delta, 24 publisher tiles per seed.
Contours are judged cell by cell: a planned cell the Rwanda DEM resolves nowhere
is never committed or published; the source declares it `uncovered`, the room
keeps it a gap, `EnsureOutcome::uncovered` itemises it and the next seed asks
again. Covered cells (fully, or in part where the DEM answers) publish as
usual. Only a project whose planned coverage the DEM answers nowhere
(`SharedOutcome::answered` false) refuses `contour_surface_uncovered`; no other
surface is admitted for contours yet.

## Declared, not available

`declared(resources)` maps the kernel's `printing::context::catalog`
`default_on` rows and `derived_layers` onto datasets. Installed-ness never
narrows a declaration. `project_dsgrid_mv` has no room and is not declared.

## `ensure`

`ensure(root, scope, dataset, source, policy, extents, focus, mode)`:

1. `plan_focused` fuses the roster's buffered extents; `focus` narrows to the
   clusters holding the named transformers.
2. `Mode::Read` reads the room's status and never creates one; `Mode::Acquire`
   opens it (and, for a catalogue dataset, installs the bundle first).
3. `assess(completed, plan.coverage)`; covered → done, `acquired: 0`.
4. Per gap cell: kernel identity → store `begin{wait_for_pending}` → the
   provider (or the bundle index) → `commit`, or `fail` and stop with the
   provider's failure. Held data is never touched by a failure; `ready` stays
   false while a gap remains.
5. `assess` again; an uncovered plan after acquiring is a warning, not a
   holding.

A second unchanged seed acquires nothing: the plan is deterministic, completed
coverage answers `assess` with no gaps, and the store refuses a repeated digest.

## Bundles

`install_bundle(root, receipt, fetch)` verifies the compressed bytes (size and
SHA-256) after `fetch(receipt, path)` writes them, expands and indexes them in a private
staging directory with `BundleIndexWriter`, checks expanded size, digest and
feature count, and promotes with `publish_staged`. A mismatch anywhere is
`reference_bundle_unavailable` and nothing is published.

## The print context

`read_print_context(root, scope, transformer, layers, contexts, catalog, mode)`
walks the kernel's `ContextPreparation` contexts:

| Source kind | Headless answer |
|---|---|
| `catalog` | the room subset; `Mode::Acquire` installs and seeds on the first print |
| `google_open_buildings`, `elevation_contours` | the held rooms; a print never seeds them (`project_dataset_local_source_required` names `ds data project-cache seed`) |
| `project_dsgrid_mv` | omitted, reason `desktop_only` |
| `survey`, `local_layer` | refused `print_context_unsupported` |

As on the desktop, a catalogue or derived layer that cannot be supplied is a
warning and an [`Omission`], never the end of the print; `Mode::Read` never
acquires and names `ds data project-cache seed --yes` / `--seed`. The document
is the kernel's `print_context_document` over the sheet-shaped capture area
used by project-cache seed (both page orientations, with the widest selected
metre buffer as a floor), its layers
in `BTreeMap` order, its `sources` naming every layer; `sha256` is what the
engine re-verifies and the run receipt records. A layer that would carry the
document past the engine's feature, byte or layer bounds cannot be supplied
either: the largest is omitted first (by bytes when the byte bound refused),
reason `print_context_too_large: …; <layer> alone carries N features`, until
the rest fits. The held coverage stays whole for smaller prints.

## Failure codes

`project_dataset_not_held`, `project_dataset_acquisition_failed`,
`dataset_provider_unavailable`, `project_dataset_local_source_required`, `reference_bundle_unavailable`,
`print_context_unsupported`, `print_context_too_large`, `catalog_invalid`,
`project_dataset_store_failed`. A shared seed ([`revisions::SharedFailure`]) adds
`project_dataset_revision_invalid`, `project_dataset_revision_stale`,
`project_dataset_publication_refused` and the host's own engine/surface codes
(`reporter_engine_missing`, `reporter_engine_incompatible`, `engine_refused`,
`engine_response_invalid`, `contour_surface_uncovered`). A command declares the
ones it can reach.
