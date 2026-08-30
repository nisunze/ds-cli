# Unified identity contract — `ds.auth-context/v1`

## Scope and current status

`AuthContext` is the provider-independent, non-secret projection a DS command
may use to reason about identity and authority. Credential bytes remain inside
the provider that obtained them. The first implemented provider is the native
Firebase refresh session already owned by `ds-client-core`; password input is
only a trusted-terminal bootstrap for that provider.

This contract does not claim that device authorization exists. A renewable
device grant must be minted, consumed, inventoried, and revoked by an
authenticated server authority. `ds-cli` must not invent a local grant, copy a
Desktop credential, or advertise link/device commands before those fixed
server operations and their typed responses exist.

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
future device completion, and device revocation remain typed operations; they
must not be represented as caller-supplied credentials.

A future Desktop provider and headless-device provider must resolve the same
canonical principal and project entitlements while retaining distinct
credentials and device attribution. Commands consume required capabilities,
not provider-specific tokens.

## Device authorization owner contract

Before `auth link begin/status/complete/approve` or `auth device
list/read/revoke` can be declared, the server owner must provide fixed routes
with bounded typed payloads for:

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

Until that server contract exists, signed-out remedies continue to name the
working trusted-terminal password flow. Advertising `headless_link_required`
with a nonexistent next command would be a false recovery path.

## Migration order

1. Keep password login/logout and all Desktop/map descriptors regression
   covered.
2. Land server-owned one-time device grants, device inventory/revocation, and
   non-secret audit events.
3. Extend the closed native core and protected-state adapters for device keys
   and renewable device sessions.
4. Add link/device CLI descriptors, MCP exposure rules, and exact negative
   tests only when those operations are reachable.
5. Migrate project-data commands one authority-owned slice at a time from the
   Desktop bridge to shared project operations; preserve concurrency fences
   and actor/device attribution on every write.
6. Introduce explicit map authority only with a provable active-map state and
   a typed map-required refusal.

Acceptance must include restart persistence, Desktop independence, exact
project selection, CLI/MCP principal equality, device-only revocation, replay
and mismatch refusals, and searches proving that no credential material enters
arguments, environment, output, logs, receipts, MCP traffic, project files, or
workspace configuration.
