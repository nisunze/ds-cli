# Release notes

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
