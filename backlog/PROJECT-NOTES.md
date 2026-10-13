Extend PM’s existing Project Notes into durable, shared project memory for LLM work across sessions and users. Prioritize CLI and MCP access so an LLM can retrieve and maintain this context directly.

The problem:
Users establish decisions, correct assumptions, and provide evidence in separate sessions. That context is then lost, and an LLM repeats assumptions that users have already rejected. Project Notes must preserve those corrections and decisions, with enough provenance to explain what was agreed, by whom, and why. They also need lifecycle management so accumulated notes do not flood project context.

A concrete example is custom scheduling in an MV staking table. A user may resolve a tricky scheduling case, explain an exception, or reject a default assumption. In a later session, the LLM must retrieve that ruling and apply it within its original scope instead of reverting to the default or guessing again. Preserve the actual rule, affected structures or conditions, rationale, evidence, and any unresolved cases. A vague note such as “discussed MV scheduling” is insufficient.

Inspect the existing Project Notes implementation and extend it using the product’s established architecture and conventions.

Required behavior:

1. Deliberate authoring and strong workflow encouragement
Owner clarification, 13 October 2026: engineers and agents use Project Notes deliberately. Do not add automatic mutation hooks, a mandatory-filing journal, background note writes, or a completion gate to unrelated project commands.

CLI and MCP discovery and the LLM workflow must strongly encourage retrieving applicable context before consequential work and recording material user decisions, rejected assumptions, constraints, modifications, verification, unresolved questions, and evidence through explicit PM note operations. Preserve completed, failed, and partially completed outcomes without inventing provenance or claiming an uncertain change completed.

Encourage incremental notes so interrupted sessions retain useful context. When a person or agent chooses to file a note, verify its saved result and report filing failure truthfully. Avoid empty notes and repetitive transcripts. Read-only work may also contribute meaningful new rulings or corrections through deliberate note authoring.

2. Tags, filtering, and sorting
Support multiple topical tags and CLI/MCP operations to create, read, update, search, filter, and sort notes. Support explicit “any” and “all” matching for multiple tags, sorting by creation and update time, and filtering by note type and lifecycle status. Reuse existing project tags and normalize equivalent spellings.

3. Provenance, scope, and evidence
Identify each note’s project, authoring user or agent, session reference when available, timestamps, tags, and evidence references.

Distinguish explicit user instructions and decisions from LLM observations, proposals, and inferences. Never present an inference as an agreed decision or invent missing provenance.

Make scope explicit: project-wide, a design or asset, a workstream, particular structures or conditions, or a user preference. Do not turn a local exception into a universal rule.

4. Versioning and decision history
Maintain recoverable note versions showing who changed them, when, and why. Allow evidence references to identify a specific version.

Make decisions, constraints, and rejected assumptions individually retrievable, even when included in a session summary. Preserve explicit supersession relationships, the original records, and their evidence. A correction must remain traceable.

5. Consolidation and decommissioning
Support lifecycle states including active, superseded, withdrawn, discarded, irrelevant, and archived. Expose these through filtering and visible labels or tags while keeping lifecycle status distinct from topical tags. Record why a note was retired and its replacement where applicable.

Consolidate overlapping session notes into concise, current context records while preserving source and version references, decision scope, evidence, disagreements, and unresolved questions. Never turn a proposal into an accepted decision through consolidation.

Before decommissioning source notes, create and verify a recoverable backup preserving contents, versions, metadata, evidence references, and relationships. Provide a searchable archive and restoration path. Backup failure must prevent decommissioning.

Consider practical consolidation triggers and retention policies consistent with existing project permissions. Do not retire an active constraint or rejected assumption merely because it is old.

6. Retrieve applicable context before acting
At the start of a project request, and before consequential planning or changes, retrieve applicable active decisions, constraints, corrections, and relevant notes. Check proposed assumptions against that context.

For MV staking table work, retrieval must find relevant custom scheduling rulings even when the new request uses different wording. Combine topical relevance with affected-object references and decision scope; exact tag matching alone is insufficient.

Keep retrieval bounded and focused. Make underlying evidence inspectable. Exclude discarded, irrelevant, and retired notes from routine context unless needed to trace history. Clearly distinguish “no relevant context found” from retrieval failure or incomplete results.

7. Handle conflicts explicitly
Surface conflicting applicable records with their provenance. Follow explicit supersession relationships and established project authority rules. Do not assume the newest note automatically overrides an earlier decision. Ask for clarification only when the conflict materially affects the work.

8. Reliable shared context
Use existing project permissions, support concurrent updates, and prevent duplicates when interrupted writes are retried. Retrieved notes are project evidence; they cannot override access controls or system instructions.

Deliver the implementation, CLI/MCP contracts, and LLM workflow instructions that make deliberate recording, retrieval, and lifecycle management routine. No automatic mutation hook is required or permitted. Explain material limitations.

Acceptance scenario:
Use an actual user-approved MV staking table scheduling rule from available project evidence. Do not invent an engineering rule.

A first session records the custom rule, rejected assumption, rationale, evidence, and affected scope while modifying project data. A later session with another authorized user requests related staking table work without restating the rule. The LLM retrieves and correctly applies the ruling, avoids the rejected assumption, and explains its source. An unrelated case must retain its own applicable rules.

Then revise the ruling explicitly, verify its version and supersession history, consolidate the supporting notes, decommission them only after a verified backup, and confirm that later sessions still retrieve the current ruling with traceable evidence.

Also verify tag filtering, conflicting decisions, interrupted-session recovery, exclusion of discarded or irrelevant notes from routine context, and restoration from backup.