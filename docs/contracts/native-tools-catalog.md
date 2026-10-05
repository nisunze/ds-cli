# Native tools catalogue export

`ds capabilities --export tools --output json` explicitly projects the complete
registered command catalogue. Ordinary root, domain and single-command
discovery remain bounded. Export performs no command execution, filesystem
writes, credential reads or runtime availability probes.

The `ds.cli.tools-catalog/v1` payload contains provenance and ID-sorted commands.
Command IDs, chapters, summaries, verbatim purposes, CLI paths, exact authored examples, effect,
authority, requirements, contract versions, refusals and output prose come
from the live `Registry` declarations. Argument JSON schemas use the existing
MCP projection, including choices, required inputs and confirmation semantics.
Unprojectable declarations refuse instead of acquiring an invented schema.

Status is `declared`, availability is `deferred`, and execution mode is `cli`.
These fields promise a discoverable native invocation, not a browser runtime
or a successful availability check. A product may independently bind a richer
native WASM descriptor that its runtime actually supports. The terrain sampling
request schema comes directly from the native terrain owner's strict request
type. Request and result schemas without such an owner are omitted; output
prose is never labelled a machine schema.

Provenance records the compiled source revision and dirty state. Its SHA-256
fingerprint covers the complete exported command facts, including owner request
schemas, so uncommitted declarations remain auditable.

The generator and drift guard share the exact same projection:

```sh
cargo run -p ds --example export_native_catalog -- --out <fresh-artifact-path>
cargo run -p ds --example export_native_catalog -- --check <committed-artifact-path>
```

The generator writes only the named fresh file and refuses collisions. The
check compares schema, all command facts and their fingerprint; build revision
and dirty-state differences alone do not count as declaration drift. It does
not build or replace `target/debug/ds`. The product artifact is
`ds-web/src/lib/tools/native-catalog.generated.json`; generation ownership
remains here. Replace it only after comparing the generated fresh artifact.

Portable vector workflows remain `ds.vector-workflow/v1` documents executed
by the existing Rust vector workflow owner. That owner ships eight embedded
examples, exposes validation and runs named graph outputs with provenance.
This catalogue adds no workflow execution algorithm, publication backend or
global sharing authority.
