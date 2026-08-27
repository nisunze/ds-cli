---
name: ds-mcp-host
description: Reach `ds` from a host that discovers tools only through the Model Context Protocol — install the host entry that launches `ds mcp serve`, then use the generated tools as `ds` commands.
---

# `ds` through an MCP host

Some hosts (VS Code agent mode, GitHub Copilot, Claude Desktop, Cursor,
Codex) learn tools only through MCP. `ds mcp serve` is that transport, from
the same executable, and nothing exists there that `ds` lacks: every tool is
one command built at startup from `ds capabilities`.

## Install the host entry

```
ds mcp install --output json                 # print the VS Code entry + its file
ds mcp install --host vscode --write --yes   # merge it into the user profile
ds mcp install --host claude-code            # claude-code, cursor, codex, generic
```

The entry belongs in the **user** profile on the PC where DS GridDesign is
installed and paired — never a workspace file, which travels to machines
that have neither. With Stable and Canary both installed, run it from the
`ds` of the app you use and keep one entry.

## Use the tools

- Tool names are command ids with `.` → `_` (`map_design_report`); the
  title is the id, and the description carries effect, authority and
  refusals.
- Effectful tools require `confirm: true`, which maps onto `--yes`. Pass it
  only when the user's intent authorizes exactly that effect and scope.
- Every result is the CLI envelope: branch on `status`; on a refusal follow
  `error.remedy` and `error.next` — do not retry unchanged.
- Discovery still happens through the `ds` skill: read a tool's description
  as you would `ds capabilities <id>`, and prefer the narrowest command.

`ds doctor` reports the executable; a read-only tool such as `shell_status`
answers without a paired desktop, and a paired one such as
`map_design_list` proves the pairing.
