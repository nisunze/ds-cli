# Project printing preparation

Start with `ds desktop printing settings --project <exact-id> --output json`.
It reads project settings and reports the saved selection, planned output IDs
and the actual selected template papers. It never infers paper from a filename.
The same leaf is exposed by `ds mcp serve --exposure commands --profile printing`.

`ds desktop printing preview --project <exact-current-project> --transformer
<name> --layout draft-layout.json --output json` renders a full draft sheet
through the native report host. Alternatively, supply `--template location.json`
with an exact layout location from the native catalogue; choose exactly one of
`--layout` and `--template`. The native owner validates its kind, identity and
revision, reads that chosen template through its authenticated retained cache,
and resolves held governed styles and the transformer's shared room. Missing
geographic inputs remain named in native omissions; preview does not acquire
geographic provider data or cloud computation.

The native host persists SVG bytes in its scoped disk cache before transport.
Contract 2 returns a `ds.print-preview-artifact/v1` receipt with its absolute
path, SHA-256, byte count, content type, exact result and authority digests,
creation and expiry times, plus native diagnostics, warnings and omissions.
CLI responses contain no inline SVG. Retention defaults to 24 hours;
`--retention 7d` selects seven days. Repeated exact requests reuse the original
file and expiry without rendering or acquiring data. Optional `--out
/absolute/new.svg` copies the validated artifact without replacing an existing
file. The Desktop reads the same admitted file for presentation. A mismatched
older Desktop response refuses and requires a matching CLI/Desktop package.

The project must match the paired Desktop's current project; account or project
changes refuse. Layout input remains bounded to 800 KB and response metadata
to 8 MiB. Rendering persists only preview artifacts: it does not save layouts,
export selections, main outputs or publication. The headless
`report project export --preview-layout draft-layout.json` uses the same native
artifact owner; `--preview-retention 24h|7d` controls that artifact's retention.
This draft SVG operation is separate from `export --intent preview`, which
produces review artifacts from saved project layouts.

An explicit export selection containing only named PDFs does not add saved
voltage-drop JSON or check that unselected analysis. Such a member and batch
complete when their selected PDFs complete. Default exports still include
saved voltage-drop JSON; selecting it explicitly retains its missing-analysis
refusal. The A4 voltage-drop report keeps its existing saved JSON companion.

`ds desktop printing view --project <exact-current-project> --output json`
reads the open Printing templates view's actual location and held page count.
Open that view with `ds map ui open --target project-printing --ref <project>`.
`desktop printing view --project <id> --catalogue` includes the already-held
native groups, preview subjects and exact location pins without a catalogue
read. Copy one `.location` object from that response (or from the headless
`report layout list --catalogue` response) into a JSON file, then consume it with
`desktop printing view --project <id> --location location.json`. Pins remain
exact, including governed document revision and content digests. Another
template cannot replace a dirty draft; save or explicitly discard that draft
first. The first intentional selection may read or acquire that exact pinned
document through its existing native template cache. With a document open,
`--page <n>` changes its held PDF camera. Select a
template and navigate its pages in separate calls. The current page count is
the held document's count, so an MV template sample reports one page rather
than claiming the full atlas is present. Reads and page turns acquire no
catalogue, style, design, geographic context or renderer result. Existing
native preview jobs continue independently of this visible navigation; it
issues no independent geographic, style, design or render acquisition.

Named setups are read and published natively, with no paired application.
Project scope always names its project; the saved selection is never read.
`ds report layout list --scope project --project <id> --output json` inspects
the named project's catalog (`--scope global` for the shared samples),
`ds report layout get --scope project --project <id> --id <setup> --output json`
reads one authored document with its current optimistic revision, and
`ds report layout save --scope project --project <id> --request
setup-save.json --yes --output json` publishes one — the kernel
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
inputs, and prepares the reference/photo caches. These are sequential durable
actions: Brain validates and saves the layout and project settings first, then
the application refreshes the sealed receipt and installs the required
reference data. Saves are durable even if a subsequent cache installation
fails — a later preparation failure never rolls back a saved layout — and a
revision conflict is never overwritten. The shared Printing setup page can
inspect and edit the resulting layout.

Read `ds desktop printing settings --project <exact-id> --output json` again
to verify the saved selection. Then use `ds desktop printing export --project
<exact-id> --transformer agasharu --force --yes --output json`
to replace the current committed artifact batch from the Desktop's local room.
Local force does not require the Cloud Run force password.
Outputs appear in the transformer's Report Files inventory and PDF preview.
The CLI returns artifact evidence without downloading a ZIP.

Add `--intent preview` to produce local-only review artifacts. Preview artifacts
do not replace main outputs, enter the publication queue, or save project
settings. Omit the flag for the ordinary main export. Preview accepts individual
transformers, including a batch; `combined_transformer` requires a main export.

To verify or copy a retained preview, add `--intent preview` to
`ds desktop printing artifact read` or `ds desktop printing artifact copy`, using
the export receipt's exact project, transformer and output id. Omit the flag to
read main outputs. An unavailable preview does not fall back to a main output;
copies still require a new destination and verify the sealed bytes.

For one A4 voltage-drop PDF without regenerating other formats, pass a local
selection file containing
`{"schema":"ds.design-output-selection/v1","geospatial":["voltage_drop_pdf"]}`
to `ds desktop printing export --selection <file>`. This PDF uses local
Chromium printing and does not require a named map print layout. Its filename is
`<Display Name> voltage drop.pdf`; `paper_size` in the receipt is `A4`.
The saved project selection chooses `voltage_drop_report: "extended"` by
default, or `"brief"` when explicitly set.
Web execution does not generate this PDF; after local publication the Web app
can preview and download it.

## Map export

`ds desktop printing map export --request recipe.json --yes --output json`
takes an administrative-code planning recipe (custom-area or district MV
pages, or one combined MV overview) and renders it in the matching signed-in
desktop through the native Reporter; cloud rendering never occurs. Project
Control retains the local artifact and its source warnings, replacing only
the selected paper. Completed printouts attach automatically through the
existing report-artifact channel; when the desktop is disconnected or the
publication fails, the artifact stays pending locally until it can publish.
To render an existing `ds.print-layout-export/v1` request headlessly, use
`ds report layout render`; `ds map print schema` lists the planning fields.
