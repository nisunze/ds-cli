# Captured native Profile sessions

The native `ds-network-client` Profile adapter consumes Network's
[`network-profile-process-contract.md`](../../../ds-network/docs/contracts/network-profile-process-contract.md).
It is transport and admission, with no linked Grid engineering, package replay,
scene calculation or analysis implementation.

A `CapturedNetworkClient` opens one `ProfileSession` from exact
`NativeProfileOpenV1` bytes in a caller-supplied disk scratch directory. One
immutable executable path, digest, identity and worker budget owns that model
for the entire session. Frames do not resolve an active module pointer or run
another metadata probe. An update cannot redirect a held session. A restarted
host admits `ProfileCheckpoint.engine` with `CapturedNetworkClient::from_pin`
and opens `ProfileCheckpoint.bytes`; missing or changed retained bytes refuse
instead of selecting a newer module.

The client binds sequence zero to the exact opening bytes, model and native
history fence. Every later request has a monotonically increasing sequence and
an explicit history fence; `request_at` preserves a supplied stale fence so the
Network owner can refuse it. Replies must match the captured identity, model,
sequence and request digest. Ordinary owner refusals consume the sequence and
preserve the held history. They retain their exact code and detail and leave
the session usable. Malformed framing, unexpected history, changed identity,
bad artifacts, transport errors, cancellation and deadlines abort the process;
there is no retry, linked fallback or implicit recalculation.

Successful replies carry a fixed `profile-NNNNNNNNNN.json` artifact with exact
schema, size and digest. The private output directory must contain only that
ordinary file. The client reads the bounded exact bytes, validates JSON without
reserializing it, and removes the consumed staging file. Native cumulative byte,
file and request bounds remain enforced independently. Checkpoint and Close
additionally bind the new full open document to the acknowledged model and
history. `last_checkpoint` exposes the exact bytes, hash, sequence and engine
pin; the existing application storage owner must persist this recovery record.
An in-memory acknowledged checkpoint is not proof of durable persistence, Save,
publication or synchronization.

The existing `ds-cli-exec` process-tree supervisor owns the retained transport.
Its `network_profile_session` constructs only the fixed declared subcommand and
path/worker arguments. A bounded writer keeps a blocked stdin pipe off the
calling thread. The reader retains at most one queued 64 KiB frame; stderr is
bounded by the existing 4 MiB output limit. A request deadline covers both
writing and receiving. Cancellation sends the existing Network control frame,
allows a 300 ms cooperative grace, then terminates and reaps the owned process
tree before joining pipe collectors. Close keeps stdin open until its reply is
verified, then sends EOF and waits at most five seconds. Drop and integrity
failures promptly terminate/reap; private staging is removed with the handle.
Linux direct-parent death and Windows Job Object behavior reuse the same
supervisor as other Network calls. Windows source support still requires actual
Windows runtime acceptance; arbitrary Linux descendants escaping their owned
process group remain outside the admitted Rust engine contract.

The explicit `profile_boundary` acceptance runs a real Network executable with
its emitted owning seed. It verifies scene/default-analysis bytes, cached reads
without computation, stale-fence refusal with continued use, exact full
checkpoint restart, protected asset and redo history preservation, and retained
engine selection. Adversarial child fixtures verify wrong pins/receipts,
symlink and unsolicited artifacts, malformed/oversized frames, blocked stdin,
hung tasks, cancellation and descendant cleanup. Ignored acceptance tests must
be invoked with their explicit owner/fixture paths; a test run that skips them
is not execution proof. Owning host traces distinguish characterized anonymous
Rust exec IPC from outgoing calls and include a socket negative control.
