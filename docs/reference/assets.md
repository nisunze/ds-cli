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
read time from the inventories the project already holds — in the application,
through the same reads its own pages use. Their rows carry a `sys:` id instead
of a minted `a_` one. **Headless, those inventories are not loaded**: `ds
assets tree` renders every system root `not loaded` and names them in
`sources_omitted`, and a `sys:` id given to `read`, `preview`, `promote` or
`tree --into` is refused by name (`origin_read_unavailable`) — a projected row
is read from the surface that owns it. The declared folders and every
catalogued `a_` asset are the headless tree, computed by the same kernel the
Assets tab uses.

The bytes are somewhere else again — project storage, behind a fresh
short-lived signed read minted per access after an authority check. Nothing
here hands out a durable link to anything above `open`.

Every command runs headless, under the restored native user or device
credential, against the project `ds auth project use` selected, on `--lane`.
No command accepts a project ID as authority, and none needs a window: until
2026-09-20 nine of them relayed through the paired desktop (`requires:
window`); that path is retired, `--desktop-descriptor` is no longer an input,
and a caller that still passes it is told `requires_window_retired`.

City maps are members of the general **tag-group-map** representation class.
The map index groups existing producer asset identities by exact tag definition
and value. An asset reused by several groups still has one byte object. The
index preserves catalogue pagination and never treats queued local printouts as
published assets. Unfiled shared maps appear under `Prints/tag groups` in the
system folder projection; explicitly chosen user folders are preserved.

Custom printouts enter the same catalogue through `assets.map.publish` after
local rendering. The owner uses the existing ingest, verified finalization,
classification and tag-link transactions. A printed geographic document is
explicitly classified `geo`; ordinary images and source GeoJSON are not map
printouts. Tagged maps appear in their groups; maps without tags appear under
Project-wide maps. Local browser custom printouts remain visible in the same
Project Control section with their existing attachment status.

Publication reuses an exact name/digest match, including after a partial
classification/link failure. A changed document is a new asset and never
silently deletes the earlier submission. Catalogue matching is bounded and
refuses an incomplete search. The command's live help describes file and tag
limits; no URL, storage path, credential or arbitrary API body is accepted.

When updating a print, discover its current asset with `assets.maps` and pass
`--replaces <asset-id>` to `assets.map.publish` (repeat for legacy duplicates).
One map nature and paper size has one current rendition per format. Do not
replace another map just because it shares the same city tag. The declaration
must retain the predecessor's format and exact tag set. The native owner reads
all named predecessors before mutation, verifies the new durable map, then
archives each predecessor with its expected version. Archived bytes remain
available as history; they disappear from the default index. A failed upload
never archives a predecessor. A partial archive failure can be retried with the
same declaration. Renaming does not require retaining duplicate current maps.

```bash
ds assets map publish --file /prints/Kyabe-A3.pdf --tag city=kyabe \
  --replaces a_123456789abc --yes --output json
```

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

A read is one catalogue round trip — `list` one page, `tree` the declared
folders plus the first page of the catalogue — folded by the kernel on this
host; never polled.

`--limit` is a page, bounded at 200 and defaulted to 50. When a page is short
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

A `--folder` filter on `list` is answered by the catalogue's own folder query.
On 2026-09-20 the canary catalogue answered that query with a 500
(`assets_service_failed`): a Firestore composite index for
`folder_id`+`created_at` is missing on that lane. `tree --folder <path>` lists
the same rows from the kernel projection and is the workaround until the
index lands.

## Writes are explicit, confirmed and audited

There are four of them — `classify`, `attach`, `ingest` and `folder` — every
one `global_write`, so dispatch requires `--yes` before any credential is
restored.

| | |
|---|---|
| `classify` | one patch of `--kind`, `--status`, `--owner`, `--folder`, `--sensitivity`, pinned to the row's current version; a change with no flags is refused before the round trip |
| `attach` | one link, `--task`, `--record` **or** `--object-type` with `--entity-id`, and `--detach` to remove it; the link is recorded on the asset, never on the object |
| `ingest` | one named local file, with the folder and class you intend: the head is recognised by the kernel, the file is digested in one streaming pass, the upload target is minted, the bytes stream to it, the row is finalised against that digest; files up to 256 MiB |
| `folder` | declares a folder (its parent must already be declared — the catalogue says so by name), or changes its defaults or its last name segment |

Two rules are worth stating because they are not obvious from any one command:

**Sensitivity never loosens by accident.** A new asset takes its folder's
default class unless you name a *stricter* one, and it can never take a looser
one implicitly. Loosening is an explicit `classify` by a caller who holds both
the capability to classify and the capability to read the class being left,
and it is audited. An asset nothing could classify is `internal`, not open.

**Reading a file changes nothing shared.** `ds assets read` writes exactly one
new local file on the host running `ds`: the bytes are verified against the
row's digest, written to a temporary sibling and renamed, and an existing file
is never overwritten. `ds assets promote` adds a layer to this machine's
prepared local layer store. Neither is behind the confirmation gate, because
neither is visible to anybody else.

## Promotion is local, and it is not cleaning

`ds assets promote` hands a geo asset's bytes to this machine's prepared local
layer store — the one `ds map local register` writes and `ds map local list`
reads, kept per lane and DS account — where it gains a durable local identity,
the store's validation, and the ability to be ordered, styled and printed. It
never uploads, and it never widens the source asset's sensitivity. The store
admits a GeoJSON feature collection of one geometry type, read from its
features; another geo format (`shp`, `kml`, `gpkg`) is refused
`invalid_payload` — convert it first, or read it out with `ds assets read`.

Cleaning, canonical column mapping and design admission are still
`ds map design upload inspect` and `stage`.

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
composes the paths the project already has.

**A window path.** See above: the paired-desktop transport is retired for
every command here.

## Refusals worth planning for

| Code | Means |
|---|---|
| `headless_signed_out` | no restored native credential on this lane — `ds auth login` or `ds auth link begin` |
| `headless_project_not_selected` | no project selected for this credential and lane — `ds auth project use --project <id>` |
| `projected_asset_read_only` | a `sys:` row or a system folder was named by `classify`, `attach` or `folder` |
| `nothing_to_update` | a `classify` with no change flag — refused before a round trip |
| `invalid_asset_id` | not a minted `a_…` id and not a projected `sys:…` one; usually a truncated paste |
| `invalid_attachment` | `--task`, `--record` and `--object-type` are three different links; name exactly one |
| `invalid_folder_path` | absolute, empty, or carrying a `.`/`..` segment |
| `invalid_out_path` | `--out` is not a new absolute path under an existing directory |
| `invalid_source_path` | `--path` is not an absolute path to an existing readable file of at most 256 MiB |
| `invalid_number` | a bound with its number: `--limit` 1-200, `--depth` 1-8, `--pages` 1-5, `--rows` 1-200 |
| `invalid_layer_name` | `--as-layer` is empty, longer than 80 characters, or holds a control character |
| `invalid_date` | a `--since` that is not `YYYY-MM-DD` or RFC 3339 |
| `invalid_link` | a `--link` that is not `pm_task:<id>`, `pm_record:<id>` or `ds_object:<type>:<id>`, or a second one — a tree read filters on one link |
| `asset_not_found` | no asset or folder has this id or path, an undeclared parent folder, a task or object the link names that does not exist; a `confidential` row the caller may not read answers this too, by design |
| `asset_class_forbidden` | the signed-in user lacks the assets capability this class or this write requires |
| `asset_version_conflict` | the row or folder moved while a write was in flight, or a folder is already declared at that path; re-read and issue the command again |
| `asset_refused` / `asset_request_invalid` | the catalogue's own rule or bound, named in the message (the rule in brackets) with the number |
| `assets_not_implemented` | this lane's ds-brain does not serve this action yet |
| `assets_service_failed` | the catalogue service faulted (HTTP 5xx, `detail.http_status`); retry once |
| `asset_too_large` | the bytes are above the 32 MiB read bound; open the asset from its own surface |
| `origin_read_failed` | the signed read expired, the bytes did not match the row's digest, or the destination could not be written; retry once |
| `origin_read_unavailable` | a projected `sys:` row was named; its bytes are served by the surface that owns it, not by the catalogue |
| `unknown_folder` | a `--folder` names a path no declared folder has; declare it with `folder`, or read the declared folders with `tree` |
| `invalid_member` | `--member` is absolute, has a `..` segment, or is not a member of the container; copy the exact path from `tree --into <asset>` |
| `asset_not_geographic` | `promote` named an asset that is not `geo` and no geo member; only a geographic asset or member promotes |
| `invalid_payload` / `malformed_descriptor` / `local_layer_refused` | the prepared local layer store's own refusals on `promote`, in the words `ds map local` uses |
| `assets_unreadable` | the catalogue answered a shape this build cannot fold; report it with the project id |
| `requires_window_retired` | `--desktop-descriptor` was passed; drop it |

`--since 01-09-2026` is refused here rather than at the catalogue on purpose: a
transposed day and month is the commonest filter mistake there is, and
`2026-01-09` for the ninth of September is a perfectly valid date that quietly
lists the wrong eight months.

## Shared reporter outputs

`assets.reference` registers catalogue metadata for an existing verified
reporter output without uploading or copying its bytes. `assets.resolve` reads
that reference through an exact tag or transformer link. City maps and sizing
tables use city tags. Missing or ambiguous results permit manual Solar entry.
Read the live command descriptors for authority and inputs, and the owning
[shared-network contract](../../../ds-solar/docs/contracts/shared-network-assets.md)
for the producer/consumer boundary.
