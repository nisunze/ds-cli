# ds-gateway-client

Portable generated gateway operation IDs, methods, parameter metadata and path
planning for use below `ds-command-kernel` and `ds-client-core`. The first slice
migrates Kernel project, admin and form reads. It has no
HTTP, credential, database, engine, CLI or UI dependency.

The copied gateway specification declares 114 paths and 126 non-CORS operations.
Many response bodies have no schema. Generated descriptors expose that fact;
the existing typed application validators continue to admit request/response
bodies. This crate does not invent missing Survey, report-artifact or printing
routes. CORS operations remain in the converted OpenAPI document and are not
callable application operations.

```rust
use ds_gateway_client::Operation;

const PROJECTS_PATH: &str = Operation::ListUserProjects.path_template();
const PROJECTS_METHOD: ds_gateway_client::Method = Operation::ListUserProjects.method();
let read = Operation::ReadPrincipalDevice.plan(&[("device_id", "device-a")])?;
# Ok::<(), ds_gateway_client::PlanError>(())
```

The small `Client<P: Policy>` wrapper delegates exactly once to the existing DS
application policy owner. Its captured context and result/refusal types belong
to that owner. Refresh coalescing and protected credentials remain in the auth
owner; held reads, offline admission, retries, domain receipts and transport
causes remain in the existing Kernel/native policy. Do not create a second
policy implementation in this crate. `Policy::Output` may be a result or a
future, allowing native and WASM execution without prescribing an executor.

Generation is a mechanical portable route projection, rather than the mandatory
reqwest executor emitted by progenitor. The Node converter is the pinned
`swagger2openapi@7.0.8`, with its dependency integrity captured by package-lock.
No external references are resolved. No Rust build runs Node, reads another
repository or downloads schemas.

From this crate directory:

```sh
npm ci --ignore-scripts --no-audit --no-fund
npm run generate -- --copy-from ../../../../ds-apis-tf/generated_swagger.yaml
npm run check
npm test
```

Ordinary regeneration uses only `spec/gateway-swagger.yaml` with
`npm run generate`. The explicit copy step updates that local snapshot and never
writes the source repository. `spec/provenance.json` records source, converted
OpenAPI, generator and output digests, converter identity and schema coverage.
Those are provenance/integrity facts, never compatibility gates for a peer.

`npm run check` refuses any stale generated output. `npm test` includes planted
duplicate operation IDs, path mismatch, external reference, unspecified schema
and invalid-command controls. Rust tests are separate in `src/lib_tests.rs` and
cover path escape refusal, closed dispatch, compile-time route coordinates and
unchanged policy refusal forwarding. Real Kernel/native migration still requires
its owning tests, zero-call held/offline controls and installed read acceptance.
