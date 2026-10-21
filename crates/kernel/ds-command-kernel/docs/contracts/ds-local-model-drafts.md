# Local model draft persistence

`ds_command_kernel::local_models` owns the pure catalogue decision;
`ds_layer_store::local_models` owns native filesystem persistence. Neither has
a Cargo dependency on `ds-network` or its engine crates. Engine wire types
are represented by host-neutral serde types, with `head_revision` as a string.
This boundary stores recovery content and grants no package validation,
export, project, or publication authority.

## Catalogue and kernel APIs

The existing `ds.local-models/v1` schema is retained. `LocalModel::draft` is
`Option<LocalDraft>`, defaulting to `None` and omitted when absent. Old
catalogues reopen without migration. A draft never changes the row's package
digest, byte count, revision, authored head, or retained `.dsgrid` bytes.

Public types in `ds_command_kernel::local_models`:

- `PackageIdentity { sha256: String, bytes: u64, model_revision: u64,
  head_revision: Option<String> }`.
- `DraftBinding { local_model_id: String, base_artifact_sha256: String }`.
- `DraftCheckpointPin { binding: DraftBinding, head_revision: String,
  sequence: u64, content_digest: String }`, serde-compatible with the engine
  pin introduced at `ds-network/run` commit `e3893f0`.
- `LocalDraft { pin: DraftCheckpointPin, bytes: u64 }`.

`LocalModel::package_identity(&self) -> PackageIdentity` captures the retained
base. `Catalogue::model(&self, scope: &Scope, id: &str) ->
Result<&LocalModel, Refusal>` returns a row with the account/lane fence.

`evaluate(catalogue, scope, Op::StageDraft { id, expected_base,
expected_draft, draft })` compares the full `PackageIdentity` and previous
`Option<DraftCheckpointPin>` before advancing the pin. The proposed binding
must name that row and base digest, and its sequence must be nonzero,
unexhausted, and greater than the previous sequence. The engine remains the
owner of journal replay and engineering validation.

Normal `Op::Revise` refuses `local_model_draft_pending` while a draft exists,
including revisions without `expected_sha256`. This slice has no implicit
discard or draft-to-package conversion operation. `Op::Forget` can remove the
row while leaving unreferenced draft content.

`draft_content_digest(bytes: &[u8]) -> String` computes lowercase SHA-256 over
`ds.grid.local-draft.v1`, NUL, the exact byte count as u64 little-endian, then
the exact bytes. `MAX_DRAFT_BYTES` is 512 MiB. Metadata validation and integrity
checking do not attest that the bytes are a replayable engine checkpoint.

## Native host APIs and CLI integration

Public APIs in `ds_layer_store::local_models`:

```rust
pub fn stage_draft_at(
    root: &Path,
    scope: &Scope,
    id: &str,
    expected_base: &PackageIdentity,
    expected_draft: Option<&DraftCheckpointPin>,
    pin: &DraftCheckpointPin,
    bytes: &[u8],
) -> Result<Outcome, StoreError>;

pub fn read_draft_at(
    root: &Path,
    scope: &Scope,
    id: &str,
) -> Result<Option<StoredDraft>, StoreError>;

pub struct StoredDraft {
    pub base: PackageIdentity,
    pub draft: LocalDraft,
    pub bytes: Vec<u8>,
}
```

For capture, the CLI retains the base identity and prior draft pin it opened,
calls `GridSession::draft_checkpoint` with that base binding, converts the
engine pin through its serde wire representation, and passes the encoded bytes
and independent expectations to `stage_draft_at`. The returned catalogue is
the committed state. Do not refresh expectations and retry a stale writer
without reconciling the session.

For recovery, `read_draft_at` returns `None` when there is no catalogue pin.
Otherwise, convert **the returned catalogue pin** to the engine pin and pass
it with `StoredDraft::bytes` to `load_draft_checkpoint`. Do not derive a pin
from the payload. The retained base still owns package resources and bindings;
the engine still owns replay and newly computed validation. Recovery does not
authorize an export or publication.

Both APIs use the existing `catalogue.lock`: exclusive for staging, shared
for recovery. Staging re-evaluates CAS under the lock and verifies the retained
base file's exact size and raw SHA-256 before writing. Draft bytes are bounded
and checked against the engine digest. Their only path is
`<scope-dir>/drafts/<lowercase-content-digest>.draft` (`draft_path(dir, digest)`
is public). New bytes are synced, atomically renamed, and their directory is
synced before atomically persisting the catalogue. On Unix the containing
directory entry is synced too. Existing content is verified and never
overwritten; malformed paths, symlinks, and corrupt content refuse.

Reads verify base bytes and the independently pinned draft's size and digest
before returning, with bounded file reads. No directory scan, orphan lookup,
or previous-checkpoint fallback exists. Temporary files and durable content
left before a catalogue commit are harmless orphans. Forget intentionally
leaves draft content rather than risking shared-content cleanup.

`execute_at` rejects `Op::StageDraft`: callers must use `stage_draft_at` with
the exact bytes. Refusal codes are `local_model_draft_conflict` for stale
expectations/binding/base integrity, `local_model_draft_corrupt` for draft
integrity/path failures, and `local_model_draft_pending` for normal revise.
Malformed metadata retains `local_model_request_invalid`; filesystem errors
retain `local_model_store_unavailable`.
