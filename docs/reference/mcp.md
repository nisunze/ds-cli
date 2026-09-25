# `ds mcp` — reference

Tier-4 reference. `ds mcp <command> --help` is the contract.

## The problem it solves

Some agent hosts — VS Code agent mode, GitHub Copilot, Claude Desktop and
Claude Code, Cursor, Codex, Gemini CLI, Google Antigravity, and Windsurf —
learn tools only through the Model Context Protocol: a JSON-RPC server on stdio
that answers `tools/list` and `tools/call`. Skills reach the hosts that read
skills; MCP reaches the rest. `ds mcp` gives those hosts the command line
**without a second product surface**.

## What the server is, and is not

| It is | It is not |
|---|---|
| The same `ds` executable, launched with `mcp serve` by the host | A separate binary, sidecar or service |
| Chapter/profile views generated at startup from unchecked `ds capabilities` schemas; live availability resolves on catalogue describe or invoke | A second command registry or hand-written command schema |
| One `ds <path> … --output json` process per `tools/call`, the CLI's envelope returned within the adapter's documented bounds | A cache, a batch, or a "convenience" tool the CLI lacks |
| Uses each live descriptor's authority to keep headless commands headless, and to make one bounded local pairing attempt for paired commands | A credential, a listener, or a network hop |
| Receipt-verified `SKILL.md` documents exposed lazily through MCP resources | A requirement to copy skills into an agent home directory |

The `mcp` domain excludes itself from the tool list. Chapter classification is
declared once on every canonical command and appears in its live descriptor.

## Exposure modes

The default broad server publishes fifteen stable tools: `ds_catalog`, the
bounded `ds_diagnostics` bootstrap, plus one router per non-catalogue chapter —
`ds_data`, `ds_project`, `ds_assets`, `ds_grid_model`, `ds_pls_cadd`,
`ds_survey`, `ds_design`, `ds_map_presentation`, `ds_vector_tiles`,
`ds_solar`, `ds_reports`, `ds_operations` and `ds_workstation`. Adding a
command does not enlarge this list.

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

Profiles include `auth-context`, `datasets`, `grid`, `grid-native`, `grid-corrections`, `pls`, `pls-desktop`, `pls-library`, `library-governance`, `survey`,
`form-factory`, `survey-projects`, `survey-media`, `survey-migration`, `design-edit`, `design-run`, `map`, `layers`,
`tiling`, `project`, `correspondence`, `solar-input`, `solar-application`, `solar-run`, `solar-dashboard`, `solar-delivery`,
`solar-portfolio-batch`, `solar-migration`, `design-migration`,
`operations`, and `project-operations`. `project` owns the plan and the task
workflow; `datasets` groups catalog discovery, held-layer GeoJSON, planned
BigQuery geography, model-line export, local-layer registration and sector
workbook delivery. Each headless read names its authorized project.
`correspondence` owns the parties, the records and threads and the
task blockers (`pm.party.*`, `pm.record.*`, `pm.task.block|unblock`) with the
plan for their vocabularies — filing letters and scheduling tasks are two
jobs, so they are two profiles. `survey`
retains map/local-data survey work;
`form-factory` owns global schemas, while `survey-projects` owns governed
aggregate/spatial/change-feed reads, project-form settings, reusable templates, and
create-from-template; `survey-media` owns a form's entries with their photo references
(`survey.entries.read`), their photo files (`survey.photo.fetch`), what this
machine holds of them (`survey.local.status`), the survey photos this machine holds
(`survey.moments.list|read`), the one rotation and its publication
(`survey.photo.rotate|publish`) and the offline file rotation. `layers` isolates
project ordering and desktop-local remote overlays; `tiling` owns governed
tile generation and catalogue membership; `project-operations` owns
map-independent background work — paired local-room materialization plus
headless transformer inventory, reversible retirement/restoration, and the
Combined Report deliverable. Room materialization uses the paired visible
project because the application owns the cache, but never opens a map or edit
context; headless project commands name their project in each request. MCP has
no active project of its own. Each profile includes `ds_catalog`,
`ds_diagnostics`, and a bounded leaf set. `survey-migration` deliberately
contains only the governed import leaf in addition to those bootstrap tools.
Migration is per domain, never one cross-domain surface: `survey-migration`,
`design-migration` (`design.migrate.plan|apply`, transformers and DS Grid
models) and `solar-migration` (`solar.migrate.plan|apply`, cities and
portfolios) each publish their own domain's plan and apply and nothing else.
Plan and apply travel together in each: an apply whose plan an agent cannot
reach is an apply nobody reviewed.

`ds mcp install` derives its host-entry key from the build lane and the
runtime platform, and never writes workspace configuration.
A profile is only an allowlist: omitted
commands are unavailable and authority, effects, confirmation, output, and
refusals are unchanged. Plain `--exposure commands` retains the previous
all-command publication temporarily for compatibility.

`grid-native` contains only native `.dsgrid` file creation, inspection,
validation, command discovery, reads/solves, revision-gated edits and exchange.
Its command leaves require no desktop, TypeScript runtime or signed-in
project. `grid-local-model` retains the paired application lifecycle,
project publication and the governed head's lifecycle (`dsgrid project
versions|retire|restore`); `clearance` is the feature-code and clearance
workflow over one working copy (`dsgrid feature-codes report|import|migrate|
export`, `dsgrid criteria show|clearance set`, `dsgrid analyse clearance`);
the broad `grid` router keeps its budget and leaves the typed edits and the
clearance workflow to those two profiles. `grid-corrections` is the bounded
typed surface for review comments on an already spotted model: exact package
inspection, command discovery, graph and report reads, clearance analysis,
and mandatory-guard `dsgrid.apply-correction`. It admits native conversion into a `.dsgrid`
working copy, while native PLS-CADD delivery remains the owner's separate gate.

The `design-edit` profile includes the same canonical `map.design.open`,
map-owned `map.design.pin` Working-set operation, and
`map.design.version.list|play|compare` leaves as the broad Design chapter
router. Their MCP invocations ask the paired application itself to navigate,
manage the visible read-only Working set, render playback, or open its bounded
comparison and wait for readiness; no VS Code or other UI host mediates those
transitions. Version feature data remains inside the application, and the MCP
receipts expose metadata or aggregate counts only.

`auth-context` is the principal handoff for MCP hosts. Its sign-in is
`account.connect`: the person approves the request in their signed-in DS
GridDesign Desktop under Account > Link a trusted device, and the tool is
called again once approved. It publishes native identity status and fresh visible
project inventory. It does not expose the legacy device-local project selector;
each project command captures its explicit `--project`. It does not
publish the terminal sign-in, logout, or any Desktop-owned device approval;
those operations are excluded from every
exposure, and no MCP answer names the terminal sign-in (see
`docs/reference/auth.md`).

`solar-input` is the narrow authenticated Solar capture surface; its project is
named explicitly per call.
The established `solar-run` profile retains seeding, preparation, execution,
result inspection, and verification so existing MCP hosts do not lose tools.

`solar-dashboard` exposes headless sealed-source result reads and local JSON/HTML
composition, with engine identity and local project-result verification. It requires
explicit source, project, batch run and city attribution and never publishes files.

`solar-portfolio-batch` includes headless portfolio calculation and publication
alongside the paired historical batch lifecycle. These calculation/publication
leaves moved from `solar-delivery` to keep that profile within its tool budget.

PLS and its libraries are split by operator workflow: `pls` contains workspace
backup, closure, terrain and diagnostics; `pls-desktop` contains the verbs that
drive PLS-CADD itself on its Windows desktop (`ds pls desktop …`); `pls-library` contains local
immutable-library verification, packing, seeding and native resolution; and
`library-governance` contains global library/example upload, publication and
lifecycle. Their union is the PLS-CADD chapter, but each typed tool surface
stays below the host's context limit.

## How a command becomes a tool

Audited 2026-09-25 against the live registry: 516 registered commands in 24
domains. Every one is a tool or a named exclusion, and nothing here names a
command: a verb registered later is published with no edit to this crate.

### Coverage

`tools::discover_tools` walks `ds capabilities` tier by tier at startup (schema
mode, so no availability is resolved) and projects each descriptor. Excluded:
the `mcp` domain itself, and `tools::NEVER_TOOLS`, each with its reason —
`auth.login` (a password at a trusted terminal), `auth.link.approve` (the
signed-in Desktop's act) and `server.serve` (a foreground host never answers).

| Domain | Verbs | Typed tools | Excluded | `confirm` gated | Desktop gate |
|---|---:|---:|---:|---:|---:|
| `server` | 10 | 9 | 1 | 0 | 0 |
| `dsgrid` | 42 | 42 | 0 | 3 | 2 |
| `dsgrid-exchange` | 4 | 4 | 0 | 0 | 0 |
| `library` | 16 | 16 | 0 | 9 | 0 |
| `pls` | 11 | 11 | 0 | 1 | 0 |
| `solar` | 55 | 55 | 0 | 13 | 16 |
| `report` | 35 | 35 | 0 | 11 | 0 |
| `survey` | 33 | 33 | 0 | 12 | 0 |
| `map` | 59 | 59 | 0 | 9 | 39 |
| `pm` | 26 | 26 | 0 | 18 | 0 |
| `assets` | 14 | 14 | 0 | 6 | 0 |
| `design` | 97 | 97 | 0 | 37 | 0 |
| `install` | 4 | 4 | 0 | 2 | 0 |
| `sre` | 2 | 2 | 0 | 0 | 0 |
| `style` | 15 | 15 | 0 | 7 | 0 |
| `tile` | 12 | 12 | 0 | 5 | 0 |
| `data` | 19 | 19 | 0 | 1 | 4 |
| `feedback` | 4 | 4 | 0 | 3 | 0 |
| `desktop` | 29 | 29 | 0 | 8 | 26 |
| `shell` | 3 | 3 | 0 | 0 | 0 |
| `workstation` | 8 | 8 | 0 | 3 | 0 |
| `mcp` | 2 | 0 | 2 | — | — |
| `account` | 1 | 1 | 0 | 0 | 0 |
| `auth` | 15 | 13 | 2 | 3 | 0 |

The per-verb table — command, typed tool, chapter router, effect, hints,
confirmation shape, desktop gate and status — is
[`mcp-tool-audit.md`](mcp-tool-audit.md), generated from one executable by
`scripts/mcp-tool-audit.py <ds>`. It is a snapshot; regenerate it after
merging new verbs. The guarantee is
`every_registered_command_is_exactly_one_mcp_tool_or_a_named_exclusion` in
`crates/ds/tests/mcp.rs`: it fails if a registered verb has no tool, a tool
has no verb, two tools share a name, a name breaks the `[A-Za-z0-9_-]{1,64}`
grammar hosts accept, or a chapter router misses or double-routes a command.

### Names

A tool is named by its command id with `.` → `_` (`map.design.report` →
`map_design_report`); hyphens stay. The title is the dotted id. No tool has
been renamed, so no alias is needed.

### Input schemas

Generated from each declared input; `additionalProperties` is false.

| Declared kind | JSON Schema |
|---|---|
| value | `string`; `enum` from a closed set; `default` as declared |
| switch | `boolean` |
| repeated | `array` of `string`; a closed set constrains each item (`items.enum`); a default becomes a one-item array |
| positional | `string`, sent after the `--` sentinel |
| — (gated command) | `confirm`: `boolean`, described with its trigger or preview |

Required inputs are listed in `required`. A descriptor that cannot be
projected faithfully — an unknown chapter, authority, effect or execution
token; a confirmation trigger or preview switch naming no declared switch; an
input named `output`, `pretty`, `no-color` or `help`, which `ds` reads as its
own — stops `ds mcp serve` from starting with `mcp_capabilities_unavailable`
rather than publishing a tool that says less than the CLI.

### Argument values

Each value travels inside its own token, `--name=value`, and operands follow
`--`. `ds` reads `--yes`, `--output`, `--help`/`-h` and `--version` as its own
wherever they stand, so a value sent as a separate token could confirm,
re-format or divert a call: before this rule a task titled `-h` answered the
help descriptor with status `ok` and never ran, and any value beginning with
`--` — a Markdown rule — was refused as a missing value. `--yes` is emitted
only for `confirm: true`.

### Annotations

Derived from the effect class and nothing else (`tools::hints`). Where a class
does not settle a question the answer is conservative.

| Effect | readOnlyHint | destructiveHint | idempotentHint |
|---|---|---|---|
| `discovery`, `read_only` | true | false | true |
| `proposal` | true | false | false — spends model credit per call |
| `local_ui` | false | false | false |
| `local_auth_state`, `local_file_write`, `artifact_write`, `machine_write`, `global_write` | false | true | false |

`openWorldHint` is false everywhere: a call reaches this executable, its owner
engines, the paired application or the DS service under the caller's own
identity. A chapter router is read-only and idempotent only if every command
it routes is, and destructive if any is. `ds_catalog` and `ds_diagnostics`
are read-only, non-destructive and idempotent.

### Errors

- A routing mistake — a tool this server does not publish, a router envelope
  without `operation`/`command`, an unknown or wrong-chapter command id — is a
  JSON-RPC `-32602` error naming the right tool where one exists.
- Once a command is resolved, anything wrong with how it was called — an
  undeclared property, a wrong JSON type, `confirm` where that invocation
  needs none, `confirm` inside nested `arguments` — is that command's
  `isError` result carrying a DS envelope: `mcp_arguments_invalid`, class
  `invalid_input`, a remedy and `next: ds capabilities <id>`.
- Every CLI refusal is an `isError` result whose `structuredContent` is the
  CLI's envelope, with its code, remedy and `next`.

### Bounds

- **Time.** Each tool call's `ds` child runs within `ds mcp serve
  --call-timeout <seconds>` (default 3600, 1–86400). Past it the child is
  stopped and the call refuses with `mcp_call_timed_out` — class
  `unavailable` for a read, `conflict` for a writing effect, whose remedy is
  to re-read the state it changes before retrying. Only the direct child is
  stopped; an owner engine it started may still be finishing. The bound in
  force is `mcp.call_timeout_seconds` in `ds_diagnostics(operation=identity)`. The server's own
  probes (`version`, `capabilities`, `desktop status`) are bounded at 120 s.
- **Long-running calls.** A host that sends `_meta.progressToken` receives
  `notifications/progress` every 10 s while the call runs, with the elapsed
  whole seconds as `progress` and no `total`. Job commands (`execution: job`)
  say "Runs as a job" in their description: they answer at once with a handle
  to poll.
- **Size.** The child's standard output is captured up to 32 MiB; beyond that
  the call refuses with `mcp_result_too_large`. An envelope above 256 KiB is
  trimmed at its largest arrays under `data` or `error.detail` — never cut
  mid-value, `status` and the error's code, message and remedy untouched — and
  reports what was trimmed in `more.mcp_truncation` (pointer, kept, total).
  Text that is not an envelope is bounded to 4 KiB and says how much was
  omitted.

### Credentials

No command emits a credential; each owner tests that. The adapter does not
rely on it: a string under `password`, `access_token`, `refresh_token`,
`id_token`, `client_secret`, `session_secret`, `private_key`, `device_code` or
`code_verifier` — at any depth, any case — and the token after `Bearer ` in
any string are replaced with `[redacted by ds mcp]` before a host reads the
answer. `token` is not on the list: it is ordinary data here (a survey
feature-code token, a host token). The published tool text never names the
terminal sign-in; see `auth-context` above.

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
   are the CLI's. The adapter adds only its own boundary — the call bound,
   the result bound reported in `more.mcp_truncation`, credential redaction
   and `mcp_arguments_invalid` — described under
   [How a command becomes a tool](#how-a-command-becomes-a-tool).
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

`initialize.serverInfo` uses only the MCP name/title/version fields. The name is
a protocol-safe package identity such as `ds-stable-windows`; the title is a
visible product identity such as `DS GridDesign — Stable on Windows`. Concise
identity and bootstrap directions are in `initialize.instructions`; structured
identity is returned by diagnostics and echoed by `ds_catalog`. The release
lane comes from the compile-time `DS_DESKTOP_LANE` package stamp; an unstamped
developer build is explicitly `development`. The runtime platform comes from
the compiled OS, with WSL distinguished from native Linux only when the kernel
release carries Microsoft evidence. This MCP identity is independent of the
existing executable-layout `install_profile` evidence.

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
requires it for that exact invocation — the same decision the CLI's gate
makes: a declared trigger switch (`mcp install --write`) decides alone;
otherwise a set preview switch (`--dry-run`) writes nothing and needs no
confirmation; otherwise the effect class decides. Read-only commands and
previews reject confirmation rather than forwarding it. Without confirmation,
the CLI's typed `confirmation_required` refusal returns unchanged.

Two command-owned inputs share a spelling with this machinery and stay the
command's own: `design.force-gate.check --confirm <code>` is a string input,
and the typed `.dsgrid` mutations (`dsgrid structure retype|describe|…`)
declare their own `--yes` switch that writes the revision. Neither command is
behind the central gate. A gated command that declared either would let an
ordinary input confirm it, so such a descriptor stops the server from
starting rather than being published.

## Reading a result

Every result is the CLI envelope: branch on `status`, read `data` on `ok`,
and follow `error.remedy` / `error.next` on anything else. Tool descriptions
carry the command's effect, authority, confirmation and preview shape,
whether it runs as a job, whether it needs the paired window, and the
refusals it can name. If `more.mcp_truncation` is present the adapter trimmed
the answer; narrow the call and ask again.

## Installing the host entry

```
ds mcp install --output json                # supported hosts plus the default VS Code proposal
ds mcp install --host claude-desktop --output json # Windows Claude Desktop proposal
ds mcp install --write --yes                # merge it into the VS Code user profile
ds mcp install --host claude-code           # also cursor, codex, Gemini CLI, Antigravity, Windsurf, Copilot, generic
ds mcp install --host claude-code --exposure commands --profile pls --write --yes
ds mcp install --host codex --write --yes   # losslessly add its derived mcp_servers table
ds mcp install --host gemini-cli --write --yes
ds mcp install --host antigravity --write --yes
ds mcp install --host windsurf --write --yes
ds mcp install --host github-copilot --write --yes
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
have neither. With both Stable and Canary installed, run `install` from each
exact `ds`; their derived lane/platform keys coexist and remain visibly
distinct.

Every result includes a host-neutral `connection` descriptor and a
`supported_hosts` table. Thin adapters translate only the root and target;
all hosts launch the same absolute executable and fixed stdio arguments.
Claude Desktop is verified on Windows at
`%APPDATA%\Claude\claude_desktop_config.json`, under a derived `mcpServers`
member such as `dsGridDesignStableWindows`. It starts `ds.exe` directly after
a restart; VS Code need not be installed or running. See the
[adapter contract](../contracts/mcp-client-adapter-contract.md) and
[adapter migration note](../migration/mcp-client-adapters.md).

Codex keeps TOML. The read-only `install --host codex` proposal verifies
`~/.codex/config.toml` and reports whether it would create, merge, or leave an
exact match unchanged. `--write --yes` losslessly adds the derived table, for
example `[mcp_servers.dsGridDesignStableLinux]`, preserving unrelated tables,
comments and formatting. An exact legacy `[mcp_servers.ds]` table is migrated;
a non-identical legacy or derived entry is shown as a conflict and never
overwritten. A changed registration reports `restart_required: true`: fully
quit and restart Codex, then start a new agent session. The install receipt and
MCP diagnostics identity report this executable's source SHA; it must match the
skill-bundle SHA from `ds doctor`.

Gemini CLI targets `~/.gemini/settings.json`; Google Antigravity separately
targets `~/.gemini/config/mcp_config.json`; Windsurf targets
`~/.codeium/windsurf/mcp_config.json`; GitHub Copilot CLI targets
`~/.copilot/mcp-config.json`. All are user-level paths on Windows, macOS and
Linux, need no VS Code mediation, preserve unrelated JSON and sibling servers,
and refuse conflicts with existing/proposed previews. GitHub Copilot receives
its verified `local` server shape. Cline is omitted until its
CLI/global-storage schema is stable enough for a safe blind merge.

All JSON adapters use the same guarded planner and atomic writer. The visible
configuration key is camelCase, for example `dsGridDesignCanaryWsl`; the MCP
protocol name is separately `ds-canary-wsl`; and hosts that render the title
show `DS GridDesign — Canary on WSL`. An exact legacy `ds` object is migrated
once. A differing legacy object or named entry refuses rather than being
silently overwritten. These names affect discovery and display only: tool
names such as `ds_catalog` and every canonical command id remain unchanged.

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
`tools/list` returns 15 tools, call `ds_diagnostics` for identity, use
`resources/list` then read `ds` and `ds-mcp-host`, and use `ds_catalog` to
route one command. A paired-desktop command can then prove its lazy pairing
refusal without affecting a later diagnostics call. In a typed profile, verify
both bootstrap tools and call an advertised read-only leaf.
