# Messaging client boundary

Brain owns messaging; `ds-cli-messaging` only translates its descriptors to
closed `ds-client-core::messaging::Command` calls. Both protected native Firebase
sessions and linked device sessions use the same adapter and wire representation.
No saved project selection enters a request. MCP is generated from those same
descriptors, without a separate messaging schema.

Messaging borrows the host's device authority through
`ConsolidatedDependencies.DeviceAuthority`; a host must pass its existing
authority to admit device bearers. Missing authority refuses them explicitly.

## Inventory of the existing brain API (2026-10-03)

The producer contract is [brain messaging](../../../ds-brain/docs/contracts/messaging.md),
its [messages-v1 fixture](../../../ds-brain/docs/contracts/messaging-wire/messages-v1/config.json),
and the strict action handlers in `internal/messaging/handler.go` and
`internal/handlers/notifications.go`.

| Door | Existing actions and state |
|---|---|
| `POST /api/v1/messaging` | `config`; account-wide `search_people`, project `search_project_people`, `list_contacts`; `list_conversations` with caller unread state, direct pagination and canonical project groups |
| Same door, addressing | `resolve_direct` uses immutable participant UIDs; `resolve_project` uses current membership; `resolve_notifications` addresses retained caller-owned system history; `list_members` reads current project members |
| Same door, messages | `list_messages` pages newest first using `cursor`; `send_message` appends human text and finalized upload references, derives sender and time, deduplicates `client_message_id` within conversation and sender |
| Same door, state | `advance_read` accepts only server-issued message cursors and moves forward; `advance_delivered` is a separate disabled receipt operation; `set_preferences` changes caller notification preference |
| Same door, push registrations | `register_installation`, `revoke_installation`, `list_installations`, with ownership epochs |
| `POST /api/v1/messaging/attachments` | `start`, `finalize`, `download`; server grants upload/download destinations, verifies bytes and scopes |
| `GET /api/v1/messaging/stream` | Bounded history catch-up and conversation invalidations, under repeated authorization |
| `GET /api/v1/messaging/account-stream` | Content-free per-account invalidation; never canonical unread state or a notification ledger |
| `POST /api/v1/notifications` | Caller-owned canonical personal and Project Work notices: `list`, `mark_read`, `dismiss`; push subscription `register_push`, `revoke_push`, `push_config` |

Conversation history has no parent-message reply or nested thread metadata.
Reply therefore uses the existing append action, not a second messaging model.
Different content under an existing send key is a named idempotency mismatch.
The original message and `created:false` identify an absorbed retry.

Project Work comment mentions (`project_comment_mention`) publish into the canonical notification spine; server PM
project facts can also appear in project conversation history. These are different
records. Retained personal notification conversations are read-only and have no
current producer. Acknowledgement marks canonical notices read; it neither
advances conversation state nor dismisses notices. Missing and unchanged ids are
reported by brain.

The one additional history input is `since_cursor` on `list_messages`, exposed
by the action handler using the existing authorized stream catch-up service.
It returns the latest bounded window oldest first, with `truncated` if older
unseen rows were skipped. History pagination remains the recovery path for those
rows. Every cursor remains opaque in the client.

## Local emulator proof

A debug-only build input, `DS_CLI_MESSAGING_EMULATOR_BUILD=1`, compiles a fixed
loopback transport for messaging and device linking at `127.0.0.1:18781`.
It accepts only explicit development fixture profiles, has no runtime destination input, and cannot compile in release mode. Ordinary
builds continue using the protected profile's gateway. Brain's
`TestMessagingCLIWorkflowEmulator` runs its handler workflow against named
Firestore and canonical membership. Supplying `DS_MESSAGING_CLI_BIN` runs those
assertions through the actual `ds` descriptors and protected device lifecycle.
The ordinary brain gate needs no Rust artifact. The combined proof must set that
variable; an API-only run is not evidence of CLI execution.
