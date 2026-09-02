---
name: ds-project-context
description: "Identify or switch the active DS project, discover one narrow command, and return a bounded asset or operation receipt."
metadata:
  ds-chapters: project
  ds-mcp-profile: project
---

# Select and work in the active DS project

Treat each CLI command as a declarative contract. Do not model its UI, API
sequence, cache, IndexedDB, Svelte, WASM, or backend implementation.

1. Run `ds desktop status --output json` and use its exact active project. If
   the user requested another project, or the current project conflicts with
   their declared scope, discover the project-list and project-switch command
   descriptors. List bounded visible projects, switch only to the exact id the
   user intended, then run status again and require the exact resulting id.
   Never switch projects merely to make a failing command succeed.
2. When the command is not already known, discover it the way the `ds` skill
   describes: search, then read only that command's descriptor.
3. Invoke the narrowest command and return its bounded result.

Two project contexts exist and they are not the same thing. The paired
application's visible project (`ds desktop status`) governs every `map.*`
command and paired write. The CLI-selected project (`ds auth project use`,
`ds auth project status`) governs every `headless_project` command — tiling,
background reports, transformer inventory and retirement — with no map, room
or Desktop. Switching one never switches the other; read the descriptor's
`authority` and check the matching context before a durable operation. For the
background family, read
[references/background-project-operations.md](references/background-project-operations.md).

For a bulk native transformer import or composed project report delivery
through the paired application, read
[references/bulk-transformer-delivery.md](references/bulk-transformer-delivery.md).
Do not load that reference for ordinary project discovery or single-room work.

For a write with plan/apply commands, run the plan first and apply only with
the user's authority and the CLI-required `--yes`. A project-context switch is
a local app-state change, not authority for a project write; re-check project
scope immediately before every durable operation.

If multiple desktops are paired, require the intended descriptor rather than
choosing one. Where `ds` lives, how to read its envelope, and what to do when
it has no matching contract are the `ds` skill's rules; follow them here.
