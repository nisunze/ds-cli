# DS Interactions

UI-only interaction algorithms and a thin browser WASM boundary. This repository
owns transient map and canvas behavior independently of application orchestration
and engineering computation. See [the contract](docs/interaction-boundary.md).

The initial extraction moves existing Rust implementations with their owning tests.
Native compilation is a test capability, not a Server or CLI installation target.

| Crate | Transient UI capability |
| --- | --- |
| `ds-map-interaction` | Screen hit ranking, drawing constraints and coordinate-grid snap intent |
| `ds-canvas` | Camera, pointer, selection, placement and retained layout/path indexes |
| `ds-canvas-graph` | Domain-neutral Canvas2D scene geometry and bounded drag intent |
| `ds-interactions-wasm` | One browser adapter over these implementations |

The browser package is prepared into `ds-web/vendor/ds-interactions/dist` by
`ds-web/scripts/run/wasm.sh`. Native and portable WASM owning tests exercise the
same crates. `ds-web/vitest.interactions.config.ts` runs a cold map consumer with
Network import refused, using the prepared Interactions and Kernel bytes.
Parameterized projection mathematics are shared through the neutral
`ds-geo-lite::projection` feature; model CRS admission remains with Network.
