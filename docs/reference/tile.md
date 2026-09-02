# `ds tile` — reference

Tier-4 reference. `ds tile <command> --help` is the contract; this document is
the part that is true of every command.

## What a tile output is

A project renders two vector-tile outputs: **survey** (its form entries) and
**design** (its transformers and DS Grid models). Each is one PMTiles archive
ds-brain builds with ds-vector-tiler, publishes under a lease, and records with
its layers, feature counts and **tilestats** — the per-field values the tiles
actually hold. `ds` holds no token and no second tiling model.

Status, preflight, plan and generate are headless selected-project commands.
They restore the native user for `--lane stable|canary` (Stable by default),
load only its UID/email/lane/audience-fenced project selection, and call the
fixed ds-brain tile contracts. There is no `--project`, URL, body, action or
Desktop descriptor override. Each receipt includes the lane and selected
project id, name and lifecycle status.

The catalogue commands — list, add and remove — remain paired for now because
their public API contracts have not been extracted. For those commands the
active project is the one the application has open, and `--desktop-descriptor`
remains available.

## When to re-tile

The legend of a tiled layer lists only the categorical values the tiles hold,
and ds-brain reads those from tilestats. Tilestats are rebuilt only by a run.
So a project **re-logs its categoricals by re-tiling** — after the operator
maintained a Data-cleaning catalog (the project's vocabulary) or restyled a
layer. Neither makes the output *dirty* (dirty tracks source data), which is
what `--force` is for:

```bash
ds tile status --lane stable                                  # what is published, dirty, running
ds tile preflight --type design --lane stable                 # the sources a run would read
ds tile plan --type design --force --lane stable              # status + preflight, nothing started
ds tile generate --type design --force --lane stable --yes    # fixed backend generation call
ds tile status --type design --lane stable                    # follow the run
```

`plan` never dispatches. It reads status, applies the same conservative
staleness ordering as the Pipeline panel (already running → no new run;
otherwise force, never published or dirty → run), and performs preflight only
when a run would dispatch. Because those are separate authenticated reads, it
refuses if the selected-project receipt changes between them. `generate`
requires `--yes` and calls the fixed backend generation action directly;
ds-brain repeats the authoritative staleness, preflight and lease decisions.

During a run ds-brain normalises every policed categorical column to the
project's canonical values (aliases → clean value, blank → the declared
default), so the tilestats it records are presence, not a spelling census.

## The catalogue

With DS GridDesign paired, `ds tile list` shows every archive the project's map can mount: its own
outputs, outputs `ds tile add` referenced from other projects, and with
`--global` the platform's reference tiles. `ds tile remove` takes ids from that
list; removing an owned output reclaims its storage, removing a reference only
unlinks it.

## What the backend enforces for you

- **The governed generation endpoint decides the run**: never built, dirty,
  or `--force` → run; current and clean → no run, even with `--yes`.
- **A blocked preflight never dispatches.** Fix what it names, then plan again.
- **One run per output at a time.** A running job is reported, not queued.
- **Membership is checked by ds-brain** on every action; `add` checks both
  projects.

## What is deliberately absent

Zoom levels, layer settings, tippecanoe options. ds-brain's tile policy owns
them per output type; a CLI door for them would fork the tile contract.
