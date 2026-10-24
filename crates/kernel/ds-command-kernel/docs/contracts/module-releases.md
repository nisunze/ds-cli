# Standalone module catalog admission

The fixed native component IDs are `solar`, `reporter` and `network`; their engine owners
are `ds-solar`, `ds-network-reporter` and `ds-network`. The shell retains its existing desktop
release namespace and is not decoded as a module. This slice introduces no
general plugin registry or installation framework.

`ds-cli/crates/shared/ds-module-contracts` (`src/module_release.rs`, re-exported
as `ds_command_kernel::module_release`) owns semantic admission of held public
metadata. It is types and pure decisions only, together with the component
catalogue and build admission (`module_engine`) and the native installation and
selection receipts (`module_install`), so the process boundary (`ds-cli-exec`)
and the engines that link it read the addon store without compiling the Kernel:

```rust
validate_module_release(component: &str, channel: &str, release: &serde_json::Value)
    -> Result<ModuleRelease, ModuleReleaseError>
```

The host supplies explicit `stable`/`canary` context and raw held metadata from
the existing public release operation. The pure owner performs no network or
filesystem reads. A visible consumer or explicit refresh acquires a catalog;
unused catalogs receive no request, and retained metadata remains available
offline. The error carries one stable `code` and a bounded rule `message`.

Anonymous page reads honor the host's offline and link facts before fetching;
an offline acquisition leaves retained metadata untouched. `isFresh` compares
complete captured/current source identities without IO. Without a current
source basis, freshness remains unknown: neither a held value nor its age
justifies claiming freshness or inventing a backend stamp/warmup request.

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
[App release governance](../../../../../../ds-brain/docs/contracts/app-release-governance.md).
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

The CLI/MCP source projection declares `core.module-stage` as a confirmed
machine write over explicit component, stable/canary channel and exact selected
asset SHA-256. It captures the packaged native release authority and delegates
acquisition and staging to this owner; no held JSON, URL, path, executable or
guard enters that effect. `core.component.selection` reads the installed-v1 or
removed-v2 record offline. An absent ordinary tree returns null; existing
linked/reparsed, wrong-type or corrupt paths refuse. This read does not certify
current executable availability.

`core.component.activate` requires a native ready receipt and caller-held
selection generation; `core.component.rollback` requires the held generation.
Both confirmed commands enter the selected lane's running Core through fixed
private owner-only routes. Core holds its existing durable SQLite job-admission
writer fence through the updater's component lock and atomic selection write.
Every owned pending job must retain a valid immutable component capture;
uncaptured legacy jobs refuse. Core has no native model editor sessions, so
caller assertions about editor dirtiness or idle status never enter admission.
The activation caller generation, retained receipt generation and protected
current generation must match under the same component lock. Rollback without
a retained previous unit refuses `module_rollback_unavailable`. No lifecycle
operation downloads, reads a release catalogue, launches an engine or requires
Desktop. Installed acceptance remains distinct from source and fixture proof.

## Browser engine unit decisions

`module_release::browser` owns the closed `browserModule` WASM wire and the
matching native decision functions. `core.module-release --runtime wasm` is
read-only inspection of this owner; native remains the default. The closed
profiles admit Network and Reporter. Reporter uses `ds.reporter-wasm/v1` and
only `render_held_svg_pdf_v1`; Solar still refuses before file access. Public
metadata inspection never installs or authorizes processing.

A browser unit contains exactly its component's JavaScript loader, WASM, and both
type files, with matching lane, immutable release folder, producer source closure,
build timestamp and engine identity. Its SHA-256 binds the complete typed
catalog with assets sorted by URL. Shape limits precede cloning; unknown keys,
mixed producers, crossed lanes, repeated/missing members and mutable URLs refuse.

The host asks `acquisition_plan` with its compiled lane, confirmed unit digest
and consent before asset transfer. It supplies actual byte hashes and sizes to
`verify_files` before executing even the loader's identity adapter. `prepare`
then admits the loaded engine identity and captured gateway/route authority.
These pure decisions perform zero acquisition, cache writes or engine calls;
they are not trusted installation grants or project authorization.

Every inspection, retained selection, installation and capture is bound to the
requested component. Network and Reporter have separate selections and locks;
activation cannot cross their units or identities. Reporter identity admits the
shared Rust SVG/PDF converter, not acquisition, composition, Chromium-based
voltage-drop printing or report publication.

`ds.browser-module-selection/v1` records active and retained previous receipts,
or an explicit removed state. Activation and rollback take the existing
`ActivationGuard`; all mutations compare the held generation before advancing
it. Removal prevents selection of an active unit and retains bytes only for
explicit rollback. Repeated removal refuses; no fallback or online refresh is
implied. A host must serialize and atomically persist the returned transition,
reverify retained bytes, and admit its current authority before use. The wire
cannot substitute caller assertions for a protected host lifecycle observation.

`capture` admits only an installed active selection against the compiled lane
and gateway, returning its exact receipt and selection generation. Missing or
removed components refuse without acquisition or fallback. The host verifies all
four retained members under the selection lock and transfers owned JS/WASM bytes
to its existing supervised worker. `loaded` re-admits the complete unit, actual
executable byte hashes and loaded identity inside that worker. A worker keeps one
unit; subsequent updates/removal affect new captures, never already captured
jobs. Cancellation terminates the assigned worker; idle incompatible instances
are reclaimed, and same-unit workers remain reusable. Capture is not project
authorization, an execution-context fence or permission to apply a late result.

The browser host, existing update/notification integration and durable cache
have matching browser installation acceptance. Worker capture is a separate
execution seam; the principal/project/edit-session/revision fence must also reach
the actual cache and apply consumers before deployed processing can be enabled.
Source decision, pool and loader checks alone do not prove governed processing
or qualify an unadmitted engine. Hosting retains its explicit unavailable
processing state until those consumers pass.

## Desktop UI WASM units

`module_release::ui` owns the pure admission and retained selection decisions
for the two closed UI component IDs `interactions` and `command-kernel`. They
are distinct from the three native engine components; no UI value is added to
`ModuleComponent`. Core retains installation and acquisition authority. This
owner performs zero network, filesystem, executable probe or native effects;
Desktop consumes Core's admitted receipt and complete bytes through its loader.

The existing `ds.module-release/v1` catalog uses `runtime: wasm`, protocol
`ds.ui-wasm/v1`, and the existing lane-bound module product IDs. It carries
exactly four existing `ModuleReleaseAsset` records: JavaScript glue, WASM,
the JavaScript type declaration and the WASM type declaration. Interactions
uses the `ds_interactions_wasm` stem; Kernel uses `ds_command_kernel`. Each
member has platform `browser`, arch `wasm32`, and kind `module-javascript`,
`module-wasm` or `module-types`. All four share one immutable bucket and path
`/modules/<component>/<lane>/wasm/<version>/<fixed-member>`, exact producer,
source closure, timestamp and build identity. The catalog unit digest includes
every catalog fact with members sorted by URL. Shape and collection limits
precede cloning; incomplete units, unknown keys and crossed facts refuse.

Every UI catalog requires a typed `build_manifest`, carrying the closed ten-field
`ds.ui-engine-build/v1` DTO that the loaded unit must return:
`schema`, `component`, `protocol`, unsigned numeric `ui_contract`, `source_sha`,
`cargo_lock_sha256`, `rustc`, `target`, `profile`, `config_sha256`. Source is
exactly 40 lowercase hex characters; lock/config hashes are 64 lowercase hex.
Only `wasm32-unknown-unknown` and `release` are admitted. Compiler text remains
exact and must have the producer's `rustc <version> (<commit> <date>)` form. The catalog producer `engines[0].manifest_sha256` is SHA-256 over compact
UTF-8 JSON with these ten keys sorted alphabetically and no trailing newline.
Its engine is `ds-interactions` or `ds-command-kernel` and release is
`<owner>@<base-version>+<source_sha>`. Loaded source must equal the catalog's
producer source. Debug/development exports are visible build facts and refuse
installation admission; none is silently filled with the installed shell's
source or compiler facts.

`inspect(component, lane, release)` admits held metadata, `verify_files` admits
all four actual member hashes and sizes before loading JavaScript, and `prepare`
copies the already admitted catalog build manifest into its installation receipt
with captured release authority. Core never executes UI WASM or JavaScript to
stage a unit. The loader later supplies the actual exported identity to
`admit_loaded`, which requires exact equality of all ten trusted manifest fields. Public metadata and these pure decisions do not mint the
native trusted acquisition grant. Existing consent/update preferences and Core
lifecycle observations remain with their existing owners.

`ds.ui-module-installation/v1` retains the complete unit, identity and authority.
`ds.ui-module-selection/v1` retains active/previous installations or an explicit
removed state, with component/lane/generation fences. `activate` and `rollback`
reuse the native `ActivationGuard`; hosts supply protected observations, serialize
and atomically persist the transition, and reverify retained bytes under their
existing lock. `capture` selects one exact retained receipt offline;
`admit_loaded` rechecks its byte set and loaded identity. Existing captures stay
on that unit while later selection changes affect new loads. These owner APIs
are not a second updater or a Tauri policy. Native Core commands, durable bytes,
loader integration and installed acceptance are separate consumer obligations.

Focused proof is `cargo test --lib module_release::ui --jobs 1`, including both
closed component fixtures, exact compiler/configuration identity binding,
unknown keys, unsupported UI ABI, lane/source/member tampering, withdrawn status,
generation races, protected lifecycle deferral and offline retained rollback.

### Explicit local UI producer staging

`core.component.stage-local` admits an already built four-member UI unit on
Linux x86_64 into the separate canary `local-dev` namespace. The operation is a
confirmed OS-user machine write. Its inputs are the absolute producer directory,
the actual `uiEngineBuildIdentity` output file, the exact source snapshot SHA and
the tested WASM SHA-256. Core never builds or evaluates these inputs. It reads
bounded ordinary files, verifies the selected WASM digest, admits the existing
typed identity and derives the complete unit digest from its captured identity
and sorted four-member byte hashes. The captured bytes are copied once into the
existing immutable store and produce the same private ready receipt and
generation-fenced activation flow.

`UiUnit::LocalBuild` serializes `local_build` instead of `release`. It declares
`ds.ui-local-build/v1`, `scope: local-dev`, component, canary channel, typed
identity and four `VerifiedFile` records. `UiAuthority::LocalBuild` records this
scope and source snapshot; it asserts no gateway acquisition, publication URL,
release date or governance status. Published unit and authority JSON retain
their existing shapes. Published acquisition cannot mint a local grant or
activate into a local store, and a local receipt cannot enter a release
selection. Local files never become production artifacts by selection.

Existing UI selection, activation, removal, rollback, capture and member commands
accept `--scope release|local-dev`, defaulting to `release`. Native engines have
no local UI scope. Local Dev explicitly selects `local-dev`; installed Canary
and production clients select `release`. The store namespaces, locks and
generations are independent, and automatic published convergence only accesses
release selections. No absent selection falls back to vendor files or another
scope. An older native UI admission implementation that lacks the local variant
must be updated as an affected shell interface; it must not reinterpret a local
unit as a published one.

Numeric `ui_contract` metadata alone does not establish compatibility. The
required protocol/schema, producer compiler/source/configuration facts,
complete byte receipt, actual loaded identity and demanded callable ABI remain
the admission controls. A self-consistent numeric addition is accepted; changing
the number without updating the producer-bound manifest still fails integrity.

Core's local producer runs the consolidated dependency checker with
`--component-only`: it checks the same declared and resolved local dependency
rules without requiring an unrelated Web checkout. CLI, Kernel, Interactions
and Shared must also remain free of a dependency on Web. The check's default
mode still includes Web; the complete application source/dependency gate and
assembled workflow acceptance remain required before application qualification.
