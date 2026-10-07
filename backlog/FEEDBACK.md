# Unclosed feedback

Snapshot of every report `ds feedback list --view not_addressed` returned on 2026-10-07: 246 unsettled (127 open, 119 waiting on a named dependency). The feedback API is the authority; regenerate this file after closing or noting reports.

Reports are grouped by the live repository that owns the code, then by component. The component is the submitter's free-text label, shown verbatim so `ds feedback list --component` finds it. Legacy labels map as follows: `ds-grid*`, `dsgrid*`, `ds/grid-*`, `ds/pls-library` → ds-network; `ds-report*`, `ds-printing*` → ds-network-reporter; `ds-desktop*` → ds-web (`src-tauri`); `ds pls desktop` → ds-cli (`ds-cli-pls/desktop`). Labels that name no repository stay **unassigned**. "Backlog" names the [OPEN.md](OPEN.md), [OPEN-X.md](OPEN-X.md) or [SEEDING.md](SEEDING.md) section citing the report; **untracked** means none does (128 reports).

| Repository | Total | Blocker | Major | Minor | Waiting | Untracked |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| [ds-cli](#ds-cli) | 92 | 11 | 61 | 20 | 49 | 60 |
| [ds-network](#ds-network) | 70 | 17 | 49 | 4 | 20 | 23 |
| [ds-network-reporter](#ds-network-reporter) | 28 | 1 | 21 | 6 | 15 | 17 |
| [ds-web](#ds-web) | 36 | 7 | 26 | 3 | 26 | 17 |
| [ds-command-kernel](#ds-command-kernel) | 4 | 0 | 3 | 1 | 1 | 1 |
| [ds-brain](#ds-brain) | 6 | 0 | 5 | 1 | 4 | 3 |
| [ds-apis-tf](#ds-apis-tf) | 1 | 0 | 1 | 0 | 1 | 0 |
| [ds-vector-tiler](#ds-vector-tiler) | 1 | 0 | 1 | 0 | 1 | 0 |
| [unassigned](#unassigned) | 8 | 0 | 5 | 3 | 2 | 7 |

## ds-cli

| Component | ID | Sev | Kind | Title | State | Backlog |
| --- | --- | --- | --- | --- | --- | --- |
| ds pls desktop (windows native controller) | `34b98f7e` | minor | idea | Add a characterized 'cable edit' workflow (open, set creep model and shift, save) with its dialogs catalogued | open | **untracked** |
| ds-cli | `d4dacd51` | major | obstacle | Fill-in/upgrade report 2026-09-30: combined ds CLI/MCP gaps (transformer summary, tags, spatial reads, Drive writes, survey, assets, feedback) | waiting: installed kernelf6fedb3 readback for item6; other grouped CLI/MCP acceptance still pending | **untracked** |
| ds-cli | `46dec14e` | major | friction | ds caps that force manual manoeuvring for one 10k-point layer: map draw max 10000 features, data inspect --rows max 5000 with no paging, prepared local layers invisible to the running app | waiting: SRE mode: addition parked until the owner says go | OPEN.md §16 |
| ds-cli / design lv | `c89fea1b` | minor | obstacle | No ds door extends an LV feeder to reach a customer beyond max_service_length | waiting: owner decision to add LV pole/span authoring during stabilization | OPEN-X.md §15 |
| ds-cli / dsgrid (project design basis) | `6febdb37` | major | idea | Project design basis: one governed, versioned record of a project's criteria, standards, library and client rulings | open | **untracked** |
| ds-cli / dsgrid structure types | `96c9ca0c` | major | idea | Structure-type lifecycle: rename, retire unused types, set-label map on retype, full name inventory | open | **untracked** |
| ds-cli / dsgrid-exchange | `4af28da3` | minor | bug | Model 2 package cannot export: 'preserved PLS source version could not be inspected' | open | OPEN.md §11 |
| ds-cli / mcp.install | `97c38bf9` | minor | idea | MCP install for a remote ds over ssh | open | **untracked** |
| ds-cli / printing setups | `9e388eeb` | major | friction | Plan & profile context styles must be sealed through the LV transformer printing selection | open | **untracked** |
| ds-cli / report project map-inputs | `86d184cf` | major | obstacle | Project-scale context acquisition fails for buildings, village/cell boundaries, national LV/MV poles and settlements | open | SEEDING.md §5 |
| ds-cli / style print create | `efd915a6` | major | bug | style print create gt/rivers refused with an opaque auth_input_invalid | open | **untracked** |
| ds-cli design config / ds-brain global template | `1f2430c7` | major | obstacle | No ds command reads or saves the global network template (eds_networks/master) | open | **untracked** |
| ds-cli dsgrid-exchange / dsgrid model link / pls desktop | `e7059959` | blocker | obstacle | No governed route from DS typed retypes back into the native PLS-CADD workspace when the origin folder digest differs (restored copy) | open | **untracked** |
| ds-cli pls desktop drivers / dialog catalogue | `4115583d` | blocker | bug | PLS-CADD desktop drivers refuse on the startup 'About PLS-CADD' box (not catalogued); autosag fails at launch | waiting: Windows release containing ds-cli b0c31944 or later, then MAIN's autosag run on a host that shows the box | **untracked** |
| ds-cli/auth | `dba03da7` | major | obstacle | auth_transient storm: map-inputs and dsgrid project download hang 10-30 min then refuse | open | **untracked** |
| ds-cli/data | `d4ae9a9f` | minor | obstacle | Raster and image analysis have no registered CLI contracts | open | **untracked** |
| ds-cli/data-elevation | `829e8ddc` | major | bug | ds data elevation attach always refused: desktop_refused 422 mismatched native elevation evidence (Desktop writes a correct result) | waiting: owner push (land-train.sh) + desktop release; then replay the six-point attach with --fallback none | **untracked** |
| ds-cli/data-elevation | `ed8b6d68` | major | bug | Stable Rwanda elevation attach rejects supported request with opaque evidence mismatch | waiting: owner push (land-train.sh) + desktop release; then replay the 373-point attach | **untracked** |
| ds-cli/data-terrain | `62b63e0e` | major | obstacle | Expose original Rwanda DEM cells for terrain import without interpolation | waiting: deployed headless original DEM-cell extraction with NoData coverage | **untracked** |
| ds-cli/data-vector | `91a7d505` | major | obstacle | Expose native polygon intersection and union for headless map authoring | open | **untracked** |
| ds-cli/design | `6ed42257` | blocker | obstacle | No headless way to save a supplied LV design as a new transformer version without reprocessing it | waiting: headless exact supplied-layer save implementation and Northern Hub byte comparison | OPEN.md §9 |
| ds-cli/design | `0988aebe` | major | obstacle | Skill gap: no geographic MV-transformer vs LV-design check; M2 model lacks transformer identity; task geometry cannot hold two routes | open | **untracked** |
| ds-cli/design | `83f3712b` | major | friction | Supplied shapefiles with 10-character DBF field names are not mapped back to canonical LV fields | open | OPEN.md §9 |
| ds-cli/design | `7f5ac170` | major | obstacle | No migration from the deprecated struct_type 'Existing' marker to existing_status across a project's transformers | open | OPEN.md §9 |
| ds-cli/design transformer + ds-brain transformer lifecycle | `81ae43eb` | major | obstacle | LV transformer lifecycle lacks CLI parity with DS Grid models: explicit --project, verified backup on retire, restore of deleted | open | OPEN.md §7 |
| ds-cli/design-lv | `c7664961` | major | bug | design lv project-save refuses an unchanged transformer with transformer_save_source_digest_mismatch | waiting: deployed fix and Agahehe project-save/status acceptance proof | **untracked** |
| ds-cli/design-lv | `f7732067` | major | bug | Headless LV project export replaces approved Unicode attribute characters | waiting: Original reporting-host reproduction and raw failing request matched to the exact saved snapshot; serialization/publication corruption is no | SEEDING.md §9 |
| ds-cli/design-migrate | `09de401e` | minor | bug | design migrate plan reports would_replace for 75 transformers that migrate apply --overwrite just copied | open | **untracked** |
| ds-cli/desktop | `641143eb` | major | bug | Desktop targeting: map design rejects --target that refusals recommend; idle desktop instances accumulate (14 running), need min-1 autoscaling lifecycle | waiting: owner push + release (part 1); owner decision (part 2) | **untracked** |
| ds-cli/desktop-adapters | `bdf57fcf` | major | bug | Exported native report adapter refuses an existing PLS frame as missing | open | **untracked** |
| ds-cli/desktop-lifecycle | `30d8149c` | minor | friction | DS CLI lacks desktop start and restart lifecycle commands | open | **untracked** |
| ds-cli/desktop-routing-and-skills | `ec6d6ea5` | major | friction | Clarify project state versus Desktop bridge instances and deduplicate stale sessions | open | **untracked** |
| ds-cli/desktop-sync | `fd5bce6a` | blocker | obstacle | Gisagara A0 contour reports cannot replace shared project publications | waiting: shared publication verification / Desktop sign-in and prior HTTP 401 work grant | OPEN.md §8 |
| ds-cli/ds-grid | `88c4cade` | major | idea | DS Grid needs chainage-based integer structure numbering | open | **untracked** |
| ds-cli/dsgrid | `e4d5b306` | major | obstacle | Retained criteria material-history selectors are inaccessible through installed DS reads | open | **untracked** |
| ds-cli/dsgrid-exchange | `f27c69f1` | blocker | bug | Model 2 FINAL exports as verified but PLS-CADD 16.81 merges 76 of 709 sections (ends on non-dead-end sets) and re-sets 15 transformer terminations | waiting: code:ds-network PLS-CADD DON section projection plus Windows PLS-CADD 16.81 restore/open acceptance | OPEN.md §11 |
| ds-cli/dsgrid-exchange | `7ce604c5` | major | bug | PLS export of Model 2 FINAL refuses on duplicate weather labels (High wind x2, EDT x2): criteria self-read 7 rows, expected 9 | open | OPEN.md §11 |
| ds-cli/dsgrid-exchange | `8f6e3ff1` | major | bug | Alignment-revised .dsgrid cannot return to PLS-CADD: export refuses on pre-existing PLS-POLE 20.01 library members, sync has no writer for the edit class | waiting: Codex lane: DS Grid to PLS-CADD return path | OPEN.md §11 |
| ds-cli/dsgrid-project | `ead11b30` | major | bug | dsgrid project download hangs indefinitely for a 1.3 MB canonical model (no receipt, no timeout) | open | **untracked** |
| ds-cli/dsgrid-replace-structure | `31498e75` | major | bug | replace-structure refuses a real IBC PLS-POLE 20.01 T-off that the PLS workspace import accepts | open | **untracked** |
| ds-cli/dsgrid-report-staking | `b78f77f0` | major | obstacle | Staking loses transformer designations when a model is re-imported from PLS-CADD | waiting: staking designation carry implementation and 33-row round-trip proof | **untracked** |
| ds-cli/grid-corrections | `43dee9a0` | major | bug | Guarded corrections reject editable profile fields without a characterized scope | open | **untracked** |
| ds-cli/grid-geojson | `fee42145` | major | bug | Exact-revision MV alignment GeoJSON export panics for a validated GIS-imported model | waiting: owner push (land-train.sh) + release | **untracked** |
| ds-cli/library | `defcbef3` | major | obstacle | Global catalog artifacts need typed verified download | waiting: Matching release installation and global library artifact immutable-coordinate/integrity acceptance | **untracked** |
| ds-cli/library | `46f54c67` | major | obstacle | Northern Hub MV member codes cannot be verified from project library inspection | open | OPEN.md §5, SEEDING.md §5 |
| ds-cli/migration | `fb90a0a5` | minor | obstacle | The two migration endpoints diverge in six cosmetic ways; an agent that learned one cannot drive the other | waiting: ds release carrying ds-cli b8813c9 | **untracked** |
| ds-cli/native-pls-controller | `e40195d1` | minor | idea | Native controller needs an audited owner-supplied allowable-span limit edit | open | **untracked** |
| ds-cli/pls | `fe345c40` | major | obstacle | PLS clearance diagnostics require Word even when PDF is unnecessary | waiting: owner-deferred PLS reporting slice after native Grid engineering comparison | SEEDING.md §21 |
| ds-cli/pls | `a87ef925` | major | idea | PLS native lab: PLS-POLE family authoring, Lite-free wire loads, Method 1/2 generation, cable authoring, library manifests and safe-dialog rules belong in ds pls / MCP | waiting: owner-scoped PLS native lab implementation and native 16.81 parity gates | **untracked** |
| ds-cli/pls | `1d73747d` | minor | idea | CLI needs exact native backup member recovery | waiting: Owner-native backup-member operation exposed through CLI/MCP and deployed Windows positive/negative proof | **untracked** |
| ds-cli/pls | `a2f2ae44` | minor | friction | PLS clearance report cannot target the open workspace | waiting: owner-deferred PLS reporting slice after native Grid engineering comparison | SEEDING.md §21 |
| ds-cli/pls-desktop | `6fe791ca` | blocker | bug | Windows readiness receipt serializes filesystem metadata and stalls | open | **untracked** |
| ds-cli/pls-desktop | `2790bfeb` | blocker | bug | pls desktop autosag/sheets refuse on the 'About PLS-CADD' startup modal that restore dismisses | open | **untracked** |
| ds-cli/pls-desktop | `c0491da0` | major | bug | PLS restore of a DS export into a native shell stops on Sheet Generation Problem (profile station -16.2); deliver cannot set paging without Word | open | **untracked** |
| ds-cli/pls-desktop | `cedcfd10` | major | obstacle | PLS desktop verbs: Word gate blocks RTF reports; About box stops autosag/check; no attach to an open project | waiting: later owner-prioritized Windows native acceptance and release of the source branch | SEEDING.md §21 |
| ds-cli/pls-desktop | `4b8a4a84` | major | bug | Windows PLS diagnostic fails before readiness assessment: Import-PowerShellDataFile unavailable | waiting: desktop bootstrap fix and affected Windows host acceptance | **untracked** |
| ds-cli/pls-desktop | `1086e058` | major | bug | PLS restore reported 88 restored but only 10 files remained in target folder | open | OPEN.md §11 |
| ds-cli/pls-desktop | `6f346aa3` | major | bug | PLS desktop restore stops at catalogued Save changes exit prompt | open | OPEN.md §11 |
| ds-cli/pm | `b14558ea` | major | obstacle | No project chat: task assignments, acceptance and closures cannot be announced to every project member | waiting: authenticated project task-event and personal-notification delivery/readback proof | SEEDING.md §17 |
| ds-cli/pm-email-assets | `68b6c668` | major | idea | Email-to-project-work should preserve the whole conversation and explicit message-to-attachment provenance | waiting: conversation ingestion and attachment identity implementation | OPEN-X.md §Parked: |
| ds-cli/project settings + network templates + global libraries | `34a69541` | major | idea | Store per-model design rules (staking/spotting/catalog) in project settings seeded from the network template, by blob reference; plus cloud DS Grid libraries | open | **untracked** |
| ds-cli/report | `f48bd93c` | blocker | obstacle | report project map-inputs cannot target an isolated server state (no --server-state-dir); hangs while the default canary store is under recovery | waiting: product:bounded-map-inputs-source-reuse-and-refusal | OPEN.md §5 |
| ds-cli/report | `74bdd848` | blocker | bug | Architecture bug: headless report export produced 113 valid reports that the cloud never saw (report stale, report_has_files false, Combined Report empty). There is no local-only compute: every artifa | waiting: Desktop + ds release carrying kernel 2997236; acceptance re-run on a Desktop | OPEN.md §7 |
| ds-cli/report | `e3aa68bd` | major | bug | Northern Hub canary still refuses legacy survey media in transformer report export | open | **untracked** |
| ds-cli/report | `290bf404` | major | idea | Atomic MV booklet printing from project JSON instead of ds-work script | waiting: implementation: shared MV booklet setup consumer | OPEN.md §5, SEEDING.md §5 |
| ds-cli/report | `8de3e8e0` | major | friction | Batch transformer export needs a declarative JSON job for layout, naming and packaging | open | OPEN.md §7 |
| ds-cli/report | `09b78d74` | major | bug | Local report export cannot resolve print styles created after the installed build (gt/cell_boundaries_print, gt/kivu_lake_print, master/drafting_errors_print ...) although ds style list shows them | waiting: stable release with ds-cli d7c1c5e + kernel f121c26 | **untracked** |
| ds-cli/report project export (batch) | `444234de` | major | bug | report project export over many transformers stalls after the first one (idle on one socket, no timeout) | open | **untracked** |
| ds-cli/report-delivery | `da000524` | minor | friction | Export delivery needs a JSON manifest with hash verification and destination receipts | open | OPEN.md §7 |
| ds-cli/report-mv-printing | `be769552` | major | obstacle | MV re-issue on stable: plan-profile refused without MV print setup, canary cannot decode stable packages, GIS drops transformer elevations | waiting: release (item 2); owner decisions (1, 3, 5, 6); item 4 open | **untracked** |
| ds-cli/report-plan-profile | `5c722c70` | minor | bug | plan-profile booklet: north arrow and legend drawn over structure labels | waiting: plan-profile renderer fix and sheets 46/92 visual proof | **untracked** |
| ds-cli/report-project | `69f60e82` | major | friction | Combined report waits on slow serial publication of all transformer reports | open | OPEN.md §7 |
| ds-cli/report-project-export | `a65872eb` | major | bug | Project report export drops verified native saved voltage-drop analysis | waiting: owner push (land-train.sh) + release | **untracked** |
| ds-cli/report-reconciliation | `d798669f` | major | obstacle | Headless report publication requires an unexplained project Desktop sync bridge | waiting: release; owner decision on Windows-native report reconciliation | **untracked** |
| ds-cli/report.export | `349c3130` | minor | friction | Local combined/transformer report export refuses project print selection and project-export's non-canonical layers | open | **untracked** |
| ds-cli/report.project.export | `17b05932` | major | bug | report project export refuses 23 re-saved transformers: saved content digest does not match fetched layers | waiting: verification:replay-all-23-resaved-transformers-on-r77 | SEEDING.md §7 |
| ds-cli/reports | `874b5212` | blocker | obstacle | Local DS report exports cannot feed canonical combined report while publication queue stalls | waiting: Implement and release verified local report-batch combined control, or install schema-2 client and prove bounded outbox publication | **untracked** |
| ds-cli/reports | `6a9d0db4` | major | obstacle | Headless CLI cannot save per-transformer A0 title-block overrides | open | **untracked** |
| ds-cli/server-sync-store | `269f81f3` | blocker | bug | Canary Server sync store becomes malformed with unlinked live WAL and SHM | waiting: Install and replay reclaim after storage expansion; reconcile default WAL/store and online heads without loss; regenerate current Northern H | SEEDING.md §8 |
| ds-cli/skills | `00a49c20` | minor | idea | Skill proposal: ds-existing-network-project-data - merge as-built and earlier-lot shapefile dumps into comparable source-tagged layers, validate, upload as project data | waiting: SRE mode: addition parked until the owner says go | **untracked** |
| ds-cli/skills | `15b36787` | minor | idea | Skill proposal: ds-mv-lv-orphans - MV alignments reaching no design transformer and transformers with no MV model, judged per alignment, answered as prepared local layers | waiting: SRE mode: addition parked until the owner says go | **untracked** |
| ds-cli/skills | `23de5782` | minor | idea | Skill proposal for the ds bundle: ds-project-scope-cleanup | waiting: SRE mode: addition parked until the owner says go | **untracked** |
| ds-cli/skills | `e6a0751f` | minor | idea | Skill proposal for the ds bundle: ds-lv-misprojected-room-healing | waiting: SRE mode: addition parked until the owner says go | **untracked** |
| ds-cli/skills | `c7dcd5ee` | minor | idea | Skill proposal for the ds bundle: ds-lv-findings-healing | waiting: SRE mode: addition parked until the owner says go | **untracked** |
| ds-cli/skills | `9924576b` | minor | idea | Skill proposal for the ds bundle: ds-headless-gotchas | waiting: SRE mode: addition parked until the owner says go | **untracked** |
| ds-cli/skills (ds-project-work, ds-boq-staking-table, ds-grid-project-model, ds-printout) | `190d8eaf` | major | friction | Canonical MV deliverables list (staking = XLSX only, never printed) is not visible to submission skills | waiting: owner push (land-train.sh) + release | **untracked** |
| ds-cli/style | `14aec381` | major | obstacle | Web-map styling is fixed-scale: no zoom-stepped overlap/size, no headless categorical colour, Desktop ignores headless publish, capture os error 32 | waiting: Remaining categorical/zoom/halo styles, PLS editors, casing/live-tile order, invalidation/defaults; affected Windows camera/zoom/UI/capture  | SEEDING.md §16 |
| ds-cli/survey | `cf92af6c` | major | obstacle | No way to measure staked-out LV line length by district or sector: survey aggregates count only, entries carry no admin fields, and geometry selection has no pagination | waiting: SRE mode: addition parked until the owner says go | OPEN.md §16 |
| ds-cli/survey | `e6b8070d` | major | idea | Design triage: filter or export the survey records (customers or any form) not yet covered by design clusters, so the next transformer to design is visible at a glance | waiting: SRE mode: addition parked until the owner says go | **untracked** |
| ds-cli/work | `e01cf456` | major | obstacle | PM: creating a free-standing task with two attachments costs 8 commands across two domains and still fails; a task needs its own cheap attach route (work task create --attach), not the design assets l | waiting: SRE mode: addition parked until the owner says go | OPEN.md §17 |
| ds-cli/workstation | `f73043ec` | major | idea | Allow moving the whole DS working data folder to an external disk for low-storage machines | open | **untracked** |
| ds-cli/workstation/chromium | `f88d24ad` | major | bug | Windows workstation browser verification rejects installed functional Chrome and Edge | waiting: Windows release and Chrome/Edge configure+verify acceptance with notepad negative control | **untracked** |

## ds-network

| Component | ID | Sev | Kind | Title | State | Backlog |
| --- | --- | --- | --- | --- | --- | --- |
| ds-grid-engine/alignments | `8363a4fc` | blocker | bug | Reversing an MV alignment needs a governed section rebuild and attachment-set remap | waiting: integration/deploy:ds-network 9afcd2f, then Model 2 exact-set restring and fresh PLS-CADD 16.81 Restore/Open with zero phase prompts; Model  | OPEN.md §12 |
| ds-grid-engine/pls-exchange | `b3bdc857` | blocker | bug | Native PLS imports fail portable capacity verification; inline steel-to-suspension handoff blocked | open | OPEN.md §11 |
| ds-grid-engine/pls-export | `c496feb0` | blocker | bug | PLS export splits continuous OPGW runs at suspension poles, causing native section merges | open | OPEN.md §12 |
| ds-grid-engine/pls-export | `b1a2b25c` | major | obstacle | Imported Model 1 cannot return to PLS: 13 untouched thin definitions block export | open | OPEN.md §11 |
| ds-grid-engine/pls-workspace-sync | `2b8cd4ae` | major | bug | Linked workspace sync rejects a single supported structure retype as whole_snapshot_design | open | OPEN.md §11 |
| ds-grid-engine/spotting-import | `ec73ef89` | major | friction | PLS import lacks whole-model spotting readiness metadata | waiting: engine update and owner strategy decision | OPEN.md §11 |
| ds-grid-engine/spotting-performance | `6b597153` | major | friction | Bounded A28 spotting exceeds 26 minutes without progress or plan | open | OPEN.md §13, SEEDING.md §13 |
| ds-grid/exchange | `91331096` | blocker | bug | 0.1.4 still cannot export Nyamagabe Model 1-B to PLS-CADD .bak: Method-2 signed wind cases not representable in CRI | waiting: engineering: characterize and prove separate signed Method-2 CRI cases in PLS-CADD 16.81 before changing the representability gate | OPEN.md §11 |
| ds-grid/exchange | `3724e4ce` | blocker | bug | dsgrid-exchange pls-bak export fails on canonical Gisagara Model 1: cable acsr 70-12mm2 member does not read back | open | OPEN.md §11 |
| ds-grid/exchange | `732fcd12` | blocker | obstacle | PLS-CADD 16.81 export refuses a model with two criterion sets, and no ds command removes one | waiting: Characterize and prove a PLS-CADD 16.81 per-case signed wind-direction serialization and round-trip mapping for Method 2; current characteri | OPEN.md §11, OPEN.md §15, SEEDING.md §15 |
| ds-grid/profile-atlas | `d8863709` | major | bug | Profile atlas threads off-centreline terrain points into the printed ground line (zigzag) | open | OPEN.md §12 |
| ds-grid/project-models | `86314bfb` | major | idea | Split one DS Grid model into several project models by a rule (admin bounds or custom) | open | **untracked** |
| ds-grid/project-models | `244906e8` | major | idea | Projects with more than one DS Grid model need an automatically combined, always-current model | open | **untracked** |
| ds-grid/spotting | `50ab45f5` | major | idea | Post-processing loop after the DS Grid limit: rule-based S325->S255 reduction and NG healing with native AutoSag/report rounds | waiting: DS Grid versus PLS engineering comparison and owner decision on any further PLS-specific workflow | OPEN.md §13 |
| ds-grid/spotting | `7a78525b` | major | friction | A complete whole-model spotting receipt cannot be applied when its plans carry many rejection rows (16 MiB params cap; preview-receipt refuses untruncated receipts) | open | OPEN.md §13 |
| ds-grid/staking | `2dda2cc4` | major | bug | Staking leaves transformer kVA blank when only the LV design transformer (not a surveyed one) sits at the MV transformer structure | open | **untracked** |
| ds-grid/standards-compiler | `7bf636c2` | major | bug | Standards compiler refuses verified current resource when its leaf has retained variants | waiting: owner push (land-train.sh) + release | **untracked** |
| ds-grid/structure-capacity | `4ff54666` | major | bug | screen_structure_usage reports 334,508-651,484% usage for canonical l-s-S800-stay.012 (stayed steel) - capacity artifact unusable | waiting: library: characterize canonical l-s-S800-stay.012 and signed longitudinal demand cases, then re-screen the exact Model 2 structures | OPEN.md §12 |
| ds-grid/terrain | `8a8b86ae` | major | idea | No governed command to fill a profile ground-survey gap from the governed DEM tied to the survey | open | OPEN.md §12 |
| ds-grid/topology | `41176f22` | major | friction | Nothing flags an MV alignment that runs backwards (station 0 at the transformer, end on the existing tap) | waiting: author typed source/tap/existing and transformer/terminal role evidence at PLS import or provide governed existing-MV relation; then test B  | OPEN.md §11, OPEN.md §12 |
| ds-network / ds-grid-engine capacity | `df1f53ae` | major | bug | Every Method-2 interaction artifact of Model 2 is unverified, so no steel pole can ever be capacity-screened | open | OPEN.md §12, OPEN-X.md §12 |
| ds-network / ds-grid-exchange pls_struct translator | `5d0fd02b` | major | bug | Imported .012 span-limit tables carry an uncharacterized between-angle policy, so a Method-2 steel pole is unplaceable off a knot | open | OPEN.md §13, OPEN-X.md §13 |
| ds-network / ds-grid-exchange workspace export | `90cf2311` | minor | bug | Whole-model PLS workspace export of Model 2 refuses on earthing before any cable or capacity edit can be proved | open | OPEN.md §11 |
| ds-network / dsgrid cable definition + dsgrid-exchange pls export | `6226c0bb` | major | obstacle | Cable creep as a temperature shift (client: 15 C) cannot be authored in DS; export writes the proportional creep model with no shift | open | OPEN.md §12 |
| ds-network / dsgrid cable model | `3d1c735c` | major | obstacle | Cable creep temperature shift has no canonical field; nominal modulus disagrees with curves and GTP | open | OPEN.md §12, OPEN-X.md §12 |
| ds-network / dsgrid design policy | `778bb573` | blocker | obstacle | author_design_policy requires structure duty profiles whose row shape is undiscoverable; Model 2 cannot get the project policy | waiting: owner: confirm nyamagabe-mv policy for Model 2; profile j-w-60d-S255.012 and j-w-S255.012; finish Model 2 clearance/cable/capacity prep | OPEN.md §13 |
| ds-network / dsgrid structure staking facts | `dacfa967` | major | obstacle | No typed command to correct a transformer's load reference (kVA) or other staking facts; network_role shape undiscoverable | open | SEEDING.md §15 |
| ds-network / retype | `d8529cff` | minor | friction | Retype keeps material-specific staking facts (E-STEEL on a wooden H-pole) | open | OPEN.md §13 |
| ds-network / shared nyamagabe structure library | `3acfd343` | major | bug | 15 shared-library capacity tables admit no straight line; a-w-S190.012 and a-w-S255.012 author one row at 120 deg | open | OPEN.md §13, SEEDING.md §13 |
| ds-network / spotting apply | `fb163694` | major | obstacle | Respot apply creates single poles in the forbidden 10-60 deg band and sections without a criterion set | open | OPEN.md §13 |
| ds-network/ds-elevation-host | `f6c511f5` | major | obstacle | Independent Rwanda TIFF and Terrarium comparison unavailable headlessly | open | **untracked** |
| ds-network/ds-grid-engine | `50024ab0` | blocker | obstacle | Model 2 30 kV criterion leaves A12 clearance repair without capacity-case proof | open | **untracked** |
| ds-network/ds-grid-engine | `4ed6b202` | blocker | bug | Plan-profile print omits connected T-off branches unless map context is captured | open | OPEN.md §11 |
| ds-network/ds-grid-engine | `ef2478d4` | blocker | bug | Spotting clearance proof uses one alignment ruling span, while AutoSag uses each section ruling span | open | OPEN.md §11 |
| ds-network/ds-grid-engine | `34ad2bea` | blocker | bug | Method-2 suspension diagrams without a negative weight-span branch pass DS but fail PLS-CADD | open | OPEN.md §11 |
| ds-network/ds-grid-engine | `54a217f4` | major | friction | Structure delete/retype in Profile must be instant: restring only the one or two incident tension sections | open | **untracked** |
| ds-network/ds-grid-engine | `97b4fb80` | major | obstacle | Engine one-shot needed: TBEA cross-arm re-issue took ~20 native PLS-CADD rounds (swap, 12 m class steps, uplift revert, strain split) | open | **untracked** |
| ds-network/ds-grid-engine | `5c7152e3` | major | idea | Plan rule-driven retypes and strain splits in DS (REG sequence, clearance, uplift, 12 m mix) | waiting: DS Grid rule-planner implementation and Model 1 acceptance | OPEN.md §13 |
| ds-network/ds-grid-engine | `3d2ddd7a` | major | idea | Compute PLS Method 2 structure usage in DS Grid (interaction diagrams, per load case) | waiting: Method 2 usage implementation and Model 1 native parity proof | OPEN.md §12 |
| ds-network/ds-grid-engine | `c6af8706` | major | question | Model 1 lateral ground point governs vertical clearance without lateral classification | open | **untracked** |
| ds-network/ds-grid-engine | `358e926f` | major | idea | Optimum spotting should run in parallel by default within a memory budget that never freezes the user machine | open | OPEN.md §13 |
| ds-network/ds-grid-engine | `908c07c1` | major | obstacle | Optimum spotting cannot use interaction-diagram structure strength: every candidate capacity_indeterminate, plan infeasible (Rev A) | open | OPEN.md §13 |
| ds-network/ds-grid-engine | `e343d5ac` | major | bug | DS stringing ignores the imported AutoSag criteria: sections ~27% tighter than PLS AutoSag, 828 section violations | waiting: Criterion-consistent stringing and native PLS-CADD AutoSag/strength acceptance for Model 2. | OPEN.md §12, OPEN.md §15, SEEDING.md §15 |
| ds-network/ds-grid-engine structure_usage + capacity | `4ba8ab15` | major | bug | screen_structure_usage takes one side at section-boundary structures: wind span ~0.5x PLS, 115 spurious uplifts (Gisagara calibration) | waiting: DS capacity evidence and complete incident-span demand are required before A17 C=1300 tension change can be accepted | OPEN.md §12 |
| ds-network/ds-grid-engine/spotting | `449b4f36` | major | bug | Spotting clearance witness uses base station despite longitudinal attachment offsets | open | **untracked** |
| ds-network/ds-grid-engine/terrain | `5acb6a57` | major | bug | Gisagara M3: off-route DEM sides create sawtooth ground and pole base heights | open | **untracked** |
| ds-network/ds-grid-exchange | `3a302e54` | major | bug | PLS export writes blank Final After Creep weather case: PLS-CADD stops on open (wc_name_to_index couldnt find weather case) | open | **untracked** |
| ds-network/ds-grid-exchange | `2c311576` | major | bug | PLS-CADD .cri cable tension limits imported ~10x too small (1042 read as N): DS sags 3.6x too slack, short spans | open | **untracked** |
| ds-network/ds-grid-exchange | `dff71def` | major | bug | Suspension structures (a-, l-...-susp) pass in uplift: PLS-CADD to DS Grid does not translate that they cannot take negative load | open | **untracked** |
| ds-network/ds-grid-exchange | `9817e17e` | major | bug | PLS export re-authors the CRI and cable files instead of carrying the imported ones; sections lose criteria | open | OPEN.md §11 |
| ds-network/ds-grid-exchange pls-cadd .bak export | `9f2316b3` | blocker | bug | Root cause 7a836b7a: DS-authored project .str (no FILENAME) and .cri (FILENAME=.\) make PLS rebase paths from ., yielding <root>012 | open | OPEN.md §11 |
| ds-network/ds-grid-exchange pls-cadd .bak export | `7a836b7a` | blocker | bug | DS-authored PLS .bak opens into PLS-CADD Repair Wizard: relative FILENAME=.\ member headers mis-rebased on Restore | open | OPEN.md §11 |
| ds-network/ds-grid-exchange pls-cadd .bak export | `6405185d` | major | bug | Correction to 7a836b7a: relative FILENAME=.\ headers are NOT the Repair Wizard trigger (Gisagara round-trip opens clean) | open | OPEN.md §11 |
| ds-network/ds-terrain-sampling | `c668908d` | major | obstacle | Headless terrain sampling lacks adaptive density and independent side profiles | waiting: local build gates and installed release verification | **untracked** |
| ds-network/dsgrid-exchange | `e258b287` | blocker | obstacle | PLS export plan accepts unsupported edited model but convert cannot emit backup | waiting: Expose Method-1 CRI readiness in exchange plan and resolve six weight-slot bindings before backup acceptance | OPEN.md §11 |
| ds-network/grid-model-composition | `07d0cf12` | major | idea | Dynamic ordered master DSGrid model and governed alignment movement | open | **untracked** |
| ds-network/lv-process | `4bad5ef3` | major | bug | LV processing always renumbers LV line numbers; no setting keeps supplied line_number | open | OPEN.md §9 |
| ds-network/lv-process | `38bd5091` | major | bug | LV batch process changes approved poles and cable sizes despite preservation settings | open | OPEN.md §9 |
| ds-network/lv-voltage-drop | `3c9c8add` | major | bug | VD network omits most LV sections for some saved designs; customers get 20-170 A service currents | open | **untracked** |
| ds-network/pls-roundtrip | `43c0295a` | blocker | bug | Supported PLS native Save reconciliation is missing for DS zone sidecars | open | **untracked** |
| ds-network/print-renderer | `5828fac5` | minor | bug | A0 contour index labels fail renderer with text paper wider than frame | open | OPEN.md §5 |
| ds-network/reporter | `7519c28b` | major | bug | Windows headless PDF report promotion fails with Access is denied | open | OPEN.md §21 |
| ds-network/service-connection-settings | `779a7895` | major | idea | Project service connection tolerance defaulting to plus 3 metres | open | **untracked** |
| ds-network/sizing | `dcacdb90` | minor | bug | Sizing: only the 30 kV MV fuse is ever used; mv_fuze_15_kv_amp in transfo_sizes is ignored, so 15 kV networks get about half the REG fuse rating | waiting: owner: correct the 15 kV fuse seeds + add 75/450 kVA rows; then the release | OPEN.md §9 |
| ds-network/spotting | `51d427b7` | blocker | obstacle | Nyamagabe B spotting previews leave 30 kV survey-point clearance violations | open | **untracked** |
| ds-network/voltage-drop | `00ad9848` | major | obstacle | Saved LV designs need non-destructive topology for voltage-drop reports | waiting: non-destructive saved-topology analysis and Agahehe byte-preservation proof | OPEN.md §9 |
| ds-network/voltage-drop-summary | `11bce3aa` | major | bug | LV summary reports reinforcement_required false despite noncompliance and required new circuit | waiting: owner push + release; re-adopt the governed A4 standard | **untracked** |
| ds/grid-staking | `94fc3681` | major | friction | Preserve incoming MV staking facts and apply project-owned styled delivery template | waiting: deploy:stable-ds-brain-mv-print-contract | **untracked** |
| ds/pls-library | `6b563fcc` | major | idea | One pinned Method 2 library needs shaded and centred stick variants across three MV projects | waiting: be239f38 provenance fix plus three-project pinned library parity implementation | **untracked** |
| dsgrid-exchange/pls-export | `59d38d86` | blocker | bug | PLS export ends an OPGW tension section at a suspension attachment | open | OPEN.md §12 |

## ds-network-reporter

| Component | ID | Sev | Kind | Title | State | Backlog |
| --- | --- | --- | --- | --- | --- | --- |
| ds-network-reporter | `b56c9ce3` | major | obstacle | No ds route renders the extended LV voltage-drop report (A4 PDF); only the app view prints it | waiting: deployed extended A4 PDF route and Agahehe acceptance proof | **untracked** |
| ds-network-reporter (cloud run) | `ac272198` | blocker | bug | Cloud reporter OOMs at 1024 MiB on export_transformers; forced combined over 86 transformers produces nothing | waiting: Install matching schema-2 ds release, then replay these three cloud exports and forced-compounded failure evidence | SEEDING.md §7 |
| ds-network-reporter/plan-profile (ds report plan-profile) | `7bc05e22` | major | bug | Plan/profile clearance hairs must be ground offset + the code extra (the checked clearance); they drew false violations | waiting: installed renderer and Model 1 spans 53–57 plan/profile print cross-check | OPEN.md §12 |
| ds-network-reporter/print-layout | `e3886c36` | minor | bug | Categorical LV pole legend leaves the unspecified fallback swatch unlabeled | waiting: push to origin/run and a reporter/client release | **untracked** |
| ds-network-reporter/printing | `757dfbf2` | major | bug | Print pens distinguish new MV from existing MV and keep buildings green | open | SEEDING.md §5 |
| ds-network-reporter/printing | `b4324cb0` | major | bug | Mandatory single DS GridDesign credit on all PDF pages | open | **untracked** |
| ds-network-reporter/printing | `99139c70` | major | bug | Mandatory single DS GridDesign credit on every printed page | open | **untracked** |
| ds-network-reporter/printing-context | `c7958c2b` | major | bug | Gisagara Desktop Reporter export rejects printing context: map expected string | open | **untracked** |
| ds-network-reporter/printouts | `b99bb1d1` | minor | idea | Name printouts from transformer.title() and put paper size at folder level | waiting: report packaging naming convention implemented and proven on a Stable Windows batch | **untracked** |
| ds-network-reporter/reports | `42f3abd1` | major | obstacle | Standalone combined-transformer A0/A3 PDFs are unavailable | waiting: Reporter combined task needs standalone A0/A3 PDF outputs and a governed CLI path to provide cached canonical layer projections plus network | OPEN.md §7 |
| ds-network-reporter/voltage-drop | `159282a4` | major | bug | Voltage-drop PDF export leaves reserved/refused transformers without a governed report | open | **untracked** |
| ds-network-reporter/voltage-drop a4 report | `e538e632` | minor | bug | A4 voltage-drop report title block shows project "gisagara" and carries no governed project logos | waiting: owner decision on the governed logo source; ds-brain change and deploy | **untracked** |
| ds-network-reporter/voltage-drop-pdf | `a23fa674` | major | bug | Kibungo delivered voltage-drop A4 PDF prints internal layout overflow diagnostic | waiting: reporter release plus fresh Kibungo print and G: replacement verification | **untracked** |
| ds-printing/lv | `ef954b7c` | major | bug | Standalone LV A0 print loses categorical network styling | waiting: governed layout and installed print acceptance | SEEDING.md §6 |
| ds-report / render-grid-plan-profile | `778a9f4d` | minor | bug | Plan & profile: labels after an elevation break drawn in the top band with leader lines crossing the scale bar | open | OPEN.md §5 |
| ds-report/combined | `0e431940` | major | bug | Combined report classifies LV ABC line as service cable | waiting: exact transformer and source feature/layer identity for the 56.97 m Gisagara row; DS report preview of projected combined archive unavailabl | **untracked** |
| ds-report/layout-render | `0daacb10` | minor | friction | report layout render now adds its own DS credit, duplicating a layout's attribution element | waiting: push to origin/run and a reporter/client release | **untracked** |
| ds-report/map-print | `78c0467d` | major | friction | Print labels need a declarative underscore-to-space display transform | open | **untracked** |
| ds-report/pdf-output | `e2cd543e` | major | bug | Vector-aware PDF size optimization for MV and LV print sets | open | OPEN.md §5 |
| ds-report/plan-profile | `04ced625` | major | idea | Submission version must be governed separately from model draft revision | waiting: implementation: governed submission fields in shared MV printing setup | SEEDING.md §5 |
| ds-report/plan-profile | `425425d9` | minor | friction | Plan/profile context pages are positional and revision-pinned: an MV model edit forces re-stamping unchanged LV context | open | **untracked** |
| ds-report/print-layout | `db129935` | major | bug | Human-readable balanced transformer titles on LV print sheets | waiting: Install pinned CLI/reporter and verify Northern Hub individual and combined A0 PDFs using saved layout revision 6a1f5f27 | SEEDING.md §5 |
| ds-report/print-layouts | `f41debca` | major | idea | LV transformer sheets and map prints have no black-and-white (monochrome) mode | open | **untracked** |
| ds-report/printing | `f0e9c2ed` | major | idea | Govern canonical MV booklet front matter as a project owned template | waiting: implementation: MV front matter and A3 setup in shared revision-fenced printing infrastructure | OPEN.md §5, SEEDING.md §5 |
| ds-report/printing | `a64d35e8` | major | idea | Mandatory DS GridDesign attribution on every DS PDF print | open | **untracked** |
| ds-report/printing | `40edf64d` | major | idea | Transformer PDF needs georeference and switchable layers | open | **untracked** |
| ds-report/printing | `a4113ec8` | major | bug | A0 transformer print contours are too faint and appear to disappear | open | SEEDING.md §5 |
| ds-report/voltage-drop-authority | `9172efeb` | major | bug | A4 reporter falsely rejects saved analysis when nested summary keys have different order | waiting: owner push (land-train.sh) + release | **untracked** |

## ds-web

| Component | ID | Sev | Kind | Title | State | Backlog |
| --- | --- | --- | --- | --- | --- | --- |
| ds-desktop | `d50cc39d` | blocker | bug | Edge compute without a visible, self-healing publication: 593 sealed report artifacts (379 MB) stuck in the Desktop pending-upload outbox behind a dead-process lock since 2026-09-18; nobody was told | waiting: Desktop release carrying kernel 2997236; acceptance re-run on a Desktop | OPEN.md §8 |
| ds-desktop | `40b53f7f` | major | bug | assets ingest through the paired Desktop always fails (422, browser refused the upload request without a status), so no document can be attached to a work task | waiting: the same proof ingest as e9b5858d | OPEN.md §17 |
| ds-desktop/bridge | `973c4b2e` | major | bug | Windows Desktop bridge freezes after style re-fetch: map.view stale, zoom/camera/ui time out on all instances; tile categorical fill needs a halo hack | waiting: Fresh affected Windows Desktop build, exclusive-handle test and paired camera/zoom/UI/PNG replay after style/tile refresh. | SEEDING.md §16 |
| ds-desktop/dsgrid-conflicts | `2dc20f97` | major | friction | Roomy DS Grid conflict-review popup backed by the shared CLI resolution flow | open | **untracked** |
| ds-desktop/geographic-data | `b98643ff` | major | bug | desktop data rwanda install without --resource times out at the Desktop frontend (504), so the documented default bulk path cannot complete the catalogue | waiting: Desktop release carrying ds-web 7baf29ce | OPEN.md §16 |
| ds-desktop/geographic-data | `33003988` | minor | bug | Wetlands dataset download fails with HTTP 422 after 12 attempts, then succeeds unprompted, so a one-shot install reports a false permanent failure | waiting: desktop download manager change, proven on a Windows Desktop build | OPEN.md §16 |
| ds-desktop/printing | `88c03fbd` | major | bug | Desktop printing export rejects current reporter result field | open | OPEN.md §7 |
| ds-desktop/resource-lifecycle | `66a97bf9` | major | idea | Adaptive DS memory and resource release without hard caps, starvation or work loss | open | **untracked** |
| ds-desktop/vector-tile-cache | `664a9beb` | major | idea | Desktop vector tile resource caching: persistent 5 GB or more with adaptive RAM reuse | open | **untracked** |
| ds-web | `011ae5a6` | blocker | bug | Project data does not render on the map: a ready map data upload has no layer class, no styles and no tiles (today's upload and the April one alike); rendering project data must need no server | waiting: Publish existing-network-steg-badea/bushengo screen style editors; restore DS map evidence capture for visual acceptance | OPEN.md §16 |
| ds-web | `638a13c0` | minor | friction | Session story 2026-09-18: one operator-day through ds on a real project - what worked, what to improve (consolidated, not urgent) | waiting: SRE mode: addition parked until the owner says go | **untracked** |
| ds-web/design-lv | `47136650` | major | obstacle | Topology-only repair for saved LV spans is missing | waiting: authoritative correction of Butezi saved cable pole claims; native ds-network analysis-only topology/load/phase projection with exact source | **untracked** |
| ds-web/desktop | `ad44f3a0` | blocker | bug | Desktop refuses a bare .shp dropped on the map and then freezes; the only working path is zip -> ds data convert -> GeoJSON -> local layer, split in two | waiting: Desktop release carrying ds-web a3f1feb9 | OPEN.md §16 |
| ds-web/desktop | `a4e7f666` | major | idea | Live layers and live folders: link local geospatial data in place, never move it, keep a GeoParquet analytical copy with provenance; same principle headless and on the Server | waiting: SRE mode: addition parked until the owner says go | **untracked** |
| ds-web/desktop combined + ds-brain/report combined | `a9b76d47` | major | bug | Desktop combined is LV-only while the headless combined adds MV models and MV Staking Table: one contract, two outputs | open | OPEN.md §7 |
| ds-web/desktop printing.export | `655b5a0d` | major | bug | desktop.printing.export --project X --transformer combined_transformer refuses unless X is the Desktop's active project | open | OPEN.md §7 |
| ds-web/desktop-assets-uploader | `e9b5858d` | blocker | bug | Desktop assets ingest always fails: "this browser refused the request without a status" | waiting: a proof ingest by an account holding assets.ingest (robot accounts have editor only) | OPEN.md §17 |
| ds-web/desktop-geodata | `dbe4d17e` | major | bug | Ground data download never settles: DEM/wetlands/powerlines bundles expand but are never indexed; status complete stays false | waiting: a reproduction on the reporter's Desktop state (czgmdwth_gisagara) | OPEN.md §16 |
| ds-web/desktop-grid-canvas | `42c76639` | major | bug | Desktop keeps stray local DS Grid model copies inside a project context; old-schema copies then break the canvas | waiting: kernel+ds-cli project-context read (ds-cli test gate busy with Codex terrain work); owner decision on the Local tab | **untracked** |
| ds-web/desktop-routes + ds-command-kernel/server | `72155a40` | blocker | bug | Combined report must honour the project's contractual outputs; desktop routes must pass --project to the kernel (no active project in the Server) | waiting: Desktop combined route on a Desktop release built from run | OPEN.md §7 |
| ds-web/dsgrid-model-lifecycle | `5400e80f` | major | friction | Retired and recreated MV model has confusing project-versus-local state in Desktop | waiting: push to origin/run and a client release | **untracked** |
| ds-web/headless-design | `c9df4098` | major | obstacle | design download plan lists artefact URLs but no ds verb executes it; every listed report and drafting artefact is readable by URL without any credential | waiting: Implement/deploy design-plan verified download and signed-expiring delivery; prove legacy public-read revocation and consumer compatibility | OPEN.md §1, OPEN.md §7 |
| ds-web/headless-design | `007c1624` | major | obstacle | Legacy (unfenced) transformer rooms cannot be read headlessly: lv project-export refuses, features select scans 0, location path resolves nothing, no headless migrate | waiting: a project that still holds an unfenced (legacy) room, to reproduce and prove a remedy | OPEN.md §9 |
| ds-web/map-styling | `dbaa3314` | major | bug | Design map: casing z-order flips on fresh load (hid LV design), PLS MV tiles unstyleable/unnumbered, tiles drawn over live edits, label defaults | waiting: Filed screen-map styling, casing/live-tile ordering, invalidation and default readability conditions; fresh Windows Desktop replay after sty | SEEDING.md §16 |
| ds-web/printing + plan-profile publication | `6890537c` | major | friction | Plan & profile prints belong to their MV model revision (attached, replaced per iteration), not to Project-wide maps | open | **untracked** |
| ds-web/profile-editing | `d6ff65b3` | major | bug | Desktop structure edit leaves ATTACHMENT_SET_PARTIAL_STRINGING (4 issues); live model cannot save or checkpoint | open | **untracked** |
| ds-web/project-work | `3cb5e3c5` | major | friction | Project Work UI needs a discoverable Finish task action | waiting: owner push (land-train.sh) + release | **untracked** |
| ds-web/project-work | `29c89e1b` | major | idea | Render Markdown and PDF attachments inline in PM items | open | **untracked** |
| ds-web/project-work/dashboard | `1352e52e` | major | bug | Project Work Dashboard opens an empty or error page in production | waiting: affected signed-in production Project Work Dashboard, offline-cache and empty-project acceptance | SEEDING.md §17 |
| ds-web/project-work/gantt | `a255fd6f` | major | idea | Gantt tasks need compact inline assignee controls and useful context | waiting: owner push (land-train.sh) + release | **untracked** |
| ds-web/project-work/plan | `59607488` | major | bug | Expanded Plan task details should push subsequent rows down instead of overlapping | waiting: owner push (land-train.sh) + release | **untracked** |
| ds-web/release | `3b5f6985` | minor | bug | Windows release font fetch restarts a timed-out partial download | waiting: push of ds-web run to origin/run (owner's call) | **untracked** |
| ds-web/reporting | `56749e69` | blocker | bug | Desktop report rejects reporter output containing survey_layers_omitted | waiting: deployed reporter/Desktop schema alignment plus a successful Burere Stable Windows export | OPEN.md §7 |
| ds-web/server-monitor | `84f68d8d` | major | bug | Production /server monitor reports snapshot unavailable despite fresh server monitor | waiting: authenticated production Server rendering and fresh authorized snapshot acceptance | SEEDING.md §17 |
| ds-web/style-center | `5f2b56e8` | major | friction | Screen transformer styling: categorical palette controls, satellite presets and pole-size regression checks | open | **untracked** |
| ds-web/sync-center | `6ffc5d52` | blocker | bug | Sync Center says all local engineering results delivered while report admission failed before queueing | waiting: owner push + release (part 1); owner decision (part 2) | **untracked** |

## ds-command-kernel

| Component | ID | Sev | Kind | Title | State | Backlog |
| --- | --- | --- | --- | --- | --- | --- |
| ds-command-kernel/design_config | `d76f6e55` | minor | obstacle | Project copy cannot seed a source configuration sheet absent from destination | open | **untracked** |
| ds-command-kernel/printing | `7da49ff8` | major | friction | Internal drafting diagnostic layers appear in client print legend | open | SEEDING.md §6 |
| ds-command-kernel/printing | `fe036819` | major | friction | LV print setup silently flattens cable categories to one pen | open | SEEDING.md §6 |
| ds-command-kernel/project-dataset-cache | `de978fdf` | major | friction | Geodata catalogue duplication and project-cache ready=false for datasets that are held and indexed | waiting: a paired Desktop run of ds desktop data rwanda status on czgmdwth_gisagara | OPEN.md §16 |

## ds-brain

| Component | ID | Sev | Kind | Title | State | Backlog |
| --- | --- | --- | --- | --- | --- | --- |
| ds-brain/formfactory | `fb36d55c` | major | obstacle | Form lifecycle delete cannot completely purge retired forms and their data | waiting: Backend/runtime bucket-inspection grant plus installed purge CLI and live complete receipts. | SEEDING.md §22 |
| ds-brain/geodata-catalog | `27c10f62` | major | obstacle | Foundation datasets have no published bundle: rwanda_upi_parcels, edcl_customers, rwanda_villages; rw-admin-villages unavailable | waiting: owner ruling: publish the foundation dataset bundles | SEEDING.md §16 |
| ds-brain/grid-catalog | `9e9f6527` | minor | obstacle | Global library explorer lacks member inventory and preview reads for existing release | waiting: Legacy release assets inventory migration and authority-owned bounded member preview through deployed ds | **untracked** |
| ds-brain/pm | `9d5926e2` | major | idea | Composite notes/tasks/milestones: checklist items with location, attachments and answered state (contract on run: ds-brain b55bb3c) | open | **untracked** |
| ds-brain/report-compute | `cb758ad5` | major | bug | Cloud individual report compute returns Service Unavailable for 113 Nyamagabe transformers | waiting: Install matching schema-2 ds client, then replay missing Nyamagabe cloud reports and verify per-room causes plus combined coverage | SEEDING.md §7 |
| ds-brain/style-center | `f012845d` | major | friction | Version print style documents and provide exact read, compare and restore | open | **untracked** |

## ds-apis-tf

| Component | ID | Sev | Kind | Title | State | Backlog |
| --- | --- | --- | --- | --- | --- | --- |
| ds-apis-tf/gateway | `2adfa7d2` | major | obstacle | Gisagara cannot publish lv_poles standards columns through known-columns route | waiting: owner: merge ds-apis-tf run into master, then deploy_local.sh canary and stable | OPEN.md §9 |

## ds-vector-tiler

| Component | ID | Sev | Kind | Title | State | Backlog |
| --- | --- | --- | --- | --- | --- | --- |
| ds-vector-tiler/rwanda-dem | `af976171` | major | bug | Rwanda DEM 2026-08-28 holds voids filled with one constant, so contours print a ladder of false lines along each patch edge | waiting: owner ruling: republish the 10 m DEM with nodata voids | SEEDING.md §16 |

## unassigned

| Component | ID | Sev | Kind | Title | State | Backlog |
| --- | --- | --- | --- | --- | --- | --- |
| ds-agent/notes | `1631a9ba` | minor | idea | Agents should consult personal and project notes and usually create project notes | open | **untracked** |
| ds-geometry/vector-buffer | `5dc0e8a1` | major | bug | Native line buffer emits self-crossing polygons rejected by DS readers | waiting: Install matching ds/kernel release and rerun exact Model 3 buffer plus shared-reader validation | **untracked** |
| ds-notes/project-work | `70f6db77` | minor | idea | Move or copy among tasks, personal notes, and project notes | open | **untracked** |
| ds-platform/map-authoring | `9d6c5f30` | major | idea | Headless georeferenced map viewing and typed digitization without Computer Use | open | **untracked** |
| ds/design-intake | `14fd08e8` | major | obstacle | Headless LV intake cannot upload to Northern Hub | open | OPEN.md §9 |
| ds/mv-delivery | `6c694716` | major | friction | Pin approved MV front matter and canonical one-shot RTF-to-PDF delivery | waiting: implementation: approved front-matter bindings in shared MV project setup | **untracked** |
| ds/pls-delivery | `03cefcea` | major | bug | MV delivery controller and context compatibility gaps | open | **untracked** |
| platform / document tooling | `2e6ab6fa` | minor | idea | Store our system fonts in the cloud so document generators run on any host | open | **untracked** |

