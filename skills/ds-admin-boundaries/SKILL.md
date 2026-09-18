---
name: ds-admin-boundaries
description: Read exact Rwanda administrative units and optionally materialize one authority polygon as a styled Desktop-local layer through ds. Use for province-to-village lookup or exact boundary map evidence, not elevation attachment.
metadata:
  ds-chapters: data
  ds-mcp-profile: admin-bounds
---

# Read exact administrative boundaries

Use the deployed CLI and read the selected command contract before invoking it.

1. Traverse one exact hierarchy leg with `ds data admin-bounds list`. Province
   is the root; each lower level requires its immediate parent code.
2. Select only a returned code, then use `ds data admin-bounds read --code
   <code>` for bounded geometry evidence.
3. Add `--geometry-out <path.geojson>` only when the task needs the polygon
   itself. The receipt otherwise reports identity, bounds, coordinate count and
   digest, because a boundary's coordinates are not a terminal answer. The file
   is one feature carrying `name`, `code` and `level`, which `ds map local
   register --geometry polygon` takes as it stands. An existing path is
   refused; nothing is overwritten.

These reads need no project and no window: `.data.scope.project` is `null`
because a country's boundaries belong to the country. A boundary is reference
evidence, never saved project data. If the authority refuses or is unavailable,
stop: do not substitute sampled points, a lattice, a bounding rectangle, or
another approximate geometry.

Use `ds data admin-bounds attach` only when enriching a caller-owned point
file with province-to-village attributes; it is a separate local-file write.

Stops at: a boundary DS does not hold — another country, or a disputed edge.
That is the operator's authoritative gazetteer, never an approximation made here.
