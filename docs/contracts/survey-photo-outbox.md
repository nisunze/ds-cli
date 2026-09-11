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
An outbox attempt uses the existing `transfer` machine. Its browser driver yields
at the first retry decision so the durable outbox owns the retry budget; it does
not run another retry loop inside each attempt. The gateway verifies the exact
stored generation and SHA-256 before success is reported. The host removes the
queue row and unpins bytes only when the kernel permits it. Cached bytes remain
available; cancellation never deletes the image cache.

The optional `expected_generation` is an opaque positive decimal storage
precondition preserved with a queued replacement. Its authorization and storage
comparison remain in ds-brain. Photo rotation is frontend image processing by
explicit owner instruction: encode the whole net turn once, then persist those
exact bytes and the inspected generation. Retry resends the same encoded image.

Server transport is documented in ds-brain's
`docs/contracts/survey-media-upload.md`. It preserves the canonical survey
project/form/entry/file path. The new endpoint and frontend must be released
together; old server rotation requests receive an explicit retirement refusal.
