# External analysis and governed DS data exchange

Owner ruling, 2026-10-12: DS retains integrated transformer/LV/MV processing,
engineering rules, quantities, reports, approval and lifecycle. Every primitive
needed by those consumers stays, including electrical and WASM geometry, data
and ingestion, layers/tiles, and printing pens, styles, layouts and recipes.
General analysis orchestration belongs to an external executor outside DS Run.

## Execution ownership

DS has no general analysis DAG, recipe store, condition/iteration framework,
Python/Bash importer, arbitrary subprocess launcher or shell MCP. An agent or
operator invokes a verified external tool through that tool's established
interface on a workstation or isolated bulk executor. No external analysis
process, dependency or child job is embedded in the live Run request path.
The same geometry primitive can remain in DS when a retained domain consumer
needs it; retire its unneeded orchestration/exposure rather than deleting it.

Direct native design repair, revision comparison, import validation, apply,
approval, save and publication keep their existing typed contracts. An
external result is evidence and cannot grant project or design authority.

## Bounded handoff

1. Export the intentionally consumed source through its declared DS command.
   Pin project, principal, lane, object/member identity, immutable revision and
   byte/content digests. Reuse admitted held bytes. An unused source, cloud
   invalidation or push triggers no acquisition or freshness check.
2. Retain the original source and describe the external operation separately:
   executor/tool identity and exact version, parameter values, explicit input
   CRS and axis order, coordinate/measurement units, input byte digests and
   source IDs. Standard formats include GeoJSON (WGS84 longitude/latitude),
   GeoPackage and GeoParquet with explicit CRS metadata. Never infer metre
   units from geographic coordinates or discard source feature identities.
3. Invoke the external executor's own supported command. Record exit/status,
   output file format/schema, output CRS/units, hashes and feature identity
   relationships. A generated feature records its exact source relationship;
   IDs do not become interchangeable ordinals. Missing or partial output is
   named and cannot be called complete coverage or a successful import.
4. Validate through the existing native DS file/schema/domain admission before
   an import. Check identities, declared CRS/units, hashes, schema, bounds and
   source revision. Preserve printing/style metadata and unrelated properties.
   A compatible file alone is not an approved engineering or design change.
5. Import/stage through the declared governed DS command. Any design effect
   requires its normal explicit approval, project/principal/revision fences,
   allowlisted changes and persisted readback. Reject moved sources, ambiguous
   matching, dirty-work replacement and changed non-selected facts. An
   external process never calls raw project APIs or bypasses this boundary.

## Verified external interfaces

The [GeoLibre Rust toolkit](https://github.com/opengeos/geolibre-rust) publishes
`geolibre list`, `manifest <id>`, `version` and `<tool> --key=value` interfaces.
Its WASI file boundary is `/work`; its Python/JavaScript adapters supply input
bytes and return output files. Discover the actual installed tool manifest
before invoking it; browser/WASI availability does not imply a native CLI.

The installed workstation GDAL 3.12.2 exposes the documented
[vector pipeline](https://gdal.org/en/stable/programs/gdal_vector_pipeline.html),
[reprojection](https://gdal.org/en/stable/programs/gdal_vector_reproject.html)
and [buffer](https://gdal.org/en/stable/programs/gdal_vector_buffer.html)
commands. When an operation changes the geometry type, declare the resulting
layer type before writing through
[set-geom-type](https://gdal.org/en/stable/programs/gdal_vector_set_geom_type.html).
A successful exit with a geometry-type warning does not prove a conformant
output. Buffer distance uses source-layer georeferenced units; explicitly
project geographic input to an appropriate metre CRS first. Pin the installed
version and exact command/parameters in each job's provenance. These tools run
outside DS Run and remain external interfaces, not DS subprocess APIs.

The controlled local specimen in
`ds-network/out/external-analysis-boundary-20261012/roundtrip.json` pins the
installed tool versions, exact commands and every file digest. Native DS
exports one line; GDAL projects it to EPSG:32736, buffers six metres, declares
the Polygon layer and writes RFC 7946 GeoJSON. Native inspection, GeoParquet
conversion and bounded readback preserve its source ID/revision and label/pen
properties. The generated polygon retains a source relationship; source-line
measure attributes remain source facts. This proves a local file boundary,
not project design approval/publication or Cloud Run memory acceptance.

## Acceptance

Retained electrical/WASM/printing-pens behavior must pass owning tests. Retired
workflow commands must be absent from CLI/MCP discovery and refuse through
routing. Qualify one real DS export → external command → native validation /
governed local import roundtrip without project design/submission writes.
Ordinary Go Run at 128 MiB is settled (owner, 2026-10-12); this removal does
not reopen its memory qualification or alter the admitted runtime budget.
Existing dispatch-refusal tests pin placement without a new container campaign.
Runtime activation and external/import acceptance remain separate from source
removal.
