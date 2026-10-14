# Standalone module catalog admission

The fixed component IDs are `solar`, `reporter` and `network`; their engine owners
are `ds-solar`, `ds-network-reporter` and `ds-network`. The shell retains its existing desktop
release namespace and is not decoded as a module. This slice introduces no
general plugin registry or installation framework.

`src/module_release.rs` owns semantic admission of held public metadata:

```rust
validate_module_release(component: &str, channel: &str, release: &serde_json::Value)
    -> Result<ModuleRelease, ModuleReleaseError>
```

The host supplies explicit `stable`/`canary` context and raw held metadata from
the existing public release operation. The pure owner performs no network or
filesystem reads. A visible consumer or explicit refresh acquires a catalog;
unused catalogs receive no request, and retained metadata remains available
offline. The error carries one stable `code` and a bounded rule `message`.

The closed `ds.module-release/v1` DTO carries component, channel, lane-bound
product ID (`rw.datasolutions.module.<component>[.canary]`), version, protocol
`ds.module/v1` for Solar/Reporter or `ds.network-engine/v1` for Network, release timestamp, governance status, optional notes and one or
two platform assets. Windows/Linux x86_64 are admitted first. An asset carries
`module-executable`, its matching protocol, immutable public HTTPS storage URL,
size/digest, build timestamp, owner source revision, producer-declared source
closure and one exact `ds.engine-build/v1` identity. The source closure is
bounded at 16 repositories and is never expanded into an invented stack vector.
An unchanged carried platform asset retains its original release folder and
all its source/engine facts.

Crossed component/lane/product, unknown schema/keys, repeated targets,
unsupported platforms/protocols, partial engine identity, missing or crossed
source closure and mutable/off-origin executable URLs refuse. Collection and
string shapes are bounded before cloning/deserialization. Legacy desktop
records are not reinterpreted. `blocked`, `stopped` and `rolled_back` remain
visible governance facts; semantic admission does not make them active.

Public catalog validation is not signature verification, download permission
or installation trust. No install, activation, process launch, subscription or
polling effect exists here. The shell's existing HTTPS/digest-verified update boundary remains
independent; this installation path claims no release signature. A module installer requires its own admitted native trust and
atomic activation boundary before any UI or CLI control can offer installation.

Brain owns authenticated publication, immutable metadata and atomic pointer
advance under `app.release.manage`; its receiving wire contract is
[App release governance](../../../ds-brain/docs/contracts/app-release-governance.md).
The protected publication envelope remains distinct from the existing public
`{success,release}` envelope.

Focused executable proof is `cargo test --lib module_release --jobs 2`: carried
source preservation, closed DTO round-trip, component/lane/legacy refusals,
malformed target/provenance/URL bounds and withdrawal-status preservation, plus Network-specific protocol/executable/producer admission.

Network assets name `ds-network` on Linux and `ds-network.exe` on Windows. Its
metadata selects the existing engine protocol; it is not relabeled as the Solar
prepared-task protocol. The common module catalog/artifact envelopes stay at v1.
No Network release is inferred from this declaration: an absent public pointer
remains absent until an exact qualified producer artifact is published.

## Native installation boundary

The detached `ds-update-runtime::modules` owner reuses the existing verified
transfer from `ds-update-runtime::transfer`. `ds-cli-auth::module_updates` loads
only the exact packaged production native profile; development catalogs and
local-Go routing refuse before transfer. The host captures that profile's
lane, reviewed HTTPS gateway origin and catalog digest. The fixed declared
`domains.releases.desktop_manifest` operation supplies one bounded public
release response without redirects or a retry/polling loop. Authentication of
publication stays with Brain; installation provenance is this native HTTPS
acquisition and its exact captured profile. There is no cryptographic release
signature or new signer in this boundary.

Only that native acquisition can mint `TrustedModuleGrant`; held catalog JSON,
caller URLs, filesystem paths and arbitrary argv cannot mint it. Acquisition
requires the caller's expected asset digest, active governance status, exact
component/lane/product/native target and current protocol. The grant is neither
a deserializable IPC payload nor a public schema-validation result. Staging
uses the existing bounded resumed transfer, reopens exact bytes, executes one
fixed supervised build-info probe and admits the producer source/closure,
qualified native compiler facts and required schemas/capabilities. Solar,
Reporter and Network remain the three fixed owners.

The host stores a complete executable at the immutable per-component/lane
`versions/<sha256>/<fixed-executable>` path, and a private native staging receipt.
`selection.json` (`ds.module-selection/v1`) contains both active and previous
installation receipts. An exclusive component lock and captured generation
fence precede one atomic replacement of that record; a reader sees a complete
old or new unit. Interrupted acquisition/receipt staging does not change the
selection. Activation reopens and verifies the staged unit and retained current
unit. Explicit rollback re-verifies the retained previous unit; a damaged
former current is never advertised as a usable next rollback target. No
selection, restart recovery or rollback reads an online catalog or launches a
fresh identity probe. Version directories are retained, so captured old jobs
continue on their original paths while compatible new jobs capture the new one.

Core owns authorization, runtime state, jobs, receipts and component management;
Desktop is an optional typed command client. Server execution is a Core background
mode, not an independent product or component command namespace.
`ModuleSelectionRecord` reads the existing
installed-v1 shape and the closed removed-v2 shape from the same `selection.json`.
`ModuleStore::remove` requires the native lifecycle guard and held generation,
takes the existing exclusive component/lane lock, increments the generation and
atomically writes `ds.module-selection/v2` with exact component/channel and
`state: removed`. It stores no active or previous unit and deletes no version
directory. Already captured jobs retain their executable and integrity receipt;
new managed captures return `module_removed` before packaged/PATH fallback.
An explicit development override keeps its existing precedence. Old v1 readers
refuse the removed shape; absence is never used to represent removal.

`selection_record_managed` reads installed or removed records without creating
a writable store. Existing installed-only selection reads return `module_removed`
for removed records. Stages admitted before removal cannot activate against the
new generation. An explicit reinstall stages against the removed generation and
activates a new installed-v1 record with no previous unit. Removal, restart reads
and rollback refusal require no release authority, transfer or executable probe.
Removal may fence an absent managed selection at generation zero, also suppressing
unmanaged fallback for that component/lane.

Hosts apply their existing automatic-update preferences and call
`admit_automatic_acquisition_managed` before loading authority or acquiring an
automatic update. Existing opt-in automatic-update policy may authorize background
acquisition, installation and activation when the native lifecycle guard admits
idle activation; a new per-download confirmation is not required by this owner.
`stage_automatic_with_admission` rechecks removal under the
component lock before any transfer/probe, including grants acquired before
removal. These are owner APIs, not a new updater, licensing policy or command
namespace. Component licensing remains future planning; calculation still uses
existing project governance. Typed Core command and native preference/session
bindings remain required before product removal or automatic-update completion
can be claimed.

`ActivationGuard` contains native host observations only. Dirty held edits,
uncaptured persistent sessions and a required host restart defer activation.
The UI supplies a native receipt ID, never dirty/idle booleans. Already captured
compatible jobs retain their path and do not become an uncaptured session.
Consumers use `External::capture_for_lane` and pass their captured installed or
request lane; a corrupt managed selection refuses without falling through to
another lane or PATH. Explicit development overrides retain their own meaning.
Legacy unmanaged `capture()` remains explicit until each owning consumer is
migrated; activating a managed receipt does not prove those consumers use it.

Focused native regression proof covers all three supervised owner fixtures,
immutable old-job continuity, stage/restart/rollback, generation races,
corrupt/incompatible/interrupted/cancelled candidates, linked paths and fixed
request/probe counts. A separately selected qualified Network artifact can run
LV processing through the actual managed resolver in an isolated regression.
These proofs do not establish public publication, installed Desktop/Server
consumption, Windows runtime behavior or a native dirty/session observation
binding. Installation/update controls remain unavailable until their declared
CLI/MCP/native effects and installed acceptance are proven.

The CLI/MCP source projection declares `server.module-stage` as a confirmed
machine write over explicit component, stable/canary channel and exact selected
asset SHA-256. It captures the packaged native release authority and delegates
acquisition and staging to this owner; no held JSON, URL, path, executable or
guard enters that effect. `server.module-selection` reads retained native
active/previous provenance through `ModuleStore::selection_managed` without
constructing a writable store, probing an executable or reading release
authority. An absent ordinary tree returns null; existing linked/reparsed,
wrong-type or corrupt paths refuse. This read does not certify current executable
availability. Headless activation and rollback remain undeclared until the
owning native session observations can supply their guard safely.
