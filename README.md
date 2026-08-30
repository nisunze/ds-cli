# ds-command-kernel

Shared deterministic command and state transitions used by native DS clients
and browser WASM. Svelte remains the presentation owner; hosts retain network,
credential storage, IndexedDB, OS, and rendering effects.

The first deliberately small slice is Firebase login-response validation. It
allows `ds` and ds-web to bind UID, email, token subject, Firebase audience,
issuer, and expiry with one implementation. Password collection, HTTP calls,
refresh-token persistence, Google UI, and session side effects remain host
adapters.

Expansion is evidence-gated. A new slice belongs here only when it immediately
serves a native `ds` operation and removes duplicated TypeScript domain logic.
Parallel mutable implementations are not allowed.

Concurrency is also evidence-gated. Login remains serial because it has no
partitionable compute. A future compute slice may expose an optional parallel
implementation when benchmarks justify it: Rayon on native hosts, and Rayon
workers in browser WASM only when the packaged deployment proves shared-memory
thread support. Every parallel path retains one deterministic serial contract
and native/browser parity tests; GC or thread availability alone is not a
reason to parallelize state transitions.

```bash
./build.sh
```
