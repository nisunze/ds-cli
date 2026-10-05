# Style authoring

`ds style` reads and writes governed style documents through the native client.
It requires a native sign-in and `--project <exact-id>` on every call (the saved selection is never read); no desktop, window or map is
involved. Use `--lane stable|canary` to choose the deployment. Catalogue reads and
publication need the backend to be reachable.

`ds style catalogue binding plan --project <id> --entity-class <exact-class>
--source-kind <exact-kind> --role <exact-role> --ink colour|monochrome --ref
<existing-print-ref>` reviews one print tuple binding to an existing `_print`
style. Copy the exact tuple and ref from the blocked print layer and its actual
style catalogue; the command supplies no guessed source, role or style body.
The plan captures the authenticated project/principal, current manifest and
style head/content digests, update time and create/preserve counts. Run the
same arguments with `binding create --expected-manifest <manifest_revision>
--expected-plan <plan_sha256> --yes` to create that one binding. A changed head
or manifest refuses atomically; an exact existing tuple is preserved and a
conflicting tuple/ref refuses. The API retains the previous manifest. This
operation requires `styles.edit` and changes no style body, print layout,
printing default or global page. Templates use ordinary project context.

There is one route, so a server answers exactly as a desktop does. The Style
Center in the application authors the same documents through the same Rust
command kernel compiled to WASM; what differs is who is typing, not what is
decided.

Start with `ds style list --project <id>`, then `ds style read --project <id> --ref <returned-ref> --output json`.
Read returns the complete authored document, backend field vocabulary and domains,
property bounds, a bounded icon list and supported second-dimension channels.
`more` reports truncation; runtime feature counts and map visibility are not inferred.
Only backend-published editor refs can be authored from the headless catalogue.

Screen and print are independent governed documents. For print work, resolve a
catalog ref ending in `_print` whose target is `print`; the bare and `_vt` refs
remain interactive-map styles. Creating a print clone is a one-time seed, not
ongoing synchronization. Subsequent appearance, label and cartography edits
address the selected ref only. Project pen overrides belong to the print layout.
Verify both the printed result and the unchanged screen counterpart after a
print-only repair. The live-map canvas does not preview physical print output.

Renderer-only buildings and contour sources are declared by the backend under the
closed `print_context/*` family. `ds style seed plan --project <id> --ref <declared-ref>` returns
that exact backend document and create-only payload; `ds style seed create
--ref <declared-ref> --yes` publishes it once. Arbitrary refs and every other style
target are refused by the shared Rust planner. After seeding, use the ordinary
guided commands to edit the source and `ds style print` to create its `_print`
variant.

`ds style print plan --project <id> --ref <screen-ref>` derives the predictable
`<screen-ref>_print` identity and shows the exact create-only clone. `ds style
print create --ref <screen-ref> --yes` publishes it. A variant is created once:
when the catalog already lists `<screen-ref>_print`, both commands refuse with
`style_exists` before any write (ds-brain would answer a second create with 409)
and name the ref to read and customise instead. Catalog sprite names are
preserved; runtime image IDs are normalized back to their authored icon names.
The print editor adds the reserved string field `print_paper_size` with
`print_a0` through `print_a5` and `print_custom`. Use ordinary `style dimension`
or Style Center controls to make widths, sizes, opacity or halos paper-aware.
The reporter injects this field from the selected layout; geographic source
attributes never need to carry it.

For example, the same governed line can be 0.60 mm on A0 and 0.35 mm on A3:

```sh
ds style dimension plan --project <id> --ref master/lv_lines_print --field print_paper_size --channel size \
  --value print_a0=0.60 --value print_a3=0.35 --other 0.30 --output json
ds style dimension set --project <id> --ref master/lv_lines_print --field print_paper_size --channel size \
  --value print_a0=0.60 --value print_a3=0.35 --other 0.30 --yes
```

The shared Rust command kernel owns these transformations for native CLI and the
visual Style Center (WASM):

| Command family | Authoring |
|---|---|
| `appearance plan/set` | Flat colour, symbol icon, base size and flat halo colour/width |
| `categorical plan/set` | Primary colour, categorical icon or label text, with typed categories and explicit fallback |
| `color-range plan/set` | Interpolated numeric colour for line, fill and circle |
| `zoom plan/set` | Zoom stops for size, opacity and symbol collision overlap |
| `preset plan/set` | Opt-in satellite contrast or neutral existing-assets appearance on one exact screen ref |
| `instruction schema/plan/set` | Discover, validate and replay the same closed JSON instruction vocabulary |
| `print versions list/read/compare/restore` | Immutable exact print documents and restore under a head fence |
| `label plan/set` | Label field, visibility, zoom bounds, size, print papers and point placement |
| `dimension plan/set/clear` | A second field on halo, opacity or size |
| `cartography plan/set` | Visibility, zoom bounds, polygon boundaries, line type, direction, casing and hatching |

`plan` returns the complete proposed document and publishes nothing. `set` and
`clear` require `--yes`. The publish operation reads a fresh backend snapshot and
uses its save target. Existing filters, zooms, metadata and unrequested label and
paint properties survive. `style label` checks `--field` against the fields from
`style read`; when a style has no label, it starts from the backend's published
label model. Style targets may be global: changing one can affect other projects
that share it, exactly as saving globally in Style Center does.

Primary categorical colour accepts `--channel color --field cable_size
--value '35=#F59E0B' --value '50=#22C55E' --other '#94A3B8'`. Icon and label
channels use the same vocabulary with a catalog icon name or literal label text
as the output. The field must be declared by the editor; domain types determine
string versus numeric match labels. `--field-type` is available when the declared
field has no domain. `--merge` retains unmentioned bands of an existing match on
the same field. A fallback is always required. Symbol categories compile finite
raster sprite recipes with the existing halo and secondary dimensions retained.
Continuous symbol colour ranges are refused explicitly because those raster
recipes cannot represent an unbounded palette.

For zoom authoring, use `--channel size|opacity|icon_overlap --base <value>` and
repeat `--stop <zoom>=<value>`. Numeric values are finite; opacity is 0–1 and
size uses the editor's bounds. Overlap takes `on|off` and changes both icon
placement flags. Stops are strictly increasing from zoom 0 through 24. The
default is `step`; `--interpolation linear` is available for numeric channels
with a first stop at zoom 0. Flat halos use `appearance --halo-color '#FFFFFF'
--halo-width 0.9` without a category field. These screen controls preserve
independent print documents.

`ds style purpose index --project <id> --target print --ink colour` reads
authored purpose groups and exact immutable role bindings. Select a returned
id with `ds style purpose plan --project <id> --purpose <id> --ink colour`.
The plan captures separate governed `printing_standard` pages under the same
native identity and explicit project, compares every authored revision and
content pin, and delegates style capture to the existing exact tuple resolver.
It returns the admitted page bodies and style captures a print consumer needs.
No default, style or template is created or adopted, and no geometry, output
or publication is produced. Missing declarations and unadopted pages refuse;
printing still requires exact saved design and held geographic inputs.

`ds style instruction schema --output json` returns the compiled JSON Schema
without login. Save one instruction to a file, then `instruction plan --project
<id> --ref <ref> --file <file>` and `instruction set ... --yes`. Pass
`--expected-digest <contentSha256>` from a reviewed `style read` with
`digestAuthority: storage` to refuse stale replay. Every save uses the backend's
storage digest fence when offered; guarded replay refuses older backends that
cannot supply that fence. Unknown JSON keys are refused. Only the named ref is
changed, so design, vector-tile and print scopes remain explicit.

Print documents ship as a standard catalogue, including every style referenced
by governed global templates. `print versions list --project <id> --ref
gt/rivers_print` returns the head, latest 200 revisions and `more`; exact older
ids remain readable. `read` and `compare` take `--revision <64hex>`. `restore`
takes that revision plus `--expected-head <64hex> --yes`, appends a new revision
and retains earlier content. Each revision records its author, time, scope and
content digest. Printing receipts seal document, style revision and layout
revision together, including the separate MV printing setup.

Label options include `--visible on|off`, `--size` within the backend's published
bounds, repeatable `--paper A0` (or `--paper all` to clear paper restrictions),
and `--placement auto|fixed`. Paper restrictions require a print style. Automatic
placement tries the backend's point anchors and respects label collisions.
Omitted options preserve existing settings. For example:

```sh
ds style label plan --project <id> --ref master/lv_poles_print --field pole_number --visible on --size 8 --paper A0 --placement auto
```

Geometry and label zooms are independent. Both `cartography plan/set` and
`label plan/set` accept `--min-zoom` and `--max-zoom`: finite numbers from 0 to 24,
including fractions. Omitted bounds preserve existing values. The shared kernel
checks the resulting minimum against the maximum, including any retained bound.
Polygon fills also accept `--boundary-min-zoom`, `--boundary-max-zoom` and
`--boundary-visible on|off` independently of fill visibility and zooms. Boundary
controls are refused for other geometry types.

```sh
ds style cartography plan --project <id> --ref master/service_areas --min-zoom 8.5 --max-zoom 24 --boundary-visible off --output json
ds style label plan --project <id> --ref master/lv_poles_print --field pole_number --min-zoom 14 --max-zoom 24 --output json
```

After reviewing the plan, use the same arguments with `set --yes`. The live
command contract also exposes these parameters through MCP.

Dimension labels use the backend domain type. If the backend has no type,
`--field-type string|number|boolean` makes it explicit; the default is string.
The primary colour field cannot also drive the second dimension. Out-of-range
amounts, incompatible layer channels and invalid typed values are refused.
Changing base size preserves an existing categorical size expression and edits its
fallback. Clearing a second dimension removes only its authored properties.

Casing widens supported numeric line-width expressions without changing their
conditions or stops; unsupported arithmetic is refused. Hatching remains a recipe
that the renderer materializes. CLI does not render images, count live features or
sample the current viewport.

Discover exact flags, ranges and return shapes with `ds capabilities <command-id>`.

Style choice has four independent dimensions. `style resolution table --project
<id>` reads the API's persisted table. `style resolve --project <id>
--entity-class lv_line --source-kind saved_design --target print --role focused`
returns one exact document and revision through the shared kernel resolver.
Changing the source or role changes the authority; an unknown combination
refuses by name. Hosts never construct a style id from a layer name.

Authored purpose groups use that same held tuple authority. Their declaration
and the separate printing-layout boundary live in the kernel's
[style purpose contract](../../../ds-command-kernel/docs/contracts/style-purpose-index.md).
Discover the read through `ds capabilities style.purpose.index`; no production
document adoption is implied by the availability of this command.

`style catalogue manifest` reads the versioned declarative standard set.
`style catalogue seed plan --project <id>` names missing documents and preserves
authored heads. Apply only the reviewed `manifest_revision` and `plan_sha256`
using `seed apply --expected-manifest <64hex> --expected-plan <64hex> --yes`.
This is an explicit API initialization; reads never seed silently.

`style catalogue inventory --project <id>` reports exact catalogue and selected
project documents, resolver mappings, explicit obsolete declarations and an
opaque continuation. Its scope is not an all-project census. Read every page
before `backup create --expected-inventory <64hex> --yes`; the API stores an
immutable exact backup. `backup read --backup <id>` reads those exact bytes.
`retirement plan --expected-inventory <64hex> --backup <id>` names ids, reasons,
replacements and blocked dependencies. It never deletes documents. The main
session must obtain owner approval before a later deletion through the API.

`style appearance plan/set --icon-overlap on|off` controls independent symbol
icon placement. Both MapLibre icon placement flags change together; label overlap
and the other screen/print document remain authored independently. Plan first.

The standard seed plan also names exact installed legacy-scaling migrations.
They append immutable original and normalized revisions only when the original
content, current head and update time match the declared baseline. Other authored
changes are preserved. Review the migration rows with the create/preserve rows;
`style_resolution_migration_required` names a consumer deployment prerequisite.
All effective paper factors now come from the stored document.
