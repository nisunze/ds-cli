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
The captured-selection acceptance moves an active pointer between two fixture
executables, verifies two tasks add zero probes, verifies resume probes only the
retained executable, and proves a corrupt retained digest refuses before probing.
