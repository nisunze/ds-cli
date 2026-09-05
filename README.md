# ds-command-kernel

Shared deterministic command and state transitions used by native DS clients
and browser WASM. Svelte remains the presentation owner; hosts retain network,
credential storage, IndexedDB, OS, and rendering effects.

The first deliberately small slice was Firebase login-response validation. It
allows `ds` and ds-web to bind UID, email, token subject, Firebase audience,
issuer, and expiry with one implementation. Password collection, HTTP calls,
refresh-token persistence, Google UI, and session side effects remain host
adapters.

The second slice admits and projects sealed Solar portfolio results by calling
the canonical `ds-solar-contracts` read operation. Native `ds` and browser WASM
therefore share schema, digest, totals, membership, and representative-graph
validation while retaining their own file/session transport.

The Project Control slice owns transformer-room scope and freshness decisions,
dirty-room preservation, memory-bounded concurrent download schedules, report
reuse/refresh decisions, and the local combined/compounded report projection.
The browser and declarative `ds` commands enter that same semantic desktop
operation. Hosts execute the plan: credentials, HTTP, IndexedDB, filesystem
writes, browser downloads, progress rendering, and cancellation remain outside
the kernel.

The Solar network-seeding slice joins two deliberately separate fact
authorities: DS Grid contributes revision-pinned MV alignment lengths and
granular city/phase tags, while local reporter rooms contribute transformers,
customers, and LV lengths. Explicit city bindings resolve different source
spellings; no fuzzy name join is permitted. The LV tag projection remains its
exact exported text and is verified against its supplied SHA-256 before use.
The result is a deterministic,
digest-pinned plan of complete Solar city documents. MV structure counts are
outside this version and are rejected rather than silently consumed. Native
`ds-report`/`ds solar` and browser WASM call this one operation; hosts retain
file selection, IndexedDB, downloads, and background cloud publication.

Expansion is evidence-gated. A new slice belongs here only when it immediately
serves a native `ds` operation and removes duplicated TypeScript domain logic.
Parallel mutable implementations are not allowed.

Concurrency is also evidence-gated. Project Control bounds independent network
requests by both a resident-room cap and a request-concurrency cap; the host may
run those admitted batches concurrently without holding the whole project in
memory. Tokio is not embedded in the browser WASM contract, and Rayon would add
overhead to a metadata projection bounded at 2,000 rows. Native async or CPU
parallelism belongs only behind this same plan when measurement shows a real
bottleneck. Every parallel executor must retain deterministic receipts and
native/browser parity.

```bash
./build.sh
```

The layer catalogue slice projects canonical backend IDs and their runtime
companions for the native CLI and browser bridge. It preserves server order,
reports truncation, and does not invent host visibility or backend assembly.

Style authoring now shares appearance, categorical second dimensions, line types,
casing and fill recipes across native and WASM. The native guided planner reads
backend editor vocabulary and bounds, preserves the complete authored document,
and resolves its save target. It performs no HTTP or rendering. Captured legacy
TypeScript fixtures verify 47 authoring cases against the same Rust operation.
