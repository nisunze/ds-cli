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
ambiguity, and restart recovery are owned by the command. The browser performs
only IndexedDB CAS, transport, auth refresh, and local version reads.
