# MCP installation descriptor migration

`mcp.install` contract v6 gives every supported host the same package-specific
server identity. Its JSON result includes the canonical `connection`,
`supported_hosts`, full `build`, skill-bundle source SHA, transport, required
environment, change state, restart handoff, protocol `server_name`, human
`server_title`, derived `registration_name`, compile-time `release_lane`, and
detected `runtime_platform`. Version 4 added the persistent Codex TOML merge.
Version 5 added Gemini CLI, Windsurf and GitHub Copilot CLI with guarded JSON
registration. Version 6 applies that guarded behavior to every automatic JSON
adapter, adds Google Antigravity at its distinct
`~/.gemini/config/mcp_config.json` target, and replaces the generic host key
with a lane/platform key.

Read-only discovery no longer needs confirmation:

```text
ds mcp install --output json
ds mcp install --host cursor --output json
```

Writing still requires both flags:

```text
ds mcp install --host cursor --write --yes
```

For example, Stable on native Windows proposes
`dsGridDesignStableWindows`, `ds-stable-windows`, and
`DS GridDesign — Stable on Windows`. Canary and Stable therefore coexist in
one host configuration. WSL uses its own identity only when kernel-release
evidence confirms WSL. The lane is stamped by the build and cannot be selected
by an install flag. Canonical MCP tool names and DS command ids are unchanged.

Claude Desktop on Windows writes only its derived member under `mcpServers` in
`%APPDATA%\Claude\claude_desktop_config.json`; restart Claude Desktop after
installation. Codex losslessly merges only its derived table under
`mcp_servers` into `~/.codex/config.toml`; `generic` remains print-only.

On the first v6 install, an existing generic `ds` member/table is migrated to
the derived key only when its entire JSON server object, or Codex command/args,
exactly matches this proposal. If it differs, installation refuses with
`mcp_config_conflict` and leaves the file untouched. Existing lane/platform
siblings are preserved, so installing Canary never replaces Stable.

Callers that consumed the old top-level `host`, `entry`, `file`, `written`,
`executable`, `source_sha`, `dirty`, `exposure`, and `profile` fields can keep
doing so. New callers should retain the complete `connection` and installation
receipt for identity checks.
