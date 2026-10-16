## Unreleased: Core-owned Solar workspace selection

Native Desktop previously retained Solar workspace selection in IndexedDB and
could reuse an opaque handle from another Core context. The existing
`solar.application` adds closed `workspace_selection_read`,
`workspace_selection_set` and explicit `workspace_selection_default` operations.
Core retains only validated handle/directory/origin/generation metadata in its
existing protected SQLite Store, under native owner/lane/project. Generation
comparison prevents late custom/default intents replacing another selection;
null means create-only. Missing or invalid selected receipts refuse and preserve
the row, with no default-on-refusal, reapproval or ongoing IndexedDB compatibility
reader. Old browser preferences are not authority. Existing approved Core custom
workspaces remain selectable; inputs, pending work, results and directories are
not deleted. Matching Core and native Shell/client updates are required; older
unknown-operation refusals stay explicit. The independent Solar engine and its
calculation/provenance remain unchanged; no protocol bump or global pin is added.

## Unreleased: exact-lane native module inventory

`ds core modules [--component ...]` now also accepts optional
`--lane stable|canary` under contract 1. Existing omission behavior is retained.
Desktop development previously inherited Core's compiled `local` inventory and
reported managed Canary engines missing. Updated native adapters discover the
optional input and inspect their captured Core lane. A Core without the input
refuses `core_incompatible` before inventory, requiring an independent Core
update. No installed-path compatibility branch is retained.
Managed receipts, source/protocol/digest checks and explicit development
provenance are unchanged; no install, data migration, global pin or engine fallback.

## Unreleased: retained native Profile Properties

Desktop Properties previously refused `profilePropertiesJson` at the retained
native boundary. Closed `core.profile-session` action `properties` now forwards
one canonical entity ID to the same managed Network worker and unchanged
Properties owner. Exact history, retained asset evidence, entity/revision/schema
and late account/lane/project/engine fences precede receipt adoption. The new
`profile_properties_v1` capability is required before request or expired-source
launch; older engines, including ones supporting table/GIS reads, explicitly
refuse until independently updated and the original is reopened. Field editability
metadata grants no authored mutation. No fallback, second engine or global pin.

# Release notes

## Unreleased: retained Network worker presentation reads

Desktop's feature-code/engineering-attribute table projections previously had no
admitted installed-Core operation. Contract-1 `core.profile-session` adds closed
`feature_codes` and `engineering_attributes` (`structures`/`sections` only), and
existing `gis_projection` now forwards typed reads to the same retained managed
Network worker. Core no longer reopens a checkpoint into a second Grid session
for GIS. Caller model/history/labels and retained original CRS remain exact.
All three demand captured `profile_read_projections_v1` before staging/request;
old producers refuse explicitly, with no embedded/browser fallback. Consumers
must update Core and install a qualified compatible Network independently, then
explicitly reopen when the immutable worker identity changes. Unknown actions,
tables, labels, foreign scope and stale history remain refusals; no generic RPC,
new identity/session, authored mutation or universal source/version pin appears.

## Unreleased: Store preparation preserves SQLite POSIX locks

Old Unix Store preparation opened and closed a raw descriptor for an existing
database, canceling the process's SQLite locks and allowing WAL/SHM detachment
beneath retained workers. The required committed Store owner now uses guarded
create-new only for an absent file and metadata/chmod for existing files;
readonly opens share the fresh-creation guard. SQLite remains the existing inode's
sole descriptor owner. Data/sidecars remain ordinary, single-link and 0600 with
same-owner admission; new parents are 0700. Existing group/other-writable parents
now refuse instead of being silently accepted. Affected native hosts must update
Core/Store and explicitly protect their intended owner directory (remove
`go+w`) or use the existing canonical protected owner; shared/project directories
and queues are never automatically chmodded, moved or deleted. Preserved raw
sidecar bytes are diagnostic evidence, not a recovered/checkpointed queue claim.
No schema, credential, global version pin or updater is added.

## Unreleased: Core-owned Profile style demand

Old Desktop Views sent `{}` when their ambient authored Profile style was absent,
so valid model opens failed missing physical pens. Existing `core.profile-session`
View now accepts `style_document: null` as intentional demand for the fixed
`ds_grid/profile` document through the same native authenticated style catalogue.
Complete authored metadata is retained within the existing live session only
after unchanged 17-pen validation and late UID/lane/audience/project/owner fences;
repeat intents reuse it. Present malformed documents remain refusals and are never
replaced. Consumers must send null for absence and update Core independently; old
Core refuses, missing/oversized/changed-scope demands refuse explicitly. No default
pens, global pins, new database, command or updater are introduced.

## Unreleased: bounded Core Profile original rebind

Old retained Profile workers lost their source after idle retirement; normal
read/Close could not recover the expired handle. The same contract-1 request
actions now retain a bounded readonly original in Core's existing SQLite Store
and may recreate only its exact initial worker on intentional use. Known expired
Close removes that source without reopening. Receipts add `rehydrated`; Desktop
consumers must check State on explicit already-open intent and restore derived
View after rebind. Older Core still refuses; corrupt/incompatible source, changed
complete history/CRS/model/engine or invalidated authored-history eligibility must
refuse without original substitution. This is not checkpoint/edit recovery or a
new queue/database/updater. Qualify and update Core independently; Shell stays
0.2.0 and other component pins/versions remain independent.


## Unreleased: explicit Core lane for paired Map preflight

Old lane-less Map commands always probed Stable credentials even when the
operator selected `DS_CORE_LANE=canary`, and `map design open` relabeled every
`auth_context_mismatch` as `project_mismatch`. Updated operator CLI preflight
uses the explicit Core lane when no command lane is declared, retaining the
legacy Stable default and declared command lane precedence. Scoped commands
refuse invalid selected lanes; Desktop enumeration remains available. Local window identity, UID/audience/project and session fences
remain unchanged. Open preserves the owning authentication refusal and remedy.
CLI/MCP consumers must distinguish authentication from actual project mismatch
and update the operator CLI; older clients retain the refusal. No background
Core protocol/update, credential migration or project switch is introduced.


## Unreleased: deliberate stopped-Core account cache transition

Old `core.serve` refused an account change with `multi_principal_unsupported` and
had no canonical same-path cache transition. New `core cache retire --dry-run`
previews a resolved-path/account/lane/member-digest-bound plan; explicit
`--digest` plus `--yes` retains the old connection, SQLite cache and fixed
sidecars as one protected epoch. A pending journal fences startup and supports
explicit same-plan continuation; move failures attempt exact rollback.

Affected Core/Desktop/CLI operators need a qualified Core advertising the new
command. Ordinary startup and older clients retain the ownership refusal; old
rows are never adopted or silently imported. Installation identity, credentials,
component selections and authored files remain unchanged. No new state root,
background owner or automatic account switch is introduced.


## Unreleased: explicit Local development pairing

Old native CLI routing refused an explicitly targeted Local development window
with a matching Canary/Stable account because host and credential lanes were
compared as one field. New typed `route_paired` captures the native credential
lane separately and admits only `--target desktop:<exact observed local id>`
with the same UID and credential audience. The window's Local identity and
session/window/project fences remain unchanged. Automatic Local selection and
installed Stable/Canary crossings still refuse. Consumers need the updated CLI;
older CLI keeps refusing, with no credential or cache migration required.


## Unreleased: lossless held transformer preview through Core

Old Desktop transformer preview could not cross the installed-Core boundary
without replacing its held receipt/style/room/context, so it explicitly refused.
New `report.project.export --held-preview <json-file>` adds the bounded closed
`ds.report-held-preview/v1` variant to existing command contract 2. Core admits
the exact caller-held inputs under its own matching UID/project/lane/audience,
uses its managed Reporter and the existing shared retained SVG artifact owner,
and preserves completed output on late context refusal. It never refreshes
report rows, publishes, opens a queue or restores a Desktop engine.

Desktop/CLI consumers need a qualified Core artifact advertising `held-preview`
for this variant; older Core remains explicitly incompatible and held inputs
remain intact. Existing export modes and independently installed components
are unchanged. The required fresh absolute `out-dir` is private scratch in this
mode; optional held `preview-out` remains an explicit fresh verified copy.


## Unreleased: captured-layout Core target discovery

Old discovery text for `report.layout.render` advertised `--target server`,
which the existing parser already refused as `unknown_target`. New discovery
names `--target core`, matching the current native-account owner; paired
`desktop` and exact `desktop:instance` targets are unchanged. CLI/MCP consumers
must use explicit `core` to override an ambient `DS_TARGET` when requesting a
native-account preview. Command contract 1 and execution behavior are unchanged;
this corrects discovery and does not restore Server/headless aliases. Existing
installed Core already accepts `core`, so no Core update qualifies the Desktop
adapter merely for this correction.


## Unreleased: independent Desktop manager updates

Old behavior: Windows Desktop Update inherited Core Update and could replace
the shell before refusing `core_update_unavailable`; required UI installation
could also select a newer public unit during an unrelated shell update.
New behavior: Windows reuses its receipt/SHA-admitted compatible Core, and both
platforms reuse required UI units only after complete native capture verification.
New public UI releases do not force shell updates to replace an installed unit;
missing or removed requirements use the existing verified Install owner.

Manager callers need a newly verified manager package to receive this fix.
Direct Windows Core Update still refuses `core_update_unavailable`. Linux keeps
its existing qualified DEB owner and explicit future different-Core receipt gap.
No data/component reset, second updater or shared component pin is introduced.

## Unreleased: explicit native lane for Desktop project navigation

Old interface: `desktop list`, `desktop project list` and `desktop project switch`
had no lane input, so dispatch observed Stable native credentials and could
correctly refuse a Canary window even on a Canary Core installation. New
interface adds the existing `--lane stable|canary` input to those commands;
the default remains Stable and command/output contracts remain unchanged.

Canary CLI/MCP callers must use `--lane canary` after installing a qualified
CLI/Core release containing this change. Older clients refuse that flag as
`unknown_flag`. No account/profile is substituted and no window is retargeted:
principal, lane, audience, instance and project fences still refuse mismatches
before an effect. Credential-independent `desktop status` is unchanged.


## Unreleased: Core structural cloud-cache recovery

Old behavior: `core.serve` refused startup when its canonical SQLite cache was
structurally corrupt, including cases initially surfaced as a failed schema
migration. New behavior: after its existing native single-owner lock, protected
connection owner and installation identity checks, Core positively diagnoses
structural corruption, retains the database/fixed sidecars with digest evidence,
and recreates only that canonical cache through the existing store owner.

Affected consumers are independently installed Core and its Desktop/CLI clients.
They keep `core.serve` contract 2 and their existing commands; install a qualified
Core artifact containing this change to receive recovery. Old queue/history
metadata is not imported or assigned to another account. Credentials, identity,
component selections, project files and input/output bytes remain preserved.
Healthy NOT NULL/CHECK or migration/schema failures, unsupported versions,
permissions, owner/auth changes and failed quarantine continue to refuse.
Desktop remains 0.2.0; no shared installation version or component pin is added.


## Unreleased: Windows manager native-only Core compatibility

Before: Windows manager component actions always passed `--runtime native`, which
older installed native-only Core contracts rejected with `unknown_flag` before
any component effect. Now: native Network, Solar and Reporter actions use Core's
existing native default; Interactions and UI Kernel retain explicit
`--runtime wasm`. Update to the newly verified manager package; no Core upgrade,
component/data migration or installation-wide pin is required. A refused stage
never activates a component. Unsupported UI or publication targets remain refusals.

## Unreleased: DS Installation Manager

`scripts/installation-manager/ds-install-manager.ps1` adds the Windows
`Setup`, `Status`, `Install`, `Update`, `Remove` and `Rollback` operator entry
with explicit component, lane and mutation consent. Its native UI selects
Desktop by default; Core is a read-only requirement, and selecting Desktop
also selects its independent Interactions and UI Kernel requirements. Core
already contains the native Kernel; UI Kernel is a separate visual artifact.
Linux uses the sibling Bash entry and the same Core/component owners.

Distribution consumers must use the package's exact fixed member receipt and
source closure. Windows Core acquisition uses the independent `component=core`,
`runtime=native` publication, admitting its ZIP/receipt pair and exact immutable
producing folder before the existing Core installer. No Desktop package is
accepted as Core and no shell version pins Core or an engine. Existing Core
replacement remains explicitly unavailable until the qualified update owner
is connected; it is never reported as a successful update without mutation.

Automatic legacy removal applies strictly to Desktop versions below 0.1.7,
using its existing data-preserving uninstaller. Current Desktop, Core, addons
and user data remain retained. Partial installation reports the completed
phases and exact blocked component; it does not claim workspace readiness. A completed
Desktop phase records the verified installer/source and installed executable
digests; retries reuse only that exact phase and continue with missing components.
Changed publication or executable bytes return to the existing installer owner.
Windows platform discovery uses the native Windows API and supports Windows
PowerShell 5.1 without requiring RuntimeInformation.OSArchitecture reflection.

## Unreleased: independently installed UI units

Windows UI audits now inspect the fixed updater lock through the existing
SID/DACL owner with metadata sharing compatible with its held writer. Credential
reads keep their original sharing fence. This fixes self-refusal during native
staging/capture while retaining foreign ACL, hardlink/reparse and single-owner
refusals. No user data or retained ACL is rewritten. UI store tests use a private
current-user known folder because managed sandbox TEMP is foreign-writable.


Component metadata, staging, selection, activation, removal and rollback move
to command contract 2, adding explicit `--runtime wasm` for `interactions` and
`command-kernel`. The `native` default and existing native engine lifecycle
are retained. UI metadata now returns the complete unit plus its digest;
staging confirms that digest, rather than one file hash. Update typed consumers
to select the new contract and inspect the runtime-specific receipt shape.

Core owns local OS-user UI installation before sign-in, using its packaged
production profile, trusted gateway acquisition and existing verified updater.
It never executes UI code. New typed capture/member commands provide retained
receipts and bounded verified chunks to the loader. UI installation currently
admits Linux x86_64; Windows UI ownership remains explicitly unavailable until
the private DACL adapter is qualified. This source change claims no published
UI unit or installed Desktop loader acceptance.

## Unreleased: Windows native transport

Native host Rust consumers must replace `transport::own_uid() -> Option<u32>`
with `transport::own_account() -> Option<Account>`, `Peer.uid: u32` with
`Peer.account: Account`, and `App.os_uid` with `App.os_account`. Unix constructors
wrap their UID in `Account::Unix`; Windows uses the complete SID returned by the
protected-state owner. Host construction, router fixtures and private-transport
consumers must update together. There is no numeric SID compatibility cast.
These native implementation interfaces are not a supported client SDK.

Typed CLI/MCP commands, `core.serve` contract 2 and the connection record v2 are
unchanged. Windows hosting now uses the protected native named-pipe adapter;
unsupported targets refuse before credential or state access. Windows public
publication remains unavailable until its own artifact admission.
Legacy TCP/bearer connections remain refused and retired Server/headless command
aliases are not restored. Desktop remains an optional client of independent Core.

Windows protected state now also refuses direct foreign-SID write grants and
foreign child-delete grants on parent directories, beyond the existing
Everyone/Authenticated Users checks. The existing `native_state_unsafe` refusal
requires repairing the owner-only ACL and retrying. Credential bytes, identities
and queued data are preserved; no automatic ACL or data migration is performed.

Core's Windows runtime also audits retained descendants under its owner lock
before opening the queue or publishing its private pipe. Broader ACLs, links or
an inventory beyond 100,000 entries, depth 64 or 16 MiB of path bytes refuse with
`server_refused`; retained files remain unchanged. Repair retained permissions
and retry. Making only the parent directory private is insufficient on Windows.

Windows Core connections carry the same result-observer shutdown signal as Unix.
Core shutdown detaches observation before connection drain; it never cancels the
durable job. Native host constructors use the updated account API on both hosts.

### Native retained Profile command

Adds `core.profile-session` contract 1: verified exact package opening and
state/snapshot/scene/checkpoint/close live with Core's managed Network process,
fenced to native principal/lane/deployment/install/project/model/history. The
Desktop consumer replaces its missing browser `openDsgrid` dependency through
a closed adapter; independently installed old Core must report
`core_incompatible` until a qualified update provides this command. Existing
paired `dsgrid.profile.open` semantics remain unchanged. Unsupported actions,
crossed identity/history and altered package digests refuse; no generic RPC,
embedded Desktop Grid engine, silent package migration or full editing/printing
acceptance is introduced. Shell remains 0.2.0.

### Native Profile display projection

The closed `core.profile-session` request now admits `view`, preparing the
existing Profile display controls around the managed Network scene. Desktop
Profile opening uses this native view instead of the retired embedded Grid
transport; the response preserves full authored history and never claims an
analysis run. Old Core remains explicitly incompatible until independently
qualified and updated; editing, analysis and other unsupported actions refuse.
Request reads use the opened no-link native file owner, retaining Windows
SID/DACL checks. Idle retirement is opportunistic on the next authorized
Profile request, with explicit close and Core shutdown reaping processes.


### Native Profile artifact handoff

Large Profile open/view/checkpoint results previously exceeded Desktop's
4 MiB process discovery output limit. Optional `core.profile-session --out`
now writes an exact bounded file using the existing create-new Core result
owner and returns a small path/size/SHA/full-scope receipt. Desktop requires
that flag's capability before opening a session, verifies the fixed private
file and keeps all native account/history/source fences. Older Core refuses
`core_incompatible` before effects and requires its independent qualified
update. Direct CLI output remains available; global stdout limits, authored
CRS, credentials and independent component versions are unchanged.

Output paths are admitted before effects. Late file I/O failure reports the
completed Profile action and scoped cleanup receipt; only a newly opened session
is closed when authorized. Existing sessions are preserved instead of silently
repeating or discarding an unknown completed action.

- Retained Profile GIS reads: the former unsupported `gisProjectionJson` Desktop
  call now maps to `core.profile-session` contract 1 action `gis_projection`.
  Profile/map consumers forward ordered label fields and exact retained
  model/history. Existing checkpoint/seed admission and the shared native/WASM
  projection retain source CRS and labels. Older Core versions explicitly
  refuse the action and require a qualified independent Core update; arbitrary
  operations, foreign scope and stale history remain refused. No new dependency
  edge, native engine protocol or Desktop engine prerequisite is introduced.

## Unreleased: complete retained Survey file handoff

Old `survey.entries.read --out` returned at most 5,000 rows. The existing
contract-2 command adds `--complete-file --out` for all original Features in an
admitted Core-held view, with a scope/count/size/SHA-256 receipt and the existing
64 MiB canonical-source bound plus its exact GeoJSON header. Default limits
and ordinary output remain unchanged. Complete consumers, including Desktop
Survey loading and retained print preparation, must discover the new flag and
verify the receipt; older Core is refused rather than silently truncating.
`--complete-file` without `--out`, or with `--include-deleted`, refuses before
authority or output creation. This adds no new storage or command owner.

- Retained Linux Profile workers survive the return or retirement of their
  caller's background thread. The existing native process owner keeps a private
  creator thread until exact child/tree termination and reap, retaining Linux
  parent-death/process-group fences and the existing Windows Job behavior.
  Constructor failure, abandoned handoff, Close, cancellation and Drop remain
  bounded. Update the independently qualified Core artifact for this fix;
  no matching Desktop/Network version or engine rebuild is required.

Retained Profile readonly selection now forwards fixed Pick/Range requests to
the same managed Network worker under `profile_selection_v1`, sharing the WASM
geometry/ranking/range owner and exact scene axis/full history. Previously the
native facade refused these Properties-selection consumers. Existing source,
asset, account/project/lane, executable and late-owner fences remain; older
workers/parsers explicitly refuse and require the matching independent owner
update. This adds no editing, calculation, save or publication authority.
