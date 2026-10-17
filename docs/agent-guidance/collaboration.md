# DS agent collaboration

This is the shared working agreement for the selected DS repositories. Root and
nearest scoped AGENTS.md files route a task to its owning contracts and checks.
They apply equally to human co-authors and coding agents. Tool-private memory is
not a second source of product authority.

## Mandatory agent instructions

Every agent, regardless of model or tool, must read and follow the workspace,
repository and nearest scoped AGENTS.md before working. AGENTS.md is the sole
agent-instruction filename. Do not create or recreate CLAUDE.md, MEMORY.md,
vendor-specific rule files, private instruction/memory indexes or handover ledgers
to bypass these rules, including in tool folders, home directories or other repos.
Use the linked owning contracts for durable decisions and the shared backlogs for
unfinished work. Tool configuration and temporary scratch may exist, but must not
become competing product authority. A necessary scoped AGENTS.md adds local rules
and links to its parent; it must not duplicate or weaken the shared agreement.
If a tool cannot use AGENTS.md, report that limitation instead of inventing another
instruction file.

## Read and scope

1. Read the current user request, workspace AGENTS.md, repository AGENTS.md and
   any nearer scoped guide. Identify the owning module and contract before editing.
2. Inspect the live branch, HEAD, worktree and index. Work directly in the canonical
   checkout on `run`. Do not create feature branches or worktrees for new work.
3. Read only the relevant contract, implementation and callers. Prefer `rg` and
   bounded reads. Do not preload every repository, historical note or domain guide.
4. A later explicit owner ruling supersedes an earlier one within its stated scope.
   Distinguish implemented behavior, accepted direction and pending owner decisions.
   A plan, old release note, example or model response is not deployment evidence.
   Surface a material unresolved contradiction; do not quietly guess a new rule.

## Ownership map

| Repository | Owns | Boundary |
| --- | --- | --- |
| ds-architecture | Cross-repository architecture and canonical component-decoupling queue | Product code/contracts/evidence and unrelated backlogs retain their owners |
| ds-command-kernel | Shared command decisions, state transitions and named native application owners | Hosts execute effects; kernel semantics do not fork by host |
| ds-network | LV/MV engineering, model facts, geometry, exchange and design-session behavior | Thin native/WASM adapters; no duplicate gateway policy |
| ds-interactions | Transient pointer, map and canvas interaction algorithms | UI-only; no Kernel/engineering dependencies and no CLI/Server implementation edge |
| ds-network-reporter | Report production, acquisition/exchange and report artifacts | Uses engineering and Solar owners; never re-solves their decisions |
| ds-solar | Prepared-input Solar calculation, portfolio and report authority | Preparation is explicit; Run is offline |
| ds-brain | Principal/project authorization, metadata and atomic governed cloud commits | No second engineering implementation; independent gateway release |
| ds-web | Shared presentation and typed browser/desktop interaction adapters | CLI/MCP lead; Interactions owns transient algorithms; no alternate product logic in TypeScript or Tauri |

Read the receiving owner's contract before changing a cross-repository payload.
Keep DS project identity distinct from GCP project/account identity. Preserve
principal, project, stable object IDs, units/CRS, revisions and digests end to end.
Do not infer identity, project scope or authorization from ambient host state.

## Small implementation loop

- Name one concrete behavior and its acceptance evidence. Finish one coherent
  vertical slice rather than opening a broad framework or parallel implementation.
- Continue within the authorized slice while a verification or review failure has
  an actionable remedy. Diagnose it, fix it, rerun the affected check and verify the
  real consumer before reporting completion. A passing helper does not close a
  broken consumer. Reuse accepted proof; repeat wider gates only when a change,
  failure or uncovered path warrants it. If blocked, name the exact blocker and
  owning next action. Stop when the agreed behavior and required proof are complete.
- Use the existing typed operation across library, CLI/MCP and presentation.
  Keep defaults, validation, refusal vocabulary and decision logic with one owner.
  New commands are self-describing and preserve explicit effects and undo/recovery.
- Bound files, collections, recursion and requests at admission. Preserve authored
  facts and engineering names. Verify malformed-input and refusal paths when touched.
- Prefer a proven community component behind an explicit adapter over maintaining
  a competing general-purpose implementation. Preserve licences and attribution;
  verify round trips and domain invariants before replacing an existing dependency.
  This discipline does not authorize a product migration by itself.
- Reads serve intentional navigation, explicit refresh or an active consumer.
  No constant polling, speculative downloads or freshness reads for unused data.
  Invalidation is retained until consumed; reuse held inputs and coalesce consumers.
  Preserve offline work, dirty edits, authorization and revision fences. Prove the
  touched acquisition path's read counts and the unused-data negative case.
  [Prompt queue sync and cheap input freshness](../contracts/ds-headless-application-runtime.md#prompt-queue-sync-and-cheap-input-freshness)
  governs cross-session reuse and intentional online freshness; apply it within
  the named owning slice rather than auditing every domain used as an example.

## Verify, then land

- Start with the affected owning tests and a negative/control case. Use each repo's
  advertised gates for wider code or release changes. A skipped emulator test,
  successful build, static screenshot or cached result is not an execution proof.
- Keep native/WASM behavior consistent where both are supported. For UI changes,
  verify the rendered result and its real data/interaction path. For artifacts,
  check reopen, units, byte/digest identity and the consumer-facing result.
- Documentation-only work reviews the diff, authority and links; it needs no
  speculative product build. When filename consumers require a separate code slice,
  follow the user's explicit execution limits and report any unrun checks.
- Re-read changed files and `git diff` immediately before landing. Stage only owned
  paths. Commit and push `run` after proof, before the next slice. Do not include
  another author's staged edits. Never reset, stash, clean, overwrite or rebase their
  work. If the same file changed concurrently, re-read and reconcile that file first.
- Record the owning contract, exact checks, commit and remaining limitation in the
  completion message. No arbitrary deadlines or large unattended agent fanout.
  A small grep/housekeeping task stays inline. Any authorized worker has one bounded
  lane; the initiating author owns its integration and unresolved work.

## Durable context and unfinished work

Put current rules in the owning contracts and their executable checks. Keep root
AGENTS.md short: ownership, invariants, task routing and real gate commands. Put
long reference material under docs/ and link it; do not duplicate it into another
instruction filename, private memory index or handover alongside code.

- `backlog/OPEN.md`: code work an agent can implement and prove independently.
- `backlog/OPEN-X.md`: the exact unresolved owner decision/authority boundary.
- `backlog/SEEDING.md`: tuning, templates/libraries, seeding and operational acceptance.

These remain in ds-command-kernel for unrelated work. New and migrated
component-decoupling code, decisions and operational acceptance use
[ds-architecture/backlog/DECOUPLING.md](../../../ds-architecture/backlog/DECOUPLING.md).
Only its named entries have moved; do not relocate other hard problems, feedback
or evidence. Split mixed obligations and continue independent
work. Routine implementation, a difficult test or an admitted control is not an
owner question. Delete fulfilled entries after proof; contracts/tests retain the
durable evidence. Coding sessions fix findings or use these backlogs, never submit
them as product feedback. No separate TODO, TEST-ME, completed-work archive or relay.

## Runtime, release and machine boundaries

- Build artifacts/containers locally and deploy the exact locally built bytes or
  image digest only when deployment is authorized. Never launch, approve or retry
  a remote Cloud Build/CI deployment, or prescribe it as a fallback. Read the owning
  deployment contract; respect its lane, source, identity and promotion checks.
- Linux Server releases build as `magese`, without root. The verified public installer
  uses `DS_GRIDDESIGN_SUDO_NONINTERACTIVE=1` for admitted NOPASSWD apt-get/dpkg;
  `sudo -v` and `sudo -n true` do not prove that permission. Dev binaries need no sudo.
- No automatic all-repository review or ds-system deployment. Retired repositories
  under _deprecated_stack are excluded unless the owner names them for a specific task.
  Existing worktree retirement follows the maintained disk-cleanup procedure and
  landing proof; the retirement rules do not authorize creating a new worktree.
- Before another memory-heavy build/browser/benchmark, inspect available RAM and
  active jobs. Below 20% available RAM, start no additional heavy job. Do not kill
  another author's process. Keep scratch, browser profiles and build output on disk,
  never tmpfs /tmp or /dev/shm. Preserve warm caches; clear admitted idle disposable
  output only near 90% disk use, following the maintained cleanup procedure.
- For an explicitly requested Claude-assisted review, honor the owner's newest
  live Opus/max-effort preference and verify the installed model surface; no Fable.
  Other explicitly chosen models remain the user's choice. Do not substitute a
  tool's private memory for these shared boundaries or invent a new model policy.
- Windows Desktop project deliverables live in the appropriate Google Drive G:
  project folder, with a verified Desktop shortcut to that folder. Compute/report
  generation runs on ds-server; a server user's Desktop is not the delivery target.

## Documentation review

For Markdown-only housekeeping, inspect the diff and linked destinations. Keep
AGENTS.md real, short and model-neutral. Preserve all current owner rules in linked
references; remove obsolete aliases and memory indexes only after reconciliation.
Do not run product tests, builds or deployment for this cleanup. If existing code
or a test loads a removed filename, record that narrow follow-up in OPEN.md; code
changes require a separate authorized slice.
