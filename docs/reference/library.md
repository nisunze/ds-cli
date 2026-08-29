# `ds library`

`ds library` is the offline, typed surface for immutable engineering-library
artifacts. The authoritative manifest, PLS-CADD ingestion, digest and package
logic is linked from `ds-network`; the CLI only reads explicit paths and writes
new local paths.

The standards seed layout is fixed:

```text
library/<library-id>/<version>/
  manifest.json
  receipt.json
  dsgrid/library.dsgrid-library
  pls-cadd/<exact-native-leaf>...
```

Schema version 1 is the only accepted schema. Empty provider/pin metadata stays
compatible with legacy schema-v1 `.dsgrid` packages. A pinned model opens only
against the exact artifact id, version, content root and semantic element
digest; there is no basename, latest-release or repair fallback.

The CLI names those immutable coordinates `--library-id` and
`--library-version`; transport verification uses an exact digest. Internal
manifests may call the same version a revision id, but callers never select it
through a decorative or latest-version alias.

`library resolve-native` is the differential-handoff gate. It requires the
library id, immutable version, expected content-root digest, canonical typed
name/invariant leaf and expected native kind. Its result names one exact native
artifact and SHA-256 for the characterized patcher; it does not copy bytes into
a model or open PLS-CADD.

The asset direction is one-way: characterized PLS-CADD members may produce
typed DS Grid rows with explicit losses. DS Grid library bytes are never
converted, regenerated or copied into `pls-cadd/`. Differential model-state
handoff must select exact native bytes from the pinned `pls-cadd/` family.

`library seed` performs no cloud operation and never opens PLS-CADD. It stages
all bytes, re-reads them, then atomically promotes a previously absent local
version. Re-running the identical seed is idempotent; any byte difference at an
existing version refuses. Publication/sync remains a separate governed service
decision.

## Governed global catalogue

The global catalogue is a separate authority from the local immutable store.
`library global read` lists global libraries, exact immutable releases, global
examples, and exact example revisions. The primary publisher commands are
`library global upload`, `publish-library`, `publish-example`, and the two
typed lifecycle commands. They use explicit flags or a local prepared
directory—never a raw server body on the command line. They change only the
governed head lifecycle (`active`, `archived`, `deprecated`, or restored) and
never overwrite or delete an immutable child. `library global fork-example`
creates a project model from one exact active example revision and records its
server-derived provenance without copying or re-uploading the source object.

These commands use the signed-in Desktop API bridge but do not require the map
or a project page to be open. Read and publisher-write commands have separate
effect/authority contracts; exact project forks additionally require project
authorization. Local `library seed` does not publish anything globally.

Short hypothetical requests and their command shapes:

```text
"Publish the prepared Rwanda PLS-CADD library without hand-building an API body."
ds library global publish-library --prepared ./rw-pls-cadd-library --yes --output json

"Archive this head, but refuse if another publisher moved it first."
ds library global library-lifecycle --library-id rw-pls-cadd-structures \
  --expected-head-release 2026_08 --expected-lifecycle active --lifecycle archived --yes --output json

"Create a project model from the exact Karongi example revision."
ds library global fork-example \
  --payload '{"project_id":"my-project","fork":{"example_id":"karongi-mv","example_revision_id":"2026.08","expected_head_revision_id":"2026.08","model_id":"karongi-copy","revision_id":"v1","display_name":"Karongi governed copy","model_kind":"mv_line","model_schema_version":"1","engine_version":"pls-cadd-pinned","reason":"Start from the proven global example"}}' \
  --yes --output json
```

A library prepared directory has a small `library.json` with a top-level
`visibility` and a `library` member containing the governed head fields and a
`release` member. `release.manifest` and `release.validation_report` are
`{ "path": "relative-file" }`; every entry in `release.assets` keeps its
server-defined `relative_path`, `class`, `provenance`, and optional
`external_definition`, plus a local `path`. The
adapter uploads the first two under `library_manifest` and
`library_validation_report`, and every asset under `library_asset`, then
replaces only those local `path` values with canonical artifact pins. An
`example.json` works identically: `revision.model`, `previews`, and
`artifacts` name local files, with `model`, `project`, and `preview` routed to
`example_model`, `example_project`, and `example_preview`. Safe relative
paths, immutable artifacts, scope, and lifecycle are still enforced by the
catalogue service.

Global publication is a governance claim about immutable bytes, validation
evidence, scope, and provenance. It is not PLS-CADD solver acceptance or an
engineering certification claim.

Execution ownership:

- `ds`: inspect, verify, catalogue, local store access, pack/unpack, plan and
  materialize a local immutable seed with receipts.
- characterized native patcher: differential project model-state edits only;
  never library-asset synthesis.
- PLS-CADD UI/solver: native calculations, checks or explicit operator visual
  acceptance, never seed ingestion.
- engineer: source authority, strength/certification claims and adoption.

After any PLS-CADD UI save, re-import and compare the saved workspace as a new
authority candidate. A parser/readback result is not native solver or
engineering approval.
