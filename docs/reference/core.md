# Core runtime and components

Core is the independently installed `ds` native runtime. Desktop is an optional
client of that owner. Run its background mode with `ds core serve --lane stable`
or `ds core serve --lane canary` under the intended operating-system account.
The process remains in the foreground until stopped. Discover `core.serve` for
its complete flags and admission contract; see [background runtime](server.md)
for job, project and synchronization controls.

Core and Desktop publish independently. A release listing identifies **DS Core
(CLI / Kernel)** as an installable runtime with its own version, release number,
publication date and artifact identity. Desktop has its own shell identity;
Interactions belongs to the Web/Desktop shell, never the native Core closure.
Network, Solar and Reporter are independently admitted components.

There is no installation-wide release pin. Every installed artifact retains its
own exact source, dependency/protocol identity and SHA-256. Native compatibility
currently uses the declared owner protocol and pinned engine provenance;
metadata does not yet declare a Core semantic-version compatibility range.
A carried artifact keeps its own immutable release identity. Missing per-artifact
publication dates remain unknown rather than inheriting a newer catalog date.

The public background command is `core.serve`, contract 2. Native automation uses
`--target core` and `--core-state-dir` where the command declares those inputs.
Retired product commands, targets and build-lane spellings have no aliases;
existing installations require explicit removal. Internal stored schema and
refusal identifiers retain their declared read compatibility.

`core.result` optionally observes the existing durable job with `--wait <seconds>`
(1..=600). Without that input its immediate behavior, result bytes and receipt
remain unchanged. Waiting writes the identical completed bytes to the absent
`--out` and adds the durable `job` to the existing `out`, `byte_count`, `sha256`
receipt. Its context, input digest and exact engine pin remain the ones admitted.
Timeout returns retryable `result_pending` with the current job and `wait_seconds`;
failed and cancelled jobs return `result_failed` and `result_cancelled` with the
job. These refusals create no output file. Detach, timeout, window closure and
Core shutdown stop observation only: none cancels an admitted job. Reattach to
the same job after restart. No automatic status polling or resubmission occurs.

Observers register before the initial durable read, wake after committed
completion/failure/cancellation, and recheck local identity and job visibility
before returning. The host admits at most 32 concurrent observers separately
from its request door; `capacity_exhausted` names scope `result_waiters` when
full. Sleeping observers hold no ordinary request permit. The client transport
deadline covers the requested wait plus 60 seconds for bounded result transfer.

Discover the typed contracts with `ds capabilities core.component.selection`
and `ds capabilities core.component.remove`. CLI and MCP project these same
commands. Machine component management requires local per-user ownership and
explicit confirmation; project calculation keeps its existing governance.

Selection reads installed-v1 or removed-v2 retained records offline. Capture
the returned generation before confirming removal. An absent selection has
generation zero; removing it fences unmanaged fallback for the component/lane.
Removal returns the committed removed-v2 receipt, increments the generation
under the owner's exclusive lock and retains immutable version directories for
already captured jobs. A stale stage cannot reactivate against that generation.

Removal uses the selected lane's already running default per-user Core
background runtime over its existing private owner-only local transport. An
absent or older runtime refuses `component_runtime_unavailable`; it never
counts as idle. Core checks
all owned pending jobs across projects under the same SQLite writer transaction
that admits native engine captures, and holds that fence through selection
removal. Uncaptured legacy jobs refuse `component_uncaptured_jobs` and retain
their stored inputs. No caller idle or dirty assertions enter admission.

This is Core's durable job lifecycle: it admits no native model editor sessions.
Removal does not perform activation or change authored model state. Existing
activation dirty-edit and restart admission remains separate. The command uses
no release authority, remote network acquisition, executable probe, process
launch or new calculation licensing; Desktop is an optional typed client.

Core background execution and component removal admit Linux and native Windows.
Unadmitted targets, including macOS, expose `server_platform_unsupported` before
credentials, retained host state, input files or execution are accessed. This
stable refusal identifier is retained; it does not expose a Server command or
product alias. Windows uses protected SID/DACL state and a mutually verified private named
pipe, retaining the existing credential and process-supervision owners. Native
transport, credential and supervision controls are required alongside actual
Core and component execution. Public Windows packaging remains unavailable
until its publication contract is admitted.

Native component staging/selection separately admit Linux and Windows x86_64
metadata targets. Unsupported targets refuse `module_platform_unsupported`
before authority acquisition or store creation. Read-only release metadata
inspection remains platform-neutral. Unadmitted engine WASM targets stay
explicitly unavailable in their producer; Core is a native application runtime.

`core.identity` reads the lane's existing protected Core installation identity
without creating it or restoring credentials. `appVersion` identifies Core;
the optional Desktop shell reports its own version separately. A missing
identity requires authenticating and starting Core once, never a shell UUID.
`core.auth.status` observes both retained providers offline without refresh or
writes. They must agree on UID, lane and credential audience. It exposes no
credential material and reports `project: null`: operations still name and
authorize their own explicit project.

Activate an already verified ready receipt through `core.component.activate`
with its exact `--receipt-id` and the generation read from
`core.component.selection`. Roll back through `core.component.rollback` at the
held generation. Both require explicit confirmation and the selected lane's
running Core. The shared durable job capture fence remains held through the
updater's component lock and atomic selection write. A stale caller generation
or ready receipt refuses; no previous unit refuses `module_rollback_unavailable`.
These operations verify retained bytes without release lookup, download,
executable probe or Desktop. They return the committed installed-v1 selection;
a staging receipt alone remains ready and is not activation.

Native Rust host consumers use `transport::own_account() -> Option<Account>`,
`Peer { account: Account }` and `App.os_account: Option<Account>`. These replace
`own_uid() -> Option<u32>`, `Peer.uid: u32` and `App.os_uid: Option<u32>`.
Unix constructors use `Account::Unix(uid)`; Windows obtains its complete binary
SID through the existing protected-state owner. A SID is never shortened to a
numeric UID. Host construction and router fixtures must update together; these
implementation interfaces do not form a supported client SDK. CLI/MCP remains
the client surface. See [release notes](../../CHANGELOG.md) for the source update.

## Installation Manager Desktop updates

The existing Windows `-Action Update -Component Desktop` and Linux
`update desktop <lane> --yes` actions select the Desktop shell only. On Windows,
Core installation now receives an explicit Install intention instead of inheriting
the caller's global Update action. An existing Core is reused only through its
SID-owned installation receipt, exact executable SHA and native `core.serve`
contract-2 capability owner. Direct Windows Core Update still refuses
`core_update_unavailable`; this change does not add a Core replacement owner.

For required Interactions and UI Kernel, both platforms retain an installed unit
only after `core component capture --runtime wasm` verifies all four immutable
members and build identity under the existing native updater lock. Its receipt,
whole-unit SHA, component, lane and generation must match the selection. A newer
public UI release does not force an independent shell update to replace that
unit. Missing or explicitly removed UI requirements use the normal public
admission, verified stage and generation-fenced installation path. Corrupt,
incompatible, crossed or refused captures remain errors, never missing-component
fallbacks. Explicit UI Update retains its own public-admission/update behavior.

Old Windows behavior could replace the shell and then report a partial result
because the dependency step incorrectly requested Core Update. Existing manager
callers require a newly digest-verified manager package to receive this fix;
installed Core/components and data are not migrated or reset. Public package and
Hosting publication are separate acceptance obligations from source regressions.

Linux continues to use its existing qualified Core DEB installer with replacement
disabled for Desktop actions; only explicit `update core` selects its qualified
APT replacement path. That owner verifies the public package/receipt and exact
executable. A future public Core differing from the installed binary still refuses
`core_update_required` during a Desktop action: the installed Linux package does
not yet retain the expected executable SHA receipt required for independent
reuse. OS MD5 metadata or an unanchored hash/version does not remove that fence.
This limitation is explicit; the current unchanged admitted Linux Core remains
usable without replacement.

## Installation Manager native component compatibility

Windows component Status, Install, Update, Remove and Rollback omit the optional
`--runtime native` argument when calling Core. Native is already the default on
current Core and older native-only contracts; those older executables reject
`--runtime` as `unknown_flag`. Interactions and UI Kernel continue to send explicit
`--runtime wasm`, so an older Core still refuses unsupported UI installation.

Consumers need the newly digest-verified manager package. Core, component
selections, receipt/digest admission, generation fences and local data need no
migration or replacement. A staging or compatibility refusal remains an error
and never advances activation.

## UI component installation before sign-in

`core.module-release`, `core.module-stage`, `core.component.selection`,
`core.component.activate`, `core.component.remove` and `core.component.rollback`
use contract **2**. The added `--runtime native|wasm` defaults to `native`.
Their existing native engine requests and protected background Core lifecycle
remain unchanged. `interactions` and `command-kernel` require explicit
`--runtime wasm`; native UI requests refuse before effects. Installation of
Network/Solar/Reporter WASM through these commands remains unavailable.
Read-only Network browser metadata inspection keeps its existing behavior.

UI installation is a local OS-user Core operation, available before Firebase
or sign-in. It requires the production native lane profile and a grant minted
by one fixed HTTPS gateway release read. Held catalog JSON only provides
inspection and the digest to confirm: it cannot authorize installation.
All four members, owner source closure and the required ten-field trusted
compiler/configuration manifest are admitted before any transfer. Core never
evaluates the JavaScript loader or UI WASM. Interactions remains absent from
the native implementation closure. UI installation and retained reads
currently admit Linux x86_64 only. Windows refuses
`ui_component_platform_unavailable` until its private DACL store adapter is
qualified; native Windows behavior is unchanged.

UI metadata inspection returns `UiUnit { release, unit_sha256, total_bytes }`.
Pass this whole-unit digest as `core.module-stage --sha256`. A successful stage
returns `ds.ui-module-stage/v1` with `state: ready`, `receiptId`, component,
channel, version, `sha256` and `expectedGeneration`. The receipt is a private
Core record, never a caller-supplied catalog, URL or filesystem path. Staging
changes no active selection. Confirm activation using the receipt ID and exact
generation. The same private component lock and atomic selection replacement
store `ds.ui-module-selection/v1`, with installed active/previous receipts or
an explicit removed state. Removal retains the previous unit for offline
rollback. Existing captures keep their immutable version bytes. These UI
operations need no running job host and hold no compute database transaction;
they grant no project, calculation or publication authority.

The Tauri loader uses two typed read-only commands, with explicit runtime wasm:

- `core.component.capture` verifies all four retained members under the updater
  lock and returns `{ generation, receipt }`. The receipt's
  `unit.unit_sha256` pins every subsequent read.
- `core.component.member` accepts that `--unit-sha256`, a component-fixed
  `--filename`, unsigned `--offset`, and `--limit` from 1 through 1048576
  (default 1048576). It reads the owner-retained installation receipt,
  re-admits its complete identity and re-verifies the requested complete member.
  Its chunk comes from the same verified handle. The result carries schema
  `ds.ui-module-member/v1`, unit digest, filename, offset, byte count, complete
  member size/SHA-256, `body_base64`, and `more` for the remaining suffix.

These reads create no store and perform no online freshness check. No private
path, database, socket, arbitrary URL or caller-held receipt enters the loader
contract. The loader must collect all four exact members, verify their hashes
and admit the actual exported identity before using the unit. Native source
and isolated fixture tests do not establish UI publication or installed
Desktop acceptance.

## Structurally damaged cloud cache

`ds core serve --lane <stable|canary>` uses the same protected native account,
installation and canonical state directory across restarts. A Core release with
the structural-cache recovery owner checks SQLite after acquiring its existing
single-host lock and verifying that identity. Confirmed page/tree or SQLite-format
corruption retains `store.sqlite` and its fixed sidecars under a unique
`store.sqlite.corrupt-<nonce>` prefix with a digest receipt, then allows the usual
owner to create fresh canonical queue/history metadata. No old rows are salvaged.

This replaces the previous unconditional startup refusal for that damage only.
It preserves credentials, connection/installation identity, installed components,
project files and input/output bytes outside the database. A constraint error
(such as `artifacts.account` NOT NULL), unsupported schema, permission/owner/auth
failure or unsuccessful quarantine still refuses; none is a request to clear all
state. Existing clients require the independently released compatible Core
artifact for this behavior; updating Desktop alone does not update Core.

### Retained native Grid Profile

`core.profile-session` contract 1 forwards one bounded closed request through
the existing protected Core transport. Open binds exact `.dsgrid` bytes/SHA-256
and verifies the owning package manifest, member digests and library pins. Core
captures a managed same-lane Network process; subsequent state, snapshot, view, scene,
checkpoint and close requests bind its opaque handle, model and full authored
history to the native principal/lane/deployment/install/project context. Scene
uses the existing native RuntimeScene DTO; unsupported actions refuse, and
cached observation never requests engineering calculation. `view` prepares the existing
Profile display policy around the admitted native scene and returns its scene JSON,
display state and command view; it never reports an analysis run. Request files
are read through the opened no-link native file owner, with Windows SID/DACL
validation retained. No arbitrary RPC,
argv, paths, alternate credentials or browser engine is exposed.

Previous Desktop package opening required the unavailable browser `openDsgrid`
binding. Its replacement is this independently qualified Core command plus the
closed Desktop adapter; older installed Core explicitly refuses
`core_incompatible` until updated. The former `dsgrid.profile.open` paired UI
command remains a request to the actual Desktop, not a synonym for this owner.
Shell remains 0.2.0 and engine/Core source versions stay independent. The first
vertical lifecycle/scene interface does not admit unfinished Profile edits,
publication or printing. Core shutdown and explicit close reap workers; idle
sessions are retired on the next Profile request after fifteen minutes. At most
eight sessions and a thirty-second native operation deadline are admitted.

## Deliberate account cache retirement

`ds core cache retire --lane canary --dry-run` inspects a stopped Core after
an intentional native account change. Apply with the returned exact `--digest`
and canonical `--yes`. The plan binds the resolved protected state directory,
lane, previous/current account fences and exact fixed cache-member bytes. The
existing native owner lock refuses maintenance while Core is running.

Old behavior: `core.serve` refused `multi_principal_unsupported` after an account
change, with no canonical same-path fresh-cache transition. New behavior adds
this deliberate command; ordinary startup still refuses another account's
connection or rows. Desktop and CLI callers must use a qualified Core containing
the command, inspect the plan and explicitly authorize it. Older clients keep
refusing; there is no automatic reset or import of old account history.

Retirement retains `store.sqlite`, its fixed SQLite sidecars and `connection.json`
under one digest-derived archive prefix, with a protected receipt and completion
marker. Atomic staged evidence and no-replace native moves preserve exact bytes.
A pending journal prevents startup from creating a new connection or database;
after interruption, the same command previews and continues only the same exact
plan under the captured current account. A move failure attempts restoration of
the complete old epoch and retains pending evidence if restoration cannot finish.

This changes only the canonical queue/history cache epoch. Native credentials,
installation identity, components and authored files remain with their existing
owners. The command neither stops nor starts Core. SQLite structural-corruption
recovery remains separate and cannot reset healthy schema, permission or owner
errors. Windows source support uses the existing SID/DACL owner; source checks
alone do not qualify an installed Windows release.


Profile callers with process output bounds use optional `--out <absolute-new-file>`
to receive `ds.core.profile-artifact/v1`. Core writes the exact bounded 72 MiB
receipt through the existing atomic create-new result owner and returns only
path, byte count, SHA-256 and the full Profile metadata without payload. Existing
files, links, invalid parents and oversized results refuse; Windows retains the
protected SID/DACL parent and opened-file checks. Omission preserves the direct
CLI result. Native Desktop requires this capability before session creation,
reads only its own fixed private output path, validates every scope/digest fence
and cleans its stage. Old Core must be independently updated; the shared 4 MiB
process stdout limit and unsupported operations remain unchanged.

Output preflight runs before a stateful Profile action. If later output I/O
fails, `core_profile_output_failed` reports the completed action and exact
session scope; a newly opened session is closed through its existing owner when
still authorized. Existing sessions are preserved and cleanup failures remain
explicit, so callers must inspect completion before retrying.

#### Retained worker presentation reads

`core.profile-session` contract 1 admits these additional closed readonly
requests, each with exact `session_id`, `model_id` and `expected_history`:

- `feature_codes` has no caller projection arguments.
- `engineering_attributes` requires `table: "structures" | "sections"` only.
- Existing `gis_projection` keeps `label_fields`: at most 64 ordered existing
  `StructureLabelField` strings, preserving order and duplicate fields. It admits
  no caller CRS; the private worker request receives the retained manifest CRS.

All three require the exact captured worker's `profile_read_projections_v1`
capability before request staging. An expired eligible original also checks its
retained engine identity and the newly captured current engine before launching
or adopting any worker. Engine source/version remains independently released;
compatibility is the declared capability plus immutable captured executable,
protocol, source/dependency identities and exact scope/history, not equality to
Core's compile-time Network source pin.

Older Core parsers refuse the two new actions. Previous Core GIS read requested
a checkpoint and opened a second in-process Grid session for projection; the
new action forwards the same existing projection to its already initialized
managed worker. Old Network producers without the capability refuse
`core_profile_native_refused` with `native_code: network_engine_incompatible`
before new request/staging; install a qualified compatible same-lane Network and
explicitly reopen the exact original when its worker-affine identity changes.
No unsupported operation, generic RPC, browser fallback, second session,
engineering calculation, edit, publication or coordinate-system substitution is
introduced. Native reply model/history/engine and artifact digest admission stay
unchanged; Core additionally verifies GIS revision and original declared CRS.

Desktop Map/Table/GIS consumers must use these closed typed actions and preserve
account/project/lane/source/history fences. Unsupported table/label fields,
excessive controls, foreign handles and stale history refuse. Core and compatible
Network artifacts must be qualified independently before claiming live behavior;
Shell version remains 0.2.0 and is not a universal component pin.

### Readonly Profile original retention and intentional rebind

`core.profile-session` contract 1 keeps the same closed action/request fields.
Previously, idle retirement discarded the only source and Close refused the
expired handle. Open now retains at most eight exact <=32 MiB original packages
in the existing protected Core SQLite Store, fenced by its complete owner scope,
model, declared CRS, package digest, complete initial history and managed engine.
Fifteen-minute worker retirement remains unchanged. On an intentional request,
an absent worker may be recreated under the same opaque handle only from that
verified unchanged initial source; no source bytes remain in steady-state RAM.
Known-source expired Close removes its row without launching a worker. Unknown
handles, incompatible/corrupt sources, changed engine/history and invalidated
original-only eligibility refuse; future authored edits cannot be reset to the
original. No queue, publication, browser model record or credentials are changed.

The receipt adds `rehydrated: boolean`; existing native Value/structural clients
accept it. Consumers that reuse an already-open cached scene must intentionally
read State and, when rebound, restore their derived View through the existing
owner. Older Core retains its unavailable refusal and cannot promise rebind.
An original-only rebind does not recover a checkpoint, undo history, edited head
or calculated result. A qualified successor Core is required for live behavior;
independent Shell, Network and other component versions are not globally pinned.

### Intentional Profile style demand

The existing contract-1 View `style_document` now uses JSON `null` to demand the
fixed authored `ds_grid/profile` document from the same project's existing native
`style_catalog` owner. Previously an absent ambient Desktop document became `{}`
and failed the unchanged physical 17-pen admission. A present document, including
`{}` or malformed pens, still reaches that admission unchanged and is never
silently replaced. Older Core does not implement this null demand and refuses;
Desktop consumers must require a qualified successor Core for this behavior.

Core binds the catalogue owner's captured native UID/lane/audience (including
an A-to-B-to-A ambient transition), immediate running-connection owner fence
before/after null demand, returned project/lane and late native completion
before adopting the exact complete document. Its <=64 KiB capture belongs only
to the existing live Held session, independent of immutable SQLite originals;
concurrent intentional Views coalesce under that owner and repeated same-owner
Views make no catalogue fetch. Missing/oversized documents or changed native
scope refuse with `core_profile_styles_unavailable`; authored physical errors
retain their existing refusal. No default style, credential path, polling,
publication, second store or new command is introduced. Explicit supplied
successor documents continue to be admitted as supplied and clear an older
null-demand capture only after successful View; malformed failures preserve the
prior capture. Closing or rehydrating
a worker drops its captured style, and the next null demand observes anew.

### Retained Profile Properties

The closed `properties` control takes the captured `session_id`, `model_id`,
complete `expected_history`, and one `entity_id`: 1..128 UTF-8 bytes, without
control characters or edge whitespace. Core admits the same canonical native
EntityId before source lookup/rebind. This read requires the captured worker's
`profile_properties_v1` capability, distinct from `profile_read_projections_v1`,
before request or expired-source candidate launch. Older managed engines refuse
`core_profile_native_refused` with `network_engine_incompatible`; explicitly
install a compatible same-lane engine and reopen the exact original. There is no
browser fallback or second Grid session.

The existing WASM `profilePropertiesJson` owner now serves the same retained
worker's facts and engineering-attribute evidence. Its complete schema-1
ModelPropertySheet preserves fields, groups, editor metadata and authored values.
Core verifies exact entity, revision and schema after the native reply and late
authority fence, before receipt adoption. Missing entities retain
`profile_entity_not_found`. Editability metadata does not admit editing: the
Core interface remains readonly, preserving package/history/CRS/assets and
independent engine identity. This source migration is not an artifact or GUI
acceptance claim.
