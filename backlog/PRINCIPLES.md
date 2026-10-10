# Principles

The standing owner rulings for everyone who works the backlog. [OPEN.md](OPEN.md) holds the unfinished code, [OPEN-X.md](OPEN-X.md) the owner decisions, [SEEDING.md](SEEDING.md) parameters, seeding and operational acceptance, [ORDER.md](ORDER.md) the order in which slices start, and [FEEDBACK.md](FEEDBACK.md) a snapshot of unclosed feedback. A ruling lives here once; the lists point to it rather than repeat it. Change a ruling only on the owner's word, with its date.

## 0. The backlog is declarative (owner, 2026-10-07)

- The backlog states only what is known **not** to work, or not to exist yet. It never claims what was fixed: no "fixed in", "already landed", "done" or progress notes.
- An entry that is handled is removed at once, the same way official feedback is closed. The commit and the owning tests are the record; the backlog is not.
- Anything can come back. A user, a `ds` interaction, a feedback report or a discovered bug that shows the problem again puts a new entry in, stating what is broken now. That is normal, not a failure.
- Write each entry as the current unmet condition: what is observed broken or absent, and what would show it works.

**What not to do — claims still in the backlog (found 2026-10-07).** These entries claim fixes, landings or passed proofs. The next coding agent that reaches one verifies it against current `run` and installed `ds`: if the problem is gone, delete the entry; if not, rewrite it as the current unmet condition. Then delete its row here. When the table is empty, delete the table.

| File | Entry | The claim |
| --- | --- | --- |
| OPEN.md §16 | Vector runner | "The kernel cause is fixed (`535a50a`)" |
| OPEN.md §23 | One Rust communication authority | "The transfer protocol landed 2026-09-10" |
| SEEDING.md §1 | Scoped backlog read deployment | "the already-landed caller-authority fix" |
| SEEDING.md §1 | Transformer tags in tiles | "The source changes are recorded on `run`" |
| SEEDING.md §6 | Nyamagabe dense-neighbor LV delivery | "install the already-landed angle-symbol/neighbour fixes" |
| SEEDING.md §6 | Northern Hub LV delivery | "install the already-landed empty-table fix" |
| SEEDING.md §8 | Canary store and Northern Hub reconciliation | "install the landed reclaim fix" |
| SEEDING.md §12 | Saved scaffold trust states | "Install the landed native preview/WASM correction" |
| SEEDING.md §13 | Installed spotting progress and Windows budget | "install the landed stage/pass/alignment progress" |
| SEEDING.md §14 | Large-model edit lifecycle acceptance | "Source guards and hot-reload tests already exist" |
| SEEDING.md §14 | Paired selection acceptance | "Deploy the landed `79ceb421` lasso fix" |
| SEEDING.md §14 | Paired geometry authoring acceptance | "proofs are retained in their receipts" |
| SEEDING.md §16 | Cache catalogue installation and Desktop acceptance | "Installed stable already reports all 44 held rows ready" |
| SEEDING.md §16 | Screen style and Windows replay | "Install the landed PNG-stream read retry" |
| SEEDING.md §17 | Authenticated Server and Dashboard acceptance | "Published Web source/asset checks already passed" |
| SEEDING.md §17 | PM events and ownership acceptance | "Backend notices and project-event outbox are already deployed" |
| SEEDING.md §17 | Remaining Assets formats | "… already have receipts" |
| SEEDING.md §21 | Installed PLS readiness | "install the landed provider-metadata/INI string projection" |
| SEEDING.md §21 | Installed native PLS controls | "verify the landed guarded attach" |

## 1. The backlog split (owner, 2026-10-07)

- **OPEN.md** — unfinished code that an agent can implement and prove without further owner input: product defects, missing native CLI/MCP capabilities, UI bindings, test repairs and integration.
- **OPEN-X.md** — unresolved owner decisions, authority and supervised steps, each naming the exact input and the boundary it controls.
- **SEEDING.md** — template building and seeding that require **no code change** (owner, 2026-10-07): templates, parameters, style documents, engineering/data seeds and library curation through existing `ds` contracts, plus deployment, install and acceptance of already-landed code. Any part that needs a code change moves to OPEN.md.
- Section numbers keep the former work-domain identity across the three files.

## 2. How to work an entry

- **Schema 5 is the final breaking `.dsgrid` schema bump** (owner, 2026-10-07). Carry future fields and external-library references compatibly. The canonical model determinism test guards this ruling.
- **Implement seeding and templating controls before curation** (owner, 2026-10-07): routine seed parameters, eventual sanitization and template choices must not stall authorized code behind more owner questions. Implement and prove the headless contracts, then seed and sanitize through them. If native PLS-CADD evidence is needed, provide the exact artifact, pins and check for the owner’s Windows Codex to prove; finish independent code first.
- **Use the live `ds` contract before coding.** First try the admitted project-owned parameters and seeds. A valid configuration ignored by the renderer, an absent required capability, incorrect computation, clipping/overflow or a broken contract is code (OPEN.md); a colour/pen/label/title/extent choice is seeding (SEEDING.md). Do not alter engineering facts to improve a picture.
- **Recheck legacy claims** against current code and tests before implementing.
- **Delete proven fixes permanently.** Keep evidence in owning code, contracts, tests or project receipts, with no completed-work archive.
- **Split mixed obligations.** Finish the independently actionable code and link only its unresolved owner decision in OPEN-X.md.
- **Do not park routine work as owner questions:** implementation, tests, already-authorized features and work awaiting another host are not owner questions. If existing contracts or evidence settle a question, proceed and remove that dependency.
- **Never-landed work** (owner, 2026-10-02): finished work found on a branch or tree that never reached `run` is landed adapted to current `run`.
- **One slice at a time per repository**, proven, committed and pushed on `run` before the next.

## 3. Priorities (owner, 2026-10-07)

1. **`ds` reliability reporting and proactive feedback** (OPEN.md block 25) is a priority.
2. First-release work: clear the reproduced CLI source gates, complete the provider-model library lifecycle (block 18), integrate the existing PLS-CADD controls (block 21), and move touched UI controls to the kernel with JSON input/preview and replacement-code deletion (block 23).
3. Fix measured renderer and compute failures.
4. Unrelated tuning and native candidate qualification continue through SEEDING.md.

[ORDER.md](ORDER.md) turns these into a start sequence.

## 4. Report as you go (owner, 2026-10-07)

**Coding sessions:** agents solve findings or record unfinished work only in the owning backlogs; they never submit feedback during coding (owner, 2026-10-07). Use OPEN.md for actionable code, OPEN-X.md for owner decisions or authority, and SEEDING.md for existing-code operational work. Remove entries after proof. Do not file a feedback report for work the coding agent is responsible for solving.

Outside coding sessions, product-use sightings follow [ds-cli-reliability-and-feedback.md](../docs/contracts/ds-cli-reliability-and-feedback.md). The product's own SRE reporting remains separate from a coding agent's backlog work.

## 5. Kernel, CLI and UI (owner, 2026-10-07)

- **Migrate touched controls for the first release.** Work one owning vertical at a time; a polished UI can follow the native contract and a usable JSON input/preview. Trace the explicitly named consumers rather than starting an all-repository rewrite.
- **Complicated TypeScript goes to the CLI; its UI becomes JSON.** When a UI rests on complicated TypeScript logic, migrate that logic into the kernel and CLI/MCP, then delete the whole UI and replace it with a JSON input editor and a JSON output view over the same CLI contract. Losing UI elements in favour of the CLI is perfectly acceptable: the product comes from the future and there is no going back.
- **Never destroy validation paths.** Keep, and keep working, every view that renders results for a person to check — Markdown, HTML, PDF, XLSX, images and maps, and the like. Keep the capacity to download those results from the UI as well; downloading the same artifacts through the CLI is welcome, but it does not replace the UI download. Removing a control is allowed; removing the way a person sees, verifies and downloads a result is not.

## 6. DS Grid engineering

- Windows owns DS Grid engineering; the server session integrates the other surfaces.
- Measure → test → fix → remeasure on exact reference revisions.
- Preserve engineering authority and record numbers as owning tests — never per-model constants or widened tolerances.
- Owner, 2026-10-07: engineering calculation is intentional. Edits accumulate without calculation or Save. Atomic localized work comes first: reuse unchanged alignments and terrain, update only affected authored geometry and tension-section results, including adjacent incident forces at shared supports. Stop edit propagation at affected tension-section ends; wider propagation is explicit reconciliation. Retained RAM is an accelerator, not a substitute for bounded computation. Preserve the same engineering results.
- Owner, 2026-10-08: **an edit lands whole.** Dragging, retyping or otherwise editing in the Profile, or the same edit through the CLI, ends with the Profile reconciled — wires, clearances, usage and markers current — without a click. A half-landed edit (moved pole, stale wires, "calculation required") is a defect. The reconcile is differential: at most the alignment where the edited structure belongs, re-solving its affected tension sections, with no terrain work unless terrain changed and never every alignment. This supersedes "edits accumulate without calculation" above for the Profile after an edit; Save stays explicit.
- Owner, 2026-10-08: treat the design as a transactional database, not a file to rewrite or transport after every operation. Native write sets, dependency indexes and exact heads determine changed-record computation, query, projection and persistence. Arrow is the typed storage/query representation, not transaction machinery. Save commits changed units and reuses unchanged objects; complete portable export is separate. Ground editing is deliberate. Typed plural alignment membership bounds work; principal/T-off relationships permit only actual local dependencies. Independent alignment families have separate bounded surfaces, with no interpolation across their intervening space. See [Profile as a transactional database](../../ds-network/docs/contracts/profile-transactional-database.md).

## 7. No single-country assumptions

Owner: "it will be adopted internationally, so no hardcoding at all." No fallback may turn one country's input into another's; code must not know rule-set names (the live Chad rule set is `rule_tchd`). The queued slices are in OPEN.md block 19.

## 8. Never a worktree unless the user controls it (owner, 2026-10-07)

Agents never create, code in or land from a git worktree or feature branch. Code directly in the canonical checkout on `run`: make the smallest coherent change, prove it, commit and push `run` before the next one, staging only your own paths and preserving other agents' uncommitted work. The only exception is a worktree the user creates and controls; an agent works there only when the user says so, and the user decides when it lands and when it goes.

Why — the painful history:

- On 2026-10-04 there were **719 unique commits**, made since 2026-09-01, that had never reached `run`. Finished features were lost in forgotten trees again and again, among them the Profile weather case, the usage labels and the dynamic combined model. The "Never-landed" entries still in OPEN.md are what was recovered from them.
- Forgotten trees were the largest thing on the box: five already-merged lanes held **194 GB**; deleting them took the disk from 83% to 63%.
- Every later investigation re-read those trees, mistook them for live work and paid again to settle what was already settled.
- A registry of trees (`git_trees/gittrees-index.json`, 2026-10-04) did not stop the loss. On 2026-10-05 the owner ruled: code directly in each canonical checkout on `run`, no worktrees, no feature branches. A commit on a branch counts as zero; only `run` counts.

## 9. Desktop tiling and data sources (owner, 2026-10-09)

Owner revision, 2026-10-10: retire and archive ds-vector-tiler entirely after
replacing its needed non-tiling consumers and removing deployment bindings.
There is no standalone vector-tiling API and no current custom dataset/BigQuery
tiling or explorer. ds-network-reporter is the single Rust compute container
and native job host. Tiling is a background derivative of complete survey
acquisition and combined-report preparation, cloud and local, using the same
embedded native engine and central verified publication. Whole survey downloads
on a PC retain reusable source data and schedule local tiling; whole combined
report preparations reuse their acquired design data. Report success is independent
of background tile success. Reports must never wait for tiling, tile uploads,
tile retries or publication: a separate durable job consumes retained report
artifacts after report completion. Cloud simple reports remain available to ordinary
authorized users; cloud combined/compounded report generation is developer-only.
Local/headless Server combined reports remain available under existing project
permissions. Published reports and tiles remain downloadable centrally. Future
short-lived custom containers and governed expert dataset jobs are outside this
migration; do not retain old APIs or hidden fallbacks for them. This supersedes
the 2026-10-09 bans on every cloud report and cloud tile job. The separately
paused stable release remains paused.

- Windows and Linux Desktop must generate survey, design and personal/project
  tiles locally through shared native Rust, hosted by ds-network-reporter.
  The selected dependency is pinned VersaTiles Rust libraries behind
  ds-tile-runtime; Martin and a bespoke engine are not alternative tracks.
  Prove actual Windows operation and explicit DS data/zoom/budget policy; no
  silent tile/feature dropping or fallback to an unselected engine.
  Replace Tippecanoe and external PMTiles conversion; remove ordinary Web tiling
  controls and standalone tile dispatch. Web retains simple report generation,
  developer-admitted combined reports, map, status and download reads.
- Survey reads use governed BigQuery snapshots. Current saved design authority
  remains Firestore and DS Grid; BigQuery is not a design transaction gate.
  This corrects the earlier blanket prohibition on Firestore for design.
- Complete unfiltered survey loads and all-transformer report preparation may
  automatically tile stale complete snapshots. Readers can contribute local
  compute; shared publication remains authenticated, revision/coverage/lease
  fenced. Partial visibility never authorizes a project-wide replacement.
- Reusable Parquet/GeoParquet snapshots serve local analytics and tiles. Keep
  Polars first; no DuckDB adoption without measured need and a later decision.
  Future large cross-project BigQuery tables mirror full design snapshots with
  project IDs and tags, never recover canonical data from display tiles.
- Freshness uses cheap maintained revision/coverage metadata. Survey/design
  refresh reads only cursor-bound changes/deletions and reuses unchanged disk
  chunks. Work areas are local masks over retained snapshots; cached-area changes
  must not redownload source data or stale complete project tiles. Acquire only
  missing coverage, preserve atomic revisions and prove request/byte economics.
- A PC contributes compute to a project; it is not a personal project asset
  or owner of results. Sync Center explains this role. Credentials/queues stay
  principal-fenced. PCs never serve tiles; consumers read central publication.
  Claim/coalesce automatic compute before expensive work across contributors.
- Cut cloud cost and unnecessary reads as measurable latency/UX requirements.
  Remove standalone tile generation and obsolete custom BigQuery tiling/explorer
  paths; cloud report-integrated derivatives use the same native reporter owner. Future expert container/custom-job APIs are outside this migration.
  Implement/prove minimal central source, lease and publication coordination. The [implementation plan](../docs/desktop-native-tiling-migration-plan.md)
  defines the serial slices and whole-product acceptance.

## 10. Cloud cost and service ownership (owner, 2026-10-10)

The intended compute/control service families are `ds-brain`, `ds-solar` and
`ds-network-reporter`, including their admitted lane/bulk variants. Remove
`ds-buildings`, `ds-data`, `ds-form-factory`, `ds-geo-exchange`, `ds-print`,
`ds-solar-weather`, `ds-system` and `ds-vector-tiler` after replacing required
consumers and reconciling invocation grants. Retire their deployments, jobs,
triggers and local repositories with preserved Git history; retain no dispatch
fallbacks. Source deletion alone does not establish live retirement.

Ordinary Brain targets 128 MiB for bounded authority, signing and commits;
unsafe work belongs behind bulk or in the native compute pool. Bulk retirement
and direct BigQuery-to-GCS survey delivery are deferred. Reports never wait for
tiling, and the cloud/native report permissions remain governed by §9.

No polling: intentional navigation, explicit refresh, canonical mutations and
actual push drive application data reads. Preserve account-wide notifications
across project navigation and one resident Desktop notification owner with
peer delivery. Reduce redundant calls without weakening authorization,
credential expiry or durable delivery. Requests are not evidence of hallucinated
calls without caller/action attribution. Measure request frequency, billed
reads, compute and latency per successful operation before claiming savings;
the Brain usage matrix is [the baseline](../../ds-brain/docs/operations/cloud-request-frequency.md).
Remaining implementation and operational proof belong in OPEN §22/§17 and
SEEDING §22, not another savings TODO file.
