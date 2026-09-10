# Project printing preparation

Start with `ds desktop printing settings --project <exact-id> --output json`.
It reads project settings and reports the saved selection, planned output IDs
and the actual selected template papers. It never infers paper from a filename.
The same leaf is exposed by `ds mcp serve --exposure commands --profile printing`.

Named setups are read and published natively, with no paired application:
`ds report layout list --scope project --output json` inspects the selected
project's catalog (`--scope global` for the shared samples), `ds report layout
get --scope project --id <id> --output json` reads one authored document with
its current optimistic revision, and `ds report layout save --scope project
--request setup-save.json --yes --output json` publishes one — the kernel
decides from the request's `expected_revision` whether that is a create or an
update, the same decision the Printing setup page makes.

`ds desktop printing prepare --request setup.json --yes --output json`
requires `project` in the request and uses normal Brain-authorized saves without
reading or switching the project shown in the paired application. It is also
available through the generated MCP surface. It does not use a native-client
profile or change another command's authentication requirements.

The request contains `project`, `layout` (the `ds.print-layout/v2` document),
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

Read `ds desktop printing settings --project <exact-id> --output json` again
to verify the saved selection. Then use `ds desktop printing export --project
<exact-id> --transformer agasharu --force --yes --output json`
to replace the current committed artifact batch from the Desktop's local room.
Local force does not require the Cloud Run force password.
Outputs appear in the transformer's Report Files inventory and PDF preview.
The CLI returns artifact evidence without downloading a ZIP.
