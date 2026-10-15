# Existing desktop update owner

`src/desktop_update.rs` owns closed shell/module inspection and shell download
admission. `server.update-inspect` is its read-only CLI/MCP projection and the
Desktop `ds_invoke` adapter calls the same owner over held metadata. No project
data is acquired by this operation. Solar, Reporter and Network use their existing
module manifest validator; inspection grants no module installation trust.

Installed/published identity has three distinct fields: `version`,
`buildNumber`, and `releaseDate`. The existing immutable `version-rN` release
encoding supplies the publication build number. A recorded `released_at`
supplies the release date. Neither a build timestamp, filesystem modification,
download time nor current clock can fill a missing publication receipt.
Development and legacy installations retain null fields where no receipt exists.
Network build-info projects its recorded `engineIdentity.coreVersion`; source or
dependency-closure qualification does not invent a publication build/release date.

Shell availability retains the existing per-platform source/build identity
comparison: a higher intentional version or later same-version platform build
can update; a carried asset or an installed build ahead of the pointer cannot.
The selected asset must match lane/product/platform/architecture/installer kind,
have HTTPS without embedded credentials and carry an exact bounded size/SHA-256.
Blocked, stopped and rolled-back releases are visible but cannot be acquired.
Local development inspects installed facts without public release acquisition.

The verified native transfer implementation lives in the detached
`ds-update-runtime::transfer` owner. The Desktop download adapter preserves its
existing calls and injects held connectivity admission alongside cancellation.
Both SHA-256 and CRC32C retain the existing resumed-transfer, bounded deadline,
private/no-link file and reopened-byte verification. Native Server consumers
use the same owner without importing Web or an engineering crate.

The host extends its existing verified transfer transaction with staging and
activation. Staging returns a native ready receipt and never launches an
installer or exits. Activation accepts only that receipt, reopens and verifies
the staged bytes, and uses the existing platform installer handoff. A failed
stage or activation preserves the running installation. Captured native jobs,
their executable paths and earlier versions are untouched by staging.

Automatic download is an intentional registered Desktop consumer. It reads on
consumer registration, a retained release invalidation, navigation or explicit
Refresh. Consumers coalesce one read; unused push/invalidation and ordinary
subscriptions perform zero reads. There is no periodic release poll or second
notification/transfer channel. Automatic work stops at ready. The shell still
requires the operator's restart handoff; no automatic exit or module activation
is inferred from catalog admission. Native idle-aware automatic activation is
not declared by this slice.

Executable proof: Kernel `cargo test --lib desktop_update`, the native stage
receipt/reverification fixtures, CLI `server update-inspect --file` execution,
and Desktop consumer/read-count/interaction tests. The public release operation
and publication authority remain unchanged.

The Desktop module adapter exposes the fixed native stage/cancel/activate/rollback
effects over `ds-update-runtime::modules`; it does not reinterpret held catalogs
as installation authority. Module staging uses the existing component progress
event and download notification row and ends at ready. Explicit activation and
rollback derive their guards from native admission/session observations; unknown
or uncaptured consumers refuse conservatively. Automatic module activation is
not declared. A completed explicit activation refreshes only that module's local
inventory against held metadata, with no release freshness read. Publication
version/build/release date belong only to a matching active executable receipt.

The opaque UI store keeps the same four-member, generation-fenced lifecycle on
Linux and Windows. Windows is unavailable until the native host registers the
existing protected-state SID/DACL owner. Registration creates no identity or
state. Component/lane roots are created with that owner's sole-account DACL;
retained directories and every retained file are audited before reads or updates.
Foreign-readable children, foreign ownership, reparse points and a second update
owner refuse without rewriting retained permissions. The shared base and existing
native engine installations are preserved. Native qualification must include the
actual published unit, removal/rollback/reopen, byte tampering and ACL negatives;
source registration alone is not installed acceptance. macOS UI installation
remains unavailable. WASM publication remains the Web/WASM lane on every OS.
