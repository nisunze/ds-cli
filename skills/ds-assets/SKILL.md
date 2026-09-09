---
name: ds-assets
description: "Browse, preview, read and govern a project's documents through `ds assets` — folder tree, bounded previews, classification, links to tasks and DS objects, and explicit ingest."
metadata:
  ds-chapters: assets
---

# Work a project's documents

Project Assets is one catalogue with two halves: folders a person declared and
filed documents into, plus system folders projected at read time from
inventories the project already holds — transformer designs and versions, MV
models, survey media, project-work attachments, reports, Solar results, local
rooms and prints. Everything goes through `ds assets`, which asks the paired
DS GridDesign application to act under the session it already holds. There is
no `--project` flag: the destination is the project the application has open.

1. Find the document. `ds assets tree --depth 2 --output json` gives the
   folders and their counts; `ds assets list --folder <path> --status durable
   --output json` gives one bounded page of rows. Both answer only what this
   user may see. A class they cannot read has no row at all, so an empty
   answer is the answer — never a reason to retry with a wider flag.
2. Narrow before you page. `--kind`, `--status`, `--sensitivity`, `--since`,
   and on `tree` a `--query <text>` or `--link pm_task:<id>` /
   `--link ds_object:<type>:<id>`. When `.data.more` is true, continue with
   `--cursor <.data.next_cursor>`; never re-read from the top.
3. Walk a container in place: `ds assets tree --into <asset-id> --output json`
   lists its members, with a shapefile arriving as one `geo` member and its
   companions resolved. Nothing is unpacked to disk.
4. Look before you copy: `ds assets preview --asset <id> [--member <path>]
   --output json` returns a bounded document — text blocks, a grid, mail
   headers, features, or a metadata card — never an image. Respect the
   `truncated` counts, and read a bound refusal as final: it carries the
   actual number, and the honest alternatives are `read` and `promote`.
5. Take a copy only when a real file is needed: `ds assets read --asset <id>
   --out <new absolute path>`. The destination must be new; an existing file
   is never overwritten. Compare the returned `digest` with the catalogue
   row's before trusting the bytes.
6. Put geometry on the map durably with `ds assets promote --asset <id>
   [--member <path>] --as-layer <name>`. Promotion is local, never an upload,
   and it cannot widen the source's sensitivity. It is not cleaning: canonical
   column mapping stays in `ds map design upload inspect` and `stage`.
7. Change shared state only on the user's explicit intent, always with
   `--yes`: `ds assets classify --asset <id> --status durable --reason <why>
   --yes`; `ds assets attach --asset <id> --task <id> --yes` (or
   `--object-type` with `--entity-id`, and `--detach` to remove);
   `ds assets ingest --path <absolute file> --folder <path> --sensitivity
   <class> --yes`; `ds assets folder --path <path> --sensitivity <class>
   --yes`.

Sensitivity never loosens implicitly. A new asset takes its folder's default
class unless a stricter one is named, and making a document more open is an
explicit `classify` by someone who may read the class being left. Never
propose a looser class to make a read succeed.

An id beginning `sys:` names a projected row — a view of something that
already has an owner. Read, preview, walk and promote it freely; `classify`,
`attach` and `folder` refuse it by name. Act on the source object instead: the
transformer, the model, the task, the report.

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

If `ds assets` answers `desktop_not_paired` or `desktop_signed_out`, the
document surface is unavailable in this session. Say so and stop; do not look
for the bytes by another route.
