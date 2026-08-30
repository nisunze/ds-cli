# MCP installation descriptor migration

`mcp.install` contract v3 preserves the default VS Code proposal and all five
existing host tokens. Its JSON result now also includes the canonical
`connection`, `supported_hosts`, full `build`, skill-bundle source SHA,
transport, and required environment.

Read-only discovery no longer needs confirmation:

```text
ds mcp install --output json
ds mcp install --host cursor --output json
```

Writing still requires both flags:

```text
ds mcp install --host cursor --write --yes
```

Claude Desktop on Windows is the first added adapter. It writes only
`mcpServers.ds` in `%APPDATA%\Claude\claude_desktop_config.json`; restart
Claude Desktop after installation. Codex and `generic` remain print-only.

Callers that consumed the old top-level `host`, `entry`, `file`, `written`,
`executable`, `source_sha`, `dirty`, `exposure`, and `profile` fields can keep
doing so. New callers should retain the complete `connection` and installation
receipt for identity checks.
