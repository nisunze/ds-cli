# `ds dsgrid-exchange` — reference

Tier-4 reference. `ds dsgrid-exchange <command> --help` is the contract.

This domain is where a `.dsgrid` comes from and where it goes: PLS-CADD
workspaces and backups in, GIS and tabular exports out. [`ds dsgrid`](dsgrid.md)
answers questions about a model that already exists; this domain manufactures
one.

## Why it is a separate domain

The split is by **effect**, not by subject.

Every command in `ds dsgrid` is `discovery` or `read_only`. `convert` is the
only command in either domain that writes files. Folding them together would
put "tell me what this package is" and "manufacture a new one" behind one help
screen and one blast radius, and a reader scanning `ds --help` would have no
way to tell which of those a domain does.

It also keeps discovery honest. A caller who only wants model identity gets a
domain whose every command is safe to invoke speculatively — which is the
property that makes `ds capabilities` worth calling at all.

## The sequence is the contract

```
inspect   what are these files, and what could they become?
plan      exactly what would a conversion do — and what would it lose?
convert   do that, and nothing else
```

Domain help lists the commands in this order deliberately: the index doubles
as the procedure.

`plan` pins the `sha256:` digest of every source it reads. `convert` re-digests
those same bytes and **refuses** if any changed. That is what makes the
sequence worth following instead of skipping to the end — the plan is a
commitment, not an estimate.

It is also why there is no `convert --dry-run`. A flag would suggest the two
differ by a switch. They differ by effect class: `plan` is `discovery` and can
be called freely, `convert` writes.

## `inspect`

Answers the first question anyone has about a pile of engineering files: *what
is this, and what can I do with it?* Until it existed, the only way to find
out was to attempt a conversion and read the failure.

```bash
ds dsgrid-exchange inspect --source ./workspace
```

A directory is read as **one folder source**, recursively, in sorted order.
Sorting is not cosmetic: the engine digests the member list, so unsorted
directory iteration would make the same tree produce different digests on
different machines. `dsgrid_exchange_inspect_is_deterministic_over_a_directory`
holds that line.

Per source, the result carries the engine's classification, the exact
`sha256:` digest, the member count, and whatever version and units evidence
the engine recovered — for a PLS-CADD workspace that is its declared version
and unit system, read out of the files rather than guessed.

Then the **capability matrix**: every conversion the engine offers from this
set, with its state and reason. Only `ready` and `unverified` are offered by
default; `--blocked` adds the rest with the engine's own explanation.

`unverified` is included on purpose — a path that exists but has not been
verified for these inputs is something a caller may reasonably attempt, and
its reason says so.

Nothing is converted and nothing is written.

For a characterized GIS archive/document, `inspect` also returns every layer's
name, feature count, geometry type, source CRS and normalized CRS. A zipped
Shapefile, GeoJSON, KML or KMZ is classified from its content as
`GisLayerSource`; the filename is provenance, not the format proof.

### Disambiguation

`ds capabilities --search inspect` returns two commands, and the distinction
is load-bearing:

| Command | Takes | Answers |
|---|---|---|
| `ds dsgrid inspect` | `--model` | **model identity** — which `.dsgrid` is this, what is in it |
| `ds dsgrid-exchange inspect` | `--source` | **source classification** — what format are these files, what can they become |

The opening words of each summary carry that difference, because that is all a
caller reads before choosing.

## `plan`

```bash
ds dsgrid-exchange plan --source ./workspace --target dsgrid --crs EPSG:32735
```

Returns the immutable, digest-pinned plan: chosen capabilities, ordered
stages, every artifact the conversion would write, the pinned digests, and —
in full, never truncated — the **blockers, warnings and losses**.

Those three are never bounded, and the rest is. They are the answer to "should
this run", and a caller who reads a truncated blocker list has been told a
conversion is safe when it is not. Everything else is capped at 25 entries
with the withheld count reported and the full total in
`expected_artifact_count`.

A plan carrying any blocker has **no executable stages at all** — that is the
engine's rule, not a `ds` convention.

### GIS to DS Grid

GIS seeding is explicit. One declared `LineString` layer becomes canonical
alignments, and an optional property supplies their labels. When an explicit
Point/Polygon source layer is also selected, Rust nodes coincident route ends,
requires exactly one terminal per source feature, assigns every other degree-1
terminal as a conceptual transformer position, and assigns internal branch
nodes as conceptual tappings:

```bash
ds dsgrid-exchange plan --source ./mv.zip --target dsgrid \
  --alignment-layer mv_lines --alignment-label-property node_id \
  --network-source-layer site_solaires --network-source-role plant \
  --network-snap-tolerance-m 0.5 \
  --crs EPSG:32633
```

The source reader normalizes characterized GIS geometry to WGS84 and the
exchange adapter projects the selected line layer into the declared metric
model CRS. The current characterized targets are WGS84 UTM
`EPSG:32601..32660` and `EPSG:32701..32760`; a geographic CRS is refused as a
model CRS.

The network analysis is source-rooted and retained as
`gis-network-analysis.json`. For every route position it carries both local
`local_station_m` and cumulative `from_source_distance_m`; therefore a child
alignment starts at its upstream graph distance rather than resetting the
network distance to zero. Tree/non-tree edges and unreachable fragments remain
explicit. The adapter does not guess a source or transformer from contextual
service-area polygons.

The same operation can complete that skeleton as a self-contained conceptual
MV model when a second source is an exact native standards backup. Every
engineering identity and every terrain provenance field is explicit:

```bash
ds dsgrid-exchange convert \
  --source ./prepared-gis.zip --source ./huye-standards.bak \
  --target dsgrid --crs EPSG:32633 \
  --alignment-layer mv_lines --alignment-label-property node_id \
  --network-source-layer site_solaires --network-source-role plant \
  --network-snap-tolerance-m 0.5 \
  --terrain-layer terrain --terrain-elevation-property z \
  --terrain-feature-class-property feature_class \
  --terrain-provider aws_terrarium \
  --terrain-dataset elevation-tiles-prod/terrarium@z12 \
  --terrain-resolution-m 30 --terrain-horizontal-crs EPSG:4326 \
  --terrain-acquired-at 2026-09-04T00:00:00Z \
  --standards-source-index 1 \
  --source-structure-type-id <id> \
  --transformer-structure-type-id <id> \
  --tapping-structure-type-id <id> \
  --support-structure-type-id <id> --maximum-span-m 99 \
  --phase-cable-id <id> --criterion-set-id <id> \
  --sag-weather-state-id <id> --phase-catenary-constant-m 1000 \
  --phase-attachment-set PHASE --phase-attachment-slots 0,1,2 \
  --out ./out
```

The standards source is translated by the existing native PLS-CADD-to-DS Grid
adapter. The GIS completion code never writes native or `.dsgrid` bytes from
scratch. It retypes the source, transformer, tapping and inserted support
structures with the selected definitions, deterministically limits support
spacing, and strings the selected phase cable through exact attachment slots.
The selected criteria remain embedded and named in the section note, but the
explicit catenary constant is a conceptual manual sag—not a claimed AutoSag or
approved criteria result.

`tag_city` and `tag_phasing` properties on the selected line layer become
ordinary canonical granular tags. Split source-outward runs retain them, and
the completion step carries unambiguous values to structures and tension
sections. No project API, frontend state, or project-resource collection is
consulted. A shared junction with conflicting scalar values retains the facts
on its alignments and receives no invented structure tag.

Terrain points become canonical `dem_raw` observations with the declared
provider, dataset, nominal resolution, horizontal CRS, EGM96 orthometric
vertical datum, and acquisition timestamp. This is provisional conceptual
terrain; it remains distinguishable from surveyed ground and can later be
superseded through the ordinary terrain lifecycle.

Polygons, points and unselected lines do not silently become engineering
tables. The exact complete source is embedded as a package asset, and
`gis-context-manifest.json` records every layer and whether it is a canonical
alignment source or contextual evidence. This preserves city limits, buffers,
solar sites and similar layers without pretending the DS Grid schema has a
canonical polygon table.

## `convert`

```bash
ds dsgrid-exchange convert --source ./workspace --target dsgrid \
  --crs EPSG:32735 --out ./out
```

The corresponding GIS seed is:

```bash
ds dsgrid-exchange convert --source ./mv.zip --target dsgrid \
  --alignment-layer mv_lines --alignment-label-property node_id \
  --network-source-layer site_solaires --network-source-role plant \
  --network-snap-tolerance-m 0.5 \
  --crs EPSG:32633 --out ./out
```

Plans, refuses a blocked plan, executes, and writes every artifact under
`--out` beside an `exchange-report.json` carrying the plan and the per-source
outcome together. The pair is the evidence: the plan says what was promised,
the outcome says what happened, and a reader comparing them needs both in one
file.

Three rules hold, and they are the reason this is safe to hand to an agent:

- **The plan is re-pinned.** A source edited between `plan` and `convert`
  fails with `source_digest_mismatch` rather than converting bytes nobody
  previewed.
- **The artifact plan owns path identity.** Output paths come from the plan
  verbatim. This command will not mint a `1-` prefix, flatten a nested
  library, or silently resolve a case-only collision — it refuses with
  `output_path_collision`, because materializing bytes at a path the plan did
  not name means the verified plan and the written tree are different
  documents. The collision check is case-insensitive, since Windows and macOS
  treat two such paths as one file.
- **It never overwrites.** An existing output path is `output_exists`, a
  `conflict`. There is deliberately no `--force`: the remedy is a new
  directory, which keeps the previous conversion's evidence intact.

`convert` is `local_file_write`, not `artifact_write`, so it does not require
`--yes`. It writes into a directory the caller named, and publishes nothing.

## Why this could ship now

[`dsgrid.md`](dsgrid.md) previously recorded `convert plan` and `convert run`
as deliberately withheld, and the reason was specific rather than a shrug:

> `ConversionRequest` carries a `SourceSet` of raw **bytes** … A JSON request
> document for that would name paths where the struct holds bytes — which
> means `ds` would own a hand-authored adapter shape that the engine never
> sees and therefore cannot check.

That objection was about a **process boundary**, and it was correct for one.
It does not apply here, because this domain **links** `ds-grid-exchange`
exactly as `ds dsgrid inspect` already does. There is no request document and
no hand-authored schema: `request.rs` constructs a `ConversionRequest` out of
the engine's own types, so the compiler is the check that was missing. A field
added, removed or retyped upstream stops this build.

What `ds` owns here is argument handling, refusal mapping, bounded projection
and the file-writing rules. It computes nothing.

## Bounds

512 MiB and 4 096 files across all sources, enforced in one place
(`sources.rs`) for all three commands. A mistyped path at a large tree fails
in a moment with `source_too_large`, not after reading it.

Loading is shared so the three commands cannot drift: a caller who learns
`source_too_large` from `inspect` gets the same code and remedy from `plan`
and `convert`, which is what makes "inspect first, then convert" a reliable
sequence rather than a hopeful one.

## Known gaps in contract 1

Stated rather than left to be discovered:

| Not exposed | Effect |
|---|---|
| `--combine-order`, `--template-pos` | native combine uses source order; explicit ordering is unreachable from `ds` |
| `--pair-don` / `--pair-num` / `--subtree` | multi-project PLS selection is available only through `--select-project <don-leaf>`, which resolves the pair and subtree from the loaded members |
| `--version` | every native output is authored as PLS-CADD 16.81. `ds-grid` accepts the flag and then rejects every other value, so it is stated in help instead of being a flag whose only job is to fail |
| `compose` | `ds-grid`'s `compose-inspect` / `compose-plan` / `compose` are not yet `ds` commands, though `--mode compose` composes several sources into one `.dsgrid` during a conversion |
| adapter output path validation | `convert.rs` currently joins the engine-provided `output.relative_path` under `--out` without rejecting absolute or `..`-bearing values. `ds-grid-cli` already has the required `validated_relative_output_path` guard (`unsafe_output_path`); port that guard and add plan/convert smoke assertions in a dedicated follow-up. |

## Ownership

`ds` computes none of this. It reads bytes and calls:

| Command | Owner |
|---|---|
| `inspect` | `ds_grid_exchange::conversion::{inspect_sources, conversion_capabilities}` |
| `plan` | `ds_grid_exchange::conversion::plan_conversion` |
| `convert` | `ds_grid_exchange::conversion::{plan_conversion, execute_conversion}` |

There is no second implementation of source classification, planning or
conversion in this repository, and there must not be one: two planners with
two tolerances disagree silently, and the caller receives a different answer
rather than a disagreement.
