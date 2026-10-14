# DS Interactions

UI-only interaction algorithms and a thin browser WASM boundary. This repository
owns transient map and canvas behavior independently of application orchestration
and engineering computation. See [the contract](docs/interaction-boundary.md).

The initial extraction moves existing Rust implementations with their owning tests.
Native compilation is a test capability, not a Server or CLI installation target.
