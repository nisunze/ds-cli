---
name: ds-survey-lifecycle
description: Manage forms and project templates through `ds survey`.
metadata:
  ds-chapters: survey
---

# Survey lifecycle through `ds`

Use this for the survey control plane, including network-aware forms. It assumes
the base `ds` skill: discover the installed command and obey its live contract.
Never call the API directly, inspect IndexedDB, extract a JWT, or implement a
missing command inside the skill.

The paired MCP may start the matching Stable or Canary application when a
signed-in desktop authority is required. That does not mean the map must be
open. Every command here takes explicit project or template identity and works
quietly through the app-owned API session.

## Keep the four objects distinct

- **Form Factory form:** one global master schema.
- **Project-form binding:** enabled state and settings for that form in one
  project.
- **Project template:** a reusable snapshot of project-form configuration.
- **Project created from a template:** a new independent project instance.

Applying a template changes an existing project. Creating a project from a
template creates a different project. Creating a template snapshots an existing
project; it does neither of those things.

## Discover before writing

```text
ds capabilities survey --output json
ds capabilities --search 'form template project settings' --output json
ds capabilities <selected-command-id> --output json
```

For a complex new master form, read the backend vocabulary first:

```text
ds survey form types --output json
ds survey forms list --query pole --detail --output json
ds survey form read --slug lv_poles_survey --output json
```

Hypothetical requests that should route here include:

- “Create valve, junction, reservoir, and pipe forms for a water-network
  survey.”
- “Make the pipe form an edge and the junction form a node for project A.”
- “Turn project A's survey configuration into a reusable template.”
- “Create a new project from that template.”
- “Disable an unavailable archived binding without touching its missing master.”

Water is only an example. Never infer topology setting keys from the domain
word. Read the live field types and the target form's settings editor.

## Author a global form

Create or update only from an explicit bounded JSON document. Read the form
immediately before an update and echo its version:

```text
ds survey form create --schema ./form.json --yes --output json
ds survey form update --slug water_pipe --expect-version 3 --schema ./patch.json --yes --output json
```

Use `survey form lifecycle` for duplicate, publish, unpublish, archive,
restore, or delete. Archive/delete are dependency-aware; `--force` deliberately
creates unavailable project/template bindings, so never add it merely to clear
a refusal.

## Configure one project's forms

Read the explicit project and one form's backend-owned editor:

```text
ds survey project-forms read --project project-a --detail --output json
ds survey project-form editor --project project-a --form water_pipe --output json
```

Build a JSON change array using only keys returned in `editor.sections`. A row
with `settings` must echo `editor.version` as `expected_version`; an enable-only
row omits it and preserves settings. Plan, inspect every row, then apply the
same file only when ready:

```text
ds survey project-forms plan --project project-a --changes ./changes.json --output json
ds survey project-forms apply --project project-a --changes ./changes.json --yes --output json
```

An unavailable binding may be cleaned independently with exactly an
enable-only `false` row. Do not edit or re-enable it until its master is
restored. The absent master is a refusal/control case, never a prerequisite for
listing forms, managing templates, creating a project from a template, or
editing another project's bindings.

## Reuse configuration

```text
ds survey template create --project project-a --name 'Water Network' --yes --output json
ds survey template read --template water_network --output json
```

Then choose exactly one intent:

```text
ds survey template apply --project existing-b --template water_network --merge-strategy preserve --yes --output json
ds survey project create-from-template --template water_network --project-name 'New Water Survey' --yes --output json
```

Use `survey template lifecycle` only for the reusable catalogue object. It does
not retroactively mutate projects created from or updated by the template.

## Map boundary

Use `ds map` only when the operation genuinely consumes map-owned local state,
such as Working Area materialization, local geometry, viewport state, or survey
record migration. Form schemas, project-form settings, templates, and project
creation stay under `ds survey` and require no open map.
