# Account notification delivery

Owner ruling, 2026-10-12: messaging uses pushed notifications. Account and
conversation HTTP streams, recurring watches, reconnect loops and polling
fallbacks are removed. Missing push support is reported explicitly; it does not
start another transport.

Existing FCM installation registration, notification preferences and canonical
HTTP message, acknowledgement and read-cursor operations retain their authority.
Notifications are account-wide across authorized project groups. Receiving a
notification never changes the active project or conversation.

The existing push envelope carries version, conversation/message identity,
kind, installation epoch and the optional project identity. Clients reject
another owner or stale installation epoch and deduplicate repeat hints. Push
does not supply authoritative message contents or unread counts. It can present
the existing privacy-safe system notification and retain an invalidation.

Conversation and project data are read only when a visible consumer needs them
or the person explicitly refreshes. Closed Messaging retains dirty identities;
it performs no conversation, message, contact or project acquisition. Opening
the consumer coalesces accumulated invalidations into one canonical read.
Notification delivery retains its own notification contract and grants no
permission to download unused project data. Account changes fence pending reads
and preserve local drafts and durable outbox recovery.

Desktop residency and visible popout behavior remain owned by
[Desktop lifecycle](../../../ds-web/docs/desktop-background-lifecycle.md).
Residency does not keep a Brain request open. Existing Firebase push runs only
where its supported runtime and permissions are available. Installed platform
delivery and removal of old-client stream traffic require matching deployment
and are operational acceptance, distinct from source tests.

## Headless registration inspection and cleanup

The native `messaging.installations` command makes one intentional read through
Brain's existing account-scoped installation owner. It returns at most ten
redacted registrations, no raw Firebase installation identity, token or ownership
epoch. Stored platform labels are historical registration metadata; delivery is
`not_proven` and source completeness is `unknown` because the existing producer
supplies no completeness receipt. This read starts no receiver and acquires no
conversation or project content.

`messaging.installation.revoke` is an explicit write using the exact hash and
ownership epoch held from the original registration receipt. Listing never
provides an epoch and cannot be used to manufacture ownership. A missing target,
a stale epoch or an installation transferred to another account returns an
idempotent `revoked: false`; a caller cannot remove the new owner's endpoint.
Malformed receipts refuse before restoring credentials or making a request.
These controls reuse the protected account and fixed messaging route. They do
not register an unsupported Desktop transport, establish live delivery, or
complete the native resident receiver and peer handoff acceptance.
