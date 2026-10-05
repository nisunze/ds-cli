# LV publication standard v1

The registry names `lv-transformer-a0-v1`, `lv-transformer-a3-v1`,
`lv-combined-a0-v1` and `lv-combined-a3-v1`. Each is a governed API document,
not a code-built layout. A3 has an independent composition. Each combined set
opens with six front-matter roles on its own paper (A0 for the A0 set, A3 for
the A3 set): cover, key map, index, legend, notes and revisions.
Canonical transformer order and numbered schedule continuations follow;
standalone PDFs include their continuations. Workbook-owned projections
retain all schedule fields. A smaller paper never silently drops columns.

`ds.print-standard/v1` captures the explicit project, immutable registry and
page-document revisions, exact Layout digests, logo slots and semantic layer
keys. Layouts own furniture, font sizes, table typography and panel policy.
There are no standard-builder colours, fonts, widths or generated style ids.
Identity pins parties, contract, model/version, issue/submission, revision,
issue date and logo PNG digests. Every page has one mandatory publisher credit.

Style choice belongs solely to `style_resolution::prepare_print` from
print-styles. The host asks it with explicit entity, source, target and role
keys; API scope precedence is already captured. `resolved_styles::consume`
validates that held answer against the requested keys, project and immutable
content, then forwards its refs and documents unchanged. It is the isolated
integration boundary until the API acquisition path is wired. LV creates,
edits and seeds no style documents or catalogue entries. Test captures are
synthetic API doubles, never database defaults or evidence of live seeding.

Required combinations: LV transformer, line, pole, customer, service cable and
tapping pole × actual live_geojson/vector_tile/cold_existing/saved_design
sources × print focused/context; LV transformer × print project for the key
map; MV line/pole × actual dataset_geojson/cold_existing/project_model sources
× print context; buildings, contours, roads and rivers × actual held sources
× print context. Context documents suppress labels and mute their own pens;
only focused rows feed quantities and schedules. Lifecycle and cable/structure
vocabularies stay declarative feature expressions in the resolved documents.
Buildings stay green in colour. Pen/map-scale scaling, fonts, halos, casing,
dash and emphasis are API document authority, never local substitutions.

Ink mode requires an API-selected colour/monochrome capture for the same keys.
The four-dimensional resolver contract does not yet declare ink selection;
print-styles must provide that capture qualifier or extend its owning contract.
No consumer implements a monochrome palette or hashes category pens.

`ds.print-standard-overrides/v1` pins template_version 1, allows bounded issue
text and existing exact-transformer furniture patches, and refuses unknown
versions/fields. There is no attribution override. The kernel's standard_layout
operation consumes held documents on native and WASM. Reporter export_lv_standard
writes each PNG before its vector PDF, standalone and mixed-paper combined PDFs,
XLSX/SHP companions and digest receipts. It publishes nothing. Missing optional
context is a recorded omission; empty workbook schedules are omitted.

Named refusals cover template missing/ambiguous/revision mismatch, identity/logo,
override version/fields, source/roster, style unknown/revision mismatch/project
context, schedule overflow and an existing output directory. No fallback changes
a template, source, role or pen after a refusal.

Reuse PageRole, Identity, Templates, registry, attribution and the held-style
consumer for MV and other print families. Reporter PDF collection/resource
compaction preserves vector pages and mixed sizes. Georeference dictionaries
and switchable PDF layers remain listed limitations. The merge gate is a
baseline/branch page raster comparison with thumbnails and a feedback/rule
trace for every intentional difference.
