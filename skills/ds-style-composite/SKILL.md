---
name: ds-style-composite
description: Style one DS map layer by two fields through `ds style`, combining colour with halo, opacity, or size for category-plus-status maps.
metadata:
  ds-chapters: map-presentation
  ds-mcp-profile: map
---

# Style a layer by two fields

The colour of a layer is its first categorical dimension; this skill can set
the guided flat appearance and add a second field without replacing the first.
Everything goes through `ds style`, which reuses the Style Center's own
authoring and save path — never edit style JSON by hand and never call the
styles API directly.

1. Find the ref: `ds style list --query <layer> --output json`. Tiled design
   layers are the `_vt` refs (`target: design_vt`); GeoJSON design layers are
   the bare `master/<layer>` refs. Style the one the user is looking at.
2. Read it: `ds style read --ref <ref> --output json`. Take the second field
   from `.data.fields`, never the `.data.colorField`. Check
   `.data.onMap.types` — the value labels you pass are typed the way the map
   carries the field. If `.data.onMap` is null the map has nothing rendered
   for the ref; ask the user to open the map on the area first, or proceed
   with the backend `.data.fieldValues`.
3. If the request changes the flat colour, raster icon or base size, plan it
   first: `ds style appearance plan --ref <ref> --color '<#hex>' [--icon
   <catalog-name>] [--size <number>] --output json`. Publish the identical
   inputs with `appearance set … --yes`. The paired application validates the
   property and numeric range from its live schema; do not guess icon names.
4. Plan the second dimension: `ds style dimension plan --ref <ref> --field <field> --channel halo
   --value <highlight>=<px>:<#hex> --value <other>=0 --output json`. Read
   `.data.expressions` and `.data.onMap` counts back to the user.
5. Publish only with the user's intent: the same flags with `dimension set …
   --yes`. Report `.data.warnings` verbatim.
6. To undo the second dimension: `ds style dimension clear --ref <ref> --yes`.

Channel choice: `halo` is the strongest differentiator (a ring; on lines a
hollow casing, on fills the outline colour); `opacity` mutes values; `size`
scales them. On raster symbol layers the ring is baked into the icon by the
backend — expect one image per (icon, colour, halo).

Do not use `--value` for more than the values that need to differ; route the
rest through `--other`. An explicit `--other` is intentional fallback coverage,
not an uncovered-value warning. Do not set the second dimension on the colour
field. Do not touch `ds map` design edits for styling.
