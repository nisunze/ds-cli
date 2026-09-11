# Configure observations, without redesigning a project by accident

Keep four objects distinct: a global master form; its enabled state/settings in
one project; a reusable project template; and a new project created from one.
An ordinary data question needs none of these changed.

## Master forms

For a new form, read `survey.form.types` once and a relevant existing master if
reuse is appropriate (`survey.forms.list`, then `survey.form.read`). Build one
bounded schema document using the returned vocabulary. Use `survey.form.create`
for creation, `survey.form.update` with the immediately read `--expect-version`
for changes, and `survey.form.lifecycle` for publication/archive/restore/etc.
A global form change can affect more than the requested project's binding.
Do not add `--force` merely to remove a dependency refusal.

## One project's bindings

Read `survey.project-forms.read --project <id> --detail`, then
`survey.project-form.editor --project <id> --form <slug>` for each form being
changed. Selected-project `survey.project-form.settings` is also a settings-read
door. Native project arguments must match the selected project; older releases
may expose different pairing requirements, so obey live authority.

Stage only keys returned in the editor's sections. Rows containing `settings`
echo that editor's version as `expected_version`; enable-only rows omit settings
and preserve existing values. Then:

```text
ds survey project-forms plan --project PROJECT --changes ./changes.json --output json
ds survey project-forms apply --project PROJECT --changes ./changes.json --yes --output json
```

Inspect the plan and apply the same authorized file. Read back the saved
bindings. On conflict, refresh/review against the new version rather than
forcing stale intent. An unavailable master permits only enable-only `false`
cleanup; it does not block work on unrelated forms/templates.

For network forms, learn node/edge and qualifying-form keys from the editor.
Do not infer them from “water”, “electricity” or a remembered schema. Preserve
observations and existing entry identities when revising collection settings.

## Reuse configuration

- `survey.template.create`: snapshot a source project's configuration.
- `survey.templates.list` / `survey.template.read`: choose and inspect a template.
- `survey.template.apply`: change an existing project; retain the intended merge
  strategy and read back the result.
- `survey.project.create-from-template`: create a different independent project.
- `survey.template.lifecycle`: change the reusable catalogue object, not projects
  already created from it.

Finish with exact object IDs, saved revisions and the project actually affected.
Do not copy another project's entries merely because its form setup is useful.
