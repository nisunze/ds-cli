# A delivery record that survives interruption

Use this for work spanning imports, seeding, composition, batches or publication.
Prefer an existing host task record. If a local record is useful and file writing
is available, keep it in the designated project deliverables directory. Do not
write project state into a reusable agent definition, shared skill or repository
root. The record summarizes evidence; it grants no permission and implements no
DS operation. Credentials, session descriptors and signed transfer URLs do not
belong in it.

## Record only what helps the next decision

| Section | Useful contents |
|---|---|
| Assignment | User's requested outcome, exact project ID, lane, audience, selected formats and destination |
| Decisions | Authorized effects with their conversation source; exclusions or unresolved choices; facts inferred versus supplied |
| Input baseline | Inventory receipt, complete membership, original source locators and available digests, model/version IDs, source document page references |
| Dependencies | Which source/model, design revision, recipe revision and context coverage each output uses |
| Progress | Intended outputs; verified results and receipt locators; incomplete work; next useful action |
| Review | Checks actually performed, sampled sheets/crops, findings, corrections and any remaining limitations |
| Delivery | Local artifact locators, governed version/attachment identities, publication readback, counts by state |

For transformer sheets, use one row per exact transformer ID and requested
format. For overviews or grouped reports, use the corresponding explicit scope.
Choose a compact representation suited to the job; this is not a server schema.

Keep these observations separate:

- **Prepared:** required source/design/context and the recipe are ready.
- **Generated:** an artifact receipt identifies the output and its dependencies.
- **Verified:** membership, freshness and relevant content checks passed; record
  which artifacts received visual inspection.
- **Published:** the requested destination has a verified artifact/attachment
  identity. This observation is unnecessary for a local-only assignment.

An artifact can be generated and still fail review. An uploaded artifact can be
stale. Neither state becomes complete by changing a label in the record.
Keep missing, failed and deliberately excluded rows visible in totals.

## Resume from evidence

1. Recover the latest user decisions and this record. Check for later corrections
   before relying on a saved authorization or source choice.
2. Verify the installed surface, requested project and the authority needed for
   the next operation. A stored receipt is historical evidence, not current login,
   project selection or availability.
3. Compare relevant live inventories and revisions with the baseline. Inspect
   partial/ambiguous publications before replaying writes. Protect dirty design
   rooms and independent user changes.
4. Mark only affected outputs stale. Preserve original sources and valid work.
   Reconcile added/retired members with the user's intended meaning of "all";
   name any resulting scope change rather than silently using yesterday's count.
5. Continue from the earliest unmet dependency, keeping unrelated work moving.
   If the next action cannot proceed, state the precise dependency and the
   evidence that would permit a fresh attempt.

A reference to an official feedback receipt may explain an incomplete phase.
Do not turn the delivery record into a parallel backlog, automatically resubmit
an existing issue or treat a historical refusal as permanent after an update.

## Handoff between agents

Pass the assignment, relevant baseline and receipts, current progress and allowed
effects. Mark historical observations with their observation time when available.
Do not pass credentials. A review helper receives immutable artifacts and bounded
read access; a writer receives explicit ownership of the affected state. Returning
control does not authorize both agents to keep mutating the same Desktop project.

Before ending a turn that cannot complete delivery, include a self-contained
status with verified counts, usable artifact links, unfinished scope and the
next action. Do not call a plan, locally rendered proof or queued job delivered.
