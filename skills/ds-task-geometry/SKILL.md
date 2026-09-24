---
name: ds-task-geometry
description: "Give a Project Work task the geometry of the DS Grid structures or alignment a comment names, through `ds pm task create --geometry-from` / `ds pm task geometry set` — typed references, a dry-run proposal, then the person's --yes. Never draw, never parse prose."
metadata:
  ds-chapters: project
---

# Put a task where the work is

A consultant's comment says "structures 74, 76 and 77 stand in a swamp". The
task that answers it has to carry WHERE, so the map and the plan show the
area. You do not draw it and nothing in the stack reads the prose: you supply
the structure numbers you read as a **typed reference**, the kernel resolves
them against the model deterministically, you show the proposal, the person
confirms.

1. Find the model. `ds dsgrid model list --lane <lane> --account <uid>
   --output json` names this machine's DS Grid working copies; take
   `.data.models[].model` for the one the comment is about. A `.dsgrid` file
   (e.g. a head from `ds dsgrid project download`) is referenced as
   `dsgrid:package:…` with `--package <path>` instead.
2. Write the reference from what the comment names, nothing more:
   structures → `dsgrid:local-<id>:structure:74,76,77`; a whole line →
   `dsgrid:local-<id>:alignment:<id-or-label>`; a stretch of it →
   `dsgrid:local-<id>:alignment:<aln>:74..77`. Numbers are the engineering
   numbers a consultant writes; an ambiguous number is refused with the
   candidates and their ids — use the id, never a guess.
3. Propose, and show it. `ds pm task create --title "<what the comment
   asks>" --kind inbox --geometry-from <reference> --dry-run --project <exact-id> --lane <lane>
   --output json` (or `ds pm task geometry set --task <id> --from
   <reference> --project <exact-id> --dry-run …` for a task that exists). Read
   `.data.proposal`: `rule.kind` (`point`, `buffered_hull`, `alignment_line`,
   `alignment_range`), `objects[]` (each with its number, id and model
   revision) and `links[]`. Put that in front of the person. Nothing was
   written and no `--yes` was needed.
4. Write only on their word: the same command with `--yes` instead of
   `--dry-run`. One revision carries the geometry and one `ds_object` link
   per structure. `work_revision_conflict` means the plan moved — re-read,
   propose again.
5. Prove it: `ds pm task geometry read --task <id> --project <exact-id> --lane <lane> --output
   json` shows `geometry` and `objectLinks[]`; `ds pm plan --project <exact-id>` and the map's
   tasks layer paint from the same value.

`--buffer-m` (default 25 m, 1..500) is the margin that turns several
structures into an area; say what you chose. `ds pm task geometry clear
--task <id> --project <exact-id> --yes` removes the geometry and only the DS Grid object links.

Read the live contract before inventing flags: `ds capabilities pm --output
json`, then `ds capabilities pm.task.geometry.set --output json`.

## When not to use this

- A survey entry or a transformer: link it (`survey_entry` link; the
  work-template subject flow). Their geometry is never copied into a task —
  `transformer:` and `survey:` references are refused by name.
- Drawing an arbitrary area — that is the map's drawing lifecycle, in the
  application, not this CLI.
- Reading or changing the model itself — `ds-grid-project-model`,
  `ds-grid-plscadd`.

If `ds pm …` answers `headless_signed_out`, the native session is not signed
in. If it answers `missing_input` for `--project`, name the exact project on
the request. Do not switch a saved selection to resolve either refusal.

Stops at: the proposal is the person's to confirm. Never pass `--yes` on a
proposal nobody read.
