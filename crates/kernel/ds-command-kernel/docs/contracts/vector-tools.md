# Vector tools and external analysis boundary

The Kernel's `vector_tools` module (feature `vector-tools`, carried by the
WASM build) is the one application owner of the vector tool catalogue, JSON
Schemas, defaults, admission, refusals, output fields and runs. The catalogue
has 94 geometry tools; 88 are roadmap descriptors. The six executable geometry
tools are buffer, sample, intersect, measure, outliers and random-points-area.
A separate collisions descriptor reads the existing reporter's output; it does
not run project collision detection. Geometry algorithms keep their library
owners: `ds-geo-lite` primitives, `ds-geo` for the layer wire and outlier
statistics, and `ds-io` for sketch assembly. Core runs no vector tools.

Every call is a closed typed operation, `VectorToolOperation`: `prepare`,
`run`, `merge_layers`, `point_coordinates`, `export_geojson` and
`sketch_features`. A tool is a `VectorToolId` (the catalogue command; the
descriptor id is an accepted alias); `vectorToolId` admits a selector and
refuses unknown and roadmap tools by name. Browsers run operations in the
compute worker as the declared `vector-tool` request kind. The worker and the
caller both decode the reply with `decodeVectorTool`, which refuses a reply for
another operation, tool or mode, a malformed receipt, missing or unexpected
result bytes and an outlier report outside its schema, while accepting additive
fields. A selected Kernel without these exports is refused as
`vector_tool_unavailable`; nothing falls back to the Network engine.

The data plane is `ds_geo::layer::Layer`. Feature data crosses only as Arrow
IPC files with the `__ds_feature_id`, `__ds_geometry_wkb` and attribute
columns; the Kernel uses the `ds-geo/browser-polars-ipc` backend. Nested
attribute cells carry `ds.property_kind=json` field metadata. Geometry WKB is
XY; elevation is a scalar `elevation_m` column. Invalid geometry retains its row
as null geometry. IPC accepts at most 20000 rows and 32 MiB.

`prepare` is the explicit request-file import: it admits the authored document,
converts inline GeoJSON ports to IPC once and returns the normalized request
without a data twin. `run` refuses inline feature data. JSON control includes
descriptors, requests, receipts, reports and dry-run previews; a preview samples
at most five whole features and 32 KiB of feature data. Receipts carry
`result: null`; the layer travels beside them. Large results receive
`render_mode=vector_tiles` and use the existing file-upload/vector-tile
pipeline; `sketch_features` is the MapLibre rendering boundary.

JSON the module emits keeps sorted object keys, including object-valued
attributes, so catalogues, receipts and GeoJSON files read byte for byte the
same on every host. `tests/fixtures/vector_tools_network_parity.json` pins the
former Network exports' outputs; receipts omit its retired `written_to` field
and the `null` lists a dry run used to insert.

The collision region summary has one owner, `vector_tools::collisions`; the
browser collisions card reads it, including each phase's presentation keys,
from the run receipt. The command kernel's `collisions` module keeps only
project-status counts and action messages.

## External analysis

DS retains every geometry, measurement, indexing, import, layer, tile and
WASM primitive required by transformer/MV processing, quantities, engineering
rules, reports and printing. A shared primitive does not become removable
because an independent analysis tool also called it. Printing styles, pens,
layouts and recipes keep their existing authorities.

General multi-step analysis runs outside DS Run through the external executor's
own declared interface and standard files. There is no custom DS analysis DAG,
condition/iterator engine, recipe store, script importer or arbitrary shell
MCP. Direct governed `design.repair.*` commands retain their own validation and
approval boundaries. They do not need a general workflow wrapper.

The [external analysis contract](ds-workflows.md) requires exact source
identity/revision/digests, explicit CRS and units, tool version/parameters,
output provenance and validated approved import. Results are evidence; they
cannot bypass native domain admission or author a design.
