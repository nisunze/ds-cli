---
name: ds-style-composite
description: Style a DS map layer by category and a second visual field through `ds style`.
metadata:
  ds-chapters: map-presentation
  ds-mcp-profile: map
---

# Style a DS map layer

Everything goes through `ds style`, which reuses the Style Center's own
authoring and governed save path. Never edit style JSON by hand, never call
the styles API, never compose a paint expression, dash array or pattern image
yourself — there is no flag that takes one, by design.

## Pick the axis first

A document has three independent axes. They compose; none replaces another.
Choosing wrong is the expensive mistake, because two of them cost a field.

| the request sounds like | axis | commands |
|---|---|---|
| "make the poles teal", "use the school icon", "bigger dots" | **base appearance** | `appearance plan` / `set` |
| "show which ones are draft", "tell approved from proposed" | **second dimension** (a field) | `dimension plan` / `set` / `clear` |
| "show which way it flows", "it's invisible on satellite", "hatch the proposed areas" | **cartography** (no field) | `cartography plan` / `set` |

## Always

1. Find the ref: `ds style list --query <layer> --output json`. Tiled design
   layers are the `_vt` refs (`target: design_vt`); GeoJSON design layers are
   the bare `master/<layer>` refs. Style the one the user is looking at.
2. Read it: `ds style read --ref <ref> --output json`. It reports the layer
   type, the fields, `.data.onMap.types` and the channels this layer offers.
3. `plan` with the flags you intend, read the result back to the user, then
   `set` with the *same flags* plus `--yes`. Plan and set are one operation
   with `apply` false or true, so what you reviewed is what publishes. Report
   `.data.warnings` verbatim.
4. `ds style <command> --help` is the contract: bounds, closed choices and
   refusals live there, not here.

## Base appearance

`ds style appearance plan --ref <ref> --color '<#hex>' [--icon <catalog-name>]
[--size <number>]`. The application validates icon names and numeric bounds
against its live schema — do not guess icon names. A flat colour or icon
replaces a field-driven colour expression, so plan first.

## Second dimension — a field

`ds style dimension plan --ref <ref> --field <field> --channel halo --value
<highlight>=<px>:<#hex> --value <other>=0`. Take the field from
`.data.fields`, never `.data.colorField`. Type the value labels the way
`.data.onMap.types` says the map carries them.

`halo` differentiates hardest (a ring; on lines a hollow casing, on fills the
outline colour), `opacity` mutes, `size` scales. On raster symbol layers the
ring is baked into the icon by the backend. Name only the values that must
differ and route the rest through `--other` — an explicit `--other` is
intentional fallback coverage, not an uncovered-value warning. To undo:
`ds style dimension clear --ref <ref> --yes`.

## Cartography — no field

How the line or fill reads as a map. Use it instead of spending a colour or a
second dimension on something that is really a drawing convention.

- **Line type** — `--line-type solid|dashed|dotted|dash-dot|long-dash|dash-dot-dot|directional`.
- **Flow direction** — `--line-type directional` draws repeating arrows, not a
  dash, so it is one entry in that same closed choice. Size and space them
  with `--direction-size` and `--direction-spacing`. This is the answer to
  "which way does the water flow" and "which way does the feeder run".
- **Contrast casing** — `--casing-color '#0F172A' --casing-width 2` draws an
  outline under the line so it stays legible over satellite imagery.
  `--casing-width 0` removes it.
- **Hatching** — `--fill-pattern diagonal-forward|diagonal-back|crosshatch|dots`
  with `--pattern-color`, `--pattern-background` (an 8-digit hex such as
  `#FFFFFF00` keeps the fill see-through), `--pattern-spacing` and
  `--pattern-stroke`. This is how a proposed service area reads as proposed.
  `--fill-pattern solid` clears it.

`--pattern-spacing` is **4, 8, 16 or 32 only** — MapLibre tiles a pattern
image, and only a power-of-two tile repeats without a visible seam.

An omitted flag leaves that part of the document alone, so adjusting arrow
spacing on a line that is already directional is one flag. Do not send
direction detail together with a non-directional `--line-type`, or
`--pattern-*` together with `--fill-pattern solid`; both are refused as
`invalid_cartography`.
