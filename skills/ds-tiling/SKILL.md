---
name: ds-tiling
description: Inspect or regenerate project tiles through `ds` when survey/design maps are stale.
metadata:
  ds-chapters: vector-tiles
  ds-mcp-profile: tiling
---

# Tile a project's outputs

Vector tiles are how the map renders a project's survey and design data, and
their **tilestats** are what the legend reads to show only the values the
project holds. A run rebuilds both. Everything goes through `ds tile`, which
reuses the application's own Pipeline client — never call the tiles API and
never touch storage directly.

1. Read the state: `ds tile status --project <id> --output json`. Per output (`survey`,
   `design`): `status`, `tiled_at`, `total_features`, `dirty` (sources
   changed since), `in_progress`.
2. Decide: `ds tile plan --project <id> --type <survey|design> [--force] --output json`.
   `.data.wouldDispatch` is the application's own rule (never built, dirty,
   or `--force`); when true, `.data.preflight` shows the sources — read
   `.data.preflight.status` (`ready` / `empty` / `blocked`) and
   `.data.preflight.empty_layers` back to the user.
3. Building tiles is edge compute. `ds tile generate` refuses with
   `native_compute_required`; the Core on this machine builds tiles from a
   prepared input through `ds core tile submit`. No edge step yet prepares that
   input from project data, so report a needed rebuild instead of forcing one.
4. Catalogue: `ds tile list --project <id> [--global]`; `ds tile add --project <id> --type <type>
   --source-project <id> --yes`; `ds tile remove --project <id> --tile-id <id> --yes`.

When to use `--force`: after `ds style` changes or after the operator edited a
Data-cleaning catalog. Neither marks the output dirty, but the tiles must be
rebuilt for the legend and the categorical styling to reflect the project's
vocabulary. Do not force a run on a clean, unchanged output.

Order of a restyle campaign: catalogs (the project's vocabulary, edited in the
application) → `ds style` → a rebuild of each output on an edge.

Do not run `generate` while `in_progress` is true. Do not read `blocked` as a
transient error — it names a source problem to fix first.

Stops at: the source data — a `blocked` tiling names a problem for the data's
owner to fix; regenerating cannot clear it.
