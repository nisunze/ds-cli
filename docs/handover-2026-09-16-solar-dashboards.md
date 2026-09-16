# Airport handover: images and Solar Rust ownership

## Latest checkpoint: laptop / WSL, 2026-09-16

The owner authorized committing and pushing this checkpoint to `run` for a
later pull on ds-server. The Solar migration remains incomplete; no new
installation, deployment, project mutation or publication was performed.
The sections below this checkpoint describe the earlier airport session.

**Rule of the land:** all Solar capabilities belong to the native Server core,
available through CLI/MCP without Svelte, browser WASM, Tauri or paired Desktop.
Svelte renders server projections and forwards typed intentions. Tauri, if
retained, supplies shell integration only. This supersedes the old campaign's
read-only TypeScript restriction. The rule is recorded in the kernel headless
runtime contract, CLI instructions and web instructions.
The CLI kernel pin advances to `fed6079` (the rule's documentation commit);
native kernel inputs are unchanged by that commit. The web instruction commit
is `f5da4a63`. The existing ds-solar `71fc7b0` requires no new commit.

The first unfinished MCP union change described below is now implemented:
`solar-dashboard` participates in the union, and only `solar_engine`,
`solar_results_read` and `solar_project_result` may overlap sibling Solar
profiles. Execution of the focused regression is pending at this checkpoint.

Confirmed in the laptop session:

- Seven native kernel Solar dashboard tests passed.
- The isolated WSL source CLI built offline via `run-linux-server.sh --cli`
  from revision `325ba45f74c13041e82304d1d871a1a6170181cd`.
- Linux `ds-cli-server` normal/build dependency graph contained 395 packages,
  with no ds-web source paths or Tauri packages.
- The isolated WSL development session is signed out; no account credentials
  were copied, no project is selected, and no server daemon was started.
- Standalone `ds-solar-cli` could not build offline because the crate cache
  lacks `openssl-probe v0.2.1`; crate downloads timed out on both Windows and WSL.
- The attempted, unvalidated native command extraction was removed. There are
  no new ds-solar implementation changes in this checkpoint.

The source CLI build does not validate the newly changed MCP test. The latest
Windows CLI was not installed. Live Aderm verification is still pending, using
exact project `arjgpydw_aderm_loc7`; the sealed run listed below remains the
verification source on ds-server.

On ds-server, pull `run` in ds-command-kernel, ds-solar, ds-web and ds-cli.
Run the focused MCP regression and required checks before packaging. Build the
native Solar sidecar, use the supported authentication flow in the appropriate
isolated development namespace, then launch the Linux server wrapper and obtain
actual project receipts. Do not use laptop compilation as live project evidence.

Remaining native ownership work includes Desktop-paired lifecycle, portfolio
management, final import/submit, report bundle/export, sync and publication in
the CLI Solar modules and `ds-web/src-tauri/src/solar_native.rs`. Finance cashflow,
sensitivity and chart composition, comparison selection and other Solar
decisions still need to leave Svelte/TS/WASM. Prove the complete headless path
and perform the required blind workflow trial after implementation is ready.

Laptop-only detailed evidence is retained under
`_shared/solar-server-20260916/`; it is not part of the pushed repositories.
Registered task worktrees use branch `work/solar-server-20260916` under
`_worktrees/solar-server-20260916/`. The workspace root checker reports the
pre-existing `.claude`, `.gitignore`, `ds-cli-skills` and `ds-mcp` entries, which
were left untouched. Other new local CLI edits in the main checkout were not
included in this Solar checkpoint.

## Earlier airport checkpoint

Stopped on the user's request for commit/push only, with no further tests or validation.
The local Linux package build was terminated. No new canary package was installed
or published. Installed `/usr/bin/ds-canary` is still the older b2843ea build.

## Branches to resume

All four product repositories are on `run`:

- ds-command-kernel: `5988d0c` — shared native/WASM Solar dashboard composer.
- ds-solar: `71fc7b0` — exact sealed city report/result source verification.
- ds-web: `4ed93969` — Site, Plant, Finance and BOQ consume Rust/WASM projections.
- ds-cli: `6c7cd81` plus this handover — headless reader, composition and bounded profiles.

Earlier images work is already on these branches: rotation is owned by Rust and
exposed through CLI/MCP; the touched photo UI consumes the owner projection.
Earlier Solar blocker work includes submission envelopes, canonical batch-digest
validation and headless portfolio calculation/publication. This handover does not
claim every remaining Solar TypeScript decision has been migrated.

## Current implementation

- `solar.results.read` contract 2 requires `source`, `project`, `run-id`, `city`,
  `section` and optional semantic `path`. It no longer uses a paired desktop.
- `solar.dashboard.compose` writes a new private `dashboard.json` and `index.html`.
  Sections: site, plant, finance, boq. Scenario defaults to hybrid; explicit
  `system` selects hybrid, solar_battery or thermal_only.
- Rust owns Site load factor; Plant renewable fraction, cost selection, yearly
  chart pairing/units, rows and degradation; Finance scalar selection and viability;
  BOQ numbering, totals and currency compatibility. Missing values stay unavailable.
- HTML has cards and BOQ tables, with no script. Plant JSON has declarative chart
  options. Plot files and online dashboard publication are not included.
- `solar-dashboard` MCP profile exposes composition and sealed source reads.
  Portfolio calculate/publish are in `solar-portfolio-batch`; `solar-delivery`
  retains catalog/delivery. This fixes its tool-budget overflow.
- Browser reads/projections fence account, project, city and scenario changes.

## First unfinished item

`crates/ds/tests/mcp.rs`, test `every_specialized_profile_is_bounded_and_catalogued`,
has another hard-coded Solar profile list around lines 1470–1511. Add
`solar-dashboard` to its profile union and permit the intentional overlap of
solar_engine, solar_results_read and solar_project_result. The earlier registry
partition test in the same file has already been updated for this overlap.
The last failure was the missing dashboard tool in this hard-coded union;
profile startup itself passed after the delivery profile split.

Do not run validation until the user resumes it. Then finish this test update,
run focused checks, rebuild the local Linux package and install its exact artifact
directly. Do not launch Cloud Build or remote CI. Run one bounded blind CLI/MCP
trial after the installed fix, using installed executable discovery and shipped
skills only. No new blind trial has been run for this dashboard implementation.

## Evidence and build state on ds-server

Workspace `/home/magese/ds-work/solar-dashboard-batches-20260916/` contains:

- batch-1-site and batch-1-site-mcp: successful local candidate composition.
- batch-2-plant, batch-3-finance, batch-4-boq: successful candidate composition.
- local-package/build.log: interrupted local packaging build; not a release receipt.

Exact sealed source: `/home/magese/ds-work/solar-aderm-20260916/ws/runs/aderm-ws-20260916-all`.
Project `arjgpydw_aderm_loc7`, batch run `aderm-ws-20260916-all`, city `aderm_beinamar`.
Source batch digest `sha256:0623d26235a54dff96afaacce3eebfec6025d9d5e15c58fec6f18528c1add11b`.
Report digest `sha256:d6a401f94b8e37644b05fdc9b3c527430a8a0ff10aa46b0fde9a7a003b564603`.
These source receipts/files remain on ds-server and will be unavailable at the airport.
Code and this handover are portable through the pushed `run` branches.

Before the stop request: focused Rust dashboard tests, source-verification test,
web check and focused WASM/mounted tests passed. CLI context budgets and 44 paired
Solar tests passed after migration. The final MCP profile-union test is still
unfinished as described above. No validation was performed after the stop request.

## Remaining batches

Continue moving Finance sensitivity, cashflow and chart composition into Rust;
unified comparison selection and other dashboard slices still contain TypeScript
decisions. Add headless plot/static chart delivery if required. Verify installed
headless access and representative visual outputs before closing any feedback.
Do not report local files or compilation as online publication.
