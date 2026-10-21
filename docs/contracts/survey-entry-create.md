# Survey entry creation

`survey_entry_create` owns the closed Survey create grammar, legacy-row identity
normalization, geometry validation and canonical admitted payload. Its `direct`
module constructs the fixed Firestore `documents:commit` request and interprets
the commit receipt for both native and browser consumers. No Brain create endpoint
exists; update/delete/restore continue through Brain.

Every platform makes the same create: Web, Desktop, Android and Core. The Kernel
survey queue (`gateway_exchange::survey_queue`, on the shared outbox store) drains
a queued create with this one direct commit whatever platform feature the build
carries, and Core's `survey entries create` makes it directly. No build routes a
create through Brain's mutate API or waits for one to; that API takes only
update, delete and restore, and refuses a create as invalid input.

The direct authority consists of the verified Firebase project, UID, canonical
email and deployment lane. Native preparation derives it from the protected principal and packaged public
Firebase project; it needs no transport credential. Desktop IO uses the published
same-owner Firebase session after UID/lane/audience checks. Browser preparation
uses that current verified Firebase session. A DS trusted-device access token is
not a Firebase credential: standalone native create refuses it before IO.
The host cannot select another origin, database, URL, method or credential kind.
Existing Firestore rules remain the authority for direct writes; this protocol
does not claim Brain-equivalent membership or project lifecycle enforcement.

The one write addresses the admitted project, optional context ancestry, form
and document ID. It uses `currentDocument.exists:false`; it never overwrites an
existing document. The original idempotency key is retained in metadata. The
shared encoder writes the dedicated `metadata.firestore_updated_at` server
transform and preserves the device `created_at` timestamp. Untouched creates do
not receive `metadata.updated_at`. Geometry remains admitted GeoJSON and is
serialized only at the Firestore storage boundary.

Only HTTP 200 with exactly one bounded write result and one valid server
timestamp transform is a commit acknowledgement. Conflict, malformed response,
transport ambiguity and missing transform do not settle the row. An existing
document is never treated as a successful replay merely because its ID matches;
this slice implements no authoritative replay lookup. The durable identity and
key remain available for later resolution. Commit does not establish mirror
convergence; that still requires the existing BigQuery-backed read.

`survey_entry_create::evaluate` keeps schema
`ds.survey-entry-create/v1` (the survey capture plan calls it; no WASM door). `plan` and `normalize` admit opaque capture bytes;
`direct_plan` additionally requires `authority` and returns the fixed `direct`
effect under `ds.survey-direct-create/v1`. `direct_receipt` consumes the same
admitted request and authority plus status/response. Native APIs call the same
functions. The old gateway receipt grammar is not a transport contract.

Core queue preparation reads exact retained SQLite bytes, binding account,
project, full lifecycle fence, full-row SHA256 and direct-plan SHA256. Receipt
verification reconstructs the direct plan from the same retained row and current
protected identity; every fence and digest must agree. Hosts do not replace the
retained payload. Only the existing fenced result transition changes queue state.
No additional retry loop, Firestore listing, rendering fallback or implicit save
is introduced.
