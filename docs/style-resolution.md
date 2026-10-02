# Governed style resolution

Status: `ds.style-resolution/v1`, owner ruling 2026-10-02.

Style identity declares `entity_class`, `source_kind`, `target` (`screen` or
`print`), `role`, and optional paper `ink`. Ink is `colour` by default or
`monochrome`; omitted and explicit colour are the same key. Colour omission
preserves the original four-dimensional wire representation. Monochrome
requires print and binds an independent authored document; it never invokes
consumer recolouring. Source kinds are declared data;
the standard set uses `live_geojson`, `vector_tile`, `cold_existing`,
`dataset_geojson`, `saved_design` and `project_model`. A print preparation uses
`focused`, `context`, `mv_context` or `project` as declared by its template.
`context` around another LV transformer and `mv_context` beneath an MV route
have different authority. Lifecycle remains a feature attribute, not a role.

`style_resolution::resolve(snapshot, key)` is the only choice function. It
selects exactly one binding in an authorized API snapshot and returns its
governed document id, immutable revision, content digest, scope and complete
document. It constructs no id, copies no sibling style and invents no pen.
Unknown and ambiguous tuples are refused by name. A mismatched document digest
is refused before rendering. The WASM `styleResolve` export calls this function;
the native CLI and print preparation call the same function.

The snapshot is `{"schema":"ds.style-resolution/v1","project_id",`
`"revision_id","seed_version"?,"bindings":[...]}`. Each binding contains
`key`, `style_ref`, `revision_id`, `content_sha256`, `scope`, `document` and
optional `presentation_tier`. The generated `request_schema()` is authoritative.
The API captures the explicit project and resolves exact scoped documents
before global documents. Consumers never apply their own scope precedence.
Changing project during an asynchronous read refuses the stale snapshot.

The seed manifest is declarative, independently versioned and available through
the Styles API. Its complete documents and exact tuple bindings are data.
Seeding creates missing documents with immutable version records and preserves
authored heads. The reviewed manifest may also declare an exact migration with
the observed head and original content digest. Today's 37 legacy paper-scaling
migrations archive the original document and persist the previously effective
all-one factors; they change no rendered size. A changed head refuses the
reviewed plan. No live/tile/print sibling follows another document.
Installation initializes the standard set and applies these reviewed migrations
through the authorized idempotent API operation before upgraded consumers are
released. An unmigrated baseline refuses resolution rather than enlarging pens.
A packaged preview is not a persisted document.

`prepare_print(snapshot, layer_keys)` consumes explicit template keys and returns
held `style_refs`, `print_styles`, `style_revisions` and `resolution_revision`.
The native renderer consumes those held documents unchanged and performs no
mutable style lookup. Paper/map-scale scaling, fonts, labels, halos, casing,
dash patterns and focus/context emphasis belong to the governed style or
printing document. Table typography belongs to the exact governed Layout.
Explicit declarative overrides remain supported; code never substitutes an
asset- or project-specific pen. Existing exact `Layout.style_refs` are legacy
authored bindings; migration to semantic keys is an API document operation.

Screen ordering uses the API's declarative presentation tier. Casing remains
beneath its primary line and labels remain its companions. Saved `user_order`
remains authoritative. There is no style-id classification in a renderer.

Retirement inventories catalogue and project-scoped documents with their exact
routing and revisions. An omitted manifest binding does not make a document
obsolete. Only explicit obsolete/duplicate declarations propose replacements.
A dry-run plan requires an immutable backup and names dependencies or incomplete
dependency review. This slice performs no live deletion. Main-session deletion
must use the API after owner approval and re-check the observed heads.

The print merge gate renders the standard fixture set on the baseline and this
branch, compares every page raster, and records thumbnails and each difference's
feedback id or owner rule. An unexplained difference is a regression, not an
accepted default change.
