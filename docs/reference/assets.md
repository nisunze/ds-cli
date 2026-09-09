# `ds assets` — reference

Tier-4 reference. `ds assets <command> --help` is the contract; this document
is the part that does not belong in any command's help because it is true of
all of them.

## Where the catalogue is

In two places, and the difference is the one thing worth learning first.

**Declared folders and the documents filed in them** live behind ds-brain,
which is the only authority on who may see them. A `restricted` or
`confidential` asset the caller may not read has no row at all: no name, no
byte count, no date, no placeholder, and it is not in any folder's count. That
is not a display rule this CLI could turn off — the row never arrives.

**System folders are not stored anywhere.** `Transformers/…`, `MV models/…`,
`Survey/…`, `Project work/…`, `Reports/…`, `Solar/…`, `Project data/…`,
`My data/…`, `Local data/…`, `Prints/…` and `Unclassified/…` are projected at
read time from the inventories the project already holds, through the same
reads the application's own pages use — one read per source, never polled,
never fanned out per transformer. Nothing a source returns is dropped: a row
the projection cannot place lands under `Unclassified/` rather than vanishing.
Their rows carry a `sys:` id instead of a minted `a_` one. They can be walked,
previewed, read and promoted; they cannot be classified, attached or filed,
because a projection is a view of something that already has an owner. Act on
that owner instead.

The bytes are somewhere else again — project storage, behind a fresh
short-lived signed read minted per access after an authority check. Nothing
here hands out a durable link to anything above `open`.

That is why every command is one named semantic operation the **paired
application** performs under the session it already holds, and why there is no
`--project` flag anywhere in this domain. The active project is the one the
application has open; a project id passed as an argument would be a claim `ds`
has no standing to make. `docs/reference/desktop.status.md` has the pairing
argument in full.

## The shape of a session

```bash
ds assets tree --depth 2                          # what does this project hold
ds assets list --folder contracts/2026 --status durable   # find the document
ds assets preview --asset a_7kq3nr2v0b1c --output json    # look at it, cheaply
ds assets read --asset a_7kq3nr2v0b1c --out /home/me/Downloads/EPC-Lot3.pdf
```

`tree` and `list` are the two doors in: every other command needs an
`asset_id`, and this is where one comes from. A container is walked in place —

```bash
ds assets tree --into a_7kq3nr2v0b1c --output json                    # its members
ds assets preview --asset a_7kq3nr2v0b1c --member Lot3/gis/poles.shp  # one member
ds assets promote --asset a_7kq3nr2v0b1c --member Lot3/gis/poles.shp --as-layer Lot3-poles
```

— by reading the archive's central directory only. Nothing is unpacked to
disk, and a shapefile is walked as one `geo` member with its `.dbf`, `.shx`
and `.prj` companions resolved, not as four unrelated files.

## Preview answers a document, never a picture

`ds assets preview` returns a `ds.assets.preview_doc/v1`: text blocks, a
bounded grid, mail headers, feature geometry, or a metadata card. That keeps
the CLI useful to an agent, which wants text, and keeps rendering in the
client, which is where the person is. A PDF answers a metadata-only document
naming its renderer, because rasterising a page in a terminal helps nobody.

A preview never fetches remote content — no external images, no stylesheets,
no fonts, no tracking pixel in an email. It is self-contained or it says it is
incomplete.

Everything is bounded, and every bound is reported with the number that broke
it. `--pages` is capped at 5 and `--rows` at 200; a document above a size or
feature bound is **refused with the bound and the actual number**, and offered
the honest alternative — read it to a file, or promote its geometry. A
truncated render presented as the document is the one answer this surface will
not give.

## Reads cost one page

A read paints the same catalogue the application's Assets tab renders: cached
first, reconciled once, never polled. A CLI session therefore adds no project
reads to a catalogue the application already has open.

`--limit` is a page, bounded at 250 and defaulted to 50. When a page is short
of the whole answer, `more` says so and `next_cursor` continues it — page with
the cursor rather than re-querying from the top, which is the difference
between one read and one read per page you have already seen. `scanned` is how
many rows the read considered, and `truncated` marks a scan that stopped at
its own bound: narrow it with `--folder`, `--kind` or `--since` rather than
raising the page.

`tree` expands `--depth` levels (bounded at 8) and reports counts below that.
A container walk lists 5,000 members before reporting `truncated`. `--link
pm_task:<id>` or `--link ds_object:<type>:<id>` narrows a tree to the assets
linked to one task or DS object; a read takes exactly one link filter, and a
second is refused rather than quietly dropped.

Offline, `list` and `tree` serve the local catalogue labelled with its age,
and a preview of bytes this device does not hold is refused by name rather
than answered thinly.

## Writes are explicit, confirmed and audited

There are four of them — `classify`, `attach`, `ingest` and `folder` — every
one `global_write`, so dispatch requires `--yes` before the bridge opens.

| | |
|---|---|
| `classify` | one patch of `--kind`, `--status`, `--owner`, `--folder`, `--sensitivity`; a change with no flags is refused before the round trip |
| `attach` | one link, `--task` **or** `--object-type` with `--entity-id`, and `--detach` to remove it; the link is recorded on the asset, never on the object |
| `ingest` | one named local file, with the folder and class you intend; there is no drag-to-upload and nothing is ever sent implicitly |
| `folder` | declares a folder, or changes its defaults or its last name segment |

Two rules are worth stating because they are not obvious from any one command:

**Sensitivity never loosens by accident.** A new asset takes its folder's
default class unless you name a *stricter* one, and it can never take a looser
one implicitly. Loosening is an explicit `classify` by a caller who holds both
the capability to classify and the capability to read the class being left,
and it is audited. An asset nothing could classify is `internal`, not open.

**Reading a file changes nothing shared.** `ds assets read` writes exactly one
new local file — the desktop writes it through one closed native command that
refuses an existing path, writes a temporary sibling, verifies the digest and
renames — so bytes never cross the bridge and an existing file is never
overwritten. `ds assets promote` adds a layer to the running map. Neither is
behind the confirmation gate, because neither is visible to anybody else.

## Promotion is local, and it is not cleaning

A previewed geometry is a session drawing: it lives in the open map, it is not
in the layer store, and it is gone on reload. `ds assets promote` hands the
same geometry to the governed local-overlay path, where it gains a durable
local identity, the owning path's validation, and the ability to be ordered,
styled and printed. It never uploads, and it never widens the source asset's
sensitivity.

A preview drawing is not a promise of promotion: the overlay path may refuse
what the preview happily drew. Cleaning, canonical column mapping and design
admission are still `ds map design upload inspect` and `stage`.

## What is deliberately absent

**An editor.** No command writes an asset's bytes, under any flag, at any
layer. `read` copies them out; nothing writes them back.

**A durable link to anything sensitive.** Only `open` assets can carry a
durable grant. Every other class is read through a fresh short signature
minted per read, and no `download_url` is ever stored — the listing states an
expiry instead.

**Content search.** `--query` is a case-insensitive substring match over name,
folder path, kind, format, status and owner, evaluated over the loaded tree so
that this CLI, the Assets tab and the Project work mount answer the same query
identically. It does not read inside documents, and there is no search
endpoint behind it.

**A second catalogue, uploader, digest or folder authority.** This surface
composes the paths the project already has. Where you see a system folder, you
are looking at an existing inventory through a different window.

## Refusals worth planning for

| Code | Means |
|---|---|
| `desktop_not_paired` | no DS GridDesign session on this machine |
| `desktop_signed_out` | running, but signed out or with no project open |
| `desktop_refused` | no such asset or folder, or the application declined; `detail.detail` carries its message |
| `projected_asset_read_only` | a `sys:` row or a system folder was named by `classify`, `attach` or `folder` |
| `nothing_to_update` | a `classify` with no change flag — refused before a round trip |
| `invalid_asset_id` | not a minted `a_…` id and not a projected `sys:…` one; usually a truncated paste |
| `invalid_attachment` | `--task` and `--object-type` are two different links; name exactly one |
| `invalid_folder_path` | absolute, empty, or carrying a `.`/`..` segment |
| `invalid_out_path` | `--out` is not a new absolute path under an existing directory |
| `invalid_source_path` | `--path` is not an absolute path to an existing readable file |
| `invalid_number` | a bound with its number: `--limit` 1-250, `--depth` 1-8, `--pages` 1-5, `--rows` 1-200 |
| `invalid_layer_name` | `--as-layer` is empty, longer than 80 characters, or holds a control character — the same bound the application holds it to |
| `invalid_date` | a `--since` that is not `YYYY-MM-DD` or RFC 3339 |
| `invalid_link` | a `--link` that is not `pm_task:<id>` or `ds_object:<type>:<id>`, or a second one — a tree read filters on one link |
| `asset_not_found` | no asset or folder has this id or path; a `confidential` row the caller may not read answers this too, by design |
| `asset_class_forbidden` | the signed-in user lacks the assets capability this class or this write requires |
| `asset_version_conflict` | the row or folder moved while a write was in flight; re-read and issue the command again |
| `asset_refused` / `asset_request_invalid` | the catalogue's own rule or bound, named in the message with the number |
| `assets_not_implemented` | the installed application or its catalogue does not serve this action yet |
| `assets_service_failed` | the catalogue service faulted; retry once |
| `offline_mode_enabled` | offline mode is on and the command needed the catalogue service; `list` and `tree` answer from the local catalogue when they can |
| `backend_unreachable` | the Data Solutions API did not answer; re-read before repeating a write, because an unanswered write may already have been applied |
| `asset_is_not_a_file` | `read`, `preview`, `promote` or `tree --into` named a `sys:` row that is a summary, not bytes — a dataset room, a report room, a print setup, a transformer version; `preview` is the whole of it |
| `asset_too_large` | the bytes are above the read bound the message names; open the asset from its own surface |
| `origin_read_failed` | the source's own read action returned nothing usable — a row naming no source object, or a signed read that expired; re-read the row and retry once |
| `origin_unreachable` | the source's bytes could not be fetched from this device |
| `origin_read_unavailable` | rows projected from this source carry no read action on this surface yet, DS Grid export outputs among them |
| `unknown_folder` | a `--folder` names a path no declared folder has; declare it with `folder`, or read the declared folders with `tree` |
| `assets_offline_write` | this device is offline and `classify`, `attach`, `ingest` or `folder` is a catalogue write; reconnect, or turn offline mode off |
| `invalid_member` | `--member` is absolute or has a `..` segment; copy the exact path from `tree --into <asset>` |
| `asset_not_geographic` | `promote` named an asset that is not `geo` and no geo member; only a geographic asset or member promotes |

`--since 01-09-2026` is refused here rather than at the catalogue on purpose: a
transposed day and month is the commonest filter mistake there is, and
`2026-01-09` for the ninth of September is a perfectly valid date that quietly
lists the wrong eight months.
