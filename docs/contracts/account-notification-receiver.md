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
