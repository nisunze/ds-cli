# Survey entry outbox v1

`surveyEntryOutbox` is a pure, bounded header-only replay authority. It admits
account-owned Survey mutation headers and returns fenced persistence, dispatch,
retry, recovery, and removal actions. Payloads remain opaque in durable storage;
the sole payload patch is an explicit `expected_version` after one update-like
conflict rebase.

The host persists each `persist` action using its `expected` owner, generation,
lease, status, and phase fence before performing an effect. Dispatch is limited
to ten FIFO heads per project/form/document, while foreign-account rows are
retained without blocking the active account. Legacy rows have no owner or
generation and are surfaced as `legacy_scope_required`; they can only be bound
by `recover_legacy` with an operator confirmation and a new UUID generation.

Results are fenced by UID, generation, lease and phase. HTTP classification,
bounded backoff, three-auth cap, one conflict rebase, strict create-receipt
ambiguity, and restart recovery are owned by the command. The host performs storage effects, transport, auth refresh, and local version
reads. The current browser adapter still uses IndexedDB CAS; its SQLite/OPFS
replacement must retain this same Kernel authority.


## Native SQLite queue

`ds-sync-store` retains opaque Survey rows in the existing native SQLite store,
partitioned by account, deployment and installation, with exact project/form/
document identity. `core.survey.queue` exposes closed admission, owned legacy
import, header/row reads and project-scoped transitions through private files.
It performs no network effects. Each transition evaluates the Kernel and commits
all fenced persistence atomically before returning dispatch effects. Reads of an
absent store/table are empty; unsafe or corrupt storage is an error.

Legacy imports preserve the original row JSON, including payloads, overlays,
timestamps and idempotency keys. Foreign and ownerless rows refuse without
assignment. A durable import receipt retains the original digest and byte length:
repeating an identical import after replay or removal does not overwrite or
recreate queued work. Different original bytes refuse the whole batch. A device
importer may delete its legacy source only after exact receipt verification and
a compare-and-delete of the unchanged source row. Import receipts and queue
writes commit together; there is no runtime fallback to legacy storage.

Native storage acceptance is independent of the Desktop migration rollout.
Drafts, media, authoritative Survey holdings and browser OPFS storage are not
migrated by this queue operation.
