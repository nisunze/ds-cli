# Distribution specialist content contract

The canonical role is
[`skills/ds-distribution-delivery/SKILL.md`](../../skills/ds-distribution-delivery/SKILL.md).
It owns delivery reasoning and composes the existing DS skills. Commands, schemas,
authorization, calculations, persistent product state and transport remain with
their existing owners. There is no new execution service or second tool registry.

## Content surfaces

| Surface | Source and responsibility |
|---|---|
| Native skill hosts | Canonical skill directory, including both referenced documents and `agents/openai.yaml`; existing skill packaging discovers the directory |
| Claude Code agent | [`agents/claude-code/ds-distribution-delivery.md`](../../agents/claude-code/ds-distribution-delivery.md); thin frontmatter adapter preloading `ds` and the canonical role |
| MCP-only host | Same canonical role bytes through the skill resource interface; supporting references need explicit supported resource access |
| Behavioral review | [`docs/evaluations/distribution-delivery.md`](../evaluations/distribution-delivery.md); fixtures and acceptance rubric, not production project state |

The Claude adapter uses the documented `skills` preload mechanism and inherits
the parent model. It does not set permission overrides, connection strings,
tool-name allowlists or a separate memory location. Installing a role does not
authorize project writes. See the [Claude Code subagent contract](https://code.claude.com/docs/en/sub-agents).

The OpenAI YAML file is native skill UI metadata; it is not a separately running
agent or a claim that another host understands Claude's frontmatter. Hosts use
their own skill/agent loaders. Keep the expertise canonical rather than copying
the role body into each adapter.

## Migration integration boundary

This change supplies content only. The existing skill installers enumerate skill
directories, so no new per-role executable or installer is needed for the native
skill. The normal release must package and verify the complete directory with the
matching bundle receipt. Do not replace a production receipt with a source-tree
installation and describe the result as release-matched.

The Claude adapter is a registration source, not an installed registration.
The migration owner can distribute it through the host's supported agent location
with both required skills. Preserve unrelated definitions; installation must not
silently replace another agent with the same name. This content change does not
modify user configuration or the currently running Claude session.

The currently inspected MCP resource reader exposes `SKILL.md` only. Supporting
reference access belongs to the CLI/MCP migration. Preserve bounded, receipt-
verified reads when adding it; do not introduce arbitrary filesystem reads.
The role's entrypoint contains its essential operating and completion rules so a
missing reference does not erase those rules. A host that cannot retrieve the
references must disclose that limitation; complete reference delivery remains an
integration acceptance condition.

An interruption record belongs to the task's host state or project deliverables
directory. It is evidence for resumption, not a new DS API, authorization store,
agent-wide project memory or defect backlog. Persistence remains host-owned.

## Acceptance

- The native skill content and full directory pass the repository's skill and
  installer gates. No command flags, calculation implementation or private
  transport is added.
- Every local Markdown reference resolves. Claude preload names resolve to the
  canonical skills; the adapter contains no duplicate role body.
- Review the behavioral fixtures for scope, identity, coverage, recovery and
  truthful completion. Mark fixture review separately from execution against live
  DS, and never describe unexecuted scenarios as passed runtime tests.
- Host registration, receipt-matched distribution and reference retrieval must
  be verified by the migration before claiming the specialist is installed and
  available in that host.

Prove production usefulness with an actual requested delivery after its deployed
dependencies are ready. The Gisagara scenario motivates acceptance; its historical
counts, names, file paths and failures are not defaults in the reusable role.
