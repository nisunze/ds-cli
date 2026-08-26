# `ds tile` — reference

Tier-4 reference. `ds tile <command> --help` is the contract; this document is
the part that is true of every command.

## What a tile output is

A project renders two vector-tile outputs: **survey** (its form entries) and
**design** (its transformers and DS Grid models). Each is one PMTiles archive
ds-brain builds with ds-vector-tiler, publishes under a lease, and records with
its layers, feature counts and **tilestats** — the per-field values the tiles
actually hold. `ds` holds no token and no second tiling model: every command is
one named operation the paired application performs under its own session with
the same pipeline client its Pipeline panel uses. That is why there is no
`--project` flag: the active project is the one the application has open.

## When to re-tile

The legend of a tiled layer lists only the categorical values the tiles hold,
and ds-brain reads those from tilestats. Tilestats are rebuilt only by a run.
So a project **re-logs its categoricals by re-tiling** — after the operator
maintained a Data-cleaning catalog (the project's vocabulary) or restyled a
layer. Neither makes the output *dirty* (dirty tracks source data), which is
what `--force` is for:

```bash
ds tile status                                   # what is published, dirty, running
ds tile preflight --type design                  # the sources a run would read
ds tile plan --type design --force               # the decision, preflighted, nothing started
ds tile generate --type design --force --yes     # the same decision, dispatched
ds tile status --type design                     # follow the run
```

`plan` and `generate` are one operation with `apply` false or true — what you
reviewed is what is dispatched. During a run ds-brain normalises every policed
categorical column to the project's canonical values (aliases → clean value,
blank → the declared default), so the tilestats it records are presence, not a
spelling census.

## The catalogue

`ds tile list` shows every archive the project's map can mount: its own
outputs, outputs `ds tile add` referenced from other projects, and with
`--global` the platform's reference tiles. `ds tile remove` takes ids from that
list; removing an owned output reclaims its storage, removing a reference only
unlinks it.

## What the application enforces for you

- **The Pipeline panel's rule decides a run**: never built, dirty, or
  `--force` → run; current and clean → no run, even with `--yes`.
- **A blocked preflight never dispatches.** Fix what it names, then plan again.
- **One run per output at a time.** A running job is reported, not queued.
- **Membership is checked by ds-brain** on every action; `add` checks both
  projects.

## What is deliberately absent

Zoom levels, layer settings, tippecanoe options. ds-brain's tile policy owns
them per output type; a CLI door for them would fork the tile contract.
