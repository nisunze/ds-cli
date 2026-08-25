# `ds map` — reference

Tier-4 reference. `ds map <command> --help` is the contract; this document is
the part that does not belong in any command's help because it is true of all
of them.

## Where the map is

Nowhere on disk. The map is a MapLibre instance inside the running DS
GridDesign webview, its temporary layers are the sketch layers a person draws
by hand, and its design layers are per-transformer rooms the design tools
edit. There is no file this domain could open instead.

So every command here is one named semantic operation over the paired loopback
bridge, performed *by* the application under the identity it already holds.
`ds` sends a request and receives an outcome. It never receives a credential,
and it never runs code inside the application — the bridge accepts a closed
set of operations and nothing else. `docs/reference/desktop.status.md` has the
pairing argument in full.

## Two tiers, and why they are not one

| | Local layers | Design layers |
|---|---|---|
| Commands | `view` `draw` `remove` `zoom` `points-along` `random-points` `outliers` `line-difference` | `design read` `layer-to-local` `upload-to-local` `select` `set` `create` `setup` `process` `save` |
| Authority | `desktop_pairing` — the app is running | `project` — signed in, project selected |
| Reaches | what the operator can see | project data |
| Survives the session | no | only through `design save` |

A local layer is never project data. `persisted` is reported on every command
that makes one, and it is always false. `map zoom --layer <id>` asks the
application to fit a CLI-owned local layer; its geometry stays in the
application and only the computed bounding box is returned.

A design command stages into the operator's local room and marks it dirty.
`design save` is the separate push, it is `artifact_write`, and dispatch
requires `--yes` before it will run. Every design result reports `staged` and
`persisted` separately so the two can never be read as one.

That split is the application's, and it exists because
`drafting_status=approved` is what stops the kernel from redesigning an
installed transformer. Marking an as-built network approved is the most
consequential property write in the product; one sentence from a model must
not be able to reach the project.

## Design/local transfer and line extension difference

The comparison workflow stays inside the application and uses one shared
`ds-web` local-layer API. The CLI only declares names and tolerances:

```bash
ds map design layer-to-local \
  --transformer agasharu --layer lv_lines --name "agasharu approved base"

ds map design upload-to-local \
  --path 'C:\Designs\agasharu.shp.zip' \
  --source-layer lv_lines --name "agasharu incoming lv lines"

ds map line-difference \
  --source-layer <incoming-layer-id> --base-layer <base-layer-id> \
  --name "agasharu extension difference" \
  --coverage-tolerance-m 0.5 --heal-tolerance-m 1

ds map design create \
  --transformer agasharu --source-layer <difference-layer-id> \
  --target-layer lv_lines --set drafting_status=draft --dry-run
```

`layer-to-local` makes a local copy of the current project design layer;
`upload-to-local` asks the desktop parser for exactly one named archive layer.
Neither operation sends feature rows through `ds`. `line-difference` asks the
Rust/WASM kernel to remove directionally aligned source portions already
covered by the authoritative base and to heal remaining extension endpoints
within the given metric tolerance. Its output is another local layer.

Only `design create` crosses back into the design room, and it stages rather
than saves. It accepts exactly one source: `--features <geojson>` or
`--source-layer <id>`. The local-layer path keeps geometry inside ds-web and
returns only counts and a staging receipt to the CLI.

## Project-scoped Fast LV setup

The application owns one project-scoped Fast LV setup. The CLI does not keep a
second copy: it discovers and updates the same local preference used by the UI.
It names survey layer keys, while ds-web owns IndexedDB addresses and processor
wiring:

```bash
ds map design setup --output json
ds map design setup \
  --survey-layer edcl_customers_survey --preset drafting \
  --dry-run --output json
```

Survey layers are additional to current design customers unless
`--survey-only` is explicit. Omit `--dry-run` only after the exact semantic
source has been confirmed. Available source inventories are bounded by
`--limit` (20 by default) and report omitted counts; selected sources and
effective settings are never truncated.

## Survey migration is an API operation

Survey migration does not manipulate the map or drive the UI. It calls the
same governed `domains.network.report` migration API the application uses,
under the signed-in desktop session:

```bash
ds map survey migrate plan --source-project arjgpydw_huye2 --output json
ds map survey migrate apply --source-project arjgpydw_huye2 --yes --output json
```

The target is always the active project, never a caller-provided id. `plan`
uses the API's real dry run. `apply` is `global_write` and is stopped by
dispatch unless `--yes` is present. Both commands have one fixed policy: copy
all survey data, preserve the source, and skip ids already present in the
target. Form-template materialization, project settings and network
relationships remain the migration API's responsibility. There are no
caller-controlled delete/move, overwrite, filter, form, or alternate-target
flags. The receipt returns only bounded counts, never survey rows.

## Explicit upload, cleaning, process and save batches

The status-page bulk workflow is available through four closed CLI operations:

```bash
ds map design upload inspect --path ./tx1.zip --path ./tx2.xlsx --parallel 4
ds map design upload stage --source TX-1=./tx1.zip --source TX-2=./tx2.xlsx --parallel 4
ds map design batch process --transformer TX-1 --transformer TX-2 --parallel 4
ds map design batch save --transformer TX-1 --transformer TX-2 --parallel 4 --yes
```

`--parallel` is bounded to 1 through 32 and defaults to the application's batch
setting. It controls the desktop-owned worker pool; it does not launch browser
tabs, processes, containers, or one manually managed WASM instance per item.
Each task gets isolated WASM/kernel state, results are returned in requested
order, and one failed transformer does not cancel unrelated items.

Inspection is read-only. Upload staging performs parsing, canonical header
mapping and Rust cleaning, but leaves successful rooms local and dirty. Process
reuses the Design Status Fast Process scheduler and also remains staged. Only
the separate batch save persists, with optimistic versions and mandatory
`--yes`. Per-item rows always distinguish `staged` from `persisted` and carry
their own warning/error so a script can retry a strict subset.

## Design version is not the save revision

Feature lineage follows the deliberate version shown by Design Status. It is
independent of the cloud room's save revision, which exists only for
optimistic concurrency. In particular, deliberate `v0` is a real initial
design version and may stamp baseline features as `v_first=0` and `v_last=0`.

Before a governed save, inspect the prospective feature stamps without
persisting anything:

```bash
ds map design version audit --transformer agasharu --output json
```

The receipt reports `design_version`, `current_concurrency_generation`,
`would_concurrency_generation`, bounded stamp histograms, and `persisted=false`. At
versions after v0, unchanged features preserve both lineage values, changed
features preserve `v_first` and advance `v_last`, and new features receive the
deliberate version in both fields. An optimistic save revision must never be
copied into either feature property.

## The two identifiers

`ds map view` reports each layer twice:

```
  layer         sketch-1f3a     what `ds map remove` takes
  analysis_id   sketch:sketch-1f3a   what the vector tools take
```

They are different id spaces. `remove` addresses the sketch layer; the vector
tools address the application's *analysis catalogue*, which also holds design
and survey layers under their own keys (`lv:lv_lines`, `live:…`). Both are
reported so a caller passes a value it was given rather than one it built.

`analysis_id` is the one identifier `ds` composes rather than receives,
because the bridge publishes no operation that lists analysis options. That
composition is checked against `loadOutlierLayerOptions` by
`tests/bridge_parity.rs`; if the application ever re-keys temporary layers,
that test fails rather than every vector-tool call quietly refusing.

**Known gap.** There is no `gis.layers` bridge operation, so a design or
survey layer's `analysis_id` cannot be discovered through `ds` — only a
temporary layer's, via `map view`. Reaching design layers by analysis id
today means knowing the application's key. A bridge operation that listed the
catalogue would close it.

## What a vector tool leaves behind

`points-along`, `random-points` and `outliers` each add their result to the
map as a new layer. That layer belongs to the analysis tool, not to this
session, so **`ds map remove` will refuse it** — the application only lets a
session remove what that session created, which is what stops an agent tidying
up from erasing an operator's work.

## Bounded by default

`outliers` is the one to know about. The application's answer carries the
entire scored feature collection — every feature, every property. Returning it
would make one call the most expensive thing in the CLI, so the default is
counts and the score summary, the flagged features are on the map where they
are useful, and individual findings are an explicit `--limit` projection whose
truncation is reported in `more`.

The same rule holds elsewhere: `map view` bounds its layer list, `design read`
bounds its property histogram (commonest first, so a truncated one still
answers the question), `design select` returns no ids and no samples unless
asked, and `design process` bounds its warnings.

## Selector semantics

`design select`, `design set` and `design process --differential-*` take the
same selector, ANDed:

| Flag | Meaning |
|---|---|
| `--layer <name>` | design layers to search; omit for all |
| `--where <key=value>` | property equals value |
| `--where <key=>` | property is **unset** — absent, null or blank |
| `--bbox <w,s,e,n>` | feature extent meets this box |
| `--id <feature-id>` | narrow to exactly these |

`--where drafting_status=` is the one worth remembering. An unmarked as-built
row carries no `drafting_status` key at all rather than an explicit `"draft"`,
so the empty form is how those rows are found. It reaches the application as
JSON `null`, which is its own predicate for the same thing.

Run `design select` before `design set` with the same selector: the count it
reports is the count `set` will report as matched.

## The differential process run

`design process` with no differential flags runs FULL and recalculates
everything, including an as-built network that was just approved. A
differential flag narrows the run to the matching `lv_lines` and freezes the
rest for that run only — the kernel treats frozen rows as approved and strips
the flag from every output, so the operator's real `drafting_status` is
untouched.

Two honest reports come back with it:

- a differential selector that matches nothing is **refused**, not widened
  into a full run. Widening would recalculate exactly the network the caller
  was protecting.
- a blocking diagnostic makes the kernel run full regardless. That arrives as
  `blocked_from_differential: true` alongside `mode`, so a caller can tell the
  freeze did not hold rather than assuming it did.

## The wire contract is proved, not trusted

Every operation and argument key this domain sends is declared once, in
`BRIDGE_OPS` in `crates/ds-cli-map/src/lib.rs`. Two things use that
declaration:

- `invoke` refuses to send a key it does not declare. An undeclared key is an
  internal failure that never leaves the process.
- `crates/ds/tests/bridge_parity.rs` proves the declaration against the
  application's own source: every operation is in the bridge's allow-list
  **and** has an executor behind it, every argument key appears in that
  operation's input schema, every locally enforced bound is the application's
  own, and the snapshot fields `map view` reads are still published.

The executor check matters more than it looks. `style.preview`,
`style.save_local`, `workspace.save` and `workspace.save_as` are in the
bridge's allow-list today and have no case in the frontend's operation switch:
a command built on any of them would compile, help correctly, document its
refusals, and fail for every caller. The parity suite is what makes that
impossible to ship.

It needs the `ds-web` checkout. Set `DS_WEB_DIR` when it is not the sibling
directory — a git worktree of this repository is two levels deeper, and a run
that cannot find it reports the path it looked in rather than passing quietly.

## Refusals a caller should expect

Beyond each command's own list, every command here can end in the pairing
state:

| Code | Means |
|---|---|
| `desktop_not_paired` | no session on this machine |
| `desktop_ambiguous` | Stable, Canary and dev running together — name one with `--desktop-descriptor` |
| `desktop_unreachable` | the descriptor is stale, or the app did not answer in time |
| `pairing_rejected` | the descriptor's secret was refused; restart the app |
| `desktop_refused` | the app answered and declined; `detail.detail` is its message |
| `desktop_operation_unsupported` | this build does not offer the operation |
| `desktop_unreadable` | the reply exceeded its bound |
| `desktop_signed_out` | a design call with no signed-in project session |

Availability is deliberately unconditional. Dispatch checks availability
*before* parsing flags, so a gate on discovery would make
`--desktop-descriptor` — the flag that names a descriptor discovery did not
find — unreachable, and would put every input refusal out of reach on a
machine without the application. `ds desktop status` is the diagnostic.

Every handler validates its own flags before it opens the bridge, so a
malformed invocation refuses the same way whether or not an application is
running. `tests/domain_smoke.rs` asserts that ordering command by command;
without it, the input contract would be untestable everywhere the desktop is
not installed, which is every CI machine.
# Survey Working Area materialization

`ds map survey download --entire-project` applies the paired desktop's full-project Working Area and waits for its existing sequential survey loader to materialize every configured survey form. The application owns authentication, Working Area state, API calls, IndexedDB and feature rows. The CLI sends only `{ entireProject: true }` and returns bounded cache counts; it never receives raw rows.

This is distinct from `map survey migrate`: migration copies governed survey records between projects, while download materializes the active project's records into its local desktop cache for map and WASM processing.
