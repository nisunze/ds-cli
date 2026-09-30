# ds-client-core

`ds-client-core` is ds-web's Tauri-independent native owner for the smallest
headless user-client contract. It validates one immutable Stable or Canary
`ds.native-client-profiles/v16` catalog entry for a closed deployment lane and
dedicated `ds-native-client` surface, signs in with Firebase
email/password, rotates a refresh credential
through an atomic injected store, and reads the signed-in account's project
directory through three closed lifecycle queries. It can also read one explicit
transformer's fixed `fields=full` snapshot for a selected project through
`POST /report` and the exact `get_transformers_data`
one-item shape.
It also reads the selected project's bounded form catalogue through exact
`POST /api/v1/project-forms` action `activate`.
It can additionally read one backend-owned settings editor for a bounded form
slug in that same selected project through the exact `settings_editor` action.
It can also capture one governed Solar city through exact
`POST /api/v1/solar`, action `desktop_snapshot`, with its project root derived
from the selected project rather than accepted from a caller.
It can finally execute one bounded aggregate over one governed form through
exact `POST /api/v1/survey/query`; project, form, field, and view authority stay
server-owned.
It can also select one bounded live spatial receipt through exact
`POST /api/v1/survey/entries/select`, with only project, form, WGS84 bbox, and
effective limit on the wire. The core verifies the backend's ordered-row digest
and mutable-mirror consistency evidence before exposing geometry bytes.
It can also read one bounded page of coalesced live rows and soft-delete
tombstones through exact `POST /api/v1/survey/entries/changes`. The inclusive
lower replication clock and opaque continuation page one immutable BigQuery
table version; this is neither Firestore snapshot/history nor hard-delete
evidence.
It can create one Survey entry through exact `POST /api/v1/entries/mutate`,
operation `create`. The selected project is injected outside the typed request;
the core validates bounded JSON/GeoJSON, canonical identity and metadata,
idempotency, the exact 201 receipt, and closed backend refusal codes. It never
encodes or addresses Firestore.
It can inspect status, preflight, and request generation for one selected
project's survey or design tiles through exact `POST /api/v1/tiles` calls. The
core exposes fixed status, preflight, generate, list, add and remove methods.
Catalogue import checks both projects at the backend and creates a destination-owned
copy; removal retires the entry and cleans owned cached storage. Catalogue rows
expose bounded metadata without signed storage URLs. Native layer reads and
canonical reordering use fixed `/api/v1/layers` actions. Reordering reads canonical
IDs and writes under one captured project/session; browser runtime IDs are refused.
It can request one compounded report deliverable, list the published compounded
archives, read the transformer lifecycle inventory, and retire or restore exact
transformers reversibly through exact `POST /report` calls on the selected
project. Those are five separate typed methods over a closed action vocabulary;
there is no generic report action, map or room dependency, destructive delete,
or Desktop bridge.

Profile v12 also binds the one-time device handoff plus inventory routes. The
core owns PKCE, the Ed25519 complete/refresh transcripts, strict result types,
protected credential serialization, and a 60–600 second memory-only Bearer
session. It never serializes or returns the access JWT as an ordinary result.
The resulting `DeviceApiAuthorization` executes the same fixed typed
project, transformer, form, Solar, and Survey operations as a refreshed
Firebase session through `AuthorizedTransport`. Each closed call carries a
provider-neutral memory bearer plus canonical UID/email and optional device ID;
it does not label a DS access JWT as Firebase or expose it to ordinary output.
`DeviceAuthContext` is the separate non-secret provider/status projection.

It intentionally provides no HTTP client, filesystem/keyring adapter, Tauri or
CLI dependency, arbitrary URL/path/method call, raw-token result, signup/reset
flow, generic project-definition/lifecycle mutation, independent mutation
engine, cache, or offline authority. A consuming host supplies:

- a privileged `Transport` implementation for the fixed network methods;
- or `AuthorizedTransport` for only the fixed authenticated domain calls;
- a private, cross-process leased and atomic `RefreshTokenStore` implementation.

Only the refresh token is durable credential material. The stored envelope also
binds it to the exact profile, Firebase UID, and canonical email. Passwords and
Firebase ID tokens are never serialized by this crate.

JWT `sub`, `aud`, `email`, `iss`, and `exp` are structural bindings on a token
obtained directly from Firebase's fixed HTTPS endpoint. This crate does not
verify the JWT signature and must not accept tokens from any other source;
ds-brain remains the cryptographic verifier and authorization authority for
every API request.

Refresh accepts Firebase's production `access_token` name and documented
`id_token` compatibility name; at least one is required and dual values must
match exactly. Numeric refresh `project_id` metadata is not the textual JWT
audience.

The catalog entry carries source revision, routing descriptor digest, exact
Firebase project/key, exact gateway origin/key, and the fixed project-list
method/path plus transformer-context, project-forms, and Solar-snapshot fixed
method/path/action projections plus the fixed Survey-query and Survey-entry
selection, changes, and entry-create method/path/operation, plus the fixed
tile method/path/action set and the fixed project-report method/path/action
set. A full
digest binds release provenance; a versioned stable audience digest contains
only deployment lane, native-client ID, and Firebase project. Profile schema,
routes, gateway host, source and public keys do not invalidate a compatible
device credential. Approved profile/catalog digests remain audit provenance,
not a refresh fence. The full packaged-catalog byte digest and
installation are a host/build-slice obligation; the crate validates each
decoded entry and exposes its deterministic digest seam. This crate has no
environment lookup or per-field override path; release tooling must generate
and install the complete catalog before a consumer is built.

The transformer read has a 64 MiB response cap and a 120-second host deadline.
Its request body is lane-neutral by contract: Standard and Fast share this
canonical persistence read. The browser's current helper adds a redundant
`lane` body key, but this `/report` action sends no processing-lane header.
It reaches the normal ds-brain device-principal authority and the
governed pipeline contract defines the canonical wire without a lane field.
Stable/Canary deployment lane identity remains profile-bound and is never treated as Standard/Fast processing
lane identity.


Native layer/style/GIS calls use one captured project and authority for each
operation. Layer reorder validates canonical IDs against a fresh map snapshot.
Style reads use the dedicated metadata-only `get_style_catalog` action: at
most 1,000 styles and 16 MiB, with no renderer sources, layers, or sidebar.
Guided style edits use that same snapshot with ds-command-kernel's planner and
publish only with apply; returned routing keys must match the planned target.
Tile catalogue receipts and GIS metadata omit signed URLs and private storage
paths.

Project GIS uploads accept an immutable host reader up to 1 GiB. The core alone
accepts the backend-issued HTTPS storage session and project-bound raw path.
The host sends bytes without DS bearer headers and without redirects. Only a
complete transfer may be registered; registered and tiles-ready are separate
receipt facts. Contract tests use mock transports and require no cloud access.

Native LV process saves require a separate typed `voltage_drop` analysis and
retain the exact hydrated `process_input` locally. The client shares
`ds-network`'s schema, status, summary, size and input/output digest validator;
configuration must equal both that input and fresh project configuration.
Only export's transient transformer nature may differ from the fenced saved
source. Neither the process input nor the full analysis is copied into
`process_metadata`. The closed `/api/v1/data` body places `voltage_drop` beside
`gdfs` in each `save_transformers_data.transformer_saves` item for one server CAS.

A verified save requires the fresh saved snapshot's network version/content
digest and ready `voltage_drop_metadata` receipt to match. The analysis SHA-256
is computed over the exact JSON value embedded in the request; the producer's
three digests remain distinct. Matching geometry alone never proves an
idempotent retry, and a missing or replaced analysis receipt never produces a
verified result. The receipt returns the verified `analysis_sha256`. Every
read and write uses the operation's explicit project and captured authority.

`Client::transformer_analysis(project, transformer, analysis_version,
content_digest, analysis_sha256, now)` returns `Result<Vec<u8>, ClientError>`.
`DeviceApiAuthorization::transformer_analysis` takes the same inputs with an
injected `AuthorizedTransport` first. Both capture authenticated authority and
an explicit project for one read; neither calculates an analysis or substitutes
local data. The version is a `u64`; both digest pins are exact lowercase
64-character SHA-256 hex strings.

The host implements the closed `transformer_analysis(TransformerAnalysisCall)`
transport method. It posts exactly these six keys to `/api/v1/data`:
`action=get_transformer_analysis`, `eds_project_id`, `transformer_name`,
`analysis_version`, `content_digest`, and `analysis_sha256`. Authentication,
identity forwarding, gateway key, native client attribution and fresh correlation
headers use the existing typed-call transport conventions. No processing lane
or mutable active-project lookup is added.

This call returns `TransformerAnalysisResponse`, retaining status, the original
entity bytes and the `Content-Type`, `X-DS-Analysis-SHA256`, and `Cache-Control`
headers. Hosts look up headers case-insensitively and reject duplicate contract
headers. `TransformerAnalysisResponse::from_reader` reads at most 32 MiB plus
one overflow sentinel and returns `TransformerAnalysisTransportError::ResponseTooLarge` on overflow;
the core also enforces the cap for injected buffered responses. The deadline is
120 seconds. Existing `TransportResponse` constructors and profile pins are
unchanged; this new operation needs its own host transport implementation.

A 200 response requires `application/json`, `no-store`, and an exact lowercase
SHA header matching the requested analysis pin. The SHA-256 of the original
bytes must also match. JSON syntax is checked through a borrowed `RawValue`;
whitespace, number spelling and fields are returned unchanged, with no
`success`/`data` envelope handling. The content digest and version stay on the
request as server-owned head fences and are not confused with the analysis SHA.
Standard 400/409 refusal envelopes retain their exact HTTP status, code and
bounded scalar details through `ClientError::service_refusal()`. Callers can
therefore distinguish missing, stale and invalid codes without guessing from
HTTP 409, retrying a moved fence or maintaining a second backend vocabulary.
