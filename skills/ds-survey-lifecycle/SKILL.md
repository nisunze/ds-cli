---
name: ds-survey-lifecycle
description: "Use DS survey data for progress, coverage, quality checks and design handoffs; prepare offline collection and manage forms or project templates. Start from the user's question, not command discovery."
metadata:
  ds-chapters: survey
---

# Survey: from field evidence to a useful answer

A survey records what someone observed, where and when. Use it to plan field
work, follow progress, check collected evidence or prepare a design input.
Survey observations are not automatically an approved design, a construction
quantity or proof that every asset was visited.

## Choose the job first

Recover the requested project, purpose, area/period and deliverable from context.
Ask only for missing scope that would change the answer. The user need not know
form slugs, field keys, command names or internal storage.

| The user needs… | Why survey data helps | Start here |
| --- | --- | --- |
| “How many poles did each team survey?” | Measure recorded activity and plan the next visit. | `survey.query`; [read data](references/read-data.md). |
| “Where do we still need to survey?” | Compare collected evidence with the intended area or asset list. | Aggregate first; `survey.entries.select` only for spatial evidence. Coverage needs a denominator. |
| “Are coordinates or required observations missing?” | Find records needing review before design or handover. | Relevant form + bounded aggregate filters; do not fetch every row by default. |
| “Give the designer the surveyed assets in this area.” | Preserve observed locations and identities as design inputs. | Bounded spatial selection; state which required attributes it cannot supply. |
| “What changed since the last delivery?” | Refresh a downstream copy without rereading everything. | `survey.entries.changes`; retain the completed checkpoint. |
| “Our team will work without connectivity.” | Prepare known forms and retain captures until publication succeeds. | [Field capture](references/field-capture.md). |
| “Set up or reuse our collection forms.” | Define observations consistently for one project or future projects. | [Configuration](references/configuration.md). |

## Use the shortest supported route

Use the base `ds` skill. Once the installed build/lane and project are known,
go directly to `ds capabilities <command-id> --output json` for the chosen
operation. Reuse that contract within the same build/session; do not repeat the
domain catalogue or read all three references for every question. Search the
Survey domain only if the named command is absent or the intent needs another
operation. Read a form only when its fields/settings are needed or unknown.

Use `auth.project.status` for native selected-project identity. List the selected
project's bindings with `survey.project-forms.list` to resolve an unknown slug;
a global Form Factory list is not a project's participating forms. Read live
availability/authority: older installations may still pair control commands to
Desktop. A map is needed only for an operation that consumes map-owned state.

`native_profile_not_configured`, a missing command or a mismatched release is
an installation/contract problem, not an empty survey. Follow its remedy once;
do not loop login, switch projects, open unrelated repositories or use raw HTTP,
Firestore, IndexedDB or extracted credentials to manufacture an answer.

## Finish with evidence the next person can use

Return the requested answer or artifact, with project, exact forms, area/time
filters, source freshness and completeness. A count of records is not a count of
unique physical assets unless the identity rule supports it. Missing/blocked,
empty and truncated results are different. Do not label a partial result “all”.

Keep exact entry identities, form revisions, receipt/digest or completed cursor
state with the requested deliverable when it must be resumed or handed off.
Use the task's destination, not a personal path. Reads do not authorize changing
forms, copying projects or publishing captures. Carry existing authorization
through the workflow; `--yes` is for the user's specific authorized write.
