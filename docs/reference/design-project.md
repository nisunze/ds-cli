# `ds design project` — offline LV workspace

Owner: `ds-command-kernel/docs/design-workspace.md`. These commands require no
Desktop, map, sign-in or network. The project label is local attribution.
Publication remains durably `pending_transport` in this first slice.

## Complete snapshot

Save this illustrative draft as `transformer.json`:

```json
{
  "schema": "ds.design.snapshot/v1",
  "transformer": "T1",
  "crs": "EPSG:4326",
  "layers": {
    "tr": {"type": "FeatureCollection", "features": [
      {"type": "Feature", "id": "tr-1", "geometry": {"type": "Point", "coordinates": [30.0, -2.0]}, "properties": {"name": "T1", "names": "T1"}}
    ]},
    "lv_lines": {"type": "FeatureCollection", "features": [
      {"type": "Feature", "id": "line-1", "geometry": {"type": "LineString", "coordinates": [[30.0, -2.0], [30.0004, -2.0]]}, "properties": {}}
    ]}
  },
  "settings": {},
  "network_config": {"sheets": {"project_settings": [{"parameter": "print_individual_page_sizes", "value": "a3"}]}},
  "include_design_customers": true,
  "sources": []
}
```

Empty settings explicitly select engine defaults. Use the actual captured
project settings and configuration for a configured design. Geometry uses
WGS84 longitude/latitude; provided feature IDs are strings. Snapshot bounds:
64 MiB, 100,000 aggregate authored/source features, at most 64 layers/sources.

```bash
ds design project init --workspace ./design-work --project local-project
ds design project write --workspace ./design-work --input ./transformer.json --operation-id create-t1
ds design project process --workspace ./design-work --run-id draft-1 --transformer T1 --background
ds design project status --workspace ./design-work --run-id draft-1
ds design project result --workspace ./design-work --run-id draft-1 --transformer T1 --out ./result.json
```

Repeat `--transformer` for an ordered batch of 1–32. `--workers` sets the native
CPU budget. `process` with the same run ID and no selection resumes captured
work, never “all transformers.” Keep the matching executable until the run
finishes. New edits cannot change captured inputs. `cancel --run-id` stops
pending jobs at boundaries; start a new run after cancellation. Background
failures are also written to `worker.log`.

## Source datasets and precedence

Each `sources` item has `id`, `role`, `sha256`, and `collection`. Roles are
`customers`, `buildings`, `poles`. `collection` is the complete GeoJSON
FeatureCollection; `sha256` hashes its recursively key-sorted compact JSON.
Customer/pole sources require points; building sources use the existing
engine's building-to-customer conversion. URLs or browser tables are never
fetched during processing. `include_design_customers: false` requires an
explicit customer/building source, which may contain zero features.

The browser and CLI share this source precedence request:

```json
{
  "schema": "ds.design.source-resolution/v1",
  "kind": "poles",
  "project": {"addresses": [], "labels": []},
  "user": null
}
```

```bash
ds design project resolve-sources --input ./source-choice.json --out ./source-selection.json
```

`kind` is `customers` or `poles`; `project: null` means no declaration.
Addresses and labels are parallel arrays, at most 64 entries. Project
declarations win. An empty pole declaration disables borrowing; customer
declarations cannot be empty. Selected opaque IDs must still be resolved to
actual snapshot datasets by their host before processing.

## Atomic edits and history

`write` replaces an existing transformer only with `--expected <revision>`.
`read --transformer T1 --out snapshot.json` exports current state; optional
`--revision <sha256>` selects history. `edit` accepts:

```json
{
  "schema": "ds.design.edit/v1",
  "transformer": "T1",
  "expected_revision": "COPY_THE_CURRENT_REVISION",
  "operation_id": "edit-t1-001",
  "mutations": [
    {"kind": "set_properties", "layer": "lv_lines", "ids": ["line-1"], "values": {"note": "review route"}}
  ]
}
```

Other mutation shapes are `add {layer, features}`, `replace_geometry {layer,
id, geometry}` and `delete {layer, ids}`. New features receive deterministic
IDs and draft status. Authorable layers: `tr`, `customers`, `lv_poles`,
`tapping_poles`, `field_notes`, `lv_lines`, `service_cables`. Generated layers
cannot be edited. Any invalid member refuses the whole batch, bounded at
5,000 affected features. Use single-part geometry matching the target layer.

```bash
ds design project edit --workspace ./design-work --input ./edit.json
ds design project restore --workspace ./design-work --transformer T1 --revision <historical-sha256> --expected <current-sha256> --operation-id undo-001
ds design project outbox --workspace ./design-work --limit 20
```

Reuse operation IDs only for identical retries. Restore preserves history.
For outbox pagination, use the last sequence as `--after`; `more` reports
additional rows. No acknowledgement is exposed before governed reconciliation.

## Local reports and printable PDFs

```bash
ds design project report --workspace ./design-work --run-id draft-1 --transformer T1 --out-dir ./delivery-1 --country Rwanda --format xlsx --format pdf_a3 --admin-bounds ./rwanda.dsab --admin-bounds-sha256 <verified-sha256>
```

The reporter consumes the completed run and its captured configuration.
`pdf_a0`/`pdf_a3` produce vector PDFs without a map. Completed report bytes are
verified, retained locally and queued. Physical printer spooling is outside
this command. Missing references/media are explicit reporter blockers; partial
exports are not queued as complete. Choose a fresh output directory on retry.
