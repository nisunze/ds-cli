# Project printing preparation

Use `ds desktop printing list --scope project --output json` to inspect the
active project's dynamic catalog, or `--scope global` for shared samples. Read
one exact authored document and its current optimistic revision with
`ds desktop printing get --scope project --id <id> --output json`.

`ds desktop printing prepare --request setup.json --yes --output json`
uses the paired application's active project and normal Brain-authorized saves.
It is also available through the generated MCP surface. It does not use a
native-client profile or change another command's authentication requirements.

The request contains `layout` (the `ds.print-layout/v1` document),
`expectedRevision` (empty when creating), and `formats` (for example
`["pdf__huye-cjic", "xlsx", "gpkg"]`). The named PDF must match the layout id.
Use `ds report layout schema` for the document grammar.

Preparation saves the project layout and export selection, refreshes sealed
inputs, and prepares the reference/photo caches. Saves are durable even if a
subsequent cache installation fails; a revision conflict is never overwritten.
The shared Printing setup page can inspect and edit the resulting layout.

Then use `ds map design report --transformer agasharu --yes --output json`.
Outputs appear in the transformer's Report Files inventory and PDF preview.
The CLI returns artifact evidence without downloading a ZIP.
