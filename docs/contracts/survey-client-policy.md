# Survey client policy

`survey::survey_evaluate` is the shared native/WASM authority for offline Survey
client decisions. `surveyEvaluate` exports the same evaluation to browser hosts;
there is no TypeScript policy fallback. Requests and results are JSON, with clocks,
identity, grants and existing state supplied explicitly by the host.

| Operation | Decisions |
| --- | --- |
| `fill`, `fields`, `draft` | Pinned form revision, visible values, validation, binary draft markers, capture filenames and submission plan. |
| `validate` | Read-only typed `ds.survey-form-validation/v1` preview of supplied fields/values and revision label; returns visible/hidden keys, runtime errors and exact input digest. No acquisition, authority or entry mutation. |
| `factory`, `project_forms` | Canonical form documents, dirty state, draft versions, save/conflict receipts, public visibility and project-form receipts. |
| `mutation` | Create capture, identity/alias normalization, update/delete/restore bodies, capability admission, geometry and optimistic deltas. |
| `access`, `policy` | Edit eligibility, editable field projection, node/edge relationships, location and connectivity rules. |
| `read` | Scoped read admission, replication cutoffs, stream summary validation, pending-write protection, retries, completion identity and dashboard aggregates. |
| `bridge` | Fixed migration/download plans and bounded receipt projection. |

Generic requests are bounded to 1 MiB and forms to 512 fields. Mutation requests
may contain one prior entry and one delta, so their combined input limit is
3 MiB; each outgoing create/update payload remains bounded to 900 KiB. Read
batches contain at most 64 compact headers; browser features remain in host
storage and are selected by returned indexes. Pending headers are also evaluated
in batches of 64, including inside the host's entries/outbox transaction.

The host obtains device/media evidence, owns credentials, persists bytes and
transactions, executes declared transport and renders outputs. It captures UID,
project and form revision before asynchronous work and discards stale effects.
Capture and mutations commit optimistic state and the outbox intent atomically.
Mirror checkpoints and replacements inspect the outbox within the same database
transaction, protecting queued entries from upstream upserts and deletions.

The native modules `survey_query`, `survey_entries_select`,
`survey_entries_changes` and `survey_control` own their typed request grammar,
validation, successful response decoding and bounded projection. Native client
core retains transport, service errors and its public `ClientError` interface.
Selection digests and changes cursors preserve their original identity fences.

Entry delivery remains governed by [create](survey-entry-create.md) and
[outbox](survey-entry-outbox.md). Media remains governed by
[photo outbox](survey-photo-outbox.md). Firestore commit acknowledgement does not
establish BigQuery convergence. Kernel portability does not itself certify a
host's persistent storage or an Android app-kill/reboot recovery path.
