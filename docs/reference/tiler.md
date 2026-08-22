# `ds tiler` — reference

Tier-4 reference. `ds tiler workspace --help` is the executable contract.

## One local compute operation

`ds tiler workspace` is the desktop/CLI door for a sealed network tiling
workspace:

```bash
ds tiler workspace --workspace /absolute/path/to/workspace --output json
```

It calls exactly:

```text
ds-vector-tiler workspace-tile <canonical-workspace-root>
```

Nothing else is accepted. There is no source path, output path, map layer,
tool path, URL, credential, Cloud Run flag, or generic argv. This makes it a
different subject from `ds map` (the paired application's live MapLibre state)
and from `ds dsgrid` (a canonical `.dsgrid` model).

The native engine reads only `snapshot/tiles.json` below the supplied root,
hash-verifies every declared snapshot input, runs the two pinned local tools,
and writes no-clobber PMTiles/result artifacts below `artifacts/tiles/`.
It does not start its HTTP listener, upload, or call Cloud Run.

## Desktop additions

The tiler process requires Tippecanoe and PMTiles. `ds` resolves them only for
the tiler child, in this order:

| Addition | Explicit developer override | Packaged location |
|---|---|---|
| Tippecanoe | `DS_VECTOR_TILER_TIPPECANOE_BIN` | `tippecanoe` beside `ds-vector-tiler` |
| PMTiles | `DS_VECTOR_TILER_PMTILES_BIN` | `pmtiles` beside `ds-vector-tiler` |

Overrides must be absolute paths. `ds` does not search `PATH` for additions,
so an unrelated system binary cannot silently outrank the packaged one. On
Windows the packaged names have the normal `.exe` suffix.

The sealed `snapshot/tiles.json` manifest carries each addition's exact version
output and SHA-256 pin. The engine itself verifies the bytes, runs
`tippecanoe --version` and `pmtiles version`, and compares those outputs before
it stages any source input. `ds` does not copy the pins or perform a second
tiling implementation.

`DS_VECTOR_TILER_BIN` is the normal explicit development override for the
engine. Otherwise `ds` finds a bundled sibling of its executable, then `PATH`;
the additions remain sibling-only or explicit even if the engine was found on
`PATH`.

## Result boundary

The engine must emit a `ds-vector-tiler.workspace-tile-result/v1` JSON
document. `ds` refuses any result unless all of these are true:

- `operation` is `workspace-tile` and the input schema is the sealed workspace
  schema;
- execution attests `location: local` plus `remote_execution: false`,
  `cloud_run: false`, and `upload: false`;
- the result is local-only and does not register a serving pointer;
- a successful run has exactly one PMTiles artifact whose logical name and
  workspace-relative selector are derived from the output name;
- each returned artifact/tool digest is `sha256:` plus 64 lowercase hex
  characters.

The `ds` result is a bounded projection. It returns `result_manifest` and
`artifacts[].path` as workspace-relative logical selectors, never an absolute
artifact path. The caller already knows the root it supplied and may resolve
those selectors only within that workspace.

## Failure handling

| Code | Meaning |
|---|---|
| `tiler_engine_missing` | The native tiler is unavailable. |
| `tiler_addition_missing` | A pinned desktop addition is absent or its override is not absolute. |
| `workspace_not_absolute` / `workspace_not_found` / `workspace_not_directory` | The one permitted input root is invalid. |
| `engine_refused` | The native tiler rejected the manifest, input/tool pin, or no-clobber output state. Its bounded stderr is in `detail.engine`. |
| `tiler_contract_mismatch` | A zero-exit response claimed something other than the required local contract. Do not replace it with an HTTP call. |

Shared serving-pointer publication, if it is ever requested, is a separate
authority-owned operation. It is not a fallback or follow-up inside this
command.
