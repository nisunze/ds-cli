# `ds desktop project` — reference

`desktop project list` reads the signed-in application's bounded project
repository. `desktop project switch` changes only the paired application's
active project context; it does not write project data and it does not drive
navigation, map controls, or design tools.

The list returns exact project ids. A switch accepts only one of those exact
ids and asks the application's existing `switchProject` transition to perform
the change. That transition owns project-scoped cache activation and closes
the prior live edit context while retaining its IndexedDB room under the prior
project key.

After switching, run `ds desktop status --output json` and verify its `project`
before any project-authority command. When multiple desktop profiles are
running, pass the same explicit `--desktop-descriptor` to list, switch, status,
and the subsequent project work.
