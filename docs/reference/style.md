# `ds style` — reference

Tier-4 reference. `ds style <command> --help` is the contract; this document is
the part that is true of every command.

## Where a style is

One MapLibre document per style ref, stored and validated by ds-brain and
rendered by the paired application's Style Center. `ds` holds no token and no
second styling model: every command is one named operation the application
performs under its own session, with the same guided appearance and
Second-dimension modules the UI uses and the same governed save payload its
*Save globally* button sends.
That is why there is no `--project` flag: the active project is the one the
application has open.

## The three axes of a document

A style document carries three independent axes, and every command belongs to
exactly one of them. Choosing the wrong axis is the most expensive mistake
here, because two of them are paid for in fields:

| axis | command family | driven by | what it answers |
|---|---|---|---|
| base appearance | `appearance plan/set` | nothing — one flat value | what colour, icon and size is this layer? |
| second dimension | `dimension plan/set/clear` | a second field | which features differ, and how do I see it? |
| cartography | `cartography plan/set` | nothing — one flat value | how does this line or fill *read as a map*? |

Cartography is field-free, which is the whole point of having it: a proposed
service area can be hatched, a water main can carry flow arrows and an MV line
can be cased for contrast without spending the colour dimension or the second
dimension on it. The three compose — none replaces another.

## Guided appearance

`appearance plan` and `appearance set` author a layer's flat colour, raster
symbol icon and base size through the live Style Center schema. They accept
only properties that make sense for the layer type and validate icon names and
numeric bounds against the open application before publishing.

## Two field-driven dimensions

A layer's PRIMARY categorical dimension is its colour — `struct_type →
circle-color` — authored by the Style Center's colour builder. The SECOND
dimension tells another field apart on a channel the colour does not use:

| channel   | circle                              | symbol (raster icon)                         | line                 | fill                 |
|-----------|-------------------------------------|----------------------------------------------|----------------------|----------------------|
| `halo`    | `circle-stroke-width` + `-color`    | `icon-halo-width` + `-color`, baked by ds-brain | `line-gap-width` (casing) | `fill-outline-color` |
| `opacity` | `circle-opacity`                    | `icon-opacity`                               | `line-opacity`       | `fill-opacity`       |
| `size`    | `circle-radius`                     | `icon-size`                                  | `line-width`         | —                    |

Everything written is a plain `["match", ["get", field], v1, out1, …, fallback]`
on those properties. There is no metadata flag; the legend, the Current State
summary and `ds style read` all read the expression shape back.

## Cartography

`cartography plan` and `cartography set` author the third axis. Four
instructions, each a name rather than a value the CLI composes:

| instruction | flags | applies to |
|---|---|---|
| line type | `--line-type` | lines |
| flow direction | `--direction-size`, `--direction-spacing` | lines, with `--line-type directional` |
| contrast casing | `--casing-color`, `--casing-width` | lines |
| fill hatching | `--fill-pattern`, `--pattern-color`, `--pattern-background`, `--pattern-spacing`, `--pattern-stroke` | fills |

**Line type** is one of `solid`, `dashed`, `dotted`, `dash-dot`, `long-dash`,
`dash-dot-dot` or `directional`. The dash names are ds-brain's published
vocabulary, not a dash array composed here — that is deliberate, because the
backend table and the editor's offline fallback had already drifted once, and
a document authored against one tuple was then named by the other.

**`directional` is not a dash.** It draws repeating arrow markers along the
line — which way the water flows, which way the feeder runs — so it is
mutually exclusive with a dash pattern and is one entry in the same closed
choice. `--direction-size` (6..48 px) and `--direction-spacing` (20..1000 px)
size and space those arrows.

**Casing** is the contrast outline drawn under a line. `--casing-width`
(0..20 px, halves allowed) and `--casing-color` are what make a bright design
line legible over satellite imagery; `--casing-width 0` removes it again.

**Hatching** rasterises a repeating tile: `--fill-pattern` is one of `solid`,
`diagonal-forward`, `diagonal-back`, `crosshatch` or `dots`, with
`--pattern-color` for the strokes, `--pattern-background` behind them (use an
8-digit hex such as `#FFFFFF00` to keep the fill see-through),
`--pattern-stroke` (1..6 px, whole pixels — a rasterised tile aliases
otherwise) and `--pattern-spacing` for the tile size.

**`--pattern-spacing` is 4, 8, 16 or 32 — nothing else.** MapLibre repeats a
pattern by tiling its image, and only a power-of-two tile meets its neighbour
without a visible seam at every edge. This is enforced as a closed choice, so
`--pattern-spacing 12` is refused by the parser with `invalid_choice` before
the application is contacted at all.

An omitted flag leaves that part of the document alone, so adjusting arrow
spacing on a line that already carries `directional` is one flag. Two
combinations are refused locally with `invalid_cartography`, because no
document state could make them right: direction detail sent in the same call
as a non-directional `--line-type`, and `--pattern-*` detail sent with
`--fill-pattern solid`, which is the instruction that removes the hatch. A
call with no cartography flag at all is refused the same way.

## The shape of a session

```bash
ds style list --query lv_poles                      # choose the ref the visible layer uses
ds style read --ref master/lv_poles                 # bare ref = Design GeoJSON
ds style appearance plan --ref gt/secondary_schools \
  --color '#008695' --icon school --size 1.2
ds style appearance set --ref gt/secondary_schools \
  --color '#008695' --icon school --size 1.2 --yes
ds style dimension plan --ref master/lv_poles \
  --field drafting_status --channel halo --value draft=3:#FFFFFF --other 0
ds style dimension set   --ref master/lv_poles \
  --field drafting_status --channel halo --value draft=3:#FFFFFF --other 0 --yes
ds style dimension clear --ref master/lv_poles --yes

# Water flow: arrows along the main, colour untouched.
ds style cartography set --ref master/water_mains \
  --line-type directional --direction-size 14 --direction-spacing 140 --yes
# Satellite contrast: a dark casing keeps a bright line legible over imagery.
ds style cartography set --ref master/mv_lines \
  --casing-color '#0F172A' --casing-width 2 --yes
# Proposed service areas, crosshatched over a see-through background.
ds style cartography set --ref master/service_areas \
  --fill-pattern crosshatch --pattern-color '#B45309' \
  --pattern-background '#FFFFFF00' --pattern-spacing 8 --pattern-stroke 1 --yes
```

`plan` and `set` are one operation with `apply` false or true — what you
reviewed is what is published.

`master/lv_poles` and `master/customers` are the bare Design GeoJSON refs.
Their tiled counterparts end in `_vt` and use target `design_vt`. Pick the ref
reported for the layer the operator is actually viewing; changing a bare ref
does not require or justify a retile.

## What the application enforces for you

- **One label type per match.** The map may carry a field as numbers while a
  schema domain lists a string sentinel; a mixed match is invalid in MapLibre
  and would silently leave the old paint. Labels are typed from the values the
  map currently renders (`.data.onMap.types`) and strays fall to the fallback.
- **Never an arm-less match** — zero values collapse to the flat fallback.
- **One second dimension per ref** — setting a different channel replaces it.
- **Base-size changes preserve a size dimension** — when size is the second
  channel, `appearance set --size` changes its fallback instead of flattening
  the authored match arms.
- **Explicit fallback means covered** — values intentionally routed through
  `--other` are reported as fallback coverage, not as false uncovered-value
  warnings.
- **The colour field is refused** as the second field; pick another.
- **Raster symbol halos are baked**: ds-brain mints one runtime image per
  (icon, colour, halo) and nests the halo match around the colour unroll. The
  authored document keeps the spec-pure `icon-halo-*`; `icon-halo-blur` is
  refused because it has no raster equivalent.
- **Cartography is composed by the application**, not by `ds`. The dash tuple
  behind a line type, the arrow marker image, and the pattern tile are all
  minted there, from the same vocabulary the Style Center's own controls use.
  A layer type with no place for a property — a fill pattern on a line, a
  casing on a fill — is refused as `desktop_refused`; `ds style read` reports
  the layer type before you ask.

## What is deliberately absent

Raw document writes, on every axis. The Style Center's JSON tab is the human
escape hatch; a CLI door for arbitrary paint would bypass every invariant
above. There is no flag that takes a dash array, a pattern image, a marker
sprite or a paint expression, and adding one would be the first thing to
reject in review.
