# MCP installation descriptor migration

`mcp.install` contract v4 preserves the default VS Code proposal and all six
existing host tokens. Its JSON result includes the canonical
`connection`, `supported_hosts`, full `build`, skill-bundle source SHA,
transport, required environment, change state, and restart handoff. Version 4
adds the verified persistent Codex TOML merge; JSON host behavior is unchanged.

Read-only discovery no longer needs confirmation:

```text
ds mcp install --output json
ds mcp install --host cursor --output json
```

Writing still requires both flags:

```text
ds mcp install --host cursor --write --yes
```

Claude Desktop on Windows writes only
`mcpServers.ds` in `%APPDATA%\Claude\claude_desktop_config.json`; restart
Claude Desktop after installation. Codex now losslessly merges only
`[mcp_servers.ds]` into `~/.codex/config.toml`; `generic` remains print-only.

Callers that consumed the old top-level `host`, `entry`, `file`, `written`,
`executable`, `source_sha`, `dirty`, `exposure`, and `profile` fields can keep
doing so. New callers should retain the complete `connection` and installation
receipt for identity checks.
