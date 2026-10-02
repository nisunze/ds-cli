# Report inventory and refresh

`report::inventory` projects `ds.report.inventory/v1` through the same native
and browser WASM `reportCommand` function. It performs no IO, authenticates no
principal and authorizes no generation, reuse, publication or deletion. Native
hosts supply facts after fencing every read to the captured owner, project,
transformer and variant. Foreign scoped facts refuse the complete projection.

Committed sets carry immutable batch, client publication/run, producing engine,
input fingerprint and output locator/digest/size/presentation identities. Their
order comes from the native committed catalog, current first. The existing
`current_output_indices` rule replaces only matching canonical outputs; retained
outputs stay in their original producing set. The projector never sorts dates
to select a generation. A single exactly matching native queue row identifies
the active generation; multiple candidates report ambiguity. Each output keeps
its own generating run and timestamp. A partial iteration never claims that
its latest run produced the entire surviving inventory.

Generation timestamps and cloud publication observations are different facts.
Legacy cloud status/URLs remain a separate observation. Their old timestamp,
success token or file presence cannot replace a local generation or establish
its publication. Cloud absence cannot delete committed local bytes. No receipt
or byte directory is a second publication queue; native sync state remains the
one queue.

A local output is `published` only when its committed batch/publication/input
and complete output pins match the native published row, the validated current
head declares identical output bytes and format/presentation, and that output's
origin names the exact acknowledged work, same input and producing engine/build.
The native upload receipt must bind its work ID to this client publication;
an identity-only historical receipt cannot supply that binding. Missing binding
remains unknown. Older heads without
per-output origins remain `acknowledgement_unverified`. Head declarations prove
record metadata at observation time, not a fresh download/rehash of cloud bytes.
Cloud-only outputs carry their own head/origin and never acquire a local run or
generation timestamp. A changed current input is explicitly `changed`/`stale`;
publication does not grant engineering or report freshness.

Read facts are `ready`, `held` (previous verified same-scope facts retained after
a refresh error), `unavailable`, or `not_observed`. Held/unavailable facts retain
their bounded error. Only all-ready reads establish complete observed inventory;
required missing outputs, mismatched native pins and changed source remain
explicit. A host must retain the previous projection during a transient read,
display held local files immediately, and refresh in the background with an
owner/project generation guard. The native IO binding is
`ds_report_artifacts::inventory::read_snapshot`, called by
`StoreHost::report_inventory_snapshot` under its existing fence. It hashes
selected current bytes, checks immutable commit/output pins, and performs no
recovery, adoption, reclaim or remote call. A missing/corrupt current output
refuses the fresh read rather than resurrecting an older shadowed output.
Historical generation times and ACK work IDs remain null: directory mtime and
sync state transition times are not generation or acknowledgement authority.
The Desktop read and background presentation are consumers of this snapshot.

Snapshots admit at most 4,096 committed sets, 4,096 native sync facts, 32 outputs
per set and 32 required IDs. Known renderer metadata and output presentation are
validated by their existing owners. Public `report.plan --action inventory`
accepts this supplied JSON snapshot without authority, with its existing 4 MiB
request-file bound. It does not claim to read the installed native store.
