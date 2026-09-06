# Printing document authority

`printing::Layout` is a closed `ds.print-layout/v1` document. Page dimensions and
frames use millimetres; typography uses points. It contains one map frame,
text/legend/scale/north/logo/rectangle elements, held PNG assets and keyed
physical cartographic styles. The generated JSON schema is `layout_schema()`.

`printing::evaluate` evaluates New, Validate, Edit (upsert one element), or
Remove. Every result contains `layout`; validation additionally returns
`valid:true`. Invalid edits produce no partial document. CLI and the web WASM
export `printingCommand` call the same evaluator. `command_schema()` describes
the closed intent grammar. The native renderer can validate the same Layout.

Spatial hit testing, snapping and retained camera sessions remain in ds-network;
they are not command-kernel WASM exports. PDF/SVG formatting, GeoJSON projection
and legend rendering belong to ds-network-reporter. Brain owns global/project
sharing and expected-revision saves. This module owns no storage or network IO.

`new` accepts optional `id`, `name`, `paper` (A0–A5, default A3) and
`orientation` (portrait/landscape, default landscape). Paper presets uniformly
fit the initial composition. `rename` changes only the name; `duplicate`
requires a different ID and carries the composition into an independent layout.
CLI `report.layout.edit --request FILE` and web authoring use these same intents.
