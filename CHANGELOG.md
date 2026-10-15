# Release notes

## Unreleased: independently installed UI units

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
