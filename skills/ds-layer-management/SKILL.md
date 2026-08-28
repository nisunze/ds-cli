---
name: ds-layer-management
description: Inspect or order DS project layers and manage validated local XYZ or raster PMTiles overlays through `ds`.
metadata:
  ds-chapters: survey
  ds-mcp-profile: layers
---

# Manage DS layers by lifecycle

Use the deployed `ds` command contracts. Do not treat every visual source as
the same kind of layer:

| Intent | Owner | Map must be open? |
|---|---|---|
| Inspect or reorder canonical project layers | `ds map layer list/reorder` | No |
| Add, hide, list, or remove a third-party XYZ/raster PMTiles overlay on this desktop | `ds map layer add/remote-list/visibility/remove` | No; an open map updates immediately |
| Add or remove session-only GeoJSON | `ds map draw/remove` | Yes |
| Mount another project's published survey/design output | `ds tile add/remove` | No |
| Parse, map canonical headers, and Rust-clean a design archive | `ds map design upload inspect/stage` | No map; signed-in project required |

Project ordering accepts only `layers[].id` returned by `ds map layer list`.
Those are canonical config ids. Never substitute `runtimeIds`, style refs,
GeoJSON keys, or an id remembered from another project. Review the complete
set of overrides, then use `--yes`; the renderer still preserves its safe
global/reference and geometry stack bands.

Remote overlays are desktop-local references in IndexedDB, not project data.
`add` accepts HTTP(S) XYZ templates containing all of `{z}`, `{x}`, and `{y}`
or raster PMTiles archives; it refuses embedded credentials. Vector PMTiles
need a governed source/style contract and must not be described as supported
by this raster overlay path. Visibility is an idempotent setter, not a toggle.

Local GeoJSON receives bounded geometry validation, not Design Data Factory
cleaning. When the request says cleaning, canonical column mapping, domains,
or a network archive, use upload inspection and staging; do not draw the raw
file and call it cleaned.

Short hypothetical requests:

```bash
# "Put imagery behind a water-network project while I work quietly in CLI."
ds map layer add --name "Water imagery" --kind xyz \
  --url 'https://tiles.example.org/{z}/{x}/{y}.png' --hidden
ds map layer visibility --layer <id-from-add> --visible true

# "Show me the real project stack, then move two known water layers."
ds map layer list --refresh --output json
ds map layer reorder \
  --order '<canonical-water-polygon-id>=40' \
  --order '<canonical-water-main-id>=140' --yes

# "Clean this contractor network archive before it becomes design work."
ds map design upload inspect --path ./water-network.shp.zip --output json
ds map design upload stage --source WATER-01=./water-network.shp.zip --output json

# "Reference a neighbouring project's published design tiles." This is not a URL overlay.
ds tile add --type design --source-project neighbouring-project --yes
```
