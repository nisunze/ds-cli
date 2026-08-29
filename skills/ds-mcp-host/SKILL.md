---
name: ds-mcp-host
description: Install and use compact chapter or typed-profile MCP surfaces from the live `ds` executable.
---

# `ds` through an MCP host

Use this when the host discovers tools only through MCP. The server is the same
`ds` executable and reports the same source SHA as its packaged skills. It adds
no identity, project state, command schema, or authority.

## Choose one installation shape

For a general agent, install the broad default. It advertises `ds_catalog` and
eleven operator-intent chapter routers (12 tools total), even as commands grow:

```text
ds mcp install --host vscode --output json --yes
ds mcp install --host vscode --write --yes
```

`install` changes this machine's integration settings, so dispatch requires
`--yes` on every invocation, including the print-only one. `--write` is what
decides whether the host file is edited.

For a narrow role, explicitly install one typed profile:

```text
ds mcp install --host claude-code --exposure commands --profile pls --write --yes
```

Profiles are `grid`, `pls`, `pls-library`, `library-governance`, `survey`,
`form-factory`, `survey-projects`, `design-edit`, `design-run`, `map`, `layers`,
`tiling`, `project`, `solar-run`, `solar-delivery`, and `operations`. `survey`
retains the map/local-data workflow;
`form-factory` owns global schemas and `survey-projects` owns project-form
settings, reusable templates, and create-from-template. Each publishes
`ds_catalog` plus at most 14 fully typed command tools. Do not install every
profile: that duplicates discovery and recreates selection ambiguity.

Use `pls` for backup recovery, workspace diagnostics and native delivery; use
`pls-library` for local immutable library verification, packing, seeding and
native resolution; use `library-governance` for global library/example upload,
publication and lifecycle. The split keeps every surface bounded. They project
live descriptors and never publish a generic
filesystem, process, PowerShell, Win32, or PLS-CADD UI escape hatch. Find a
changed/new operation through `ds_catalog` instead of guessing its generated
tool name.

The entry belongs in the user profile on the PC where DS GridDesign is
installed and paired, never a travelling workspace file. Run installation
from the exact Stable, Canary, or development `ds` you intend the host to use.
The receipt's `source_sha` must match `ds doctor`'s skill-bundle SHA.

## Broad-server routing

1. Call `ds_catalog` with a bounded query or chapter.
2. Call the returned chapter tool with `operation: "describe"` and the exact
   canonical command id.
3. Call that same chapter with `operation: "invoke"`, descriptor-conforming
   `arguments`, and top-level `confirm: true` only when user intent authorizes
   the exact effect and the descriptor requires confirmation.
4. Branch on the returned DS envelope. Follow `error.remedy` and `error.next`;
   never retry unchanged or reconstruct a refusal through another surface.

A wrong-chapter command returns the matching router. Unknown properties and
arbitrary argv are refused before dispatch. Project and desktop identity remain
owned by the selected command, not by the MCP session.

MCP reads the selected command's live `authority` descriptor before an
`invoke`: `none` stays headless and never starts DS GridDesign. A paired
authority may make one bounded Windows installed-app launch only when no
session and no explicit `desktop-descriptor` are present. Discovery and
`describe` never launch the app. If the app is signed out or cannot pair in
the bounded wait, follow the returned DS refusal and remedy; do not retry by
inventing another descriptor or identity.

## Typed-profile routing

Use the advertised leaf tool directly after reading its schema and description.
Its title is the canonical command id; its result is the same CLI envelope.
Omitted commands are unavailable through that profile, not forwarded through a
generic call. Use `ds_catalog` only for bounded discovery inside the profile.

Keep `--exposure commands` without a profile only for temporary compatibility
with hosts configured for the previous command-per-tool surface.
