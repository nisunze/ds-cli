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

No product keeps a copy of this export: Desktop's Tools page reads installed
Core's domains and descriptors through `ds capabilities` itself.

General analysis runs through external executors and standard data files. The
DS catalogue retains the named domain and data contracts; it declares no custom
analysis DAG, workflow store or arbitrary shell execution.
