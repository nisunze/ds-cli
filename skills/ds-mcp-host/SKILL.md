---
name: ds-mcp-host
description: Install the same `ds` executable as a compact chapter-based MCP server or one bounded typed workflow profile, then route through live command contracts.
---

# `ds` through an MCP host

Use this when the host discovers tools only through MCP. The server is the same
`ds` executable and reports the same source SHA as its packaged skills. It adds
no identity, project state, command schema, or authority.

## Choose one installation shape

For a general agent, install the broad default. It advertises `ds_catalog` and
ten operator-intent chapter routers (11 tools total), even as commands grow:

```text
ds mcp install --host vscode --output json
ds mcp install --host vscode --write --yes
```

For a narrow role, explicitly install one typed profile:

```text
ds mcp install --host claude-code --exposure commands --profile pls
```

Profiles are `grid`, `pls`, `survey`, `design-edit`, `design-run`, `map`,
`project`, `solar-run`, `solar-delivery`, and `operations`. Each publishes
`ds_catalog` plus at most 14 fully typed command tools. Do not install every
profile: that duplicates discovery and recreates selection ambiguity.

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

## Typed-profile routing

Use the advertised leaf tool directly after reading its schema and description.
Its title is the canonical command id; its result is the same CLI envelope.
Omitted commands are unavailable through that profile, not forwarded through a
generic call. Use `ds_catalog` only for bounded discovery inside the profile.

Keep `--exposure commands` without a profile only for temporary compatibility
with hosts configured for the previous command-per-tool surface.
