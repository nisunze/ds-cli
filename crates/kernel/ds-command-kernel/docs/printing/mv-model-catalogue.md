# Project MV model sources

`dsgrid.project.list --mv-picker` (contract2) consumes the authoritative project
head metadata through `ds-client-core::grid_models::catalogue`. The paired
Desktop uses the same list planner, decoder and bounded metadata walk. It never
downloads model packages. The shared native layer-store cache retains complete
heads by principal/project/lane/credential audience and coalesces intentional
consumers. Warm and offline consumption perform no project-head transport read. Invalidation
records only a marker; the next online consumer resolves it. An explicit refresh
may reacquire heads. A known authorization refusal revokes retained access until
an explicit authorized refresh succeeds.

`printing::mv_models` projects one row per promoted, active logical MV model.
Names are presentation; copied project names do not establish or defeat source
identity. Local unpublished copies and copies of other projects do not become
project heads and are never closed, saved, focused or changed by this read.

An optional declared held inventory (`map.profile.view --include-held`) supplies
atomic native worker revision/history/identity facts and complete base
provenance. Only a matching project/model/head revision/digest/version can supply
a current held source. One exact copy returns `held`; several exact copies
return `choose_working_copy` with explicit candidates; none returns `load_needed`.
A historical or foreign copy cannot silently substitute for a current head.

`--source-binding` admits an explicit native candidate against that inventory.
CLI and Desktop consume one shared `grid_project_catalogue::consume` path.
Wrong source project or inventory account/project refuses before cache or
transport access. Preview admission precedes configuration/style/scene reads,
then rechecks the retained head and captured scene revision before rendering.
The hashed preview inputs retain the exact source binding in addition to source
bytes, layout/styles/pens, engineering revision and engine identity. Job status
and file consumption continue through their original native authority fences.

The output `ds.mv-model-catalogue/v1` carries `project_id` and `models`: logical
identity/name, exact head revision/digest/version, state, optional unique source
and explicit candidates. Each source binds worker identity, native manifest
identity, revision, history witness and CRS. Overview and MV plan/profile
consumers use this same projection for their selected held scenes; neither
creates another cloud model-list decision.
