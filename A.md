# A — next-session architecture and removal contract

Owner directive: stop implementation and further code scanning. This document
records the required destination and the known limits of the completed review.
Read it before resuming Design or generalized reconciliation work. It is a
contract, not evidence that the whole system already satisfies it.

## 1. Definition of completion: replace and remove

Adding Rust is not a completed migration. The corresponding production
TypeScript decisions, transformations, state transitions and computational
fallbacks must be removed when the Rust owner takes over.

Every future slice must report:

- Production TypeScript lines before and after, lines deleted, lines added and
  net change, with exact files and the baseline commit.
- The named behaviors transferred to Rust and the obsolete functions,
  fallback branches, stores and callers deleted.
- Remaining TypeScript adapters, each justified by a host effect: rendering,
  event capture, browser storage, transport, workers or platform interaction.
- Generated bindings, tests and documentation counted separately. Formatting,
  renamed files and generated-code changes cannot be sold as domain removal.
- Evidence that both UI and CLI call the replacement and the previous domain
  implementation is no longer reachable.

A green Rust test suite alone does not close a migration. Do not retain a
TypeScript implementation as an emergency or compatibility compute fallback.
If the Rust/WASM owner is unavailable or incompatible, return an explicit
unavailable/refused outcome. Do not silently execute the old implementation.
An explicit supported Rust cloud/edge placement is a separate matter: it is
part of permanent hybrid computation, not permission to revive TypeScript.

### What was actually measured this session

The last recorded `ds-web` commit, `860dd735`, removed 51 lines and added 82
across code, tests and documentation. That is **not a verified net production
TypeScript reduction count**. Subsequent error-presentation fixes added code.
No full removal inventory was completed; do not claim otherwise.

Source-selection precedence was moved into the shared Rust kernel. The broader
UI edit room, settings transformations, scheduling and persistence were not
fully migrated. The new native workspace therefore proves a headless slice,
not that the existing UI and CLI already use one complete lifecycle owner.

## 2. One command center, one semantic contract

UI and CLI must invoke the same named command with the same versioned domain
parameters. Neither may independently decide defaults, source precedence,
selection semantics, revision rules, edit behavior or reconciliation policy.

The command center owns semantic admission, state transitions and execution
planning. It calls the owning domain implementation; it must not absorb network
physics, report formatting or every domain's algorithm into a monolithic crate.

Host adapters may translate actual file handles, browser blobs and credentials
into admitted immutable input references. They may not change the meaning of
the command. A map selection is a way to supply explicit feature identities or
geometry, never hidden process input or project authority. Scope is `project`;
map state is presentation and interaction context.

Command contracts must distinguish domain parameters from execution context:

- Domain: object identities, immutable input revisions, explicit source
  manifests, settings/configuration revisions and the requested operation.
- Execution: host capabilities, selected placement, resource budget, progress,
  cancellation and the applicable policy/grant revision.
- Result: typed outcome, input fingerprint, implementation identity, artifacts,
  revision changes and pending publication/reconciliation state.

An operation ID identifies one immutable request. A retry cannot substitute
newer input, different settings or a different semantic operation. New work
requires a new identity. Do not expose generic engine opcodes, arbitrary URLs,
SQL, shell arguments or caller-supplied transport bodies as this interface.

## 3. Permanently hybrid computation

Cloud-native capability is permanent. This work must not dismantle it.
Browser-to-cloud and desktop-to-edge are intended placements. Browser WASM and
desktop-native offline execution remain supported where the operation's
declared capabilities and available inputs permit them.

One command contract must survive placement changes. Algorithms and report
production stay separate from IO. Local inputs, acquisition, artifact storage
and publication differ by host; the meaning of the engineering operation does
not. There is no duplicated client/cloud decision authority.

For Solar, `ds-solar-weather` belongs to the IO boundary. Weather preparation
changes rarely across repeated city iterations, so its output is a reusable,
versioned input to computation, not a step to rerun on every design change.
Pin the weather dataset identity, digest, spatial/temporal coverage and
preparation version in each run's input manifest. Reuse the locally available
matching input offline; refresh or replace it explicitly when its coverage or
version must change. Acquisition and weather preparation remain outside the
city computation and report-production contracts. A missing or incompatible
weather input must be reported explicitly, never silently downloaded,
substituted or regenerated. This is the intended ownership boundary, not a
claim that the current implementation was inspected or changed.

Placement is explicit, policy-controlled and visible in the receipt. A failure
must not silently change placement, algorithm, input snapshot or credentials.
If policy permits another placement, that transition must be a declared,
observable step with preserved request identity and validation requirements.

Independent work may run concurrently within a shared CPU and memory budget.
Avoid independent nested pools each claiming all processors. Progress must
describe actual stages/jobs; cancellation and crash recovery must have explicit
durable semantics. Do not promise FPS or throughput improvements from the
language change alone.

## 4. Generalized authenticated reconciliation

Reconciliation must become one durable, centralized protocol and state-machine
pattern across Design, Solar, Survey, settings, layers/styles, portfolios,
reports and attachments. These are required consumers, not a claim that their
current implementations were reviewed or already unified.

Centralization means one shared client/runtime reconciliation contract and one
governed server admission pattern. It does not mean shared raw database access,
a generic write endpoint, or putting every domain's merge rules in one module.
Domain owners retain typed validation, authorization requirements and semantic
conflict resolution.

Required properties:

1. Local changes, their immutable payload references and their outbox intents
   commit atomically. Process death cannot save a change without its intent or
   acknowledge an upload whose durable local state was never committed.
2. Work is fenced to project, principal, credential audience and deployment
   lane. No ambient service account, ADC, borrowed browser credential or
   caller-supplied project label grants remote authority.
3. Local content hashes and server revisions are distinct. Every remotely
   mutable object carries its applicable server base revision. Local history
   alone cannot fabricate that base.
4. Delivery is retryable and idempotent. The protocol must tolerate duplicate,
   delayed and out-of-order delivery. A repeated operation with changed input
   conflicts. Do not claim exactly-once networking.
5. Dependencies are explicit: referenced object/source revisions precede the
   result or attachment which depends on them. Partial completion is recorded,
   not flattened into success. Batches return bounded per-item outcomes.
6. Artifacts are immutable, content-addressed and size/digest verified. Upload
   completion and governed publication are separate states. Missing media must
   be explicit; it cannot silently disappear from a delivered report.
7. Acknowledgement names the exact admitted operation, input revision and
   artifact set. Clearing an outbox entry requires that matching receipt.
8. Conflicts preserve local work. Geometry/topology and engineering settings
   cannot default to last-writer-wins. Each domain declares valid automatic
   merges and the conflicts requiring human resolution.
9. Tombstones, retries, leases, worker claims, interruption, cancellation,
   credential expiry and authorization refusal have explicit durable states.
   Queue inspection remains possible without contacting the network.
10. Compaction and garbage collection require retained acknowledgement/history
    evidence. Pending inputs and artifacts cannot be pruned because a UI closed
    or an upload attempt returned a transient error.

The eventual shared envelope should carry schema/operation identity, project
and actor binding, object identity, base revision, input fingerprint, dependency
references, policy revision and typed domain payload reference. Its exact wire
schema and owning shared native crate remain to be settled from the actual
consumers in a later authorized session. Do not invent that implementation now.

## 5. Product governance, licensing and kill switches — required on resumption

This is committed next-session scope, deferred only because this session ran
out of resources. It is not an optional future feature. When work resumes,
implement it alongside the incomplete contracts below.

The owner retains permanent control of installed desktop products and their
capabilities. Policy must be able to disable a product/version, operation or
compute placement independently of releasing another desktop.

The Governance tab must expose an authorized installation and licensing
inventory: stable installation/device identity, product/build, operating system,
organization/project association, license and entitlement status, installation
and last-seen times, and where the application is installed. Distinguish
installation path and organization/site from reported geographic location;
record provenance and freshness, and leave unavailable metadata unknown.
Track current and past authenticated users on each device, with dated
associations, and bounded usage patterns such as feature use, sessions, compute
placement and failures. Define collection scope, access, retention and audit
contracts; inventory must not imply live knowledge of an offline device.

Governed commands must support license assignment/revocation, user blocking,
device or installation disabling, minimum supported versions, forced-upgrade
requirements, and product/operation/placement controls. UI and CLI must use
the same command contract. Record actor, reason, policy revision, target scope,
requested action and device acknowledgement; distinguish requested, delivered
and enforced controls. An upgrade requirement must preserve drafts and pending
reconciliation, and explicitly block governed work if a required upgrade cannot
be applied. Scope user blocking separately from a shared device's other users.

Policy/grant identity, verification, expiry, offline validity, revalidation and
revocation behavior must be explicit. Already disconnected devices cannot learn
about a new server revocation immediately; document the maximum permitted
offline window rather than promising both indefinite offline authorization and
instant remote shutdown.

Define an enforceable maximum offline stay using signed, bounded offline
grants and explicit expiry/clock-rollback behavior. Expiry blocks new governed
work according to policy while preserving local data and the recovery/sync
path. Renewal and reconciliation must use authenticated transport; access
revocation must have a defined recovery-only path rather than stranding data.

Transformer deletion requires acknowledged synchronization to the Recovery
Center, including transformers created and designed entirely offline. An
offline delete may only atomically retain the recoverable transformer revision,
required inputs/artifacts and a pending deletion intent. It may hide the item
from the active view with a visible pending-recovery state; it must not purge
the payload. Publish creation/history and recovery evidence in dependency order
before finalizing deletion. Only an authenticated acknowledgement identifying
the exact recoverable revision and recovery record may unlock final deletion
under retention policy. A transport success alone is insufficient. Retries,
restart, upgrades, license expiry and user blocking must preserve that evidence.
The contract governs application deletion and garbage collection; it cannot
promise recovery after an offline device is lost or its storage is externally
erased before any synchronization.

Separate permission to perform new governed work from preservation of local
data and queued evidence. Define read/export/recovery behavior deliberately.
Do not erase drafts or falsely acknowledge pending work when a switch changes.
Implementation waits only for the owner's instruction to resume; the scope
above is already requested and must not be deferred again as speculative work.

## 6. Offline reporting and Design acceptance

The first native slice must remain able to create/import a transformer,
resolve explicit local inputs, edit, process, produce a report/printable PDF,
retain artifact bytes and queue reconciliation without a map or Desktop.
Project configuration, libraries, templates, fonts, images and reference data
must be explicit local inputs when an offline operation requires them.

Printable PDF generation and physical printer spooling are different effects.
The current slice proves the former, not platform printer integration. Missing
references, media and configured formats must produce visible blockers. A
partial report is never a complete delivery.

Future assisted transformer/LV drafting may use roads, buildings, contours and
imagery as versioned evidence. It must produce a reversible proposal and
reviewable changes while UI observers can follow progress. Human validation
remains the final decision. Routing inference and DSGrid physics accuracy are
outside the present implementation scope.

### Transformer location consistency, version comparison and retrieval

Reserve a focused implementation slice on resumption for version comparison
and retrieval. The owner reports that their contract is strong but largely
unimplemented; this session has not investigated that implementation status.

A transformer name is a display label, not proof of object identity or spatial
continuity. Preserve stable transformer identity and immutable revision IDs,
and record replacement/import lineage explicitly. Each design revision must
retain its transformer location, coordinate reference system, declared bounded
design area (including a rectangle where supplied), source/input fingerprints
and the revision used for processing. Do not invent a declared design boundary
from the current viewport or silently overwrite historical spatial evidence.

Comparison must identify relocation, replacement under the same name, departure
from the recorded design area, and mismatch between a transformer location and
the inputs or results retained with it. Compare normalized coordinates with
explicit units and a versioned tolerance policy. Record before/after locations,
displacement, boundary relationship, affected revisions/results and the rule
that raised the inconsistency. Missing identity, CRS or boundary evidence must
produce an explicit unknown, not a clean comparison. An intentional relocation
may be valid, but acknowledgement alone cannot make old results current.

Expose these findings in Design status and version comparison through the same
kernel result for UI and CLI. Preserve detection and resolution history; a
user's acknowledgement or declared replacement reason must not erase the
original evidence. Geometry overlays are an optional view of the comparison,
not a requirement to detect or retrieve it headlessly.

Retrieval must select an exact immutable revision and return its authored
geometry, settings, pinned inputs, provenance and associated result references,
with missing local dependencies reported explicitly. Historical retrieval must
not modify the current head. Restoring a revision creates a new revision with
recorded lineage and an expected-head check; it must not rewrite history or
silently reuse results computed for another location/input fingerprint.

Acceptance must include a transformer designed inside a declared rectangle,
then replaced at an unrelated location under the same name: retain both
versions, expose the spatial discontinuity and stale-result relationship,
retrieve either exact version offline when its data is local, and preserve
the finding through reconciliation. Also distinguish a policy-tolerated small
coordinate correction, an explicitly acknowledged relocation, and insufficient
spatial evidence. Isolate this work when resources return; no implementation
or further scanning is requested now.

## 7. Known state and uninvestigated weaknesses

Known from this session:

- `ds-command-kernel` commit `33a8cae` adds a native offline Design workspace,
  version-fenced edits, captured Rayon jobs, durable report bytes and an outbox.
  The outbox explicitly reports `pending_transport`; authenticated Design
  reconciliation is not implemented there.
- `ds-cli` commit `ced5674` exposes that slice and pins the kernel/browser
  repository revisions. The existing UI edit room still has another lifecycle.
- Production native PDF rendering already exists in the reporter despite an
  older document describing printing as experimental. Export policy can still
  refuse a requested format, which the offline adapter must preserve.
- The initial source-settings comment contradicted actual precedence. Missing
  configuration was also being turned into absence. The Rust resolver and
  browser error handling address this narrow case.
- Tests exercised local history, competing writers, retry identity, interrupted
  jobs, captured inputs, native processing, background launch and actual
  XLSX/PDF generation. Those tests do not prove the entire hybrid architecture.

Solar portfolio batch command boundary — implemented:

- `src/solar_batch.rs` owns the strict `ds-solar.portfolio-batch/v1` request,
  ordered catalog membership checks, bounded scheduling, exact source reuse,
  lifecycle, cancellation acknowledgement/retry and interruption recovery.
  `solarPortfolioBatch` exposes the same command through WASM. Unknown fields,
  invalid settings and unavailable hosted overrides are refused.
- UI and `ds solar portfolio batch start --request` submit identical required
  parameters to one effect host. Status/cancel share the same kernel state.
  The old single-portfolio CLI spelling is a one-row request adapter; its
  separate completion/replay coordinator and the UI's worker loop are removed.
  The remaining TypeScript performs IO, durable storage, events and rendering;
  it does not replace the Rust scheduler if WASM fails.
- Owner/project-fenced state is persisted before effects. Batch ids cannot be
  replayed. Queued work stops on cancel; native cancellation retries until
  acknowledgement without overlapping requests. Committed results win races.
  Restart inspection marks unfinished work interrupted and never auto-runs it;
  sealed per-row artifacts remain readable/exportable by their exact run ids.
- Calculation and processor allocation remain in `ds-solar`. This change does
  not make the paired batch effect host a standalone headless daemon. Native
  batches use prepared offline inputs; the browser retains hosted calculation.
  Hosted active cancellation and custom source/draft overrides are unavailable
  and explicitly identified, not emulated with TypeScript. GCP was not tested.
- Verification covers real WASM scheduling, source ordering, membership pins,
  independent failures, queued/native cancellation, duplicate ids, recovery,
  UI terminal-state retention and CLI parity. A live dev CLI batch completed
  offline; a second batch cancelled during source calculation without starting
  aggregation. CLI artifact reads address the successful row directly. The dev
  build's missing exact release manifest still prevents publication admission;
  local artifacts remain valid and this change does not weaken that check.
- Production TypeScript/Svelte accounting for this slice in `ds-web`: 424 lines
  removed, 311 added, **113 fewer** overall (tests and Rust excluded). This
  includes the new IO interpreter and wire types, not just deleted schedulers.

Potential weak parts, **not investigated further**:

- Solar processor utilization still needs sustained-load and Cloud Run quota
  testing. A focused local follow-up moved city batches, nested charts and
  portfolio drafts onto one shared Rust CPU pool, enabled optimized development
  calculation/PNG builds, and added concurrent portfolio UI runs. On the local
  16-logical-CPU workstation, the seven-city run with charts completed in 6.4 s
  (an earlier local run took roughly 14 s), with a sampled native peak near
  15 core equivalents. This is a local observation, not a controlled cloud
  benchmark or a promise of continuous 100% utilization. Concurrency-invariant
  outputs and the cloud adapter's compilation were checked; actual cloud
  execution, memory pressure and sustained concurrent-request behavior remain
  unverified because GCP billing is unavailable. Continue with pinned inputs,
  per-stage timings and container CPU/memory limits when resources return.
- Duplicate TypeScript settings, selection, edit, scheduling and state logic
  may remain reachable despite equivalent Rust behavior.
- Domain-specific outboxes may disagree on identity binding, retries,
  dependencies, acknowledgement, conflict handling or cleanup.
- Existing Solar/Survey reconciliation may be reusable, or may encode domain
  assumptions that prevent safe generalization. No reuse claim is made.
- Source manifests, form/library/configuration versions and report media may
  be incompletely pinned across some paths.
- Browser and native/cloud execution may not yet agree on every default,
  refusal, cancellation behavior or result revision.
- Main-thread work, full-snapshot copies, nested concurrency, cache lifetime
  and oversized intermediate results may still limit performance. The native
  workspace has input/result bounds, not a proven operating-system memory cap.
- Native resumption currently hashes the host executable. This is strong build
  fencing but can be costly for large debug binaries and may be too coarse for
  future upgrade-compatible recovery.
- Offline media availability, signed references, printing configuration and
  physical printer integration may prevent a real project's complete offline
  delivery even when the basic report/PDF flow succeeds.
- Lease expiry, multi-device geometric edits, actor changes, tombstones and
  policy revocation may expose unresolved reconciliation cases.

**Resource limitation:** I did not have resources to investigate these possible
weaknesses. The owner stopped further scanning at the usage limit. They are
hypotheses for later verification, not confirmed defects or completed fixes.

## 8. How to resume

Treat this document as the next implementation agenda. Required closure items,
based on the known state above and without a new investigation this session:

1. Integrate and stabilize the pending changes after verifying their actual
   state; do not mistake a local commit or passing slice for a shipped feature.
2. Unify the UI edit lifecycle and CLI behind kernel commands, remove covered
   TypeScript, and publish exact production deletion/addition/net counts.
3. Replace Design's `pending_transport` boundary with authenticated durable
   reconciliation and generalize the protocol for the named domain consumers,
   preserving their domain-specific admission and conflict rules.
4. Implement the Governance tab, licensing/installations inventory, policy
   enforcement, maximum offline stay and Recovery Center deletion gate in
   section 5. These are required work on resumption, not distant follow-ups.
5. Close the full offline reporting/printing boundary and consistent hybrid
   execution contracts; verify missing media, physical spooling, interruption
   and refusal behavior rather than treating PDF generation as total closure.
6. Verify the suspected weaknesses in section 7 within bounded slices and
   close confirmed gaps. Their implementation status remains unverified; this
   list does not turn hypotheses into findings.

First reconcile the actual Git/worktree state and the latest known commits;
this document is not a release receipt. Some final stabilization changes were
still in progress when the owner stopped implementation. Do not assume all
work was integrated, pushed, installed or deployed.

Begin with the user's current token budget and choose one bounded removal /
stabilization slice. Establish its TypeScript baseline and consumers, implement
one shared command contract, switch both UI and CLI, delete superseded logic,
and prove the removal count and behavior. Generalize reconciliation through
that same architecture without weakening domain ownership or permanent hybrid
compute. Stop claiming completion at the exact boundary the evidence supports.

Do not resume implementation, auditing, kill switches, deployment or broad
scanning merely because this document exists. Wait for a new owner instruction.


/////////////////////
human observations

network template settings should be offline first - that is it. offline first. understandable.
LV poles assembly in project network settings is always dangling as unsaved in an unjustifiable way. 
and Tchad_rule is not correctly using the tchad assemblies. check the seeding for the current project and tell me how to reconcile. 
Transfoemrs page - navigation to it is not feelign like offline first. it takes tooooo much - normally it is better to navigate instantly and then fill the gaps with background sync. reconsiliation should not make it feel slow and should be not that costly.

## Implemented offline version slice (2026-09-05)

`design_versions` owns native/WASM capture, immutable room digests, fenced
local catalog commits, retrieval and Data Factory (`ds-geo`) comparison. The
UI and paired CLI call the same kernel path. Dirty/local-only transformers
can be versioned without cloud registration or status refresh. Project settings
are captured when cached; missing settings, stable identity, CRS and source/result
fingerprints remain missing evidence, never invented defaults.

Versions retain independent layer, metadata and optional project-config rooms.
Local identities are `local-<digest>`; display order is not a server `vN`.
Computed comparisons retain exact content pins, metadata differences and bounded
review collections, can reopen offline, and project only into temporary layers.
Named maps were explicitly deferred by the owner. This does not restore or edit
a historical head.

Synchronization is **pending_transport**, not delivered: the existing server
version-create endpoint snapshots server head and cannot acknowledge offline
bytes. Implement an authenticated exact-content admission/receipt lane before
draining these intents. Do not replay create_version later against changed
server head. Native workspace persistence is not yet connected to this browser
archive; the CLI still uses the paired application's storage.

Retention ruling: synchronization does not evict survey or local version data.
Project storage UI reports indexed record counts and browser app usage/quota;
it does not claim physical free disk measurement. Category cleanup currently
removes explicitly downloaded cloud version copies only. Broader category
inventories/cleanup need reviewed reference and pending-intent protection.
