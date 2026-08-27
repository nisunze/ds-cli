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
| Chapter/profile views generated at startup from `ds capabilities` descriptors | A second command registry or hand-written command schema |
| One `ds <path> … --output json` process per `tools/call`, envelope returned verbatim | A cache, a batch, or a "convenience" tool the CLI lacks |
| Paired with the running DS GridDesign exactly as a terminal would be | A credential, a listener, or a network hop |

The `mcp` domain excludes itself from the tool list. Chapter classification is
declared once on every canonical command and appears in its live descriptor.

## Exposure modes

The default broad server publishes 11 stable tools: `ds_catalog` plus project,
grid-model, PLS-CADD, survey, design, map-presentation, vector-tile, Solar,
report, and operations routers. Adding a command does not enlarge this list.

```text
ds mcp serve --exposure chapters
```

Use `ds_catalog` for a bounded query or chapter listing. Call the selected
chapter with `operation: "describe"` and the exact command id, then call the
same chapter with `operation: "invoke"` and descriptor-conforming `arguments`.
Unknown and wrong-chapter ids never become argv; the refusal names the correct
router when one exists.

Specialized profiles publish conventional typed leaf tools for one workflow:

```text
ds mcp serve --exposure commands --profile pls
```

Profiles are `grid`, `pls`, `survey`, `design-edit`, `design-run`, `map`,
`project`, `solar-run`, `solar-delivery`, and `operations`. Each includes
`ds_catalog` and at most 14 leaf tools. A profile is only an allowlist: omitted
commands are unavailable and authority, effects, confirmation, output, and
refusals are unchanged. Plain `--exposure commands` retains the previous
all-command publication temporarily for compatibility.

## Confirmation

The CLI requires `--yes` for effectful commands. Typed leaf tools declare
`confirm`; chapter calls place it at the outer envelope, never inside nested
arguments. `confirm: true` maps onto `--yes` only when that exact live command
requires it. Read-only commands reject confirmation rather than forwarding it.
Without confirmation, the CLI's typed refusal returns unchanged.

## Reading a result

Every result is the CLI envelope: branch on `status`, read `data` on `ok`,
and follow `error.remedy` / `error.next` on anything else. Tool descriptions
carry the command's effect, authority, and the refusals it can name.

## Installing the host entry

```
ds mcp install                       # print the VS Code entry and its file
ds mcp install --write --yes         # merge it into the VS Code user profile
ds mcp install --host claude-code    # other hosts: claude-code, cursor, codex, generic
ds mcp install --host claude-code --exposure commands --profile pls
```

The entry points at **this** executable and belongs in the **user** profile
— `%APPDATA%\Code\User\mcp.json` on Windows, `~/.config/Code/User/mcp.json`
on Linux — never in a workspace file. The server must run on the PC where DS
GridDesign is installed and paired; a workspace file travels to machines that
have neither. With both Stable and Canary installed, run `install` from the
`ds` of the app you use, and keep one server entry.

Codex keeps TOML: `install --host codex` prints the data; translate it into
`~/.codex/config.toml` under `[mcp_servers.ds]`. The install receipt and MCP
initialize result report this executable's source SHA; it must match the skill
bundle SHA reported by `ds doctor`.

## Verifying

`ds doctor` reports the executable and skills. In the broad server, confirm
`tools/list` returns 12 tools, use `ds_catalog` to route `shell.status`, then
describe and invoke it through `ds_operations`. A paired-desktop command can
then prove pairing. In a typed profile, call an advertised read-only leaf.
