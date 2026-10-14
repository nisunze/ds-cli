# Standalone module catalog admission

The fixed component IDs are `solar`, `reporter` and `network`; their engine owners
are `ds-solar`, `ds-network-reporter` and `ds-network`. The shell retains its existing desktop
release namespace and is not decoded as a module. This slice introduces no
general plugin registry or installation framework.

`src/module_release.rs` owns semantic admission of held public metadata:

```rust
validate_module_release(component: &str, channel: &str, release: &serde_json::Value)
    -> Result<ModuleRelease, ModuleReleaseError>
```

The host supplies explicit `stable`/`canary` context and raw held metadata from
the existing public release operation. The pure owner performs no network or
filesystem reads. A visible consumer or explicit refresh acquires a catalog;
unused catalogs receive no request, and retained metadata remains available
offline. The error carries one stable `code` and a bounded rule `message`.

The closed `ds.module-release/v1` DTO carries component, channel, lane-bound
product ID (`rw.datasolutions.module.<component>[.canary]`), version, protocol
`ds.module/v1` for Solar/Reporter or `ds.network-engine/v1` for Network, release timestamp, governance status, optional notes and one or
two platform assets. Windows/Linux x86_64 are admitted first. An asset carries
`module-executable`, its matching protocol, immutable public HTTPS storage URL,
size/digest, build timestamp, owner source revision, producer-declared source
closure and one exact `ds.engine-build/v1` identity. The source closure is
bounded at 16 repositories and is never expanded into an invented stack vector.
An unchanged carried platform asset retains its original release folder and
all its source/engine facts.

Crossed component/lane/product, unknown schema/keys, repeated targets,
unsupported platforms/protocols, partial engine identity, missing or crossed
source closure and mutable/off-origin executable URLs refuse. Collection and
string shapes are bounded before cloning/deserialization. Legacy desktop
records are not reinterpreted. `blocked`, `stopped` and `rolled_back` remain
visible governance facts; semantic admission does not make them active.

Public catalog validation is not signature verification, download permission
or installation trust. No install, activation, process launch, subscription or
polling effect exists here. The shell's existing signed update boundary remains
independent. A module installer requires its own admitted native trust and
atomic activation boundary before any UI or CLI control can offer installation.

Brain owns authenticated publication, immutable metadata and atomic pointer
advance under `app.release.manage`; its receiving wire contract is
[App release governance](../../../ds-brain/docs/contracts/app-release-governance.md).
The protected publication envelope remains distinct from the existing public
`{success,release}` envelope.

Focused executable proof is `cargo test --lib module_release --jobs 2`: carried
source preservation, closed DTO round-trip, component/lane/legacy refusals,
malformed target/provenance/URL bounds and withdrawal-status preservation, plus Network-specific protocol/executable/producer admission.

Network assets name `ds-network` on Linux and `ds-network.exe` on Windows. Its
metadata selects the existing engine protocol; it is not relabeled as the Solar
prepared-task protocol. The common module catalog/artifact envelopes stay at v1.
No Network release is inferred from this declaration: an absent public pointer
remains absent until an exact qualified producer artifact is published.
