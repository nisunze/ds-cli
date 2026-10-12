---
name: ds-vector-tools
description: "Process local GeoJSON with compiled Rust vector tools through ds CLI/MCP: measure geometry, buffer zones, sample lines or find line crossings, with JSON results and no project or Desktop."
metadata:
  ds-chapters: data
---

# Native local vector work

Use these commands when an agent needs local geometry results. Discover the
compiled tools before preparing a separate GIS runtime. The same native Rust
operations serve the map and the headless executable.

Start with the installed contract:

```
ds capabilities --search vector --output json
ds capabilities data.vector.measure --output json
```

Measure unfamiliar input to identify geometry classes and units. Choose the
operation matching the question: `buffer` for zones, `sample` for points along
lines, or `intersect` for line-crossing points. Read its exact live descriptor;
these are not polygon overlay, dissolution or line densification tools.

Inputs are WGS84 longitude/latitude GeoJSON. A file and inline JSON text reach
the same native document planner. Through MCP, inline JSON is a **string**
containing serialized GeoJSON, not an object argument. Keep large documents
in files rather than command arguments. The `datasets` typed
profile exposes these local tools; the broad server routes them through
`ds_data`. No account, project, network request or paired Desktop is needed.

Inspect skipped geometry reasons and `more` before treating a result as
complete. A source-feature bound can withhold input, while a result bound can
withhold inline output; writing an output file does not restore input that was
never processed. An empty crossing or sampling result can be a valid answer;
read its `note`. Retain source identity/properties and the output receipt when
passing the result to another explicit consumer.

For other source formats, discover `data.inspect` and `data.convert`; follow
their actual format and CRS contracts. A requested operation absent from the
installed surface is a capability gap: follow `ds` discovery and feedback.
For elevation curves, adaptive point reduction, span-aware density or optional
side profiles, use `ds-terrain-sampling`; fixed-distance geometry sampling does
not establish a representative terrain surface.
For display, hand the produced GeoJSON to `ds-map-local-data` or
`ds-layer-management`; for authorized corridor dataset reads, use
`ds-cloud-datasets`.

General analysis uses an external executor's own interface and standard files
outside DS Run. DS has no custom general analysis DAG or recipe store. Discover
the exact DS export and validation/import commands, retain source identity and
revision/digests, declare CRS/units and external tool version/parameters, and
validate the output before an approved native import or design change. Retained
geometry primitives and governed direct repair commands remain their owners.

Stops at: the native map workflow for display, an external GIS executor for
general analysis, or the operator for an unavailable control. Hand over source
identity, revision/digests, declared CRS/units and operation evidence. Return
external outputs with tool version/parameters for governed DS validation and
an explicitly approved import; retained geometry remains native DS authority.
