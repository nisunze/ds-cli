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
`ds_diagnostics` plus twelve operator-intent chapter routers (14 tools total),
even as commands grow:

```text
ds mcp install --host vscode --output json
ds mcp install --host vscode --write --yes
```

The first command is a read-only proposal and also reports `supported_hosts`.
`--write` selects the machine-setting change, so dispatch then requires
`--yes` before adapter code runs.

For a narrow role, explicitly install one typed profile:

```text
ds mcp install --host claude-code --exposure commands --profile pls --write --yes
```

Profiles are `grid`, `pls`, `pls-library`, `library-governance`, `survey`,
`form-factory`, `survey-projects`, `survey-migration`, `design-edit`,
`design-run`, `map`, `layers`, `tiling`, `project`, `solar-input`, `solar-run`,
`solar-delivery`, and `operations`. `survey` retains the map/local-data workflow;
`form-factory` owns global schemas and `survey-projects` owns governed
aggregate/spatial reads, project-form settings, reusable templates, and
create-from-template. `survey-migration` isolates governed bulk import, while
`solar-input` isolates selected-project input capture. Each profile publishes
both bootstrap tools, `ds_catalog` and `ds_diagnostics`, plus bounded fully
typed leaves. Do not install every profile: that duplicates discovery and
recreates selection ambiguity.

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
On Windows, `--host claude-desktop` targets Claude Desktop's verified
user-level `mcpServers` configuration and Claude Desktop launches `ds.exe`
directly after restart; VS Code is not involved.
For Codex, first inspect `ds mcp install --host codex --output json`, then run
`ds mcp install --host codex --write --yes`. It preserves unrelated TOML and
refuses a conflicting `mcp_servers.ds`; after a changed write, fully quit and
restart Codex and begin a new agent session. VS Code is not involved.

## MCP-only bootstrap

After initialization, call `ds_diagnostics` with `operation: "identity"` to
confirm the absolute executable, version/source SHA, target, build/install
profiles, selected MCP exposure/profile, and shipped skill-bundle identity.
`doctor`, `shell.status`, and the bounded capabilities index are available
through the same read-only diagnostics tool when the agent has no shell.

Skill guidance is lazy. Call `resources/list`, select one receipt-listed
`ds-skill://bundle/<skill>/SKILL.md` identifier, then call `resources/read` for
that exact resource. Read the smallest receipt-current skill that governs the
task; do not preload every skill or require a writable local skills directory.
The same structured identity and skill-resource availability are echoed by
`ds_catalog` after reconnects.

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
`invoke`: `none`, `headless_user`, and `headless_project` never start DS
GridDesign. A paired authority may make one bounded Windows installed-app
launch, lazily, only when no session and no explicit `desktop-descriptor` are
present. Discovery, diagnostics, resources, and `describe` never launch the
app. If the app is signed out or cannot pair in the bounded wait, follow the
returned DS refusal and remedy; do not retry by inventing another descriptor
or identity.

## Typed-profile routing

Use the advertised leaf tool directly after reading its schema and description.
Its title is the canonical command id; its result is the same CLI envelope.
Omitted commands are unavailable through that profile, not forwarded through a
generic call. Use `ds_catalog` only for bounded discovery inside the profile.
Pass `confirm: true` only when the invocation's live descriptor conditionally
requires confirmation and the user's intent authorizes that exact effect and
scope.

Keep `--exposure commands` without a profile only for temporary compatibility
with hosts configured for the previous command-per-tool surface.
