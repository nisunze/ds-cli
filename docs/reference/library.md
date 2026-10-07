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
  dsgrid/library.dsgrid-template
  pls-cadd/<category>/<exact-native-leaf>...
```

Schema version 1 is the only accepted schema. Empty provider/pin metadata stays
compatible with legacy schema-v1 `.dsgrid` packages. A pinned model opens only
against the exact artifact id, version, content root and semantic element
digest; there is no basename, latest-release or repair fallback.

The CLI names those immutable coordinates `--library-id` and
`--library-version`; transport verification uses an exact digest. Internal
manifests may call the same version a revision id, but callers never select it
through a decorative or latest-version alias.

Seed provenance is a source-ruling identifier: nonempty, at most 128 bytes,
without control characters or leading/trailing whitespace. Keep the full
authority and source-digest ruling separately under that identifier.

Verify the exact bundle named by `manifest.dsgrid_bundle_path` with
`manifest.dsgrid_bundle_sha256`. A standards seed emits a DS Grid template;
`library verify` also accepts a `.dsgrid-library` release and authenticates
its container bytes rather than choosing by filename extension.

`library resolve-native` is the differential-handoff gate. It requires the
library id, immutable version, expected content-root digest, canonical typed
name/invariant leaf and expected native kind. Its result names one exact native
artifact and SHA-256 for the characterized patcher; it does not copy bytes into
a model or open PLS-CADD.

Native kinds come from characterized native inspection and are recorded in
the manifest. For a PLS pole definition the exact kind is
`structure_definition`, not `structure`. Require the expected kind from the
model/source ruling; there is no kind alias or missing-member inference.

The asset direction is one-way: characterized PLS-CADD members may produce
typed DS Grid rows with explicit losses. DS Grid library bytes are never
converted, regenerated or copied into `pls-cadd/`. Differential model-state
handoff must select exact native bytes from the pinned `pls-cadd/` family.

`library seed` performs no cloud operation and never opens PLS-CADD. It stages
all bytes, re-reads them, then atomically promotes a previously absent local
version. Re-running the identical seed is idempotent; any byte difference at an
existing version refuses. Publication/sync remains a separate governed service
decision.

`library prepare-publication` is the only local-seed to governed-publication
bridge. It verifies and copies the exact manifest-declared DS Grid bundle and
native PLS-CADD members into a fresh prepared directory, then writes the typed
`library.json` and validation report accepted by `library global
publish-library`. It does not publish, overwrite, synthesize a native asset, or
claim solver/engineering approval.

## Governed global catalogue

`library global download` exports one pinned `library-manifest`,
`library-validation`, or `example-model`. Library artifacts require
`--library-id` and `--release-id`; example models require `--example-id` and
`--revision-id`. All require `--expected-digest` (64 lowercase hexadecimal
characters) and a fresh `--out` file in an existing directory. Exact source
coordinates, the content-addressed Storage locator, byte count and SHA-256 must
agree before any file is created. The receipt contains no delivery URL.

This is a backup export, not project admission, format migration or corpus
closure. A model file may omit separately held native library members. Never
retire global metadata until its whole declared native corpus has a verified
external backup and required project-owned references. Project and template
models share the same ownership and import/edit/render contracts; the
`template` lifecycle does not create another library authority.

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

These commands use the restored native user or device credential. Read and
publisher-write commands have separate
effect/authority contracts; exact project forks additionally require project
authorization. Local `library seed` does not publish anything globally.

An absent or non-visible library, example, immutable release/revision or indexed
member returns `catalog_not_found`. Its remedy uses visible catalog identities
and exact release pins; it does not name a transformer or another project.
Authentication and transient failures retain their native refusal codes.

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

## Model-bound interoperability libraries

Discover `library.model.create|attach|detach|clone|show`. These generic native
commands share the DS Grid package/library codecs and are available through the
`grid-local-model` MCP profile. PLS-CADD remains an exchange adapter; the lifecycle
does not run an external solver or claim strength adequacy.

`create` captures reusable definitions from the exact `--model` revision, fenced
by `--expected-sha256`, under explicit `--library-id` and `--library-version`.
It excludes project routes, terrain and unrelated customer documents. Verify
native dependency coverage: an uncaptured native resource is not made equivalent
by capture. `attach` fences both the model and `--release` bundle, adopts exact
equivalents and retains native bytes and a verified portable cache. Use repeated
`--element` IDs for an explicit subset; omitted selects all release elements.
Same names with different definitions refuse `no_cloud_equivalent`.

`show` reports exact release pins, per-native-resource equivalence and managed
export admission. Page resource rows with `--offset` and `--limit`. Missing
equivalents block a managed PLS-CADD export; an exact native-byte match still
does not prove strength-case coverage or native solver acceptance.

`clone` requires a distinct library identity and retains exact definitions and
native bytes. `detach` requires the whole immutable pin including `--content-root`
and retains native bytes, cached releases and earlier project history. Every write
uses a new `--out` path and changes no active model or cloud head.

Publish the resulting model through `dsgrid.publish-version` with explicit
project authorization and the reviewed expected head. Deliberate cross-project
adoption uses the destination project's own authorization; a library identity
does not grant it. Later changes to a library head never reinterpret pinned models.


Indexed governed member byte readback uses `library global download --kind library-asset --library-id <id> --release-id <id> --relative-path <exact-inventory-path> --expected-digest <64-lowercase-hex> --out <fresh-file>`. The authenticated resolver selects the exact immutable inventory entry; its signed generation-bound delivery is checked for the fixed storage origin, content-addressed object, byte length and SHA-256. JSON exposes the verified pin and generation, never the signed URL. Missing legacy indexes remain a named refusal; paths are never inferred from file names or summary counts.


Model-library member admission is a local exact-byte gate. `managed_export_allowed` does not attest to cloud registration, review state, strength coverage or native load-case applicability. The caller must read the exact governed release and its qualification scope before promoting a project delivery; `solver_approval` remains false.
