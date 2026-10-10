# GIS, geospatial and general data workflows: delivery evidence

This directory is implementation input for the existing [DS Workflows contract](../../docs/contracts/ds-workflows.md), not a new runtime, supported Python interface or completed-work archive. The copied bridges demonstrate missing composable controls. Remove a bridge once its equivalent native operations and acceptance are proven; keep durable semantics in the owning contract and tests. Implementation remains subject to the existing stabilization boundary in OPEN §24.

## Owner direction

Compose typed operations to solve a one-time problem without first publishing a reusable workflow. Save a workflow when reuse has value. GIS/geospatial analytics must chain with ordinary non-geospatial data cleaning, joins, field mapping and reporting. Geometry is optional; a table-only workflow must be first class. Avoid a special Gisagara or pole-restore command.

The source scripts were copied, not moved. `script-index.json` records source paths, host, SHA-256 and byte counts. They retain absolute paths and operational history intentionally; they are reference specimens, not portable entry points. Do not execute the mutation scripts from this directory. No project datasets, credentials, dependency directories, generated workbooks, images or GeoPackages are checked in here. Original project files and verification receipts remain under `/home/magese/projects/gisagara/`; Windows originals remain in `C:/Users/nisun/ds-work/tmp/`.

## Actual compositions and effects

| Composition | Inputs and pure steps | Effect boundary |
| --- | --- | --- |
| Snapshot comparison | 85 historical GPKGs, DS-exported live snapshots, exact initial roster; select 87, exclude allowed changes, normalize CRS, match, compare selected fields and geometry, aggregate | Text/PDF and diagnostic files only |
| Selected-field restoration | Historical and live poles; same-position join, selected `struct_type` diff, group patch by expected old/new value | DS select/set/save with revision fences; saved readback verifies every feature |
| Approval assignment | Explicit initial87 roster, independent of matched restore rows | Assign `drafting_status=approved` across each design, save and verify |
| Canonical report | Saved post-restoration snapshots, approved plan ratings, saved nature, network config and reference asset | DS reporter writes XLSX/GIS; approved presentation-only sheet removal; verified delivery |
| Geometry review | Historical/saved snapshots; bidirectional point anti-join and line coverage difference; retain transformer and source-layer provenance | Two GeoPackages, local map layers/groups and exact changed-transformer Working set |
| General table cleaning | CSV/XLSX or document extraction cache; rename, cast, normalize nulls, map domains, dedupe, cardinality-checked join, filter, aggregate | Table/file preview or explicitly authorized patch sink; no geometry requirement |

## Temporary data without moving source files

1. Accept a path/asset reference at its current location and hash exact bytes. A temporary workflow can consume an already extracted/cached file directly. Never require moving or uploading a document merely to read it.
2. Bind project/principal, input revision/digest, layer/sheet, schema, CRS, units and explicit null policy. Byte equality is different from semantic equality.
3. Cache admitted extraction/normalization by input digest and operation version. Changing one source invalidates dependent steps only. Cache entries inherit source access and retention; extraction is not an independent permission grant.
4. Keep intermediates on disk in a run-owned directory, not `/tmp` or `/dev/shm` on ds-server. Stream large frames and bound whole-frame operations. Do not duplicate warm dependencies or large datasets into saved recipes.
5. Receipts reference inputs/outputs, hashes, row counts, ambiguity/unmatched counts, versions and effect state. Retain enough authorized source material to reproduce a decision. Do not silently delete referenced inputs while a plan/run still needs them.
6. Save promotes the parameterized recipe, never incidental absolute paths, secrets or an entire scratch directory. Bind new inputs and replan on reuse; saved recipes do not inherit permission to mutate a different project.
7. Cleanup is explicit and scoped to owned unreferenced scratch. Existing original files were left in place for this delivery. This is preservation, not a policy to keep every temporary file forever.

## Engineering decisions that must survive composition

- Initial scope is exactly 87, including Substation, with no additional sites. Restoration/comparison excludes Zihare, Ruhuha and Upgrade Mbeho. Substation has no historical baseline: unknown is not an addition or a zero-count baseline. Approval includes all87 independently.
- Restore the type at the same physical pole position. Never balance type totals, choose nearest poles, move geometry or add/remove poles to force parity. The run's 0.001 m point normalization is a case parameter, not a universal default. Ambiguous duplicate matches must refuse or remain explicit; the prototype plan/audit did not establish a reusable one-to-one matcher for arbitrary data.
- Results:301 type changes across13 designs,7,894 matched pole positions,217 unmatched live positions untouched; all87 saved/verified,44,207 features approved. Counts and all geometry preserved. These totals are evidence for native acceptance, not constants for a new dataset.
- Any proposed transformer size is `plan_kva`, per owner. A positive plan value can project to report `tr_size` without demand recalculation. `0`/missing does not establish a proposed rating. Do not infer proposals from calculated sizing sheets. The delivery did not rewrite saved `tr_size`; its projection changed disposable reporter inputs only.
- June approvals govern Mbeho100, Kimana160, Bazenga160 and Rugarama160kVA. Kirwa retains its existing100kVA transformer and has zero transformer supply. Keep site participation, planned rating and supply quantity distinct. Fill-in participation does not imply supply.
- Final supply65:50kVA21,100kVA26,160kVA15,200kVA1,250kVA2. Zihare's current saved50 replaced a previously cached report100; this was a stale/report-input difference, not a change made by pole restoration. It has no positive `plan_kva` in the captured snapshot.
- Missing/untyped poles remain unresolved; approval does not repair algorithmic defects. Kibungo's17 untyped unmatched live poles were not inferred as S140. Do not claim approval is a technical validation of every attribute.
- Save filled some missing same-transformer `transfo` references. The verifier allowed only missing-to-same-owner normalization and reported it. Native patch previews must expose this separately, never silently broaden the allowlist.

## Geometry review semantics and limits

The read-only bridge compares seven layer families:transformers,poles,tapping poles,customers,LV lines,service cables,spans. It omits derived print labels. EPSG4326 inputs are compared in EPSG32735 metres and delivered in4326. Point match tolerance0.001m; line coverage buffer0.01m; line fragments shorter than0.1m omitted. Declare these explicitly in the recipe and receipt:they are not native defaults or proof of survey precision. Line comparison uses buffered union coverage, not the direction-aware DS `map.line-difference` algorithm; it can mask distinct nearby parallel geometry and does not resolve topology/ownership. A moved feature appears on both sides, not as a confirmed identity-based move.

Output:216 addition points,174 removal points,480 addition line fragments,384 removal line fragments,51 changed transformers. Point counts include tapping poles, not just the pole-count schedule; fragment counts are not original feature counts. Split/resegmented lines can compare as unchanged coverage. Attributes such as labels/types/sizes alone are not geometric changes. Validate against known identical, moved, parallel, split, duplicate and reversed-line controls before promoting the prototype algorithm.

## Failures and lessons

- Two desktop edit workers overlapped on Rwinzuki during tail reassignment. Optimistic concurrency rejected the second save with `transformer_changed`; one save/readback succeeded. Do not describe this as successful disjoint scheduling. Native scheduling needs exclusive project/transformer leases, persistent per-target state and no shared receipt-path races. Discard/reload the rejected room without replaying effects; it ended clean.
- An after-export/verified marker is not sufficient crash recovery by itself. Resume must validate fences, source digests and complete receipt lineage; writes and verification markers should be atomic. Idempotent open can retry readiness; a mutation retry requires owning semantics.
- `--target`/DS_TARGET did not target the older map.design command surface. Use admitted `--desktop-descriptor` for those commands. Never read pairing secrets. Other commands do admit `--target`: discover each live contract.
- Desktop readiness timed out while the room later became ready. Bounded status/readback recovered it. Same counts/status do not replace full readback checks.
- Feature.id is absent on some service cable rows. The bridge used stable ordinal only while asserting whole before/after equality; a reusable system needs admitted stable identity or explicit refusal, not arbitrary ordinal joins.
- Native supply still used `tr_size`/prefix rules rather than fully respecting plan/nature. Positive plans were projected into disposable report inputs; Kirwa quantity input had `tr_size=null` only to obtain zero supply. GIS came from a separate projection with Kirwa100 and approved plans intact. No numeric workbook patch and no model size mutation. Replace with separate native planned-rating and supply contracts.
- Workbook removal used ZIP/XML to remove only Transformer Sizing and its relationships/defined names. Remaining worksheet bytes were verified identical. This is a presentation-only bridge, not a permitted way to hide Dirty Categories or quantity errors. Final sheets:InfoTable,LV Summary; no Dirty Categories.
- Parallel GPKG inspection shut down another worker. Sequential parsing succeeded. Two source groups registered through `map.local.register` did not hydrate into the running map; four flat session layers were loaded separately. Pins51 verified. Two registered groups are not proof of two visible groups. Capture failed with a Windows file-lock error; no successful screenshot evidence was obtained.
- File copies through Windows G: were hash-verified. Server rclone was not used. Public deliverables exclude scripts, transport ZIPs and receipts. Review GPKGs/diagnostics belong to Working records.

## Native product boundaries and acceptance

Reuse the existing DS Workflows contract:typed DAG, registered Rust table/vector operations, explicit source admission, pure transforms, bounded sinks, plan hash and revision fences. No embedded Python/SQL/JavaScript or second transformation engine. CLI/MCP use the same authority; UI is a workflow editor/preview renderer. Support ephemeral one-time runs, composition of named subflows with explicit inputs/outputs and version pins, and deliberate saving after reuse is demonstrated.

Required reusable capabilities:read GPKG/XLSX/CSV and cached document extractions; schema profile/column mapping; cardinality-checked joins; selected-field diff; point/line differences; group/aggregate; guarded patch projection; constant assignment over an independent scope; file sinks including GPKG; refreshable grouped local-map sinks; Working set sink; native report plans with independent planned-kVA/supply rules; presentation sheet selection.

Prove with both this geometry/field reconciliation and an unrelated table-only cleaning/join. Refuse unknown CRS, ambiguous identities, changed heads, unsupported ops and unconfirmed effects. Test cancellation, partial success, retry and resume without duplicate effects, dirty-room preservation and all non-allowlisted fields. Report cold/warm/incremental timings, bytes read, network calls, cache hits and peak memory. Do not claim these bridges prove general correctness or performance.

Related feedback sightings (historical evidence; coding work belongs in OPEN):
959dd4d6-c75c-44da-b54a-7689cf59d7e4 reconciliation;
bea33682-c468-429a-a01b-6f6359adf5e7 GPKG comparison;
2a0f9de2-a0c9-43e0-8441-31dea6460c32 readiness;
4d4f9198-b6d8-4762-b046-758350f4e022 untyped poles;
6b369e94-89f2-43a0-a8c5-74b8fab0dcf7 approved-plan reporting;
feae1d8e-271a-4a27-96cc-459b53c50ba5 retained-transformer supply;
bde219c8-14e4-4634-8ccc-72da36a2adb0 bidirectional GPKG review;
9a4de06f-2245-40cb-b710-ed801cbc88ed GPKG worker lifetime;
50d066a1-5b5b-496d-b194-88efb412de61 grouped local hydration.
