# Project printing preparation

Use `ds desktop printing list --scope project --output json` to inspect the
active project's dynamic catalog, or `--scope global` for shared samples. Read
one exact authored document and its current optimistic revision with
`ds desktop printing get --scope project --id <id> --output json`.
Publish a shared sample or a project-only layout with
`ds desktop printing save --request setup-save.json --yes --output json`; the
request contains `scope`, `layout`, and `expectedRevision`.

`ds desktop printing prepare --request setup.json --yes --output json`
uses the paired application's active project and normal Brain-authorized saves.
It is also available through the generated MCP surface. It does not use a
native-client profile or change another command's authentication requirements.

The request contains `layout` (the `ds.print-layout/v2` document),
`expectedRevision` (empty when creating), and a versioned `selection`. Paper
and file format are independent:

```json
{
  "schema": "ds.design-output-selection/v1",
  "prints": [{"layout_id":"huye-cjic","enabled":true,"formats":["pdf","jpeg"]}],
  "geospatial": ["gpkg"],
  "tabular": ["xlsx"]
}
```

At least one selected print must match the saved layout id. Legacy `formats`
tokens remain readable and are upgraded to this document on save. Use
`ds report layout schema` for both the layout and output-selection grammar.

An optional `views` object binds transformers to this layout's named viewport
slots. For example, `{"agasharu":{"primary_map":{"mode":"camera",
"center_wgs84":[29.7,-2.5],"scale_denominator":1000,"rotation_deg":4}}}`
pins Agasharu while other transformers continue to fit automatically. These
bindings are merged under the saved layout id; they do not clone or mutate the
paper template.

Preparation saves the project layout and export selection, refreshes sealed
inputs, and prepares the reference/photo caches. Saves are durable even if a
subsequent cache installation fails; a revision conflict is never overwritten.
The shared Printing setup page can inspect and edit the resulting layout.

Then use `ds map design report --transformer agasharu --force --yes --output json`
to replace the current committed artifact batch from the Desktop's local room.
Local force does not require the Cloud Run force password.
Outputs appear in the transformer's Report Files inventory and PDF preview.
The CLI returns artifact evidence without downloading a ZIP.
