---
name: ds-library-seeding
description: Seed and verify one immutable parallel DS Grid/PLS-CADD library version, then resolve an exact digest-pinned native member.
metadata:
  ds-chapters: pls-cadd
  ds-mcp-profile: pls
---

# Seed one immutable engineering library

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

Stops at: native acceptance — solver acceptance, strength adequacy and
engineering approval come from PLS-CADD and the engineer, never from a digest.
