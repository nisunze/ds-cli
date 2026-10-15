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

This is the **only** way a `ds` command moves a live window's project. An
ordinary paired command whose selected project is open in no eligible instance
refuses with `desktop_project_not_open` and names this one; it never switches a
window on its own. The switch also proves it landed: the same instance, the
same principal, and that project open when the instance answered.

After switching, run `ds desktop status --output json` and verify its `project`
before any project-authority command. When several instances are running, name
one with `--target desktop:<instance_id>` — from `ds desktop list` — on the
list, the switch, the status check and the project work that follows, so all
four are about the same window. See [`ds desktop`](desktop.md).

`desktop list`, `desktop project list` and `desktop project switch` accept
`--lane stable|canary`, using the shared native credential lane grammar. The
default remains Stable. Canary callers must pass `--lane canary`; older builds
refuse that input as `unknown_flag` and require a qualified CLI/Core update.
This selects the caller's protected native session for routing. It does not
change a window's profile, copy credentials, switch accounts, or relax the
existing principal, lane, audience, instance and project fences. A mismatched
window continues to refuse `desktop_target_mismatch` before any effect.
`desktop status` remains a credential-independent observation of the named
window and needs no lane argument.

For the friendly Local development window, name its exact observed instance
with `--target desktop:<instance_id>` and the real protected native credential
lane with `--lane canary` or `--lane stable`. New CLI builds admit this explicit
pair only when the window and native principal have the same UID and credential
audience. The window remains Local; automatic Local selection and installed
Stable/Canary crossings remain refused. Existing project, session revision and
window generation fences still apply. Older CLI builds refuse this combination
with `auth_context_mismatch`; update the CLI, without copying credentials or
changing the development profile.
