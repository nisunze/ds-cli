# MCP client adapter contract

`ds mcp install` has one host-neutral connection descriptor. It binds the
absolute running executable, fixed `mcp serve` arguments, stdio transport,
exposure, optional typed profile, server name, complete build identity,
verified skill-bundle source SHA, and required environment. A client adapter
may only transform that descriptor into a verified host configuration shape.
It does not discover tools, serve MCP, select a different executable, or add
arguments.

Each adapter declares one stable host token, display name, supported operating
systems, configuration root, merge support, and restart requirement. Its path
resolver returns only a verified user-level target. No adapter may write a
workspace configuration or guess an undocumented target.

The verified adapters are:

| Token | Root | User-level target | Automatic merge |
|---|---|---|---|
| `vscode` | `servers` | platform VS Code user `mcp.json` | yes |
| `claude-code` | `mcpServers` | `~/.claude.json` | yes |
| `claude-desktop` | `mcpServers` | Windows `%APPDATA%\Claude\claude_desktop_config.json` | yes |
| `codex` | `mcp_servers` | `~/.codex/config.toml` | yes; lossless TOML merge |
| `cursor` | `mcpServers` | `~/.cursor/mcp.json` | yes |
| `gemini-cli` | `mcpServers` | `~/.gemini/settings.json` | yes; conflict-safe |
| `windsurf` | `mcpServers` | `~/.codeium/windsurf/mcp_config.json` | yes; conflict-safe |
| `github-copilot` | `mcpServers` | `~/.copilot/mcp-config.json` | yes; conflict-safe |
| `generic` | `mcpServers` | none | no; print only |

Claude Desktop is deliberately Windows-only until another platform path and
shape are verified. A non-Windows `ds` refuses that adapter rather than
printing or writing an entry that cannot launch the selected executable.

Automatic JSON merge replaces only the `ds` member under the adapter's root,
preserves sibling servers and unrelated root settings, stages beside the
target, verifies that the source did not race, retains a backup, and renames
atomically. Repeating the same install is idempotent. Malformed roots,
symlinks/reparse points, special files, unsupported writes, unknown hosts,
unresolved profile paths, and executable/host OS mismatches are refusals.

The Codex adapter applies the same target, staging, race, backup and atomicity
policy to TOML. It losslessly adds `[mcp_servers.ds]`, preserving unrelated
tables, comments and formatting. An existing exact command/args match is
idempotent. A different, partial or structurally ambiguous `ds` entry is a
conflict with existing/proposed previews and is never overwritten.

Gemini CLI and Windsurf use the plain JSON `command`/`args` dialect. GitHub
Copilot CLI uses its local-server dialect: `type: local`, `command`, `args`, an
empty `env`, and `tools: ["*"]`. Their verified user-level paths are identical
across Windows, macOS and Linux. Each preserves sibling servers and unrelated
root settings, treats an exact `ds` match as idempotent, and refuses a
non-identical existing `ds` entry rather than replacing it. Cline is not an
adapter while its CLI/global-storage schema remains unsettled.

Adding a host requires a verified user-level path and root, a table entry,
entry/path/platform/merge tests, reference documentation, and LF-normalized
sources. It must not change `mcp serve` or create a second MCP schema.

## Adapter verification sources

The user paths and dialects for the added guarded adapters are pinned to their
client owners' documentation:

- [Gemini CLI MCP configuration](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md)
- [Windsurf Cascade MCP configuration](https://docs.windsurf.com/windsurf/cascade/mcp)
- [GitHub Copilot CLI MCP configuration](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-mcp-servers)

[Cline's current configuration documentation](https://docs.cline.bot/getting-started/config)
names moving global storage under `~/.cline/data/settings`; it does not yet
justify a DS-owned blind merge adapter. Its supported `cline mcp` surface can
be reconsidered when that on-disk contract is explicit and stable.
