# `ds tile` — reference

Tier-4 reference. `ds tile <command> --help` is the contract; this document is
the part that is true of every command.

## What a tile output is

A project renders two vector-tile outputs: **survey** (its form entries) and
**design** (its transformers and DS Grid models). Each is one PMTiles archive
ds-brain builds with ds-vector-tiler, publishes under a lease, and records with
its layers, feature counts and **tilestats** — the per-field values the tiles
actually hold. The native client owns authentication; tiling remains server-owned.

Every tile command names its project with `--project`; the saved selection is
never read (the Server has no active project). Status, preflight, plan and
generate restore the native user for `--lane stable|canary` (Stable by
default) and call the fixed ds-brain tile contracts for that exact project.
There is no URL, body, action or Desktop descriptor override. Each receipt
names the lane and the project id it was given.

List, add and remove also execute through the native selected-project client.
They require no desktop. Add asks the backend to copy a published source into
destination-owned storage. Remove retires the catalogue row and cleans its owned
storage; it requires `--yes`. List returns bounded metadata without signed URLs.

## When to re-tile

The legend of a tiled layer lists only the categorical values the tiles hold,
and ds-brain reads those from tilestats. Tilestats are rebuilt only by a run.
So a project **re-logs its categoricals by re-tiling** — after the operator
maintained a Data-cleaning catalog (the project's vocabulary) or restyled a
layer. Neither makes the output *dirty* (dirty tracks source data), which is
what `--force` is for:

```bash
ds tile status --project <id> --lane stable                                  # what is published, dirty, running
ds tile preflight --project <id> --type design --lane stable                 # the sources a run would read
ds tile plan --project <id> --type design --force --lane stable              # status + preflight, nothing started
ds core tile submit --input tiles.prepared.json --key tiles-001             # edge build on this machine
```

`plan` never dispatches. It reads status, applies the same conservative
staleness ordering as the Pipeline panel (already running → no new run;
otherwise force, never published or dirty → run), and performs preflight only
when a run would dispatch. Because those are separate authenticated reads, it
refuses if the selected-project receipt changes between them.

Tiling is edge compute. `ds tile generate` no longer reaches the retired hosted
tile worker: it refuses with `native_compute_required` and names
`ds core tile submit`, which builds tiles from a prepared `ds.tiles.prepared/v1`
input with the Core on this machine. No edge step yet prepares that input from
project survey/design data.

During a run ds-brain normalises every policed categorical column to the
project's canonical values (aliases → clean value, blank → the declared
default), so the tilestats it records are presence, not a spelling census.

## The catalogue

With DS GridDesign paired, `ds tile list --project <id>` shows every archive the project's map can mount: its own
outputs, outputs `ds tile add --project <id>` referenced from other projects, and with
`--global` the platform's reference tiles. `ds tile remove --project <id>` takes ids from that
list; removing an owned output reclaims its storage, removing a reference only
unlinks it.

## The governed reference publications

`ds tile global …` is the other catalog: the product's reference layers, which
belong to no project. They carry no project id and no project fence — the
gateway's `global_tiles.manage` is the whole of their authority — so every one
of these commands takes `--lane` and nothing about a host.

```bash
ds tile global catalog --domain network_template --search school   # eligible sources
ds tile global status --tile <tile-id>                             # one publication job
ds tile global list --domain network_template                      # what is live
ds tile global access --tile <tile-id> --mode restricted --yes     # stop it being mounted
```

`ds tile global generate` refuses with `native_compute_required`: the hosted
reference worker is retired and no edge owner publishes a new global archive yet.

A publication exposes one layer per source table, and a layer's style document
is keyed by that layer name — `gt/<layer>`, with `gt/<layer>_print` for paper.
Two publications may therefore not expose the same layer name; the backend
refuses the second one and names the publication that holds it.

`access` is the reversible lever. `--mode restricted` with nobody named leaves
the publication, its archive and its styles intact and stops every project
mounting it; a later `--mode all` puts it back. What the call carries becomes
the whole policy, so an allowed reader left out of it loses the grant.

`ds tile remove --project <id> --tile-id <id> --scope global --yes` is the irreversible one.
It retires the row, reclaims the archive, and deletes the style documents of
every layer no other live publication exposes — naming the ones it keeps and
who keeps them. When it cannot reclaim an archive, it removes nothing and says
which object it cannot delete.

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
