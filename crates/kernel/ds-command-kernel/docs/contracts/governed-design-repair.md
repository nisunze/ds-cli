# Governed design reconciliation and repair

`design_repair` owns pure audit, exact comparison, evidence-backed proposal and
selected projection. `ds-design-workspace` owns atomic local persistence;
`ds-client-core` owns an authorized saved-transformer property repair and its
exact readback. A workflow composes the same functions through the existing
Rust DAG runner. No repair step runs a design process or silently normalizes
retained data.

Native layer imports and `Workspace::write` replacements also use the shared
[design import admission](../../../../../../ds-network/docs/contracts/design-import-admission.md).
They validate incoming geometry and identities and preserve complete approved
rows before persistence. Explicit typed edits, restores and selected repairs
remain distinct deliberate operations. This does not claim that Brain's generic
authorized full save distinguishes import intent from deliberate authoring.

## Retained facts and authority

`design::decode_retained` admits a snapshot without assigning IDs, filling
transformer references, changing network settings or applying creation
defaults. It validates the existing snapshot schema, explicit EPSG:4326,
FeatureCollection/Feature/property/geometry shapes, source digests, bounded
counts and duplicate typed top-level identities. Legacy missing identities
and ownership remain observable facts; a property mutation separately needs
a unique stable identity. Numeric GeoJSON IDs retain their original JSON
value, including integers larger than JavaScript's exact integer range.

Every repair request binds `Scope {project, principal, lane, audience}`. The
pure engine compares these exact strings; an authenticated host must also
authorize them. A workflow file is never authentication. Local snapshot
revisions use `design::digest` over the complete native snapshot. Server
fences are a separate nonzero `server_version` and RFC 8785
`layers_content_digest` over the actual layer map. They must be supplied
together and cannot substitute for the native snapshot revision.

An `EvidenceSource` records exact layers, their native content digest,
scope, source identifier, optional historical revision, optional raw-file
SHA-256, explicit CRS and per-field units. The host that reads a file checks
its raw-byte pin. The pure engine checks the retained layer digest and binds
the complete evidence envelope into the proposal. This records provenance;
it does not infer that an import caused a defect. Geometryless table records
are admitted for key comparison. Geometry evidence needs explicit
EPSG:4326, valid coordinates and no implicit reprojection.

## Read-only audit and comparison

Audit accepts an explicit policy for fields, references and connectivity and
returns exact finding totals plus bounded details. It names malformed
geometry, duplicate aliases, incompatible declared values and conflicting
transformer ownership. Missing stable identities, unresolved references,
cross-transformer point co-location and unreachable line components are
diagnostic candidates. They grant no deletion, deduplication, reconnection or
customer reassignment authority. Two meters may share coordinates.

Connectivity uses exact recorded line vertices and explicitly declared roots.
An existing tapping pole may be a root. Without an explicit complete-source
claim, the result states `connectivity_source_incomplete`; it does not call a
component orphaned. This bounded diagnostic is not a replacement for the
network engine's topology analysis or report preflight.

The caller chooses one match operation:

- `exact_position` requires a common explicit EPSG:4326 and explicit `xy` or
  `xyz` dimensions. Point coordinates must equal exactly; no nearest search,
  tolerance, movement or inferred elevation is admitted. Equivalent JSON
  numeric spellings such as `31` and `31.0` describe the same coordinate.
- `keys` uses 1–16 explicit source/target property pairs. Scalar strings,
  numbers and booleans retain their JSON types. Missing/null keys are
  unmatched. Repeated keys on either side make all records of that key
  ambiguous; source order never chooses a winner.

Both operations report matched, unmatched and ambiguous source/target
populations independently of the detail limit. Key matching works without
geometry, independent of feature identities or positions. Restoring types by
position does not imply balancing quantities, changing counts or approving
unmatched records.

## Proposal and selected projection

`propose` takes one optional pinned evidence source, the actual retained
target, a target fence, a declared field policy, a per-layer field allowlist
and explicit operations. `restore` maps source properties to target
properties after the chosen match. `assign` names an independent set of
stable target identities and explicit values. This permits a separately
reviewed approval scope that includes records without historical matches.

A cell is tagged `{ "kind": "missing" }` or
`{ "kind": "value", "value": ... }`; explicit JSON null remains distinct
from absence. A missing source property leaves the target unchanged unless
the mapping explicitly sets `remove_if_source_missing`. Units must match the
declared source and target policy; conversions require a separate operation.

Each changed cell records target layer/identity, original feature digest,
field, exact before/after cells and any exact source row/feature/field
evidence. No-op changes are omitted. Two operations addressing the same
changed target cell refuse. The proposal binds the complete request, source
digest, match counts, ordered changes and full projected target revision.

Mutation identities use `id`, `properties.id`, `properties.doc_id` or
`properties.feature_id`. The selected raw path/value is retained for server
transport. No ordinal identity is synthesized. Every alias on a selected
feature must be unique across the layer, including aliases at different
paths on other features. Conflicting aliases or duplicate match evidence
refuse the complete proposal.

`apply_selected` requires explicit confirmation, the proposal digest and a
nonempty unique list of proposed change IDs. It independently recomputes the
whole proposal from the supplied actual source and target; recomputing a
forged proposal's hash does not make it admissible. It checks scope,
revisions, feature digests, before cells and the selected list before
projecting any effect. Undoing only the selected cells must recover the
complete original snapshot exactly as JSON values. Geometry, feature order,
counts, identity, source inputs, network settings, locks and every
non-selected property remain equal. The pure result says `persisted: false`.

The persistence owner must atomically fence and save the selected projection,
then perform exact saved readback. A retry is bound to its operation/request
digest. Cancellation before that explicit effect boundary is non-mutating.
Repair has no process, resize, renumber, rebuild, phase rebalance, blanket
demotion, hidden default or report-generation call. Report preflight remains
required after a successful repair.

## Closed property authority

Caller policies may narrow these fields and domains; they cannot expand this
native catalogue. Geometry, identities, ownership, locks, provenance and
unknown fields are never repairable through this operation.

| Layer | Admitted engineering properties |
| --- | --- |
| `tr` | `plan_kva`, `tr_size`, `transfo_size`, `main_cable_size` |
| `lv_poles`, `tapping_poles` | `struct_type`, `material`, `assembly_type`, `stay`, `flying_stay`, `earthing` |
| `lv_poles` | `existing_status` (lifecycle status: bounded text or null, never a number) |
| `lv_lines`, `service_cables` | `cable_size`, `cab_size`, `service_cable_size`, `length` |
| `customers` | `meter_type`, `category`, `connection_pole_id`, `connection_override_pole_id`, `service_cable_size`, `peak_power_w`, `num_houses` |

These layers also admit an explicit `drafting_status: "approved"` assignment;
demotion, deletion or null approval is refused. Engineering quantities and
ratings require finite nonnegative numbers no greater than 9,007,199,254,740,991;
`num_houses` requires an integer. Engineering categorical fields admit bounded
strings of at most 2,048 UTF-8 bytes without control characters, or such
numbers. Engineering absence/null is preserved or explicitly
repairable only when the reviewed field policy permits it. Legitimate zero
ratings stay zero; repair does not invent planned supply.

## Shared descriptors and proof

`describe()` returns versioned descriptors for `design.repair.audit`,
`design.repair.compare`, `design.repair.propose` and
`design.repair.apply-selected`. Closed input and typed output JSON schemas
are generated from the same serde types used by `evaluate`; schema references
are expanded for the existing workflow normalizer. The projection operation
is pure. Saving a project remains an authorized host effect.

Admission bounds are 64 MiB per retained snapshot, 32 MiB per evidence source,
100,000 aggregate matching/audit records, 512 KiB per policy/request, 16 MiB
per proposal, 5,000 changed cells and at most 500 detailed findings or
differences. Full counts remain visible when details are omitted. Matching
indexes each side once; selected projection indexes each touched layer once.

`tests/governed_repair.rs` proves retained-read parity, typed IDs/keys,
approved-data preservation, subset selection, explicit approval scope,
missing/null distinction, duplicate ambiguity, stale/forged evidence,
confirmation, catalogue and units/CRS refusals, valid tapping roots,
shared-coordinate negative controls, bounded full counts and descriptor/
dispatcher parity. Workspace/client owning tests prove persistence and
transport separately; the pure engine does not claim those effects.

## Native persistence proof

`ds-client-core/tests/client_contract.rs` exercises the property-only wire, one saved readback, exact retained layers, corrupt readback and scope refusal. The owning suite passed 262 unit, 198 client-contract and 19 journal tests. `ds-design-workspace/tests/repair.rs` proves SQLite CAS, replay, historical readback, preservation, atomic invalid selections, pending outbox and zero processing jobs; the complete workspace suite also passed. Timings are synthetic regression timings, not project performance claims.

The publication boundary performs no preliminary saved-room download: the server transaction independently verifies the exact version/content/preimages. Only a successful operation receipt admits one exact saved readback. A failed write therefore performs no transformer download.
