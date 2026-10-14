# Native Network process client

`crates/ds-network-client` is the native transport and result-admission owner for
the offline Network executable. It uses the existing `ds-cli-exec` supervisor
and Network's versioned contract vocabulary. It does not link the Network,
Grid engine or physics calculation implementations. Its current source closure
still includes Network's model vocabulary and Kernel's portable geometry;
captured closure qualification remains required before independent release.

Discovery reads executable metadata without starting a process. An intentional
operation captures one executable path, hashes its bounded regular file and
checks its bounded `build-info` response. The response must name the Network
protocol, native runtime, required capability, positive bounded CPU budget and
consistent source/closure pins. An absolute `DS_NETWORK_ENGINE_BIN` override
admits a development artifact explicitly and labels the result. Self-reported
source pins and a development override are not installation or signature trust;
the existing module release/admission/update owner supplies that authority.

LV processing accepts complete prepared request bytes. Historical request and
result schemas remain explicit read compatibility. A consumer pins the JSON
object ordering needed by its existing artifact contract; an incompatible
engine refuses rather than changing result bytes or running a linked fallback.
The process receives no project credentials, source URLs or publication grant.
Hosts retain acquisition, project authorization, save and artifact transport.

Each call stages private files under a caller-supplied disk scratch directory.
It verifies the exact input digest, fixed result filename/schema, ordered jobs,
terminal outcome, complete artifact byte count/digest and unchanged executable.
Missing, incompatible, malformed, incomplete or corrupt engines produce typed
refusals. Cancellation uses the existing supervisor's bounded cooperative grace
and process cleanup. A completed result after ignored cancellation is refused.
Temporary staging is removed when the call finishes.

The supervisor owns Unix process groups, Linux direct-parent death and retained
PID ownership until group cleanup, and Windows Job Object cleanup. Ordinary and
typed-stdin calls terminate on deadlines or excessive output before joining their
bounded collectors. `capture_pinned` reopens an explicit retained executable
without consulting an active pointer, environment override or PATH. The calling
application must validate the retained digest and identity before using it.
`CapturedNetworkClient` performs that admission once for a run or session. Its
LV and prepared-design calls reuse the same canonical executable and identity
without another discovery or metadata probe. A serialized `NetworkEnginePin`
retains the exact executable path, digest, worker width and identity. Resuming
admits that retained path once, even when an update has changed the active module
pointer. A missing or changed retained executable refuses; it never substitutes
the new active version. The host's available CPU count may change after restart,
but the admitted worker width and engineering identity remain pinned.

Linux arbitrary descendants that escape their group require separate containment;
the Rust engine's threads terminate with its direct process. Windows source
support does not establish Windows runtime acceptance.

Owning native client acceptance explicitly invokes the real executable against
pre-extraction byte fixtures and adversarial child fixtures. Ignored acceptance
tests are run with their required owner/fixture paths; an ordinary test command
that skips them is not execution proof. The supervisor's owning tests cover
completion, cancellation, deadlines, input blocking, output limits and reaping.

The same captured client admits Grid query, cheap manifest inspection, decoded
inspection and publication facts through their exact Network contract capabilities. Each call
supplies opaque package bytes and, when applicable, a typed prepared request.
The receipt binds request bytes for query/publication and package bytes for
manifest or decoded inspection. The client checks the fixed `query.json`,
`manifest.json`, `inspection.json` or `publication.json` artifact, complete byte
count/digest, exact captured identity, unchanged staged inputs and executable,
and the result's source pins.
It retains complete owner results without decoding the package or recalculating
engineering in the host. The observed query variant forwards newline-complete
owner progress during execution, after checking its schema and captured request
digest. A frame above 64 KiB or a mismatched digest refuses the result. The shared
supervisor bounds all retained stderr and queued observation bytes by its existing
output limit and preserves cancellation, deadlines and process-tree cleanup.

Manifest inspection must retain `decoded: false`: reading its manifest does not
validate tables or package members. Publication's `source_revision_id` remains
an owning fact; library/cache identity may hash exact package bytes rather than
the authored geometry revision. Host authentication and publication effects
remain outside the executable. Query refusals retain the exact owner code,
structured detail, remedy and invalid-input classification. Cancelled Grid
results and responses that arrive after cancellation cannot be admitted. Explicit
decoded inspection requires `grid_inspect_v1` and returns the Network-owned
library and extent summary; hosts only bound and present those facts.

The explicit `grid_boundary` acceptance runs the real schema-5 fixture through
all four operations and compares exact prior owner artifact bytes. Adversarial
executables exercise source/revision/operation mismatch, corrupt receipts and
artifacts, changed input, symlink output, absent capabilities, mismatched progress
input and cancellation.
The owning host trace admits only characterized anonymous Rust exec IPC and
counts outgoing calls separately; the computation executable retains its strict
zero-network-syscall requirement. A socket control must demonstrate that the
trace detects prohibited network attempts.

The captured-selection acceptance moves an active pointer between two fixture
executables, verifies two tasks add zero probes, verifies resume probes only the
retained executable, and proves a corrupt retained digest refuses before probing.

The durable Design workspace stores this pin with its immutable input revisions.
Preparation of an existing run and reads of completed results do not probe an
engine. Pending execution admits the retained artifact once, then shares that
selection across its bounded job workers. Missing or changed engines retain the
inputs and pending work. Historical linked-host run records remain readable;
pending records without a retained executable pin explicitly refuse rebinding.
Saved report inputs retain the exact producer analysis bytes and can be exported
without a running or installed engine. Cancellation of a child keeps that job
pending, while publication remains a separate host operation.

Grid publication preparation also consumes the captured executable. Network
returns package validity, protected-generation admission, the package source
revision and schema/engine facts. The workspace retains project authorization,
head fences and publication effects. It does not unpack the package, open an
engineering session or derive a source revision. Normal workspace dependencies
exclude Network, Grid engine/exchange/physics and Profile calculation crates;
portable wire/model vocabulary remains shared. Library adoption/detach and
protected combined publication tests explicitly run with the actual executable.
