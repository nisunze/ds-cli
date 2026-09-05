# Style authoring

`ds style` reads and writes governed style documents through the native client.
It requires a native sign-in and selected project; no desktop or map is required.
Use `--lane stable|canary` to choose the deployment. Catalogue reads and publication
need the backend to be reachable. Local overlay commands remain usable offline.

Start with `ds style list`, then `ds style read --ref <returned-ref> --output json`.
Read returns the complete authored document, backend field vocabulary and domains,
property bounds, a bounded icon list and supported second-dimension channels.
`more` reports truncation; runtime feature counts and map visibility are not inferred.
Only backend-published editor refs can be authored from the headless catalogue.

The shared Rust command kernel owns these transformations for native CLI and the
visual Style Center (WASM):

| Command family | Authoring |
|---|---|
| `appearance plan/set` | Flat colour, symbol icon and base size |
| `dimension plan/set/clear` | A second field on halo, opacity or size |
| `cartography plan/set` | Line type, direction, casing and fill hatching |

`plan` returns the complete proposed document and publishes nothing. `set` and
`clear` require `--yes`. The publish operation reads a fresh backend snapshot and
uses its save target. Existing filters, zooms, labels, metadata and unrelated paint
survive. Style targets may be global: changing one can affect other projects that
share it, exactly as saving globally in Style Center does.

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
