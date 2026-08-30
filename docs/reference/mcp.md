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
| Chapter/profile views generated at startup from unchecked `ds capabilities` schemas; live availability resolves on catalogue describe or invoke | A second command registry or hand-written command schema |
| One `ds <path> … --output json` process per `tools/call`, envelope returned verbatim | A cache, a batch, or a "convenience" tool the CLI lacks |
| Uses each live descriptor's authority to keep headless commands headless, and to make one bounded local pairing attempt for paired commands | A credential, a listener, or a network hop |
| Receipt-verified `SKILL.md` documents exposed lazily through MCP resources | A requirement to copy skills into an agent home directory |

The `mcp` domain excludes itself from the tool list. Chapter classification is
declared once on every canonical command and appears in its live descriptor.

## Exposure modes

The default broad server publishes fourteen stable tools: `ds_catalog`, the
bounded `ds_diagnostics` bootstrap, plus one router per non-catalogue chapter —
`ds_data`, `ds_project`, `ds_grid_model`, `ds_pls_cadd`, `ds_survey`,
`ds_design`, `ds_map_presentation`, `ds_vector_tiles`, `ds_solar`,
`ds_reports`, `ds_operations` and `ds_workstation`. Adding a command does not
enlarge this list.

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

Profiles are `grid`, `pls`, `pls-library`, `library-governance`, `survey`,
`form-factory`, `survey-projects`, `survey-migration`, `design-edit`, `design-run`, `map`, `layers`,
`tiling`, `project`, `solar-input`, `solar-run`, `solar-delivery`, and
`operations`. `survey`
retains map/local-data survey work;
`form-factory` owns global schemas, while `survey-projects` owns governed
aggregate/spatial/change-feed reads, project-form settings, reusable templates, and
create-from-template. `layers` isolates
project ordering and desktop-local remote overlays; `tiling` owns governed
tile generation and catalogue membership. Each includes `ds_catalog`,
`ds_diagnostics`, and a bounded leaf set. `survey-migration` deliberately
contains only the governed import leaf in addition to those bootstrap tools.
A profile is only an allowlist: omitted
commands are unavailable and authority, effects, confirmation, output, and
refusals are unchanged. Plain `--exposure commands` retains the previous
all-command publication temporarily for compatibility.

`solar-input` is the narrow authenticated selected-project capture surface.
The established `solar-run` profile retains seeding, preparation, execution,
result inspection, and verification so existing MCP hosts do not lose tools.

PLS and its libraries are split by operator workflow: `pls` contains workspace
backup, closure, terrain and diagnostics; `pls-library` contains local
immutable-library verification, packing, seeding and native resolution; and
`library-governance` contains global library/example upload, publication and
lifecycle. Their union is the PLS-CADD chapter, but each typed tool surface
stays below the host's context limit.

## Why a chapter, rather than one tool per command

Publishing one tool per command makes the MCP surface grow with implementation
detail. Every command `ds` adds costs every connected host context it spends
before it has chosen anything, and a long undifferentiated tool list makes
first-hop selection less reliable, not more informed.

The opposite extreme is worse. A single `ds_call` tool taking a command id and
an argument bag minimises the count but deletes the semantic hints an agent
needs to choose safely: PLS-CADD patching, a survey read, a design save, a
Solar run and a platform-health query all look interchangeable. Chapter routers
keep the routing information and drop the schema bulk.

Publishing each chapter as one union schema over its commands would move the
same cost inside eleven very large tool definitions, and would make a chapter's
schema change whenever any command inside it changed. So the chapter envelope
stays small and stable, full input typing is delivered on demand through
`operation: "describe"`, and the invocation is then validated against the
canonical descriptor before dispatch.

## Where the chapter boundaries fall

A chapter is an **operator-intent** boundary, not a repository or crate
boundary, which is why the chapter table is not the domain table:

- PLS-CADD inspection, reference closure, terrain reconciliation, capacity and
  exact native-library resolution are one native delivery workflow, so `pls`
  and `library` share `ds_pls_cadd`.
- Survey acquisition and bounded local geospatial preparation belong together;
  LV design mutation does not. The `map` domain is therefore split across
  `ds_survey` and `ds_design` at this layer.
- Vector-tile publication has its own preflight/generate/catalogue lifecycle
  and global-write effects, so it is not folded into map presentation: styling
  an existing layer is not regenerating and publishing its tile archive.
- Canonical `.dsgrid` work stays distinct from native PLS-CADD work even when
  one delivery round-trip uses both.

Chapter descriptions name the operator concern and its main operation groups.
They must stay true when a command is added inside the chapter; a description
that enumerates flags or commands would be a second description of a command,
and would have to be maintained against the registry it is derived from.

## What chaptering may never change

Chaptering is discovery compression. It has no behaviour of its own, and these
hold for every exposure mode and every profile:

1. The live command descriptor remains authoritative for arguments,
   availability, authority, effect, confirmation, refusals and output.
2. The adapter dispatches the same handler the CLI does. It contains no
   project, survey, PLS-CADD, tile or Solar logic.
3. `confirm: true` is honoured only where that exact command's contract
   requires it. A chapter cannot grant confirmation to its neighbours, and a
   read-only command rejects it rather than forwarding it.
4. Project and desktop identity are resolved by the command, never by hidden
   MCP session state. A profile introduces no identity or project override
   argument.
5. Result envelopes, artifact receipts, bounded-output rules and error codes
   are identical to the CLI's.
6. Protocol logs stay off MCP stdout.
7. The `mcp` domain is never exposed as a chapter command, so an MCP client
   cannot reach `mcp install` or start a second server.
8. An unknown or wrong-chapter command id refuses with the correct router and
   a bounded next action. It is never forwarded as argv or as shell text. The
   `arguments` object is not permission to accept arbitrary CLI text: it
   carries one canonical command id whose values are validated against that
   command's live schema.

A profile is an allowlist over the same registry. A command a profile omits is
unavailable through that server; it is never reimplemented locally.

## Desktop readiness for MCP invocation

Authority is read from the same tier-3 descriptor used for the tool schema;
there is no MCP-side list of desktop commands. `authority: none` means local
owner/process work and MCP does not probe or launch DS GridDesign. Catalogue
and `describe` are discovery too, so they never launch it regardless of the
selected command.

For an `invoke` whose descriptor says `desktop_pairing`, `desktop_user`, or
legacy `project`, MCP first reads `ds desktop status`. `headless_user` and
`headless_project` never do so. If no Desktop session is present and
the caller did not name `--desktop-descriptor` (nor set
`DS_DESKTOP_DESCRIPTOR`), the installed Windows package may start its fixed DS
GridDesign executable once, then waits at most 10 seconds for its loopback
descriptor. Stable and Canary are selected from the invoking `ds.exe`'s exact
recognized sibling layout first; side-by-side installs therefore do not become
ambiguous. `LOCALAPPDATA` is only a fallback when that sibling identity is
absent, and a genuinely ambiguous fallback still refuses. MCP never launches a
second app when status already reports one, and it never launches a different
app in place of a named descriptor.

Failure remains a normal DS envelope: `desktop_not_paired` carries the bounded
remedy to start/sign in, and `desktop_signed_out` remains a refusal rather than
an implicit login. Automatic launch is intentionally unavailable outside the
installed Windows package; start and pair the application manually there.

The launch gate is entered only during `invoke`, after argument and confirmation
validation, and only for an authority that requires Desktop. Server startup,
MCP initialization, catalogue discovery, diagnostics, resources, local-file
commands, `headless_user`, and `headless_project` never launch or poll the
Desktop. One invocation makes at most one launch attempt and one bounded wait;
failure returns the ordinary command envelope and does not terminate MCP.

## MCP-only identity, diagnostics, and skills

Every exposure publishes `ds_diagnostics` with four read-only operations:

| Operation | Result |
|---|---|
| `identity` | Absolute executable, version/source/dirty/target/build profile, labeled Stable/Canary path evidence when available, native-client and Network pins, selected MCP exposure/profile, and skill-bundle identity |
| `doctor` | The unchanged `ds doctor --output json` envelope |
| `shell.status` | The unchanged `ds shell status --output json` envelope |
| `capabilities` | The unchanged bounded tier-1 `ds capabilities --output json` envelope |

`initialize.serverInfo` uses only the MCP name/title/version fields. Concise
identity and bootstrap directions are in `initialize.instructions`; structured
identity is returned by diagnostics and echoed by `ds_catalog`. An executable
outside the recognized Windows Stable/Canary sibling layout reports
`install_profile: unlabeled`: the server does not guess a Linux package channel
or call it development. Packaging needs a future stamped channel if those
lanes must be distinguished away from the desktop layout.

At startup only the packaged bundle's bounded receipt metadata is indexed. When
that receipt matches this CLI source SHA,
`resources/list` returns one `ds-skill://bundle/<receipt-id>/SKILL.md` resource
per shipped skill. `resources/read` accepts only one of those closed URIs,
then runs the same complete inventory/digest verification as doctor and returns
that one UTF-8 document.
It cannot read a caller path, a nested reference, a symlink, or an arbitrary
file. Resource metadata carries the receipt contract/source/source SHA and
dirty state. No skill is preloaded into initialization and no writable
Codex/Claude/Copilot skills home is required.

## Headless support matrix

| Surface/runtime boundary | Starts without Desktop/map/project/auth | What resolves lazily |
|---|---:|---|
| initialize, tools/list, identity, resources/list/read | yes | Packaged receipt and selected skill bytes only |
| diagnostics doctor | yes | Local command availability, shell, and skill installation evidence when explicitly called |
| catalogue index | yes | No command runtime; exact/query/chapter summaries resolve live availability only when requested |
| `authority: none` | yes | Local file or declared external engine at invocation |
| `authority: headless_user` | yes | Native user authentication at invocation; no Desktop launch |
| `authority: headless_project` | yes | Native auth and selected headless project at invocation; no Desktop launch |
| `authority: desktop_pairing` / `desktop_user` / legacy `project` | yes | Exact paired Desktop and any command-specific map/project readiness at invocation; one bounded Windows launch may occur |

Map-open state is not inferred from the chapter name. The selected command's
canonical authority and ordinary refusals remain the only runtime contract.

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
ds mcp install --output json                # supported hosts plus the default VS Code proposal
ds mcp install --host claude-desktop --output json # Windows Claude Desktop proposal
ds mcp install --write --yes                # merge it into the VS Code user profile
ds mcp install --host claude-code           # other hosts: claude-code, cursor, codex, generic
ds mcp install --host claude-code --exposure commands --profile pls --write --yes
```

`mcp.install` remains `machine_write`, but its declared `--write` switch is the
central confirmation trigger. A proposal writes nothing and can be queried
blindly; `--write` is refused without `--yes` before adapter code runs.

Without `--write`, `install` prints the entry and the file it belongs in and
changes nothing. With `--write` the merge is atomic: the merged document is
staged as a sibling temp file, fsynced, and renamed over the target, and any
pre-existing file is preserved as `<file>.bak`. An interrupted merge therefore
cannot leave a host with a truncated configuration.

The entry points at **this** executable and belongs in the **user** profile
— `%APPDATA%\Code\User\mcp.json` on Windows, `~/.config/Code/User/mcp.json`
on Linux — never in a workspace file. The server must run on the PC where DS
GridDesign is installed and paired; a workspace file travels to machines that
have neither. With both Stable and Canary installed, run `install` from the
`ds` of the app you use, and keep one server entry.

Every result includes a host-neutral `connection` descriptor and a
`supported_hosts` table. Thin adapters translate only the root and target;
all hosts launch the same absolute executable and fixed stdio arguments.
Claude Desktop is verified on Windows at
`%APPDATA%\Claude\claude_desktop_config.json`, under `mcpServers.ds`. It
starts `ds.exe` directly after a restart; VS Code need not be installed or
running. See the [adapter contract](../contracts/mcp-client-adapter-contract.md)
and [v3 migration note](../migration/mcp-client-adapters.md).

Codex keeps TOML: `install --host codex` prints the data; translate it into
`~/.codex/config.toml` under `[mcp_servers.ds]`. The install receipt and MCP
diagnostics identity report this executable's source SHA; it must match the
skill-bundle SHA reported by `ds doctor`.

## Server migration note

Existing stdio hosts keep both chapter and typed-profile exposure. They will
see one additional read-only tool, `ds_diagnostics`, and the standard MCP
resources capability. Hosts that ignore resources continue to use tools.
Callers that read the former non-standard `serverInfo.sourceSha` or
`serverInfo.dirty` fields must move to `ds_diagnostics(operation=identity)` or
the `ds_catalog` identity object; `serverInfo` now stays within valid MCP
fields. Broad and typed tool-count assertions must include the diagnostics
tool. No HTTP transport, managed service, host installer adapter, command
business logic, or UI behavior is introduced by this server change.

## Verifying

`ds doctor` reports the executable and skills. In the broad server, confirm
`tools/list` returns 14 tools, call `ds_diagnostics` for identity, use
`resources/list` then read `ds` and `ds-mcp-host`, and use `ds_catalog` to
route one command. A paired-desktop command can then prove its lazy pairing
refusal without affecting a later diagnostics call. In a typed profile, verify
both bootstrap tools and call an advertised read-only leaf.
