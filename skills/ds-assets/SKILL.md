---
name: ds-assets
description: "Browse, preview, read and govern a project's documents through `ds assets` — what changed lately (timeline), folder tree, version history, bounded previews, classification, links to tasks and DS objects, and explicit ingest."
metadata:
  ds-chapters: assets
---

# Work a project's documents

Project Assets is one index with two halves: folders a person declared and
filed documents into, plus system folders projected from what the project
already holds — transformer designs and their versions, MV models, survey
media, project-work attachments, reports, Solar results and prints. ds-brain
builds and serves that index; `ds assets` reads it headless. Use the exact
`--project <id>` on every project command. The native credential
authenticates the call; the Server does not consult a window or saved
project selection.

1. Find the document. `ds assets list --project <id> --output json` is the
   timeline: newest change first, across uploads and every system folder —
   "what changed on this project lately". `--order name` sorts by name;
   `--folder <path>` lists one exact folder (`Transformers/TX-104`,
   `contracts/2026`). `ds assets tree --project <id> --depth 2 --output json`
   gives the folders and their counts. Both answer only what this user may
   see. A class they cannot read has no row at all, so an empty answer is
   the answer — never a reason to retry with a wider flag.
2. Read the history. A row with `versions` (the chip `v3 · 12 versions`) has
   earlier versions: `ds assets versions --project <id> --asset <id>
   --output json` lists them newest first — label, when, who, milestone,
   reason, which is current — each with its own asset_id.
3. Narrow before you page. `--kind`, `--status`, `--sensitivity`, `--since`.
   When `.data.more` is true, continue with `--cursor <.data.next_cursor>`;
   never re-read from the top. `assets_index_moved` means the index was
   rebuilt between pages: start again without `--cursor`. `--refresh`
   rebuilds the index when `index.stale_sources` names what you need.
   On `tree`, `--query <text>` and `--link pm_task:<id>` /
   `--link ds_object:<type>:<id>` search the catalogued uploads only.
4. Walk a container in place: `ds assets tree --project <id> --into <asset-id> --output json`
   lists its members, with a shapefile arriving as one `geo` member and its
   companions resolved. Nothing is unpacked to disk.
5. Look before you copy: `ds assets preview --project <id> --asset <id> [--member <path>]
   --output json` returns a bounded document — text blocks, a grid, mail
   headers, features, or a metadata card — never an image. Respect the
   `truncated` counts, and read a bound refusal as final: it carries the
   actual number, and the honest alternatives are `read` and `promote`.
6. Take a copy only when a real file is needed: `ds assets read --project <id> --asset <id>
   --out <new absolute path>`. The destination must be new; an existing file
   is never overwritten. Compare the returned `digest` with the catalogue
   row's before trusting the bytes.
7. Put geometry on the map durably with `ds assets promote --project <id> --asset <id>
   [--member <path>] --as-layer <name>`. Promotion is local, never an upload,
   and it cannot widen the source's sensitivity. It is not cleaning: canonical
   column mapping stays in `ds map design upload inspect` and `stage`.
8. Change shared state only on the user's explicit intent, always with
   `--yes`: `ds assets classify --project <id> --asset <id> --status durable --reason <why>
   --yes`; `ds assets attach --project <id> --asset <id> --task <id> --yes` (or
   `--object-type` with `--entity-id`, and `--detach` to remove);
   `ds assets ingest --project <id> --path <absolute file> --folder <path> --sensitivity
   <class> --yes`; `ds assets folder --project <id> --path <path> --sensitivity <class>
   --yes`.

Sensitivity never loosens implicitly. A new asset takes its folder's default
class unless a stricter one is named, and making a document more open is an
explicit `classify` by someone who may read the class being left. Never
propose a looser class to make a read succeed.

An id beginning `sys:` names a projected row — a view of something that
already has an owner. List it and read its versions freely; its bytes are the
owner's (`read`, `preview` and `promote` answer `origin_read_unavailable`),
and `classify`, `attach` and `folder` refuse it by name. Act on the source
object instead: the transformer, the model, the task, the report.

Read the live contract before inventing flags:
`ds capabilities assets --output json`, then
`ds capabilities assets.<command> --output json`.

## When not to use this

- Cleaning or admitting a data file for design — `ds map design upload
  inspect` / `stage`, or the `ds-layer-management` skill.
- Temporary map layers, viewport staging or session-only geometry —
  `ds-map-local-data`.
- Obtaining and reading a delivered report workbook — `ds-report-consumption`.
- Tiles, styling, or anything about how the map draws — `ds-tiling`,
  `ds-style-composite`.

If `ds assets` answers `headless_signed_out`, follow the native account-link
contract. A project access refusal is final for that account and project.

Stops at: the document's own application — `ds` lists, previews, classifies and
links the bytes; opening or editing them is the operator's tool.
