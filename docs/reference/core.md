# Core runtime and components

Core is the independently installed `ds` native runtime. Desktop is an optional
client of that owner. Run its background mode with `ds core serve --lane stable`
or `ds core serve --lane canary` under the intended operating-system account.
The process remains in the foreground until stopped. Discover `core.serve` for
its complete flags and admission contract; see [background runtime](server.md)
for job, project and synchronization controls.

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

Core background execution and component removal currently admit Linux only.
Windows and macOS discovery must expose `server_platform_unsupported` before
credentials, retained host state, input files or execution are accessed. This
stable refusal identifier is retained; it does not expose a Server command or
product alias. Windows qualification requires protected SID/DACL state and a
mutually authenticated private local transport, then actual installed Core and
component execution. Producer target mapping and Linux tests are not that proof.

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
