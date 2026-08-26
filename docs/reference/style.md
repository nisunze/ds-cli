# `ds style` — reference

Tier-4 reference. `ds style <command> --help` is the contract; this document is
the part that is true of every command.

## Where a style is

One MapLibre document per style ref, stored and validated by ds-brain and
rendered by the paired application's Style Center. `ds` holds no token and no
second styling model: every command is one named operation the application
performs under its own session, with the same pure module its Second-dimension
panel uses and the same governed save payload its *Save globally* button sends.
That is why there is no `--project` flag: the active project is the one the
application has open.

## Two dimensions, one document

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

## The shape of a session

```bash
ds style list --query lv_poles                      # choose the ref the visible layer uses
ds style read --ref master/lv_poles                 # bare ref = Design GeoJSON
ds style dimension plan --ref master/lv_poles \
  --field drafting_status --channel halo --value draft=3:#FFFFFF --other 0
ds style dimension set   --ref master/lv_poles \
  --field drafting_status --channel halo --value draft=3:#FFFFFF --other 0 --yes
ds style dimension clear --ref master/lv_poles --yes
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
- **The colour field is refused** as the second field; pick another.
- **Raster symbol halos are baked**: ds-brain mints one runtime image per
  (icon, colour, halo) and nests the halo match around the colour unroll. The
  authored document keeps the spec-pure `icon-halo-*`; `icon-halo-blur` is
  refused because it has no raster equivalent.

## What is deliberately absent

Raw document writes. The Style Center's JSON tab is the human escape hatch; a
CLI door for arbitrary paint would bypass every invariant above.
