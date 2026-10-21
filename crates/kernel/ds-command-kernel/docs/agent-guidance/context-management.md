# Agent context management

**Status:** shared documentation  
**Scope:** Codex, Claude Code and other coding agents working in this workspace

This guide keeps agent context small enough for accurate code work while retaining
the project facts needed to finish a task. It governs instruction placement and
work decomposition; it does not replace an owning product contract.

## Research basis

Official OpenAI guidance says Codex loads AGENTS.md files from global and root
locations down to the working directory, with nearer files applied later. Because
those instructions are injected before the task, every unconditional line consumes
context on every turn. OpenAI recommends pruning accumulated instructions, avoiding
mandatory full-document reads, and using progressive disclosure for task-specific
workflows. It also distinguishes active-run compaction from durable, reviewable
artifacts and recommends subagents for independent work while keeping dependent
steps in the main agent.

Official Anthropic guidance describes the same cost model: project instructions
are persistent context, skills and reference material should load on demand, and
subagents provide isolated context. It recommends keeping project instructions
short, clearing context between unrelated tasks, and using focused compaction for
long sessions. Current Claude Code can read AGENTS.md directly; by default it does
so when no project CLAUDE.md is present. This workspace therefore keeps one
cross-tool instruction system instead of duplicating rules.

Sources, reviewed 2026-10-14:

- [OpenAI: Rethinking skills and prompts](https://developers.openai.com/blog/rethinking-skills-and-prompts-for-gpt-6-astra)
- [OpenAI: Codex prompting guide](https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide)
- [OpenAI: Multi-agent](https://developers.openai.com/api/docs/guides/agents-api/multi-agent)
- [OpenAI: Memory and compaction](https://developers.openai.com/cookbook/examples/agents_sdk/building_reliable_agents_memory_compaction)
- [Anthropic: Extend Claude Code](https://code.claude.com/docs/en/features-overview)
- [Anthropic: Project memory and AGENTS.md](https://code.claude.com/docs/en/memory)
- [Anthropic: Claude Code best practices](https://code.claude.com/docs/en/best-practices)
- [AGENTS.md open format](https://agents.md/)

## Put information in the cheapest correct place

| Information | Location | Loading rule |
| --- | --- | --- |
| Rules true for every workspace task | workspace AGENTS.md | always |
| Repository ownership, invariants, task router and checks | repository AGENTS.md | when that repository is selected |
| Rules for one subtree | nearest scoped AGENTS.md | when files in that subtree are touched |
| Product behavior and schemas | owning contract | only for the affected behavior |
| Repeatable optional workflow | skill or maintained workflow document | only when invoked or matched |
| Unfinished code or owner decision | OPEN.md, OPEN-X.md or SEEDING.md | only when relevant |
| Test output and reproduction evidence | disk-backed verification artifact | summarize; open details on demand |
| Current implementation truth | code, tests, Git status/diff/log | inspect fresh |

Links in AGENTS.md are routers, not a reading list. Do not load a linked document
unless the task matches its row. Imports and duplicated text still consume context;
moving prose behind an unconditional import does not solve overload.

## The small implementation loop

1. **Orient.** Confirm working directory, branch and status. Read the workspace,
   repository and nearest scoped AGENTS.md. Identify the owner and one relevant
   contract with bounded searches.
2. **Establish evidence.** Reproduce the behavior or identify the exact contract
   delta. Read the implementation, its real consumer and owning tests. Avoid a
   repository-wide survey unless the question is repository-wide.
3. **Implement one slice.** Keep one agent responsible for a decision chain and
   every file that must change together. Preserve concurrent edits.
4. **Verify narrowly.** Run the affected test plus a meaningful negative or
   control. Widen checks only when the changed boundary or a failure justifies it.
5. **Land or record.** Inspect the diff, commit only owned paths, and record
   genuinely unfinished work in the existing owning backlog. Do not create a
   session handover file or completed-work archive.

Documentation-only work reviews wording, links, authority and the diff. It does
not run product builds or deployment.

## Context-efficient tool use

- Search before reading. Prefer rg and bounded line ranges over whole-file dumps.
- Batch independent reads, but keep mutations and dependent checks sequential.
- Save large logs to the workspace verification area; report the command, result
  and relevant lines. Do not paste an entire build log into the conversation.
- Reuse a passing result until code, inputs or an uncovered path changes.
- Treat source inspection, isolated tests, built artifacts and installed execution
  as different evidence. State which one was proved.
- Recover after compaction from the task request, Git state, owning contracts,
  committed history and saved evidence. Conversation summaries are navigation,
  not product authority.
- Start a fresh session for an unrelated task. Do not carry a long debugging
  transcript into a new feature.

## Delegation without context flooding

Use a separate agent when the work is independent, read-heavy, naturally parallel
or isolated to a disjoint repository. Work directly for short searches, dependent
steps, single-file changes and decisions that need the active conversation.

Every delegated prompt must name:

- one objective or question;
- exact repositories or paths;
- allowed mutations and explicit exclusions;
- the evidence or return shape expected;
- whether it may commit, push, build or contact an external system.

Agents editing the same file do not work in parallel. One agent owns integration
and final verification. A worker returns conclusions, exact paths, commands,
test results and blockers; it does not return a transcript of every file read.
Give an independent worker fresh context. Pass parent history only when the task
depends on decisions that cannot be stated compactly.

## Maintaining the instruction system

Review an AGENTS.md rule when it is repeatedly ignored, duplicated or requires
exceptions. Before adding a line, ask:

1. Does every task in this scope need it?
2. Can the agent infer it from code or a standard tool?
3. Is it a conditional workflow that belongs behind a link?
4. Is it enforceable automation better implemented as a gate rather than prose?
5. Does it conflict with a nearer guide or current owner contract?

Remove stale model workarounds as models and harnesses improve. Keep the rules
model-neutral and concrete. For Claude Code, require a version with direct
AGENTS.md support and verify the loaded project instructions; do not add a
duplicate CLAUDE.md compatibility layer to this workspace.
