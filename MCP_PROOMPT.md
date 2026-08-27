# DS MCP tool chapters — suggestion

**Status:** design suggestion only; no runtime belongs in `ds-server`  
**Date:** 2026-08-27  
**Implementation owner:** `ds-cli`

## Decision

Classify every command into a stable operator-intent chapter, then expose that
classification in one of two deliberate MCP shapes:

- a broad server uses the small set of chapter routers described below;
- a specialized sub-MCP profile publishes only its chapter's fully typed leaf
  tools.

The current MCP wrapper proves the transport and reuses the correct executable,
but advertising 105 tools is too expensive for routine discovery. It consumes
host context, makes selection less reliable, and exposes implementation-level
command growth as MCP surface growth.

Do not replace those tools with one untyped `ds_call` tool. The chapter is the
unit of classification and installation. Each chapter owns one coherent
operator concern and routes to the existing live `ds` command contract.

## Proposed MCP surface

| MCP tool | Existing `ds` command families | Operator intent |
|---|---|---|
| `ds_catalog` | `capabilities`, `doctor`, version | Discover chapters, search commands, and read one exact contract. |
| `ds_project` | `desktop.*`, `work.*` | Establish project context and manage project plans, tasks, assignments, and records. |
| `ds_grid_model` | `dsgrid.*`, `dsgrid-exchange.*` | Inspect, validate, project, revise, import, and export canonical grid models. |
| `ds_pls_cadd` | `pls.*`, `library.*` | Inspect or patch PLS-CADD deliveries and resolve exact native engineering-library assets. |
| `ds_survey` | `map.survey.*` plus non-design `map.*` geometry/local-layer commands | Obtain survey data and perform bounded local geospatial review or preparation. |
| `ds_design` | `map.design.*` | Read, stage, process, report, save, or discard transformer/LV design work. |
| `ds_map_presentation` | `style.*` | Read or change project map styling and its secondary visual dimension. |
| `ds_vector_tiles` | `tile.*` | Inspect, plan, generate, add, or remove vector-tile outputs. |
| `ds_solar` | `solar.*` | Prepare, run, inspect, publish, and export Solar work. |
| `ds_reports` | `report.*` | Discover report tasks and export or bundle verified report artifacts. |
| `ds_operations` | `sre.*`, `shell.*`, `feedback.*` | Inspect platform health, manage shell reachability, and report product gaps. |

This reduces the advertised surface from 105 command tools to 11 chapter tools
without deleting a capability.

## Why these boundaries

The chapter is an operator-intent boundary, not a repository boundary.

- PLS-CADD inspection, reference closure, terrain reconciliation, capacity, and
  exact native-library resolution belong together because they form one native
  delivery workflow.
- Survey acquisition and local geospatial preparation belong together, but LV
  design mutation does not. The existing 32-command `map` domain should
  therefore be split across `ds_survey` and `ds_design` at the MCP layer.
- Vector-tile publication deserves its own tool because it has a distinct
  preflight/generate/catalogue lifecycle and global-write effects.
- Map presentation is separate from tiles: styling an existing layer is not
  regenerating or publishing its underlying tile archive.
- Canonical `.dsgrid` work is distinct from native PLS-CADD work even when a
  delivery round-trip uses both chapters.

## Compressed chapter call shape

This shape is for the broad server, where advertising every leaf tool would
recreate the 105-tool problem.

Every chapter tool should use the same small envelope:

```json
{
  "operation": "describe",
  "command": "pls.reference-closure",
  "arguments": {},
  "confirm": false
}
```

`operation` has two values:

- `describe` returns the live command descriptor: input schema, authority,
  effect, confirmation requirement, refusals, examples, and next action.
- `invoke` validates `arguments` against that same descriptor and invokes the
  existing handler.

Example invocation:

```json
{
  "operation": "invoke",
  "command": "pls.reference-closure",
  "arguments": {
    "workspace": "C:\\delivery\\PLS-CADD WORKSPACE"
  },
  "confirm": false
}
```

The `command` value remains the canonical `ds` command ID. The MCP adapter must
not invent parallel names such as `close_references` or duplicate command
schemas in a second registry.

## Discovery flow

The normal agent flow becomes:

1. Call `ds_catalog` with a search phrase or chapter name.
2. Call the selected chapter with `operation: "describe"` and the returned
   command ID.
3. Call the same chapter with `operation: "invoke"` and arguments conforming to
   the live descriptor.

For a host that already knows the exact stable command contract, step 1 can be
skipped. The server must still validate the invocation against the live
descriptor.

`ds_catalog` should support:

```json
{
  "query": "reference closure",
  "chapter": "pls-cadd",
  "command": null
}
```

It returns bounded chapter or command summaries and the exact next call. It
must not return all command schemas in one response.

## Description strategy

Each MCP tool description should describe the chapter in one compact paragraph
and name its main operation groups, not enumerate every flag or schema.

Suggested `ds_pls_cadd` description:

> Work with native PLS-CADD deliveries and pinned engineering libraries:
> inspect pole capacity and references, diagnose or compare DON assignments,
> reconcile terrain, label deviations, verify delivery, and resolve exact
> native assets. Describe a command before invoking it.

Suggested `ds_survey` description:

> Obtain project survey data and work with temporary local geospatial layers:
> view, draw, remove, focus, sample points, detect outliers, compare incoming
> lines, and plan or apply survey migration. Describe a command before invoking
> it.

Suggested `ds_vector_tiles` description:

> Inspect and manage project vector-tile outputs: status, source preflight,
> generation planning, confirmed generation, and catalogue membership.
> Describe a command before invoking it.

Descriptions should be stable even when commands are added inside a chapter.

## Safety and authority invariants

Chaptering is only discovery compression. It must not alter behavior.

1. The existing command descriptor remains authoritative for arguments,
   availability, authority, effect, confirmation, refusals, and output.
2. The MCP adapter calls the same command handler as the CLI. It contains no
   project, survey, PLS-CADD, or tile business logic.
3. `confirm: true` is accepted only according to the selected command contract.
   A chapter cannot grant confirmation to neighboring commands.
4. Project and desktop identity are resolved by the existing command, not by
   hidden MCP session state.
5. Result envelopes, artifact receipts, bounded-output rules, and error codes
   remain identical to CLI results.
6. Protocol logs remain off MCP stdout.
7. The `mcp` domain is never recursively exposed as a chapter command.
8. Unknown or wrong-chapter command IDs refuse with the matching chapter and a
   bounded next action; they are never forwarded as arbitrary shell text.

## Why not one giant tool

A single tool with `{ "command": "...", "arguments": {} }` minimizes the
tool count but removes the semantic hints that allow an agent to choose safely.
It also makes PLS-CADD, survey, design-save, Solar, and platform operations look
equally interchangeable.

Chapter tools retain useful routing information while avoiding 105 advertised
schemas. Eleven stable descriptions are small enough for hosts to load and
specific enough for reliable first-hop selection.

## Why not one giant union schema per chapter

Publishing every command's full schema through `oneOf` would move the same
token cost inside 11 very large tool definitions. It also makes each chapter
schema change whenever a nested command changes.

The chapter envelope should remain stable. Full input typing is delivered on
demand by `operation: "describe"`, and the invocation is then validated by the
canonical descriptor before execution.

## Optional sub-MCP profiles

Chaptering and sub-MCP servers solve different problems:

- chapters reduce the number and size of tools inside one server;
- sub-MCP profiles let a host load only the product areas relevant to its role.

The default broad entry should remain one `ds` server exposing all 11 chapter
routers. A general coding agent can then cross survey, model, PLS-CADD, tiles,
and reports without changing connections.

For narrower hosts, the same executable may expose allowlisted profiles:

| MCP server entry | Typed command selection |
|---|---|
| `ds-grid` | catalogue, `.dsgrid`, exchange, and report commands |
| `ds-pls` | catalogue, PLS-CADD, and native-library commands |
| `ds-survey` | catalogue, survey, and temporary local-geodata commands |
| `ds-design-edit` | catalogue and bounded design read/select/stage commands |
| `ds-design-run` | catalogue and design process/save/report lifecycle commands |
| `ds-map` | catalogue, presentation/style, and vector-tile commands |
| `ds-project` | catalogue, desktop project context, and Project Work commands |
| `ds-solar-run` | catalogue and Solar preparation/run/result-inspection commands |
| `ds-solar-delivery` | catalogue and Solar final/portfolio/report/export commands |
| `ds-operations` | catalogue, SRE, shell, and feedback commands |

Possible launch shape:

```text
ds mcp serve --exposure commands --profile pls
ds mcp serve --exposure commands --profile survey
ds mcp serve --exposure commands --profile map
ds mcp serve --exposure commands --profile solar-run
```

These are filtered views over one registry, not separate implementations or
services. They must use the same executable, descriptors, dispatcher, desktop
pairing, and result envelopes.

A specialized profile should normally publish the fully typed leaf tools for
its included chapters. For example, a PLS-CADD profile can expose the seven
current `pls.*` and seven current `library.*` tools. Fourteen conventional,
typed tools in an explicitly selected engineering server are preferable to one
opaque arbitrary-arguments router, and far preferable to loading all 105 tools.

If a profile would exceed roughly 15 leaf tools, split it by operator workflow
instead of silently returning to a large catalogue. The current `map.design.*`
family is a natural candidate for separate edit/review and process/save
profiles. Solar can similarly separate run inspection from final publication
and export.

Do not install every profile by default. Doing that repeats `ds_catalog`,
creates selection ambiguity, and loses much of the context saving. Install the
single broad `ds` entry for general agents, or one role profile for a narrowly
scoped host. Multiple profiles are an explicit user choice.

Profiles are discovery allowlists only. They cannot change authority, effects,
confirmation, output bounds, or the meaning of a command. A command omitted by
a profile is unavailable through that server; it is not reimplemented locally
or forwarded as arbitrary argv.

## Conventional typing versus maximum compression

There is a real tradeoff, and the implementation should name it explicitly.

| Shape | Benefit | Cost | Use when |
|---|---|---|---|
| Typed leaf tools in a sub-MCP profile | Normal MCP schema validation, direct tool selection, simpler skills | More visible tools inside that selected profile | The host has a known workflow such as PLS-CADD, survey, tiles, or Solar. |
| Eleven chapter routers | Very small global catalogue; stable as commands grow | Requires describe-then-invoke; nested arguments are validated by DS rather than the host | A general agent needs cross-product reach from one server. |
| All command tools | Full host-visible schemas | 105-tool context and selection burden | Temporary compatibility only. |

The chapter router's `arguments` object is not permission to accept arbitrary
CLI text. It accepts one canonical command ID, validates against that command's
live schema, and dispatches the existing handler. No shell string or free-form
argv crosses the boundary.

## Skill refinement

Skills should become conventional MCP routing guidance rather than a second
tool catalogue.

The top-level DS skill should teach this flow:

1. Select the installed `ds` MCP server or the relevant role profile.
2. Use `ds_catalog` for bounded discovery.
3. Use the selected chapter with `operation: "describe"`.
4. Invoke through that same chapter and branch on the returned DS envelope.
5. Follow typed remedies; never reconstruct a refused result through repository
   inspection or a direct API.

Workflow skills should declare only the chapters they need. For example:

| Skill | Required chapter tools |
|---|---|
| PLS-CADD terrain round-trip | `ds_grid_model`, `ds_pls_cadd` |
| Temporary survey/map review | `ds_survey`, optionally `ds_map_presentation` |
| LV design revision | `ds_project`, `ds_design` |
| Tile regeneration | `ds_vector_tiles` |
| Solar workflow | `ds_solar`, optionally `ds_reports` |

Skill text should retain the domain safety sequence and acceptance gates, but
must not copy full command schemas, enumerate the entire command catalogue, or
encode command availability from memory. The live chapter `describe` result is
the contract.

The bundled `ds-mcp-host` skill should change from “tool names are command IDs
with dots replaced by underscores” to profile-aware routing. For a typed
sub-MCP it should use the advertised leaf tool directly; for the broad server it
should follow catalog, describe, then invoke. It should explain profile
installation, confirmation, and DS-envelope handling without teaching 105
generated tool names.

Skills and MCP configuration should be packaged from the same `ds-cli` build
and report the same source SHA. A host profile named by a skill must be
installable by that exact executable. Tests should fail when a skill names an
unknown chapter or a profile omits a chapter the skill declares as required.

## Lessons retained from the retired `ds-mcp`

The retired repository is a tombstone, not a source of live contracts or code.
Its history nevertheless records several design invariants worth retaining in
the new thin wrapper:

- One registry and one dispatcher served every transport. Chapter and profile
  views should be generated from the current `ds` registry in the same way.
- Descriptors carried effect, execution mode, authority, bounds, idempotency,
  destructive status, and availability. Chapter routing must preserve all of
  that metadata rather than infer safety from names.
- Dispatch followed a fail-closed order: availability, authorization, binding,
  bounded invocation, bounded output.
- Principal and project were server-owned context, not caller-selectable tool
  fields. Profiles must not introduce identity or project override arguments.
- Discovery was deterministic and caller-independent. Authorization occurred
  at invocation, while genuinely local-only capabilities could be excluded
  from transports where they could never work.
- Multi-call state used opaque owner-issued handles and explicit revisions;
  transport sessions did not become application state or authority.
- Oversized engineering results refused instead of returning a misleading
  truncation. The current DS artifact-and-envelope policy should remain the
  authority for bounded chapter results.
- Tool availability remained visible with a typed reason. A missing runtime
  should not cause an agent to invent an alternative implementation.

The lesson not to retain is the old repository's breadth. It accumulated its
own orchestration, provider, chat, capability-placement, and domain-tool code,
which made it a second product surface. The new architecture must stay inside
the single `ds` executable as a generated MCP transport. Do not restore hosted
`ds-mcp`, copy its historical schemas, or add product runtime to `ds-server`.

Protocol-specific statements in the retired history are also historical. The
new wrapper must follow the protocol version negotiated and tested by its
current MCP SDK rather than copying an old protocol narrative.

## Implementation working agreement for Codex

Coding happens on the remote host named `ds-server`, but product code belongs
in `/home/magese/data-solutions/ds-cli`. This `ds-server` repository holds the
prompt and host documentation only; do not add DS product runtime here.

Codex must pair actively with the installed Claude CLI to reduce context use
and finish faster. Codex remains the lead and owns scope, architecture,
integration, verification, and the overall judgment of the delivery. Claude's
output is a proposal until Codex has reviewed its diff and independently
verified the behavior.

Delegate bounded, non-overlapping work such as:

- inventorying the live registry, MCP schemas, and test surface;
- proposing or checking the exhaustive command-to-chapter classification;
- implementing one isolated classification/profile slice with focused tests;
- reviewing skills for copied schemas and stale generated tool names;
- running a bounded test group and returning a concise failure receipt;
- adversarially reviewing confirmation, authority, stdout framing, profile
  escape, and arbitrary-command risks.

Codex and Claude must never edit the same worktree concurrently. Create two
worktrees from the same verified `origin/run` revision, for example:

```text
/home/magese/data-solutions/_worktrees/ds-cli-mcp-chapters-codex
/home/magese/data-solutions/_worktrees/ds-cli-mcp-chapters-claude
```

Claude may make uncommitted edits only in its assigned worktree. Codex reviews
and selectively integrates them into the Codex worktree. Preserve existing
dirty state before creating either worktree. Do not commit, push, deploy, or
write MCP host configuration without explicit owner permission.

Claude should return concise receipts rather than full logs:

- base revision and files changed;
- behavior implemented or reviewed;
- tests run and exact outcomes;
- remaining risks or disagreements;
- confirmation that it did not commit, push, deploy, or edit outside its
  assigned worktree.

### Do not sit idle

Codex must continue useful, non-overlapping work while Claude runs. If Claude
inventories the registry, Codex designs the invariants and test matrix. If
Claude implements classification, Codex works on parity, refusal, confirmation,
documentation, or skill tests. If Claude runs a long build, Codex audits tool
descriptions and context budgets or exercises the real stdio server.

Check Claude at natural integration points instead of rapid polling. When a
bounded task finishes, assign another independent task if useful work remains.
Codex must not become only a coordinator: it performs the central architecture
and integration itself and keeps the combined implementation coherent.

The final assessment must distinguish:

- what Claude proposed;
- what Codex accepted or rejected and why;
- what Codex independently verified;
- any unresolved product decisions;
- whether the result is ready for owner review and commit permission.

Passing tests alone is not completion. Codex must judge whether ordinary MCP
hosts can understand the surface, whether CLI discipline remains intact, and
whether discovery burden fell without hiding safety-critical contracts.

## Compatibility and rollout

Keep the current command-per-tool publication temporarily as an opt-in
compatibility mode:

```text
ds mcp serve --exposure chapters   # proposed default
ds mcp serve --exposure commands   # temporary compatibility mode
```

Suggested rollout:

1. Add a single command-to-chapter classification to the canonical command
   registry.
2. Fail tests when a new command has no chapter or belongs to multiple
   chapters.
3. Publish chapter tools from that classification.
4. Generate optional server-profile allowlists from the same classification.
5. Refine the bundled skills to name chapter tools and profiles, not generated
   command tools.
6. Add parity tests proving every MCP-routable command is reachable through
   exactly one chapter.
7. Compare tool-selection quality and context size against the 105-tool mode.
8. Make chapter exposure the default, retain command exposure for one
   deprecation window, then reconsider whether it is still needed.

Hosts that support MCP tool-list change notifications may refresh when the
exposure mode changes. Runtime command additions should normally leave the 11
top-level tool definitions unchanged.

## Acceptance criteria

- Broad MCP discovery advertises no more than 12 DS tools.
- A specialized typed sub-MCP profile targets no more than 15 leaf tools; a
  larger family is split by operator workflow.
- Every currently published DS command is reachable through exactly one
  chapter, except the intentionally excluded `mcp` domain.
- A user can discover and invoke PLS-CADD patching, survey-data work, and
  vector-tile generation without loading unrelated command schemas.
- Chapter routing does not duplicate command handlers or validation schemas.
- CLI and MCP return the same authority, effect, refusal, and result envelope
  for the same command and arguments.
- Effectful commands cannot bypass their existing confirmation requirement.
- Adding a command to an existing chapter does not increase the MCP tool count.
- Unknown commands and wrong-chapter calls return a bounded correction rather
  than executing arbitrary input.
- Every published profile is only a filtered view of the canonical registry;
  profile selection never changes a command's governance.
- Bundled skills and MCP profiles report the same `ds-cli` source revision and
  fail verification if a required chapter is missing.

## Recommendation

Adopt chapter exposure as the default MCP surface. Keep the executable and
canonical command registry exactly where they are: in `ds-cli`. Treat this file
as a host-side design note only; do not add DS product runtime to `ds-server`.
