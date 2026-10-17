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

`ds-sync-store` retains newly captured opaque Survey rows in the existing native
SQLite store, partitioned by account, deployment and installation, with exact
project/form/document identity. `core.survey.queue` exposes closed admission,
header/row reads and project-scoped transitions through private files. It performs
no network effects. Each transition evaluates the Kernel and commits all fenced
persistence atomically before returning dispatch effects. Reads of an absent
store/table are empty; unsafe or corrupt storage is an error.

New captures become durable in SQLite before the application reports success.
Admission retains payloads, overlays, timestamps and idempotency keys, and refuses
foreign or ownerless rows without assigning ownership. Existing authoritative
Survey entries repopulate through the declared API read path. Old pending browser
entries are not carried forward: there is no legacy import, migration bridge or
runtime fallback to legacy storage.

Drafts, media, authoritative Survey holdings and browser OPFS storage remain
outside this queue operation.
