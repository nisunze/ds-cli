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
| The provider door (ds-brain `data-distribution`), bundle bytes, credentials | the host, through [`Provider`] and the fetch it lends |

This crate decides nothing about geometry or document shape and never opens a
socket.

## The registry the kernel echoes

`datasets::{buildings_dataset, contours_dataset, catalog_dataset}` produce the
`Dataset` documents — byte-identical to the desktop's — whose digest is the
room address. Buildings and contours go through
`ds-brain:query_print_context`; a catalogue layer is a project subset of the
machine's installed national bundle (`desktop:reference_resource_cache`).
Contours are one room dataset carrying both kinds; the assembler splits them
back into `contours_index` / `contours_intermediate`.

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
| `google_open_buildings`, `elevation_contours` | the rooms through the host's provider |
| `project_dsgrid_mv` | omitted, reason `desktop_only` |
| `survey`, `local_layer` | refused `print_context_unsupported` |

As on the desktop, a catalogue or derived layer that cannot be supplied is a
warning and an [`Omission`], never the end of the print; `Mode::Read` never
acquires and names `ds data project-cache seed --yes` / `--seed`. The document
is the kernel's `print_context_document` over the design's buffered rectangle
(the widest selected buffer, through the kernel's metric buffering), its layers
in `BTreeMap` order, its `sources` naming every layer; `sha256` is what the
engine re-verifies and the run receipt records.

## Failure codes

`project_dataset_not_held`, `project_dataset_acquisition_failed`,
`dataset_provider_unavailable`, `reference_bundle_unavailable`,
`print_context_unsupported`, `print_context_too_large`, `catalog_invalid`,
`project_dataset_store_failed`. A command declares the ones it can reach.
