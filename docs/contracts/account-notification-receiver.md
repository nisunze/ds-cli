# Account activity receiver

The shared `ds-client-core::account_notifications` policy consumes the existing
`domains.messaging.account_stream` route. One primary Desktop shell owns one
abortable receiver for its explicitly authenticated UID and install lane. Hidden
or close-to-background window state does not own the receiver. Popouts consume
its content-free events. Closing the primary window retains credential renewal
when its process owns residency or still has a visible popout; explicit Quit
or OS shutdown cancels it.

Owner ruling, 2026-10-10: notifications are account-wide across authorized project
groups; active project selection cannot suppress delivery or change silently.
One account/lane receiver must fan out invalidations to visible peers. The
current implementation is one receiver per process, not that singleton.
The owner's post-power-cut clarification means one Desktop notification owner
remaining in the tray, not a warm cloud Brain instance. Additional visible
workspaces must not accumulate hidden resident processes when closed. Explicit
Quit releases the owner and terminates it; a crash or OS restart cannot leave
stale ownership or start duplicate receivers. The native host now bounds fully
hidden residency with an OS exclusive file lock per installed lane. Competing
visible workspaces stay independent; closing an additional workspace confirms
process teardown or keeps it visible. Closing the last popout rechecks residency.
The lock inode is retained, but process exit releases ownership without PID
scanning, polling or stale-file deletion. This is a process bound, not a shared
receiver, credential handoff or push transport. Linux host and subprocess proof
are in [Desktop lifecycle](../../../ds-web/docs/desktop-background-lifecycle.md);
installed Windows execution remains unqualified.
Browser reads are navigation/refresh/push-driven; automatic browser account and
conversation long polls are removed. A missing push capability must remain
truthful, not activate polling. The remaining native transport is described
below as a limitation, not fulfillment of the no-poll requirement.

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
is no collection/timeline scan. Gateway buffers these responses, making the
repeated HTTP reconnect loop long polling. It fails the owner's no-poll
acceptance and must be replaced before the deployment sprint. Only visible clients perform existing
canonical reconciliation. The receiver is not a second message/unread store.

The host owns notification presentation and window actions only. Content-free
signals justify “Messaging activity”, with no guessed sender, body or unread
claim. Linux has one replaceable OS card and one abortable click waiter, limited
to ten seconds. Logout/account changes close it. A click restores the primary
window and opens the exact conversation/message under its current account.
