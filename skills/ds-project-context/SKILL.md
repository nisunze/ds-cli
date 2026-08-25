---
name: ds-project-context
description: "Use the deployed DS CLI for project-scoped work: identify or switch the active project, discover the narrow command for an intent, and return a bounded project asset or operation receipt."
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

For a bulk native transformer import or composed project report delivery, read
[references/bulk-transformer-delivery.md](references/bulk-transformer-delivery.md).
Do not load that reference for ordinary project discovery or single-room work.

For a write with plan/apply commands, run the plan first and apply only with
the user's authority and the CLI-required `--yes`. A project-context switch is
a local app-state change, not authority for a project write; re-check project
scope immediately before every durable operation.

If multiple desktops are paired, require the intended descriptor rather than
choosing one. Where `ds` lives, how to read its envelope, and what to do when
it has no matching contract are the `ds` skill's rules; follow them here.
