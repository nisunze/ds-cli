# Survey working-area forms

**Status:** implemented 2026-09-20. Parent: [The application kernel](ds-application-kernel.md)
§2, [Surface order](ds-surface-order.md) (kernel + `ds` first, the UI reads).
Sibling store family: [layer state](ds-layer-state.md) §2 (preferences).

## 1. The defect this settles

The map's working area loaded the survey entries of EVERY active form of the
project as soon as a working area was applied: ds-web's `loadAllSurveyData`
walked `survey_layers` and ran one `query_entries` per form. A project with a
dozen forms paid a dozen reads before the operator asked for any.

## 2. The decision

`survey_working_area_forms::evaluate` (schema `ds.survey-working-area-forms/v1`,
WASM `surveyWorkingAreaForms`) answers, for one project's form catalogue — the
`survey_layers` of the assembled layer document the Layers API already returns,
so no new backend read is spent — and the selection the host remembers for
(lane, DS account, project):

| Op | Answer |
| --- | --- |
| `read` | every form with `selected`, its `layer_ids` and a `count` the host already knew (never fetched); `loads` = the selected slugs in catalogue order; `chosen` (did the operator ever choose here); `stale` (chosen slugs the project no longer has, reported never loaded); `remedy` when nothing loads |
| `select {forms}` | replace the selection with exactly these forms; `unknown_form` names every slug that is not a form of the project; `no_forms_named` for an empty list; duplicates fold; `changed: false` when nothing moved |
| `all` / `none` | replace with every form / with no form — both are choices |
| `clear` | forget the choice: `next_selection: null`, back to never chosen |

Rules:

- **Never chosen loads nothing.** An absent selection is `chosen: false`,
  `loads: []` and the remedy that names the select command. "All" is a
  choice the operator makes, never a default the kernel supplies.
- **Project fence.** The document must be about the named project or the
  answer is `project_context_changed`; a stale document after a switch is
  never applied.
- **Transitions return what to persist**: `next_selection` is the entry
  (`null` = remove it) and `changed` says whether the host writes at all.
  The kernel holds no store.

## 3. Hosts

- **`ds` natively and on a Server** (`ds survey working-area forms|select|clear`,
  `--target desktop|server`): the document through the native client
  (`ds-layer-ops`), the selection in `ds-layer-store::working_area_forms` —
  a sibling file of the drawer's `layer-visibility.json` under the same
  `<lane>/<uid>/<project>` key and the same one-file/one-lock/atomic-replace
  mechanics (`ds-layer-store::scoped`). A Server route answers the same
  three operations for the project the caller names.
- **Browser** (hosted web, no Server): the document is the held `MapState`,
  the selection lives beside the layer visibility preferences in IndexedDB,
  and the vendored WASM answers the same question. The working area loads
  `loads` and nothing else.

## 4. Proof

`tests/fixtures/survey-working-area-forms-cases.json`, replayed by
`tests/survey_working_area_forms_cases.rs` and by ds-web over the vendored
WASM (byte-identical copy under `src/tests/fixtures/survey-working-area-forms/`).
