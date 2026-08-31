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

Adding a host requires a verified user-level path and root, a table entry,
entry/path/platform/merge tests, reference documentation, and LF-normalized
sources. It must not change `mcp serve` or create a second MCP schema.
