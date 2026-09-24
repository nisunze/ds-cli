# Style authoring

`ds style` reads and writes governed style documents through the native client.
It requires a native sign-in and `--project <exact-id>` on every call (the saved selection is never read); no desktop, window or map is
involved. Use `--lane stable|canary` to choose the deployment. Catalogue reads and
publication need the backend to be reachable.

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
print create --ref <screen-ref> --yes` publishes it. Catalog sprite names are
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
| `appearance plan/set` | Flat colour, symbol icon and base size |
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

`style appearance plan/set --icon-overlap on|off` controls independent symbol
icon placement. Both MapLibre icon placement flags change together; label overlap
and the other screen/print document remain authored independently. Plan first.
