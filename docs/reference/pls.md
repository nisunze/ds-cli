# `ds pls` — reference

Tier-4 reference. `ds pls <command> --help` is the contract.

## What this domain is

Seven of `ds-grid-tasks`' typed file tasks, exposed as commands. That crate
exists so a host does not have to know how a `.don`, a `.012` or a workspace's
reference closure is read: it takes a typed request, loads the exact bytes,
checks identity, calls the owning operation, and returns a bounded typed
result.

So this domain is thin on purpose. It parses no PLS format, resolves no
reference and compares no station. Those live behind the task boundary.

## Digest pinning is not optional

`compare-don` is **digest-pinned**: the task requires an expected `sha256:` for
each source and refuses without one. That is the guard working — it forces a
caller to state what they think the file is, so a comparison run later against
a changed file fails instead of quietly answering about different bytes.

Obtaining a pin does not require shelling out. Run it without pins and the
refusal hands both back:

```
$ ds pls compare-don --baseline ./issued.don --candidate ./revised.don
invalid_input: this comparison is digest-pinned; both sources need an expected SHA-256
  → pin the digests below with --baseline-sha256 and --candidate-sha256
  observed: baseline: sha256:fd7dcf…; candidate: sha256:a91e04…
```

This does not weaken the pin. The task still recomputes and compares at run
time; the digest's value is that it was recorded when a decision was made and
re-checked when the work runs. `pls_compare_don_refuses_a_wrong_digest` proves
the check is real.

The tasks also require **absolute** paths. `ds` canonicalizes what you pass
rather than making that a sharp edge.

## Three outcomes, not two

`compare-don` does not answer "are these different". It separates:

| Count | Meaning |
|---|---|
| `agreeing` | same structure at the position, same library name |
| `name_equivalent` | same structure, different name, reconciled by a declared `--equivalent` |
| `differing` | a different structure at that position |

Positions are matched by station within `--tolerance` metres (default 1.0), so
a resurveyed alignment does not read as a wholesale substitution.

## Paging bounds come from the tasks

The tasks bound their result sizes tightly and refuse anything larger:

| Command | Bound | Source of the number |
|---|---|---|
| `pole-capacity read` | 64 items | `MAX_DESCRIBE_LIMIT`, a public const, referenced directly |
| `reference-closure` | 32 translations | read from the task's published request schema at run time |

Neither number is written out in this repository. That is deliberate: the
first version of this domain defaulted `--limit` to 50 for both, and every
`ds pls reference-closure` call failed with the task's own `invalid_limit`
because 50 is over its bound of 32. A copied bound is a bound that drifts.

`pls_pole_capacity_limit_default_is_inside_the_task_bound` holds the line for
the default specifically, because a default over the bound makes the bare
command unusable while every flag-driven call still works.

## A task's refusal is not a `ds` refusal

When a task declines, `ds` returns `task_refused` and puts the task's own code
and detail in `detail`:

```json
{"code":"task_refused","detail":{"code":"invalid_limit","detail":"limit must be between 1 and 32"}}
```

The task's code is deliberately not promoted to `error.code`. `ds` documents
the codes *it* can emit as a closed set; a task's vocabulary is its own and may
grow without notice. A caller branches on `task_refused` and reads
`detail.code` for the specific reason — and in human mode both are printed, so
a remedy that says "read detail" has something on screen to read.

`terrain-reconcile`, `deviation-labels`, and `delivery-verify` are the bounded
exception: each
maps the owner task's larger diagnostic vocabulary into the short, declared
operator codes visible in its live command contract. The original owner code
remains in `detail["task-code"]`. This keeps common repairs branchable without
letting every low-level parser condition become a permanent CLI contract.

## Terrain waterfall and visible deviation labels

Both commands take one closed workspace, a JSON/GeoJSON point batch, ordered
routes, and an absent output path. Point input may be an array,
`{"points":[...]}`, a DS `command.rows` envelope, or a Point
FeatureCollection. Rows carry native projected `x_m`, `y_m`, `z_m` (or XYZ
geometry), with optional `code`/`feature_class`, `flag`, and `description`.
Routes may be `{"routes":[{"id":...,"coordinates":[...]}]}` or a
LineString FeatureCollection. Multipart geometry refuses.

Run the exact same evidence twice, changing only the mode:

```bash
ds pls terrain-reconcile \
  --workspace ./baseline \
  --points ./points.json \
  --routes ./routes.geojson \
  --horizontal-crs 'EDCL Rwanda TM' \
  --vertical-datum 'project surveyed TIN' \
  --out ./reconciled \
  --dry-run --output json

ds pls terrain-reconcile \
  --workspace ./baseline \
  --points ./points.json \
  --routes ./routes.geojson \
  --horizontal-crs 'EDCL Rwanda TM' \
  --vertical-datum 'project surveyed TIN' \
  --out ./reconciled \
  --yes --output json
```

The terrain task appends the corrected batch as distinct evidence, including
coincident rows. Baseline active records and the inactive block remain exact.
Only surveyed route endpoints receive seams; free ends remain listed in the
receipt. A global delta alone is never treated as complete repair.

Then derive visible feature text from route order:

```bash
ds pls deviation-labels \
  --workspace ./reconciled \
  --points ./points.json \
  --routes ./routes.geojson \
  --internal-code angle-point-new \
  --start-code deviation-start \
  --end-code deviation-end \
  --preserve-occupied-endpoints \
  --out ./labelled \
  --dry-run --output json

ds pls deviation-labels \
  --workspace ./reconciled \
  --points ./points.json \
  --routes ./routes.geojson \
  --internal-code angle-point-new \
  --start-code deviation-start \
  --end-code deviation-end \
  --preserve-occupied-endpoints \
  --out ./labelled \
  --yes --output json
```

The label writer replaces only selected feature-code byte ranges. XYZ, flags,
descriptions, unrelated rows, reserved bytes, headers, and inactive content
remain unchanged. These labels are terrain text, not alignment PIs.

Finish with the one-receipt native readback, using the untouched baseline and
the exact point batch again:

```bash
ds pls delivery-verify \
  --baseline ./baseline \
  --workspace ./labelled \
  --points ./points.json \
  --output json
```

The verifier requires the delivered count to equal baseline plus supplied
points, preserves the complete baseline terrain prefix and supplied XY/flags/
descriptions, reports the elevation-delta range and median, proves exact NUM
alignment and DON structure bytes, runs attachment closure, and re-reads every
phase/OPGW section support chain. `verified: false` is a real receipt when a
native model has sections but no complete support-chain surface; it is not an
instruction to invent one. Solver completion and engineering approval remain
outside this command.

## `section-orientation` takes a document, not flags

Its request needs the alignment's ordered structure numbers and the boundary
kind at each end — a nested object. Growing a flag per nested field would
produce exactly the "enormous collection of ambiguous flags" a typed request
document exists to avoid.

So it takes `--request <path>`, and publishes the contract:

```bash
ds pls section-orientation --schema --output json
```

That schema is the task's own, so it cannot drift from what the task accepts.

## What is not here yet

`ds-grid-cli` carries more PLS surface than this — structure ingest, emit,
roundtrip, method audit, the Structure Locations and Usage table, the
Available Structure List projection, the Oracle submit/status/result loop, and
PLS post-processing. Those sit behind `ds-grid-exchange`'s PLS adapter and the
Oracle spool rather than behind `ds-grid-tasks`, so each needs its own request
translation rather than a typed task to call.

These commands are exposed only where `ds-grid-tasks` publishes a typed task;
there is no generic native-patch adapter or argument-vector escape hatch.

## Ownership

Every command calls one function in `ds-grid-tasks`:

| Command | Task |
|---|---|
| `pole-capacity read` | `describe_pole_capacity` |
| `reference-closure` | `inspect_pls_reference_closure` |
| `section-orientation` | `diagnose_pls_section_orientation` |
| `compare-don` | `compare_don_assignment` |
| `terrain-reconcile` | `reconcile_pls_terrain` |
| `deviation-labels` | `label_pls_deviations` |
| `delivery-verify` | `verify_pls_delivery` |
