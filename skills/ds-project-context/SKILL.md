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
command that needs a rendered map. Project Management and `ds report project`
commands take required `--project` on every request and ignore the CLI's saved
selection. Other headless commands may still use `ds auth project use`; read
their live descriptor before calling them.
Switching one context never switches the other; read the descriptor's
`authority` and project argument before a durable operation. For the
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

Some per-project operator state is kept on the host, per lane, DS account and
project, and is neither of those contexts: the layer drawer's remembered
visibility (`map.layer.list|show|hide`) and the working area's form selection
(`survey.working-area.forms|select|clear`). Both take `--project` for one call
(the saved CLI selection when omitted) and `--target server` for the running
Server; selecting a project never changes them, and they never change a
selection.

What this machine holds for a project — store by store, retained or
cleanable — is `workstation.local-data.status --project <id>`; free the
cleanable replicas (synced survey photos, verified sync downloads) with
`workstation.local-data.clean --project <id> --yes`. Both read the Server's
state root with no credential and no running Server; the browser answers the
same command over its own stores. Never clean by deleting files or IndexedDB.

Cloud-resident reference data — land parcels by UPI and existing customer
connections — is never seeded, installed or downloaded (`dataset_cloud_only`);
read it where it lives with `data.upi.lookup --upi <id>` and
`data.customers.query --village|--cell|--bbox|--transformer …`, both bounded
and receipted, against the selected project.

Touched version and attachment boundaries instead require explicit `--project`:
`design.version.*` and `design.attachment.*` capture that project for native
authorization and never consult the Web active project or require Desktop.
Check each live descriptor; do not switch a saved project to satisfy these calls.

Stops at: the operator's authority — a project outside the audience fence is
granted to the identity, never switched to from here.
