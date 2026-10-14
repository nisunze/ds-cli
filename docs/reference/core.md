# Core components

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
