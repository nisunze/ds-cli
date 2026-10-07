---
name: ds-library-seeding
description: Capture and pin an immutable DS Grid model library, or seed and verify a parallel native library with exact digest-pinned members.
metadata:
  ds-chapters: grid-model, pls-cadd
  ds-mcp-profile: pls
---

# Seed one immutable engineering library

Use the `ds` skill first. Choose the model-bound workflow below for a
`.dsgrid` source, or the parallel native seed fast path for a curated PLS-CADD
source. Each preserves exact native evidence without impersonating a solver.

## Capture and pin a model library

Discover `library.model.create`, `library.model.attach`,
`library.model.clone`, `library.model.detach` and `library.model.show` through
the live CLI; they are also in the `grid-local-model` MCP profile. These are
provider-neutral model contracts. An external solver remains an adapter and
an acceptance authority for its own characterized scope.

Capture an exact package with `library model create --model <file.dsgrid>
--expected-sha256 sha256:<source> --library-id <id> --library-version <version>
--out <fresh.dsgrid-library>`. Read the returned bundle digest and exact pin;
verify that release through `library.verify`. Capture retains reusable
engineering definitions and their exact native bytes, with source-model
provenance and `solver_approval:false`; route, terrain and customer documents
are not reusable library payload.

Attach with `library model attach --model <file.dsgrid> --expected-sha256
sha256:<source> --release <file.dsgrid-library> --expected-library-sha256
sha256:<release> --out <fresh.dsgrid>`. Use the returned resulting digest for
`library model show --model <fresh.dsgrid> --expected-sha256 sha256:<result>`.
Read every page of its bounded member report. `exact_native_bytes` is an exact
pinned-byte match; `retained_project_evidence` is a model-owned plane;
`no_cloud_equivalent` must be resolved from an exact reviewed supplier. The
`managed_export_allowed` flag is byte coverage only: it does not establish
cloud registration, native strength/load-case applicability or engineering
approval. Unknown native dependencies remain blocking.

Clone only with a deliberate new library identity/version and the exact
source-release digest. Detach only the displayed artifact/version/content-root
pin and expected model digest. Both write fresh files; detach retains native
bytes and release evidence. Publish a resulting model only through the normal
explicit project, reviewed head and native package fences in this skill.
Local library capture or adoption cannot grant project or catalog authority,
and global publication does not grant a native solver verdict.

Use the `ds` skill first and require `ds capabilities library.seed --output
json` to expose the flags below. The five-minute path is local, deterministic,
and never opens PLS-CADD or publishes cloud bytes.

## Five-minute fast path

Set one ruled source and a new immutable coordinate. These example values are
complete shell values; change them once for the actual curated source:

```bash
SOURCE="$PWD/curated-source"
STORE="$PWD/library-store"
LIBRARY_ID="new-design"
LIBRARY_VERSION="2026-08-27-v1"
ROLE="new_design"
PROVENANCE="curated-source-ruling-v1"
NATIVE_NAME="pole.012"
NATIVE_KIND="structure_definition"
VERSION_ROOT="$STORE/library/$LIBRARY_ID/$LIBRARY_VERSION"
```

Keep the full source/digest ruling under the provenance identifier; discover
its accepted limits in `library.seed`. Select the actual native name and
expected kind from that ruling and the characterized inventory. The pole
example uses `structure_definition`; do not infer missing native members.

Discover, seed, and re-run the identical seed:

```bash
ds capabilities library.seed --output json

ds library seed \
  --source "$SOURCE" \
  --out "$STORE" \
  --library-id "$LIBRARY_ID" \
  --library-version "$LIBRARY_VERSION" \
  --role "$ROLE" \
  --status review_pending \
  --provenance "$PROVENANCE" \
  --yes --output json

ds library seed \
  --source "$SOURCE" \
  --out "$STORE" \
  --library-id "$LIBRARY_ID" \
  --library-version "$LIBRARY_VERSION" \
  --role "$ROLE" \
  --status review_pending \
  --provenance "$PROVENANCE" \
  --yes --output json
```

Require the second receipt to report `idempotent: true`. Read the exact pins
from the promoted manifest, then verify and resolve:

```bash
BUNDLE_PATH="$(jq -r .dsgrid_bundle_path "$VERSION_ROOT/manifest.json")"
BUNDLE_SHA256="sha256:$(jq -r .dsgrid_bundle_sha256 "$VERSION_ROOT/manifest.json")"
CONTENT_ROOT="sha256:$(jq -r .content_root_sha256 "$VERSION_ROOT/manifest.json")"

ds library verify \
  --release "$VERSION_ROOT/$BUNDLE_PATH" \
  --digest "$BUNDLE_SHA256" \
  --output json

ds library resolve-native \
  --store "$STORE" \
  --library-id "$LIBRARY_ID" \
  --library-version "$LIBRARY_VERSION" \
  --expect-digest "$CONTENT_ROOT" \
  --native-name "$NATIVE_NAME" \
  --native-kind "$NATIVE_KIND" \
  --output json
```

## Stop and repair

- `seed_version_conflict`: choose a new immutable version; never overwrite.
- `library_verify_failed` or `library_digest_mismatch`: obtain the exact pinned
  release and digest.
- `native_name_missing`, `native_name_ambiguous`, or `native_kind_mismatch`:
  check the exact name/kind against the model/source ruling and pinned
  manifest. If the immutable source or mapping is wrong, seed a new version;
  never use basename/latest fallback.
- Any inferred source authority or certification scope: stop for the engineer.

Successful seed/verify/resolve proves exact bytes, mappings, declared losses
and pins. It does not prove native solver acceptance, strength adequacy,
visual acceptance, or engineering approval. Differential project state may
reference the resolved native member; DS Grid asset bytes never become
PLS-CADD structures, cables, criteria, or opaque resources.

For governed indexed byte readback, discover `library.global.resolve-member`
and `library.global.download`. Download with `--kind library-asset` and exact
library/release/inventory-path/digest pins into a fresh file; require verified
bytes and storage generation. A missing legacy member index is a refusal,
never permission to infer paths from file names or summary counts. Local
`managed_export_allowed` is byte coverage, not cloud qualification. Read back
the governed state before claiming a cloud equivalent or promotion.

Stops at: native acceptance — solver acceptance, strength adequacy and
engineering approval come from PLS-CADD and the engineer, never from a digest.
