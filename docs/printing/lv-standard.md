# LV publication standard v1

`printing::standards::registry()` owns the four starting templates:
`lv-transformer-a0-v1`, `lv-transformer-a3-v1`, `lv-combined-a0-v1`, and
`lv-combined-a3-v1`. A3 has its own measured composition and readable schedule
type. The combined collections begin with six A0 pages: cover, key map,
sheet index, legend, general notes, and revision table. Network sheets follow
in canonical transformer order. Dense schedules produce numbered continuation
sheets; standalone transformer PDFs include them too.

Identity is explicit: project ID and display label, client, contractor,
consultant, contract, model version, issue, submission, revision, fixed issue
date, and held PNG logos with SHA-256 digests. Submission is independent of
model revision. Rendering never guesses a date, fetches a logo, or changes
engineering identities. Every page has one compulsory publisher credit.

Focused layers alone supply labels, quantities and schedules. Adjacent
circuits are muted, unlabelled context. Semantic paper pens separate existing,
new and unclassified infrastructure, cable sizes and pole types. A pinned
project-wide sorted category vocabulary keeps those pens consistent across
sheets. Buildings remain green in colour; monochrome preserves lifecycle
differences through weight, dash and hollow symbols. Supplied contours retain
their whole held geometry and are clipped by the map frame.

`ds.print-standard-overrides/v1` pins `template_version: 1`. It permits
`ink_mode`, bounded `title_block` issue/submission/revision/date and author
fields, and exact-transformer furniture patches using the existing
`PrintOverride` schema. Unknown fields and versions refuse. Scoped overrides
also live in the versioned layout document through `set_transformer_override`;
the ordinary expected-revision update transaction saves them headlessly.
The registry publishes the complete identity and override JSON schemas.

Hosts invoke `standard_layout` and `standard_pens` through the same printing
command boundary used by WASM. The reporter's `export_lv_standard` local task
owns file generation: preview PNG, vector PDF, mixed-paper collection,
tabular/geographic companions and SHA-256 receipts. It does not publish.
Missing optional context is listed in the receipt. Missing pole/customer
layers print empty schedules. Refusals name `print_job_version`,
`print_identity_invalid`, `print_logo_invalid`, `print_override_version`,
`print_override_invalid`, `print_roster_invalid`, `print_source_invalid`,
`print_palette_invalid`, `print_schedule_invalid`, or `print_output_exists`.
Rendering errors preserve their original diagnostic behind `export_blocked`.

Reuse `PageRole`, `Identity`, the schema registry and compulsory attribution
for other print families. PDF hosts should use reporter `print_attribution`
resource deduplication and `pdf_collection` assembly to retain vector geometry,
paper sizes and a single credit. MV layout/engineering decisions stay with
their owning slice. Georeference dictionaries and switchable PDF layers are
not part of this version.
