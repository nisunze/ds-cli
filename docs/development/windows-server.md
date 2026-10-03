# Native Windows server

The Windows server links the same kernel, Grid, Solar and compute-runtime crates
as the Linux server. Its local transport is an account-restricted Windows named
pipe. The queue, commands and project fences stay in the shared native owner.
There is no Tauri application or browser in the server build graph.

## Development

From the `ds-cli` checkout, in native Windows PowerShell:

```powershell
.\scripts\run-windows-server.ps1 -Lane canary -Plan
.\scripts\run-windows-server.ps1 -Lane canary -PrepareOnly
.\scripts\run-windows-server.ps1 -Lane canary
```

The source launcher uses the installed public profile catalog, with only its
development marker changed; Rust still validates the complete catalog. An
explicit development catalog can be passed with `-ProfileCatalog`. The launcher
never copies credentials or guesses a deployment endpoint. Run it as the Windows
account holding the native identity for the requested lane. A protected-state
or identity refusal stops startup and leaves that identity intact.

State defaults to `%LOCALAPPDATA%\ds\server-dev\state\ds\server\<lane>` and is disjoint from
installed server state. `-DevelopmentRoot` and `-StateDirectory` support another
dedicated development namespace. The native owner creates private directories;
the launcher must not precreate them with ordinary inherited permissions.

The persistent default Cargo target is the CLI checkout's `target` directory,
with six build jobs. `-TargetDirectory` or `DS_SERVER_CARGO_TARGET_DIR` selects
another persistent target. Source edits are incrementally rebuilt at the next
launch; there is no Rust watcher in this launcher yet. It never calls `cargo
clean`, deletes Go caches, builds standalone Solar/Reporter executables, or runs
work on ds-server. The linked native engines are rebuilt only when Cargo's
dependency graph requires it.

Measured on MAGESE (12 threads, 15.6 GiB RAM), 2026-10-03: the initial native
CLI/server build took 7m28s; an unchanged warm build check took 4.23s. These
are development measurements, not release packaging or full test-suite times.
Changed crates and different test feature sets can require substantial new
compilation even with a persistent target. Linux/server-offload timings were
not measured because ds-server is reserved for stabilization.

`-CliArguments` drives the same source executable without starting a host. CLI
arguments pass through unchanged: name their lane and development state explicitly
when the live command contract requires them. The source launcher sets a dedicated
`XDG_STATE_HOME` so implicit server state stays inside development too. Discover flags using the source
CLI's `capabilities` command; installed release skills do not describe a debug
binary with newer commands.

A web renderer remains a separate client. This transport does not make a
browser able to speak named pipes or replace the web Grid WASM binding. Native
Grid acceptance commands can run without a renderer. A browser-to-native Grid
binding must use the governed native commands and render their returned facts.

## Package and install

`scripts/build-windows-server.ps1` produces a ZIP and an adjacent
`ds.windows-server-artifact/v1` receipt. It builds only the native `ds.exe` and
takes exact release inputs for the two process engines, the public profile
catalog and matching CLI skills. Those inputs are prepared by their existing
owners, then verified through the extracted CLI's actual engine discovery and
`doctor`. Native Solar linked into `ds.exe` is checked separately from standalone
Solar. No source code or build script from ds-web enters compilation.

```powershell
.\scripts\build-windows-server.ps1 -Lane canary -Version 0.1.0 `
  -OutputDirectory 'G:\Project\Working\Windows server' `
  -ProfileCatalog 'G:\Project\Working\Release inputs\catalog.json' `
  -SkillsBundle 'G:\Project\Working\Release inputs\ds-cli-skills' `
  -SolarExecutable 'G:\Project\Working\Release inputs\ds-solar.exe' `
  -ReporterExecutable 'G:\Project\Working\Release inputs\ds-report.exe'
```

All five native repositories must be clean, on pushed `run`, and the CLI's
kernel/client-core pins must match the linked kernel. The resolved Cargo graph
is checked for web/Tauri inputs. Sources are checked again after compilation.
The extracted archive must reproduce every payload digest and exact release
identity before the builder exposes either output. Existing exact-version
outputs are never overwritten; persistent compiler caches are retained.

The installer takes an expected SHA-256 obtained from the reviewed build receipt
or public release authority. It performs bounded extraction, rejects links and
unsafe/duplicate Windows paths, checks every manifest member, and executes only
the verified archive's native identity probes before activation:

```powershell
.\scripts\install-windows-server.ps1 -Lane canary `
  -Artifact 'G:\Project\Working\Windows server\DS GridDesign Canary Server_0.1.0_x64.zip' `
  -Sha256 '<exact reviewed artifact SHA-256>'
```

Stable and Canary install into separate per-user `DS GridDesign [Canary] Server`
folders and coexist with Desktop. Each exact artifact gets its own version
directory; `current.json` activates it atomically after verification. A running
older server continues using its own executable until explicitly restarted.
The installer prints the installed `start-windows-server.ps1` launch path and
does not start a server, change PATH, copy credentials, or remove earlier
versions/caches. Launch under the signed-in user, rather than as SYSTEM, because
native protected state belongs to that Windows account.

The installed starter defaults to `%LOCALAPPDATA%\ds\server\<lane>`. The Rust
owner enforces private state and a single host per state directory. A Windows
process query also lets the shared sync runner reclaim only provably dead
workers of this install; unobservable processes retain their leases.

## Public release integration

Building/installing an exact artifact is independent of publishing a release.
The shared release authority still needs to register `headless-zip` as the Windows
headless asset family, validate its native artifact receipt, preserve Desktop and
Linux Server assets, and expose a `windows-server` download target. That change
belongs to the release/gateway owner. These scripts do not publish, call Cloud
Build or install a candidate as a release implicitly.

## Validation

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\test-windows-server-package.ps1
cargo test -p ds-cli-server --lib
```

The transport tests exercise concurrent real Windows pipe requests, request
deadlines/body bounds, account checks, private state, address squatting and clean
restart. The package tests exercise traversal, Windows device/alternate-stream
names, links, duplicate paths, exact-byte integrity and lane isolation. A public
download and an authenticated host/job run remain additional acceptance gates.
