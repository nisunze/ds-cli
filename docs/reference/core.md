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
