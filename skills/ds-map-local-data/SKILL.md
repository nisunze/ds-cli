---
name: ds-map-local-data
description: Stage DS map views and panels, capture still PNG evidence, and manage session-only GeoJSON through `ds`; video stays third-party.
metadata:
  ds-chapters: survey
  ds-mcp-profile: survey
---

# Stage map evidence and local data

Use the deployed CLI as a declarative contract.

1. Start with `ds map view --output json`. Retain the current `bbox` if the
   task may need a temporary zoom.
2. Add homogeneous GeoJSON with `ds map draw`. Omit `--zoom` by default.
   Require `persisted: false`, and retain the returned `layer` and `bbox`.
3. Zoom only when it materially helps the task. Use `ds map zoom --layer
   <layer-id>` so the application computes the extent from its own CLI-owned
   local layer; no feature geometry should cross the CLI boundary. Do not zoom
   both during and after draw. Make one focus move, then restore the original
   bbox after inspection unless the user wants the map left focused.
4. Before removal, refresh `ds map view`. Remove only the exact `layer` id
   whose row says `this_session: true`. Never pass `analysis_id` to remove.

Do not add duplicate display layers merely to obtain a bbox. `--layer` accepts
the local `layer` id reported by `ds map view`, not its `analysis_id`, and only
for a layer created by this paired CLI session.

Local layers are temporary map-session data. Do not describe them as saved,
synced, or project data.

This skill owns session GeoJSON and evidence only. For canonical project-layer
ordering or desktop-local XYZ/PMTiles references, use `ds-layer-management`;
those operations do not require the map to be open. For archive parsing,
canonical column mapping, and Rust cleaning, use the design upload inspect /
stage contracts rather than `map draw`.

## Capture one reproducible still frame

Use the map commands as a sequence; none is a generic UI driver.

1. Discover the exact live contracts for `map.ui.open` and
   `map.evidence.capture`. Keep navigation under `ds map zoom` and property
   edits under `ds map design select` / `ds map design set`.
2. Open only a named application surface: `attribute-table`, `style-center`,
   or `selection-properties`. Pass a ref published by `ds map view`,
   `ds style list`, or the design-selection receipt. Never invent a CSS
   selector, click, script, coordinate, or keystroke.
3. Let the paired application settle its own map and UI, then capture either
   `--scope map` or `--scope app` to an absolute `.png` path in an existing
   directory. Retain the returned byte count, SHA-256, dimensions, view and UI
   state with the image.
4. A replacement is explicit and confirmed: use `--replace --yes`. Otherwise
   choose a fresh filename.

Short hypothetical tutorial sequences:

```bash
# Show a customer table, then capture the whole application.
ds map ui open --target attribute-table --ref master/customers
ds map evidence capture --scope app --out /evidence/01-customers-table.png --output json

# Focus a styled water main, open Style Center, and capture the map itself.
ds map zoom --layer water-main-demo
ds map ui open --target style-center --ref master/water_mains
ds map evidence capture --scope map --out /evidence/02-water-direction.png --output json

# Stage a governed property edit before showing the existing properties panel.
ds map design select --transformer TX-1042 --layer lv_lines --feature-id 17
ds map design set --transformer TX-1042 --layer lv_lines --feature-id 17 --set drafting_status=draft --dry-run
ds map ui open --target selection-properties --ref TX-1042.lv_lines/17
ds map evidence capture --scope app --out /evidence/03-property-step.png --output json
```

The CLI produces deterministic still PNG evidence only. Screen recording,
audio, editing and publishing remain third-party tools; `ds` can stage the
frame those tools record but never starts or controls a recorder.
