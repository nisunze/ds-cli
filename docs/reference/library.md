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
