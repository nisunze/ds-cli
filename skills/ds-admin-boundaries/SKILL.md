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
3. Add `--to-map` only when the task needs the exact authority polygon in the
   Desktop. It uses the ordinary derived local-layer and hierarchy Style Center
   path; it does not send coordinates through the CLI. A successful receipt is
   returned only after the Desktop-local IndexedDB commit is acknowledged.

Treat `.data.scope.project: null` as national-reference ownership and
`.data.desktop_active_project` only as UI context. A materialized boundary is
Desktop-local evidence, never saved project data. If the authority refuses or
is unavailable, stop: do not substitute sampled points, a lattice, a bounding
rectangle, or another approximate geometry.

Use `ds data admin-bounds attach` only when enriching a caller-owned point
file with province-to-village attributes; it is a separate local-file write.
