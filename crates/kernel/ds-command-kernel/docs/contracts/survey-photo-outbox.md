# Survey photo outbox

`survey_photo_outbox` owns the durable row lifecycle through `apply`: one row,
a clock and a closed event (`admit`, `recover`, `drive`, `result`,
`manual_retry`). The Kernel's `media_outbox` store applies it to the rows it
holds (`docs/contracts/durable-store.md`); a reply without a row settles it.
There is no WASM door: hosts queue bytes through the store; the survey queue
(`gateway_exchange::survey_queue`) schedules each upload as a step of its
entry, leases it on its size lane and carries it.

The kernel decides admission, retry deadlines
and the ten-attempt budget, failed or blocked state, recovery, manual retry and
cleanup. It reuses `response_policy` and accepts typed `transfer::Cause`
observations. Recovery does not replenish an exhausted budget. Scope, a stash
generation and a dispatch lease fence late results. A row without a scope stays
blocked; the current account alone is not evidence.

The store persists bytes and row changes in one SQLite transaction. The drain
(`gateway_exchange::survey_media`) claims due rows, sends each ordered target
through `transfer::browser` and settles what it observed; the transfer yields at
its first retry decision, so this outbox owns the retry budget. A page drains
one account at a time under a Web Lock and wakes at the reply's `wake_at`. The
gateway verifies the exact stored generation and SHA-256 before success is
reported. A settled upload's bytes move into the byte cache; nothing here
deletes cached bytes.

When a capture/import produced a thumbnail, the optional `companion` is a
canonical `{original stem}_thunder.jpeg` JPEG target. The kernel validates that
relationship and emits one ordered upload plan: original first, then thumbnail.
The store holds both blobs with the row, the drain verifies every target before
it reports success, and both leave the queue only when the row settles. A
thumbnail is therefore never independently retried or allowed to survive a
failed original.

`survey_photo_media` is the separate byte API for capture/import. It admits
JPEG and PNG source bytes up to 50 MiB and 50 MP, applies available JPEG EXIF
orientation plus one net clockwise turn, caps the normalized original at 2048
px, and creates a white-composited JPEG-quality-85 thumbnail at 320 px. The
WASM API accepts and returns typed byte arrays, never JSON/base64 photo data.
`surveyPhotoMediaPlan` decides processing versus exact-byte preservation and
refuses rotations of unsupported images/attachments. Browser workers execute
that decision. `surveyPhotoNormalizeRotation` and `surveyPhotoAccumulateRotation`
own draft quarter-turn arithmetic; `surveyPhotoThumbnailPath` owns companion
names in display bindings as well as durable uploads. There is no TypeScript
format/rotation/naming fallback.

The optional `expected_generation` is an opaque positive decimal storage
precondition preserved with a queued replacement. Its authorization and storage
comparison remain in ds-brain. Photo rotation uses the Rust byte API in a browser worker: encode the whole net turn once, then persist those
exact bytes and the inspected generation. Retry resends the same encoded image.

A gateway inspection advertising `revision_protocol: 1` gives the queued row an
immutable `revision` (its `generation`) and holds it 500 ms before the first
attempt. A row may carry a bounded `supersedes` chain of at most 32 prior
revisions (rows imported from the retired browser outbox do), which the gateway
may rebase over. A photo still waiting to upload refuses another rotation at
`media_outbox` admission until it settles.

A cancelled transfer can already have committed. The gateway may rebase only
over a named predecessor belonging to the same authenticated account, object,
and original generation baseline. Its newly issued PUT still has an exact
storage-generation condition. A revision-aware storage PUT 412 asks the outbox
to retry through gateway START; foreign edits remain terminal START conflicts
and digest/length failures remain terminal. Old gateways keep their existing
conditional-upload behavior and do not allow pending supersession.

Server transport is documented in ds-brain's
`docs/contracts/survey-media-upload.md`. It preserves the canonical survey
project/form/entry/file path. The new endpoint and frontend must be released
together; old server rotation requests receive an explicit retirement refusal.

## Survey media address

That canonical path is one of three. `survey_media_address` owns the whole
grammar, natively through `parse`/`wire` and in WASM through
`surveyMediaAddress`; requests carry `ds.survey-media-address/v1` with the op
`parse {path}` or `wire {path}`. Three writers produced three stored layouts:
`web` (`{project}/forms/{form}/{entry}/{file}`, every browser upload),
`projects` (the same under a `projects/` root) and `field_app`
(`projects/{p}/forms/{f}/{e}/{field}/{file}`, what the field app's
`entry_engine.dart` uploads, one folder per media field). ds-web's client
parser admitted only the first and ds-brain's `surveyMediaPath` composed only
the first, so a photo taken on a phone could be shown but never inspected,
rotated or replaced.

`parse` answers `{layout, project, form, entry, field?, file, object_path,
thumbnail_object_path}`; `field` is present for `field_app` only.
`object_path` is the reference itself, normalised only of a leading slash —
the stored object's name is evidence, never something the kernel rewrites —
and `thumbnail_object_path` is `thumbnail_path_for(object_path)`, the outbox's
one companion rule, never re-derived. Segment (`^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`,
never containing `..`) and filename (`^[A-Za-z0-9_.-]{1,255}$`, never `.` or
`..`) rules are transcribed from ds-brain, so an admitted reference is one the
gateway can compose; a reference whose `_thunder.jpeg` companion could not be a
bounded object name is refused with it. The refusal vocabulary is exactly one
sentence — `Media path must identify a project, form, entry and file`, the
browser's existing words — so no host string moves. A bucket-qualified
spelling is a host cache key, not an address: hosts strip the bucket first.

`wire` answers the body for ds-brain's `survey_media` actions:
`{project_id, form_slug, entry_id, file_name}` plus `layout` and, for
`field_app`, `media_field`. The web layout omits both, so today's requests stay
byte-identical and an unreleased gateway keeps serving them. ds-brain
recomposes the path from `layout` rather than trusting a client-sent string,
and authorization is unchanged: it is keyed by `project_id`, so choosing a
layout cannot reach another project. Hosts marshal only; no TypeScript, Go or
Dart re-derives any of this. `tests/fixtures/survey-media-address-cases.json`
is the proof, replayed by the kernel and byte-identically by ds-web through the
vendored WASM.
