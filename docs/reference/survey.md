# Survey control plane

`ds survey` manages the API-backed survey lifecycle without requiring an open
map or an active project in DS GridDesign. Pass the exact project id whenever a
command is project-scoped. The paired MCP host may launch the matching Stable
or Canary desktop application to obtain its signed-in authority, but the CLI
never reads IndexedDB, copies a JWT, or owns token refresh.

Four related objects have separate lifecycles:

1. A **Form Factory form** is a global master schema. Use `survey forms list`,
   `survey form read`, `survey form types`, `survey form create`, `survey form
   update`, and `survey form lifecycle`.
2. A **project-form binding** enables a master form for one project and stores
   that project's settings. Use `survey project-forms read`, `survey
   project-form editor`, `survey project-forms plan`, and `survey project-forms
   apply`.
3. A **project template** is a reusable snapshot containing project-form
   configuration. Use `survey templates list`, `survey template read`, `survey
   template create`, `survey template apply`, and `survey template lifecycle`.
4. A **project created from a template** is a new, independent project. Use
   `survey project create-from-template`. Applying a template instead modifies
   an existing project.

## Safe discovery order

For a new complex form, first read `ds survey form types --output json`, create
the master schema from a bounded JSON document, then read the target project's
bindings and the new form's backend-owned settings editor. Do not invent
network keys: the editor's `sections`, `field_state`, and `capabilities` are the
authoritative vocabulary for that form.

For example, a hypothetical water-network request can use node forms for
valves, reservoirs, or junctions and edge forms for pipes. The domain is only
an example—the same workflow applies to electrical, telecom, road, or other
survey networks. The LLM should discover the legal settings from the live
editor instead of assuming field names from the example.

## Project-form changes

`--changes` names a UTF-8 JSON file containing 1 to 32 unique rows:

```json
[
  {
    "form_slug": "water_junction",
    "enabled": true,
    "settings": {
      "is_network_element": true,
      "network_element_type": "node"
    },
    "expected_version": 3
  },
  {
    "form_slug": "water_pipe",
    "enabled": true
  }
]
```

A row containing `settings` must echo `expected_version` from the immediately
preceding `survey project-form editor` response. An enable-only row deliberately
has no settings revision and preserves existing settings. Always run `survey
project-forms plan` before the confirmed `survey project-forms apply`; apply
rechecks live editors and refuses stale or unknown settings.

Unavailable bindings are returned separately from resolved forms. A missing or
archived master may be cleaned without restoring it by planning and applying an
enable-only `false` row. Settings edits and re-enabling remain refused until
the master exists and is active again.

## Templates and projects

These two requests are intentionally different:

```text
ds survey template apply --project existing-water --template water-network --merge-strategy preserve --yes
ds survey project create-from-template --template water-network --project-name "New Water Survey" --yes
```

The first changes an existing project's project-form configuration. The second
creates a new project instance. Creating a template is different again: `survey
template create` snapshots a named source project's current configuration into
a reusable catalogue object.

## Complex-form lifecycle and refusals

Keep the master-form lifecycle separate from a project's binding lifecycle. A
safe complex-network sequence is: discover `form types`; create the master
schema; read it; update it with the returned version; publish it; use the
project-form editor to configure the explicit project binding; then unpublish,
archive or delete only with the exact dependency result the backend returns.
`survey form lifecycle` is the master transition door; `project-forms plan` /
`apply` are the project-settings door; neither creates a project template or a
new project instance.

Every transition can refuse. A stale `--expect-version` is a concurrency
refusal; archive/delete can refuse live bindings unless an operator explicitly
uses `--force`; a missing or archived master is returned as an unavailable
binding and permits only an enable-only `false` cleanup. Follow that refusal
through the same `ds survey` contract—do not open the map, reconstruct settings
from a cached form, or make template management conditional on an unavailable
form.

Commands under `ds map` remain reserved for operations that genuinely consume
map-owned local state, such as Working Area transfer or survey-data migration.
Form Factory, project-form settings, project templates, and project creation
are API control-plane operations and stay usable with no map open.
