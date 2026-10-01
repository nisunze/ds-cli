# Account activity receiver

The shared `ds-client-core::account_notifications` policy consumes the existing
`domains.messaging.account_stream` route. One primary Desktop shell owns one
abortable receiver for its explicitly authenticated UID and install lane. Hidden
or close-to-background window state does not own the receiver. Popouts consume
its content-free events; conversation content streams remain visible-only.

The gateway verifies the bearer; the native session handover also checks exact
JWT subject, audience, issuer and expiry against the install pin. Credentials
remain in memory, are scrubbed on drop, and are never copied to a device store or
borrowed from CLI profiles. Logout/account change revokes the generation and
cancels transport/actions before accepting another account. Token renewal asks
the same primary shell for its own fresh token.

## Rolling wire compatibility

The receiver requests the fixed `ready=v2` selector on the existing account
stream route. Omitted selectors preserve the old exact `{schema, revision}`
response; empty, duplicate or unsupported selectors are refused before stream
authentication. This keeps open older clients compatible with the new backend.

Negotiated account `ready` schema 1 retains `revision` and adds `last_signal`:
null, or the exact existing `AccountSignal` identity (`schema`, `conversation_id`,
`message_id`, `kind`, and `project_id` only for projects). The backend obtains
it from the initial exact-document snapshot already read by the stream. No
new query, storage row, history log or cursor is introduced. Old ready responses
remain accepted; unknown fields and malformed identities are refused. A first
ready seeds the baseline without presenting historical activity. Later ready
identities and live invalidations share a bounded 256-entry in-memory dedupe
window. Timestamps never infer unread state or missed messages.

The feed overwrites one account document. Several updates while disconnected can
coalesce to the latest identity: this promises a useful activity notice, not a
per-message delivery ledger. Old servers without `last_signal` retain foreground
canonical reconciliation but cannot prove native reconnect-gap notices.

## Bounds and cost

Response: 1 MiB, frame: 16 KiB, request deadline: 55 seconds (includes the
backend's bounded cold-start initialization). Reconnect uses server-advertised
1–30 second delay and capped exponential failure backoff. The integrated backend
holds a watch for 28 seconds and advertises 30 seconds after a quiet watch,
about 62 requests/hour per idle signed-in shell. Active reconnects can approach
124 requests/hour. Each watch reads/watches one exact account document; there
is no collection/timeline polling. Only visible clients perform existing
canonical reconciliation. The receiver is not a second message/unread store.

The host owns notification presentation and window actions only. Content-free
signals justify “Messaging activity”, with no guessed sender, body or unread
claim. Linux has one replaceable OS card and one abortable click waiter, limited
to ten seconds. Logout/account changes close it. A click restores the primary
window and opens the exact conversation/message under its current account.
