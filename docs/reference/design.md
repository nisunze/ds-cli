# `ds design` — reference

Tier-4 reference. `ds design <command> --help` is the contract; this document
is the part that does not belong in any command's help because it is true of
all of them.

The product contract these commands serve is
`ds-brain/docs/contracts/design-collaboration-roadmap.md`.

## Where design collaboration is

Not on disk, and not reachable with a credential this process holds.

Saved selections, attachments, tags and comment threads are governed project
state behind ds-brain, which is the only gateway and the only authority: it
decides who may write, it arbitrates two people editing the same record in the
same second, and it refuses a write authored against a version that has since
moved. So every command here is one named semantic operation the *paired
application* performs under the session it already holds. `ds` sends a request
and receives an outcome. It never receives a credential, and it never runs code
inside the application — `docs/reference/desktop.status.md` has the pairing
argument in full.

That is also why there is no `--project` flag anywhere in this domain. The
active project is the one the application has open; a project id passed as an
argument would be a claim `ds` has no standing to make.

## Why this is not `ds map`

Every command here is metadata. None of them needs a map instance, an edit
session, or an open design room: a selection is a list of stable identities, an
attachment is bytes with a media type, a tag is a value from the project's own
vocabulary. `ds map` owns local map state; this domain owns none. Putting these
under `ds map` would make headless collaboration require an open map, which is
exactly what the contract says it must not.

## The shape of a session

```bash
ds design selection list                                  # what is saved
ds design selection read --selection sel-week-32          # who is in it, right now
ds design selection assign --selection sel-week-32 \
  --title "Review LV designs" --owner nixon@example.com --yes

ds design attachment list --kind mv_model --object mv_line_a
ds design attachment publish --kind mv_model --object mv_line_a \
  --path ./MV_LINE_A.bak --version rev_2 --yes

ds design tag list --kind lv_transformer --object kigali_a
ds design tag set --kind lv_transformer --object kigali_a \
  --definition transformer_scope --values additional_scope --yes

ds design comment list --kind lv_transformer --object kigali_a
ds design comment post --thread thread-clearance --body "Agreed, re-spot it." --yes
```

## The three rules the whole domain rests on

### 1. A read never substitutes

`ds design selection read` evaluates membership server-side and reports every
member as `present`, `changed` or `missing` — under the label it was saved
with. Nothing is swapped in for a member that has gone. Because a transformer
rename mints a NEW document identity and retires the old one, a renamed member
reads as missing under its old name. That is the honest answer, and it is the
one a person needs in order to go and find where it went.

### 2. What is assigned is pinned

`ds design selection assign` re-evaluates membership, refuses if it moved since
the read, and writes an immutable receipt carrying the selection's version, its
member digest and the exact transformer ids that resolved at that moment. The
task carries a link to the selection, never a copy of the transformer data — so
editing the selection afterwards cannot change what somebody was asked to do.

### 3. Nothing overwrites

An attachment revision is immutable: each one owns its own storage object, its
own server-verified SHA-256 and its own generation, so publishing a new `.bak`
for a later model version sits alongside the earlier one. A comment is
append-only; there is no `ds design comment edit`, because there is no such
server action. Retiring an attachment or archiving a selection is soft and
reversible, and `--restore` brings it back.

## Bounds, and how they are reported

Every list is bounded and every bound is reported. `--limit` is a page (1–200)
and the matched `total` always comes back, so a short page says so rather than
ending quietly. An attachment whose revisions exceed the server's page reports
`more: true` on that attachment rather than looking complete.

A comma-separated list flag is bounded locally as well as on the server, so an
over-long `--transformers` or `--values` is refused before a round trip that
would have been rejected anyway.

## Where `ds` deliberately stops

**Publishing a large attachment.** The paired desktop reads a named path
through a bounded reader, so `ds design attachment publish` refuses a file
larger than that bound with `attachment_too_large` and names the Attachments
dialog, which streams from the file picker. Truncating the file to a preview
and registering a revision against the wrong bytes would be worse than
refusing.

**Redacting a comment.** Redaction is a moderator's audited action that clears
text the server does not retain. It stays in the application, where the
moderator can read what they are about to remove before they remove it.

**Promoting a tag definition to a global template.** That is the one design
action that leaves the project boundary, and it carries its own capability. It
belongs to the governance surface, not to a headless command.

## Refusal codes

| Code | What it means |
|---|---|
| `design_not_permitted` | the signed-in user may read but not change these records |
| `design_version_conflict` | the record moved while the command was in flight; re-read and retry |
| `design_project_read_only` | the project is archived or expired and accepts no changes |
| `invalid_design_anchor` | the anchor names a reserved document or a kind that does not exist |
| `attachment_too_large` | the file exceeds the desktop's bounded path reader |
| `invalid_value_list` | a comma-separated flag was given but carries no values |
| `too_many_values` | a list flag carries more entries than the record accepts |
| `missing_comment_target` | neither `--thread` nor a complete `--kind`/`--object`/`--title` |

The pairing refusals (`desktop_not_paired`, `desktop_ambiguous`,
`desktop_unreachable`, `pairing_rejected`, `desktop_signed_out`) are the shared
set every bridge domain uses; `ds map --help` documents them once.
