# `ds mcp` — reference

Tier-4 reference. `ds mcp <command> --help` is the contract.

## The problem it solves

Some agent hosts — VS Code agent mode, GitHub Copilot, Claude Desktop and
Claude Code, Cursor, Codex — learn tools only through the Model Context
Protocol: a JSON-RPC server on stdio that answers `tools/list` and
`tools/call`. Skills reach the hosts that read skills; MCP reaches the rest.
`ds mcp` gives those hosts the command line **without a second product
surface**.

## What the server is, and is not

| It is | It is not |
|---|---|
| The same `ds` executable, launched with `mcp serve` by the host | A separate binary, sidecar or service |
| A tool list generated at startup from `ds capabilities`, descriptor by descriptor | A hand-written tool table that can drift |
| One `ds <path> … --output json` process per `tools/call`, envelope returned verbatim | A cache, a batch, or a "convenience" tool the CLI lacks |
| Paired with the running DS GridDesign exactly as a terminal would be | A credential, a listener, or a network hop |

The `mcp` domain excludes itself from the tool list.

## Confirmation

The CLI requires `--yes` for effectful commands, and an MCP host cannot
press a prompt. Every such tool therefore declares one extra boolean property,
`confirm`. `confirm: true` maps onto `--yes`; anything else runs the command
without it, and the CLI refuses exactly as it would on a terminal. The
refusal — code, remedy, next command — comes back to the host as the tool
result with `isError: true` and the full envelope under `structuredContent`.

## Reading a result

Every result is the CLI envelope: branch on `status`, read `data` on `ok`,
and follow `error.remedy` / `error.next` on anything else. Tool descriptions
carry the command's effect, authority, and the refusals it can name.

## Installing the host entry

```
ds mcp install                       # print the VS Code entry and its file
ds mcp install --write --yes         # merge it into the VS Code user profile
ds mcp install --host claude-code    # other hosts: claude-code, cursor, codex, generic
```

The entry points at **this** executable and belongs in the **user** profile
— `%APPDATA%\Code\User\mcp.json` on Windows, `~/.config/Code/User/mcp.json`
on Linux — never in a workspace file. The server must run on the PC where DS
GridDesign is installed and paired; a workspace file travels to machines that
have neither. With both Stable and Canary installed, run `install` from the
`ds` of the app you use, and keep one server entry.

Codex keeps TOML: `install --host codex` prints the data; translate it into
`~/.codex/config.toml` under `[mcp_servers.ds]`.

## Verifying

`ds doctor` already reports the executable and skills. For the MCP side:
open the host's MCP view, confirm the `ds` server started, and call
`shell_status` — a read-only tool that answers from this machine without a
paired desktop. A paired-desktop tool such as `map_design_list` proves the
pairing.
