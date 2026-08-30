# Unified identity contract — `ds.auth-context/v1`

## Scope and current status

`AuthContext` is the provider-independent, non-secret projection a DS command
may use to reason about identity and authority. Credential bytes remain inside
the provider that obtained them. Implemented providers are the native Firebase
refresh session and the distinct DS device credential owned by
`ds-client-core`; password input remains a trusted-terminal bootstrap and is
not copied into device authorization.

## Bounded context

The context contains only:

- canonical UID and email, when signed in;
- deployment lane;
- non-secret client/profile/catalog-entry/audience digests;
- normalized authority capabilities;
- an optional device-local selected project projection;
- credential-provider kind;
- optional public device identity and public-key fingerprint;
- an independent, non-authoritative paired-map observation;
- session state.

It contains no password, access token, refresh credential, authorization
header, session secret, private device key, endpoint override, or generic
transport input. Access JWTs remain in process memory inside the closed native
client core. Existing protected refresh rotation and UID/email/lane/audience
project fencing remain authoritative.

`auth.status` contract 2 preserves its contract-1 fields and adds
`auth_context`. Resolving status may rotate protected provider state and may
read the selected-project fence. A stale identity, lane, or audience fence is
therefore a typed `project_context_stale` refusal rather than an omitted or
silently accepted project.

## Project address, not UI state

“Selected” or “active” project means only one device-local project address and
its authority fence. It is not project data, a loaded map, an open edit room,
an unsaved selection, a viewport, or proof that Desktop is running. A browser
provider may persist this bounded address in IndexedDB; a headless provider
may persist the equivalent address in protected per-user native state. Neither
storage location becomes project authority.

The address is useful only after it was chosen from a fresh visible-project
directory, remains bound to the canonical principal, lane, profile/audience,
and (for device sessions) exact device and entitlement revision, and is
re-authorized by the server on every project operation. Desktop and headless
devices deliberately do not copy one another's selected address.

The selected-project address and the map's active project are separate facts.
They may be equal, different, or one may be absent. A composed provider can
report `map_state` as `unavailable`, `paired`, or `active` with the map's own
active project address. A native/headless identity resolution reports
`unobserved` and never probes or launches Desktop merely to fill this field.

Ordinary `user` or `project` operations act through their canonical principal
and device-local selected project even when no map exists or the map displays a
different project. Only an explicitly `map`-capable command may require active
map state. A map observation is useful context, not an implicit limit or a
fallback source of project authority.

## Open-map coherence

When a map is open on the same project, a committed CLI/API change must become
visible through the same local projection and render path as a UI-originated
change. This is synchronization after a governed commit, not permission for
the CLI to write browser storage directly and not a second map-dependent
mutation path.

The shared-data authority must emit a bounded change carrying exact project,
domain/entity identity, committed revision or generation, canonical actor,
device/provider attribution, and operation identity. The browser consumes that
ordered change into its IndexedDB projection and the map redraws from that
projection. A UI mutation uses the same commit and change path, so its visible
result is not a separate implementation.

Coherence rules are:

- apply a change only to the matching lane, audience, and active map project;
- deduplicate by operation identity and reject out-of-order generations;
- never overwrite unsaved map/edit-room state silently;
- surface revision conflicts for rebase or explicit operator resolution;
- retain tombstones and removals so deleted objects cannot reappear from a
  stale cache;
- acknowledge a CLI write only from the governed commit receipt, not from an
  optimistic map update;
- allow push/event delivery for immediate refresh, with a fenced change-feed
  catch-up after disconnect or restart.

If the map is absent or open on another project, the CLI operation remains
valid and headless. The relevant map catches up when that project becomes
active. Interactive, unsaved UI state remains map-owned and is never inferred
from IndexedDB or changed by a project-only CLI command.

## Normalized capabilities and compatibility

Existing descriptor tokens retain their exact observable meaning:

| Descriptor token | Normalized requirements |
| --- | --- |
| `none` | `none` |
| `headless_user` | `user` |
| `headless_project` | `user`, `project` |
| `desktop_pairing` | `desktop` |
| `desktop_user` | `user`, `desktop` |
| `project` | `user`, `project`, `desktop` |

Legacy `project` remains Desktop-backed. Removing its Desktop requirement
would weaken a live public contract, so provider-independent project commands
continue to use `headless_project` until each legacy command is migrated behind
a shared backend/native-core operation and its descriptor contract is
versioned. No legacy token implies `map`; an interactive viewport requirement
must be introduced explicitly after the paired session can prove active-map
state.

An authenticated native session advertises `none` and `user`. It advertises
`project` only when an exact selected project passes the existing principal,
lane, profile-audience fence. It never advertises `desktop` or `map`.

## Provider boundary

The credential-provider adapter resolves one `AuthContext` and reports a
provider kind. Resolution may refresh/rotate protected state, but its result is
always the bounded context above. Login bootstrap, project selection, logout,
device completion, and device revocation remain typed operations; they
must not be represented as caller-supplied credentials.

A Desktop provider and headless-device provider must resolve the same
canonical principal and project entitlements while retaining distinct
credentials and device attribution. Commands consume required capabilities,
not provider-specific tokens.

### Guarded automatic arbitration

Provider choice is capability- and identity-bound, not a preference for a
running UI. Desktop/map authority continues to use the paired application. A
user/project operation may use paired Desktop automatically only when its
bounded session observation exactly matches the canonical match key: lane,
stable credential audience, and Firebase UID. Canonical email is deliberately
not part of this key. Project equality remains a separate operation-target
fence.

When there is no exact Desktop match, the operation uses the durable headless
device provider. A valid headless project operation therefore remains valid
when Desktop is closed, signed out, or showing another project. A mismatch is
never repaired by switching an account or project, and map availability is
never treated as a limit on headless authority. Same lane/audience with a
different UID, or any different lane/audience, prevents Desktop arbitration.
An explicit map-independent operation proceeds through its own device
provider, while a map-attached/shared-projection operation refuses with
`auth_context_mismatch`; it never bridges or injects state across that identity
boundary. Every command receipt retains the provider and public device
attribution that actually authorized it.

There is one map runtime and one IndexedDB implementation. Map-local or
IndexedDB-backed operations attach to the existing DS map application through
its typed bridge. If it is absent, a future bounded launcher slice may start
that same application/runtime in the background—including on a Linux server—
with the exact persistent-storage identity, lane, audience, canonical UID, and
project, then attach through the same bridge. Window visibility is only
presentation; unattended does not mean map-engine-forbidden. The launcher
must refuse every identity/storage mismatch and must not introduce a second
headless browser, direct IndexedDB adapter, emulator, or local-state engine.
The current CLI has no proven identity-bound launcher contract, so this slice
preserves the bridge seam and leaves auto-launch as an explicit next slice.
There is bounded precedent: MCP already launches the exact installed Windows
application once, with no arguments, null standard streams, and a ten-second
descriptor wait. The Linux follow-up may extend only that policy to the exact
installed `/usr/bin/ds-web-desktop` (or an exact package sibling) when a real
graphical session exists. With no display it must return a typed refusal. It
must not add Xvfb, a headless-browser stack, service manager, alternate state
root, or retry fan-out.
Map-independent governed cloud operations continue to call their fixed
`ds-brain`/native-core contracts directly and do not launch a map runtime.

For an eligible typed operation, precedence is exact matching map runtime
first, executed inside that runtime through the closed bridge without
exporting its Firebase token; otherwise a valid protected headless-device
credential; otherwise the command returns the typed link/login remedy. A
project-bound operation adds exact selected-project equality to the
lane/audience/UID match. Exact map authority therefore reduces or eliminates a
second CLI sign-in, while a different map account, lane, audience, or project
never steals or limits an explicit headless operation.

Map and headless project selections remain separate. For a map-attached
operation, an exact-identity map may supply its own active project when the
headless provider has no selected project; the CLI must not force `auth project
use` first. When both selections exist they must be equal or the map-attached
operation refuses. A `HeadlessProject` operation may intentionally use its own
selected project independently and must not attach to or mutate a different
open map.

Registry dispatch scopes one non-network observation of every protected
headless provider. Only the shared typed Desktop invocation seam consumes it:
immediately before its POST it snapshots UID, lane, stable audience, project,
and session revision, performs exact arbitration, and carries that snapshot as
top-level transport metadata. The application rechecks the same fence
immediately before operation dispatch. Pure backend commands never call this
seam and never require a map.

The durable headless credential is the protected Ed25519 private key plus
public device metadata bound to device id, canonical UID, lane, and stable
`credential_audience_sha256`. The exact profile/catalog digests approved at
link time remain provenance for audit and context reporting; ordinary release
changes do not globally revoke device credentials. A five-minute DS access JWT
is minted only by a signed complete/refresh exchange, stays in process memory,
is sent as an ordinary Bearer token on fixed native-core calls, and is
zeroized rather than stored or projected. The backend checks the exact active
device row on each call so one-device revocation takes effect independently.

## Current MCP principal handoff

The typed `auth-context` MCP profile is a bounded view of the existing live
CLI contracts: identity status, device link begin/status/complete, public
device inventory/revocation, fresh visible-project inventory, exact
visible-project selection, and selected-project status. A person may establish
Firebase refresh with trusted-terminal password login or explicitly approve a
distinct device link. The MCP child receives only public projections; pending
secrets, passwords, access tokens, refresh credentials, private keys, and
protected state paths never enter MCP traffic.

The profile deliberately omits password login, logout, and the Desktop-owned
`auth link approve` operation. Its tools are generated from the same live command descriptors as the CLI, so
their authority, arguments, effects, refusals, and result envelopes are
unchanged. The MCP adapter also excludes password login and Desktop approval
from its broad compatibility exposure; no profile choice can make either tool
appear.

## Desktop-owned device approval

`auth link approve --request <id> --device-fingerprint <sha256:hex> --lane
<stable|canary> --yes` is the one device-link operation owned by paired
Desktop. The CLI passes no credential. It invokes the fixed
`auth.link.approve` bridge operation first with `confirm: false`, verifies that
the returned pending preview preserves the exact request, fingerprint, lane,
`ds.api` scope, expiry, and renewable flag, then repeats the same closed
arguments with `confirm: true`. Registry confirmation refuses the command
before the bridge opens when `--yes` is absent.

This approval command is CLI-visible for a person who can compare the public
fingerprint on both devices, but is never an MCP tool. The result is public
device/binding/decision metadata only; canonical credentials, access JWTs,
proofs, private keys, and Desktop session material never enter the bridge
receipt.

## Device authorization owner contract

The fixed server routes and bounded payloads provide:

1. creating a short-lived request bound to lane, profile/audience, catalog
   digest, receiving public key, nonce, PKCE-style challenge, and exact scopes;
2. approving or denying it from an already authenticated principal with
   explicit confirmation and audit attribution;
3. consuming it once for a distinct renewable device credential bound to the
   receiving private key;
4. listing only public device/session metadata and revoking one exact device;
5. refusing replay, expiry, fingerprint/lane/profile/principal/audience/scope
   mismatch, revoked state, and unattended approval with stable error codes.

The completed credential must use the existing protected-state adapter (plus a
protected device-key adapter) and remain independent of Desktop lifetime. MCP
may begin, inspect, and complete a request after human approval, but may never
accept a password/token, approve any request, or read Desktop credential
stores.

## Remaining migration order

1. Keep password login/logout and all Desktop/map descriptors regression covered.
2. Migrate project-data commands one authority-owned slice at a time from the
   Desktop bridge to shared project operations; preserve concurrency fences
   and actor/device attribution on every write.
3. Introduce explicit map authority only with a provable active-map state and
   a typed map-required refusal.

Acceptance must include restart persistence, Desktop independence, exact
project selection, CLI/MCP principal equality, device-only revocation, replay
and mismatch refusals, and searches proving that no credential material enters
arguments, environment, output, logs, receipts, MCP traffic, project files, or
workspace configuration.
