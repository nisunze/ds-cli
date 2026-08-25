---
name: ds-map-local-data
description: Add session-only GeoJSON layers to the DS map, remove only layers created by the paired CLI session, and focus or restore the viewport with minimal movement. Use for temporary local map data, not project survey or design edits.
---

# Manage local map data

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
