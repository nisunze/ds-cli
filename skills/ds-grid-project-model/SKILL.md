---
name: ds-grid-project-model
description: Create, import, select, and publish DS Grid project models through the governed local-model lifecycle. Use for local `.dsgrid` working copies and immutable project versions, not PLS-CADD conversion.
metadata:
  ds-chapters: grid-model
  ds-mcp-profile: grid-local-model
---

# Manage a DS Grid project model

Use the `ds` skill first. Keep three states distinct:

- acquisition creates or imports a model in the paired application's local store;
- active means one local model occupies Profile and editing;
- publication registers one immutable revision in the selected project and does not change the active model.

Read the live descriptor before each command:

```text
ds capabilities dsgrid.model.list --output json
ds capabilities dsgrid.model.create-local --output json
ds capabilities dsgrid.model.import-external --output json
ds capabilities dsgrid.model.set-active --output json
ds capabilities dsgrid.publish-version --output json
```

Start with `ds dsgrid model list --output json`. Then choose exactly one local acquisition:

- `ds dsgrid model create-local ... --output json` creates an empty model and normally makes it active.
- `ds dsgrid model import-external --path <absolute.dsgrid> ... --output json` acquires a package without activating it.

Use the returned opaque model id with `ds dsgrid model set-active --model <id> --output json` only when that model should occupy Profile. Repeating it for the active model is idempotent.

Local list, create, import, and set-active require a paired application but no project. Never add or infer a project argument, inspect IndexedDB, send package bytes through the bridge, or treat local activity as project authority.

Publish separately with `ds dsgrid publish-version`. For a new project model, name the local model plus the authored display name and declared kind. For an existing project model, name its generated id and the expected head when known; do not pass a new name. Add `--yes` only after the operator authorizes this exact project write. Require the receipt to report `status: published`, its immutable revision and digest, and `active_model_changed: false`. A failed local binding after publication does not undo the committed version.

Do not retry a moved-head conflict or change projects to force publication. Re-read the project model, review the new head, and ask for a fresh publication decision.

## PLS-CADD sources

A PLS-CADD workspace or `.bak` is not a local-model import. Discover and use `dsgrid-exchange.inspect`, `dsgrid-exchange.plan`, and `dsgrid-exchange.convert` to produce a new `.dsgrid`; validate it, then acquire that package with `model import-external`. Never add a second convert-and-publish route to this workflow.
