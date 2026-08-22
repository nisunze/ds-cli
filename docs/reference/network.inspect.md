# `ds network inspect` — reference

Tier-4 reference. `ds network inspect --help` is the contract; this document
explains the parts a contract cannot.

## What it is for

A `.dsgrid` package is a complete authored grid model — canonical tables in
Arrow IPC, referenced engineering assets, and a manifest attesting to both.
Printing all of it would be tens of megabytes and would not answer the
question anyone actually has, which is almost always one of:

- *Which model is this file?*
- *Is it the revision I think it is?*
- *Does it contain enough to do the thing I am about to do?*

`inspect` answers those first, cheaply, and lets you ask for the rest by name.

## The cost model

This is the part worth understanding, because it is why calling `inspect`
before anything else is free.

A `.dsgrid` is a zip. Its `manifest.json` member already carries the model's
identity, per-table row counts, per-table fingerprints, and a record for every
member. Reading it requires **no Arrow decode at all**.

| Projection | Source | Decodes tables |
|---|---|---|
| *(default)* identity + inventory | manifest | no |
| `--include tables` | manifest | no |
| `--include members` | manifest | no |
| `--include library` | decoded snapshot | **yes** |
| `--include extent` | decoded snapshot | **yes** |

The response reports which happened:

```json
"decoded": false
```

So a script that walks a directory of packages to find one model can do so
without decoding any of them.

## Output

### Default

```json
{
  "path": "/…/humble-pole.dsgrid",
  "byte_len": 71768,
  "format": "dsgrid",
  "model": {
    "id": "pls-import-fnv1a64:f091cb70",
    "revision": 0,
    "crs": "EPSG:32735",
    "format_version": 1,
    "schema_version": 1,
    "fingerprint": "fnv1a64:f091cb7021191169"
  },
  "inventory": {
    "populated_tables": 35,
    "total_rows": 138,
    "package_members": 60
  },
  "decoded": false,
  "more": {
    "available_projections": ["tables", "members", "library", "extent"]
  }
}
```

`model.fingerprint` is the snapshot fingerprint computed by `ds-grid-model`.
Two packages with the same fingerprint hold the same authored content. It is
the right field for a cache key or an idempotency check.

`model.revision` is the authored revision. It is monotonic within a model id
and says nothing about any other model.

### `--include tables`

Row counts per canonical table, populated tables only — an empty table is
absent rather than reported as zero. Table names are the canonical serde
tokens from `ds-grid-model`, so a table renamed upstream is renamed here.

### `--include library`

The invariant names of the model's structure types, cables and resource
leaves — the vocabulary the model is authored against. Useful before a
conversion or a composition, to see whether the library a model expects is the
one you have.

### `--include extent`

Bounding boxes in the model's own CRS (metres), separately for route nodes,
structures and terrain. `null` where the model has no finite point of that
kind — which is itself worth knowing.

### `--include members`

The package's member inventory with byte lengths. Mostly a forensic tool for a
package that will not open.

## Bounds

- Files above **512 MiB** are refused before being read (`model_too_large`).
- `--limit` caps every listed collection; default 50, maximum 5 000.
- Truncation is reported in `more.truncated`, naming the field and the count
  withheld. A list is never silently shortened.

## Ownership

`ds` computes none of this. It reads bytes and calls:

- `ds_grid_exchange::dsgrid::inspect` — manifest and member inventory
- `ds_grid_exchange::package::unpack` — full verified decode
- `ds_grid_model::GridModelSummary::for_snapshot` — the summary

Those are the same functions the DS GridDesign desktop links. There is no
second implementation of the `.dsgrid` format in this repository, and there
must not be one: two readers with two tolerances disagree silently, and the
caller receives a different answer rather than a disagreement.

## Related

- `ds network inspect --help` — the contract
- `docs/contracts/cli-output-contract.md` — envelope and exit codes
- `ds-network/docs/contracts/` — the `.dsgrid` format's own contracts
