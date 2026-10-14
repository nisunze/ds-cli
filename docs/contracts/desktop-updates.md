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
