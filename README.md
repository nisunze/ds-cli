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

```bash
./build.sh
```
