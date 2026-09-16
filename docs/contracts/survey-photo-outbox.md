# Survey photo outbox

`survey_photo_outbox` owns the durable row lifecycle, exposed natively through
`apply` and in WASM through `surveyPhotoOutboxStep`. Requests and replies carry
`ds.survey-photo-outbox/v1`. Each bounded request contains one row, a host clock,
and a closed event. Settled replies explicitly contain `row: null`.

The kernel decides admission, concurrency (two active uploads), retry deadlines
and the ten-attempt budget, failed or blocked state, recovery, cancellation,
manual retry and cleanup. It reuses `response_policy` and accepts typed
`transfer::Cause` observations. Recovery does not replenish an exhausted budget.
Scope, a stash generation and a dispatch lease fence late results. Older rows
require explicit provenance binding; the current account alone is not evidence.

Hosts persist bytes and row changes atomically, enforce cross-tab execution
ownership, report account/project identity, and execute timers and network IO.
An outbox attempt uses the host's existing `transfer` machine for each ordered
target. Its browser driver yields at the first retry decision so the durable
outbox owns the retry budget; it does not run another retry loop inside each
attempt. The gateway verifies the exact
stored generation and SHA-256 before success is reported. The host removes the
queue row and unpins bytes only when the kernel permits it. Cached bytes remain
available; cancellation never deletes the image cache.

When a capture/import produced a thumbnail, the optional `companion` is a
canonical `{original stem}_thunder.jpeg` JPEG target. The kernel validates that
relationship and emits one ordered upload plan: original first, then thumbnail.
The host atomically pins both local blobs, verifies every target before it
reports success, and unpins both only after the row is settled. A thumbnail is
therefore never independently retried or allowed to survive a failed original.

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

A gateway inspection advertising `revision_protocol: 1` enables an immutable
local `revision` (equal to stash `generation`) and a bounded `supersedes` chain
of at most 32 prior local revisions. `PrepareSupersede` checks scope, gateway
support and state before hosts use a pending original. `Supersede` retains both
remote baselines, emits an abort for the old lease, and coalesces the new pair
for 500 ms. The browser atomically replaces both pinned blobs and the row,
broadcasts cancellation to another tab, and never rereads mutable cache bytes
mid-upload. If the old row settled while the next turn encoded, the same
kernel transition can be admitted as a fresh row retaining its predecessor.

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
