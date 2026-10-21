# Native transformer and local-data analytics slice

Owner consolidation, 10 October 2026. Implementation input for the existing DS workflows vertical, not a new runtime. Official consolidation: `69b459d3-63f0-44bc-8667-8c19b9f092ac`. No product implementation is included here.

## Why these scripts exist

The reconciliation needed external Python, Shapely, SciPy, pyproj and workbook manipulation over DS outputs. These were disposable bridges. Their repeated matching, comparison and verification logic belongs in native, CLI-first DS capabilities. A designer must be able to compare two transformer versions, inspect local data and resolve integrity problems without installing an external GIS analysis stack or asking an LLM to invent a report.

The same typed primitives should support project versions, survey observations, imported local layers and nonspatial tables: metric nearest and spatial joins with ties; geometry equality and tolerances; added/removed/changed features; coverage and intersection; duplicates and validity; endpoint-to-asset matching; connected components and root reachability; field differences, normalization and quantity summaries. Reuse existing native buffer/intersect/measure/sample controls. Preserve the difference between feature identity, position, spelling and aggregate totals.

## Composition and execution boundary

Use a small project-owned JSON/YAML plan chaining atomic Rust-owned operations. CLI discovers and executes first; MCP consumes the same schemas and Desktop renders previews/results. Do not embed arbitrary Python/SQL/JavaScript as workflow authority. Support one-off ephemeral plans and promote proven reusable recipes without forcing every investigation into a permanent template.

Bind project, principal, roster, source version/content hashes, CRS, tolerances, survey selection completeness and freshness. Read files at their current paths; do not move temporary inputs to make a recipe work. Retain a digest-bound receipt and lifecycle policy for ephemeral outputs. Cache source-bound machine-readable extraction of attached external documents, invalidate it when source bytes change, and retain extraction provenance. Do not commit customer datasets, credentials or generated deliverables here.

Index/chunk large vector reads with explicit feature, memory, distance and output limits. Refuse incomplete selections and ambiguous joins rather than silently truncating or selecting the nearest record without a tie policy. Cancellation and resume operate at explicit boundaries between mutations; every saved item needs its own source fence and readback proof.

## Repair is not network processing

Separate read-only analysis, evidence-backed field repair, topology editing, engineering processing and report publication. Missing approved attributes or a known customer identity do not inherently require drafting, pole placement, numbering, sizing or voltage-drop reruns. Apply a previewed allowlist of fields/features and prove unrelated geometry, types, sizes and customers unchanged. `plan_kva` remains the authored proposed rating. Never silently overwrite approved ratings through a diagnostic process.

Network processing is appropriate only when the intended engineering change requires it and its protected-field/geometry contract is explicit. It cannot recover physical truth from a missing record, settle ambiguous transformer identity, deduce whether co-located meters are duplicates or establish that parallel circuits are redundant. Surface these as evidence-bearing decisions. Approval protects authored choices and records accepted engineering exceptions; it must not suppress missing-data warnings.

## Lessons that must become admission and repair tests

* Rwegera: restore the April geometry by exact position, not aggregate counts. Forty-eight extra poles were removed; approved 50 kVA was preserved. The April source itself lacks 77 span cable sizes and has disconnected candidates. Historical parity is not proof of integrity.
* Kibungo: restore 194 exact-position April stay values; retain 17 unmatched stay requirements unresolved. Fill missing attributes from admitted evidence without overwriting existing approved values.
* Rebero: nine missing line sizes were supported by uniform complete span coverage; seven also had exact April geometry matches. Coverage must be complete before propagating a carrier attribute.
* Five supposed orphan cables (EP Gisanze, Kirwa, Ndaro, Rugarama, Tamba) all match surveyed customers exactly at an endpoint. The design-layer-only predicate was unsafe. EP Gisanze was deleted before the survey condition arrived, then restored; the remaining deletion batch was stopped. Preserve survey observations and reconcile source-identified design customers instead of deleting their cables. The old removal script is a counterexample, never an approved recipe.
* Native feature creation generates identities for legacy idless features. Verification must distinguish additive identity normalization from changed substantive attributes, while checking existing explicit IDs remain stable. Creation defaults to draft unless explicitly set; inspect readback.
* Duplicates may represent multiple meters or circuits. Zero-length services may represent a customer at a pole. Exact graph disconnection may result from legacy endpoint representation. Label candidates separately from proven faults.
* Native diagnostic processing changed a zero `tr_size` to 50 despite disabled sizing, without listing protected changes. That result was not published. Whole diagnostic output must never be a shortcut for a scoped repair.
* Combined-report omissions of tapping terminal assemblies/stays require native quantity lineage and a delivery gate. Externally patched workbooks are not canonical native output.

## Required native acceptance

Replay the session through CLI recipes and owning tests, with no required external geoprocessing packages. Include same-position changed types, duplicate names at different positions, ties/unmatched identities, multiple meters, parallel circuits, survey-only customer endpoints, incomplete survey reads, stale versions, cancellation/resume, approved `plan_kva`, zero supply, 17 unresolved stays and tapping-fitting quantities. Preserve valid existing vector controls as negative controls. Reports must fail closed on blocking findings; accepted exceptions require explicit authority and provenance. CLI/MCP/skills must expose the same findings and repair boundaries.

Scripts are evidence, not an executable supported interface. `script-index.json` records original paths, exact hashes and limitations. Read scripts before reuse; hard-coded scope/paths and mutations are historical. Current audit outputs, delivery gates and project receipts remain in Working records. Historical README quantities describe their original snapshot, not today's network.

Owner clarification: Rwegera added poles are erroneous algorithmic midspan insertions. The restored April geometry is the accepted baseline; do not reintroduce those 48 poles through a processing or repair pass. Include this regression as a protected-geometry acceptance fixture.
