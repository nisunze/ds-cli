# Corridor seeding acceptance and current authority

Read this when reviewing a seed or diagnosing a misleading surface. These are
sampling/coverage decisions, not a duplicated CLI schema. Read the live native
descriptor for parameters and current availability.

## Choose density without inventing accuracy

Start with the native defaults, then justify them against source resolution,
smallest relevant landform, route geometry and actual engineering intervals.
Inspect point density both along station and across the corridor. Many points
on one line do not establish two-dimensional support; a high project-wide count
does not repair an empty local patch. Local spacing statistics, edge lengths,
covered area and unsupported runs should explain the admitted pieces.

Use nonzero bounded station randomness for the owner's irregular seed; enable
bounded lateral observation randomness when a scattered corridor cloud is
wanted. Retain exact mandatory anchors and stable observation identities. Record
the realized settings and seed. Native correlated lateral jitter can produce a
natural observation pattern without changing heights or the nominal profile.
Avoid excessive jitter that leaves useful support or creates near-duplicates.

A maximum station gap is an engineering sampling limit. A maximum triangle edge
is surface admission. Observation width is sampling eligibility. A side offset
is a nominal cut. Display width is presentation. Do not interchange these.
The adaptive sampler's current sign is negative right / positive left relative
to traversal; another historical Profile API can use different vocabulary.
Use the owner of the requested cut and retain its declared sign in the receipt.

The native contract's example of four segments over a declared 65 m interval
requires spacing no larger than 16.25 m even if the global gap permits more.
This illustrates interval tightening, not a universal span or seed prescription.
The Gisagara preview used a 1 m discrete baseline and approximately 0.1 m
reduction tolerance. Those values document that run; they do not certify raster
accuracy or continuous error between queries.

## Review observable behavior

| Case | Required result |
| --- | --- |
| Independent corridors 200 m or 1 km apart | Separate bounded domains; no triangles or ground across the empty space. Distances are regression examples, not a universal split threshold. |
| Nearby independent routes with overlapping footprints | A physical point may have plural membership; each admitted domain stays independent. Sharing cannot expand either footprint into an unsupported patch. |
| Connected parent and T-off | Declared supported pieces/observations may be shared; family membership cannot bridge a hole, no-data or unsupported run. |
| Dense DEM over sparse alignments | Corridor-limited irregular derived samples, not a project rectangle of grid points or a hull-wide blanket. The underlying raster may remain continuous. |
| Sparse survey, collinear samples or one-sided support | Preserve measured XYZ; reject or qualify unsupported two-dimensional interpolation. Never turn point count alone into coverage. |
| Transverse affine slope | Nominal centerline and side cuts follow the unchanged surface; changing random observation positions produces no alternating-side sawteeth. |
| Narrow crest or valley and short engineering interval | Retained anchors/extrema and sufficient nearby samples; thinning satisfies both discrete height error and interval density. |
| Missing patch, route endpoint or one-point supported run | Explicit gap or point-only support; no zero fill, endpoint stacks, extrapolation or joining across gaps. |
| Same source, inputs, settings and seed | Replay the sampled content with the same positions/heights and retention decisions; do not require volatile timestamps to match. |
| Different random observation seed | Positions can change; source bytes and heights at identical XY do not. Random side observations never move nominal side cuts. |
| Model materialization | Original source rows survive; explicit derived ground basis is read back; profile, structure grounds and clearance share that ground and its gaps. |

Review point clouds in plan at whole-route and local scales. Keep sampled points,
nominal cuts and source classes visually distinguishable. A profile should show
real landform and breaks clearly. Beautification changes presentation or
supported sample selection, never observed elevations. A pretty screenshot is
not numerical evidence of source fidelity, density or coverage.

## Source contracts on ds-server

Canonical files live under `~/data-solutions`; read current `run` contracts for
coding. Packaged skill provenance does not prove an installed feature exists.

- `ds-network/docs/contracts/adaptive-terrain-sampling.md`: exact-XY single-source
  sampling, seed semantics, adaptive reduction, optional nominal side cuts,
  engineering interval tightening, discrete error and artifact authority.
- `ds-network/docs/contracts/profile-transactional-database.md` section 6 and
  section 10 (owner decisions 2026-10-08): deliberate Ground scope, plural
  membership, independent principal surface domains, bounded footprints,
  unsupported gaps and local neighborhood invalidation. Its present-boundaries
  section explicitly identifies missing implementation; do not advertise the
  requirement as shipped behavior.
- `ds-network/docs/contracts/alignment-ground-profile-basis.md` (2026-10-07):
  one selected derived basis, original source preservation, explicit stations
  and runs, millimetre endpoint quantum, and one common engineering ground.
- `ds-network/docs/contracts/terrain-ops-contract.md`: observation source
  precedence, no-data, native geometry ownership and the distinction between
  a flat-cloud TIN primitive and admitted bounded surface domains.
- `ds-network/docs/contracts/terrain-sampling-evidence/README.md`: pinned
  Gisagara M3 regression. Flagged DEM observations match original-coordinate
  Rwanda readings; alternating lateral positions explain sawteeth. The later
  adaptive preview is evidence of sampling, not authorization to reset a model.

For an explicitly requested triangulation implementation, inspect current
`ds-grid-engine/tests/architecture_guards.rs`, owning manifests and the
resolved Server dependency guard before selecting a dependency. Current
terrain primitives are owned by `ds-geo-lite`; the historical flat-cloud TIN
is not a membership or footprint engine. Unsupported authored breaklines are
refused by the adaptive surface owner until constrained triangulation is
supported. Do not relax architecture or coverage checks to make a seed look
continuous. Implement a demonstrated missing capability in its native owner,
not this skill, the CLI adapter or TypeScript.
