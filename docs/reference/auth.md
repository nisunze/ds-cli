# Native authentication and project context

`ds auth` is the initial native authority seam shared with `ds-web` through
`ds-client-core`. It is independent of the paired Desktop bridge. Existing
commands that declare `desktop_pairing`, `desktop_user`, or legacy `project`
authority are unchanged and still require DS GridDesign.

The release resource is exactly `ds-client-profiles/catalog.json` under the
running product root. Linux packaging compiles either `/usr/lib/DS GridDesign`
or `/usr/lib/DS GridDesign Canary` as that root; Windows uses the executable's
sibling resource. The bounded catalog contains exactly nested `stable` and
`canary` entries and its exact bytes must match the digest compiled into `ds`.
If packaging has not supplied both values, auth commands report
`native_profile_not_configured`. Debug builds alone may name one complete
`development: true` bundle with `DS_NATIVE_CLIENT_PROFILE_BUNDLE`; there are
no per-field environment overrides.

Catalog schema v3 retains the four exact transformer-context fields: `POST`,
`/api/v1/data`, `get_transformers_data`, and the `context` projection. They are
validated as exact bytes and do not create a generic request surface. Because
these fixed call fields participate in the credential audience, upgrading
from a v1 credential intentionally appears signed out and can require
`ds auth login` followed by `ds auth project use`; credentials and project
contexts are never silently migrated across that audience change. Schema v3
also fixes project forms to `POST /api/v1/project-forms`, action `activate`;
the v2-to-v3 audience change can likewise require login and project selection
again rather than silently widening an older credential.

The current ds-cli CI release build intentionally injects neither the catalog
digest nor product root, so its auth surface is typed unavailable. The desktop
packaging owner must generate/stage the catalog and inject both compile-time
values before an installable release may claim native auth is configured.

`auth login` defaults exactly to Stable and reads a hidden controlling-TTY
password. `--password-stdin` explicitly reads one line, bounded to 4096 bytes.
Passwords and tokens are never accepted in argv or environment variables.
MCP children cannot open the prompt. Only a rotating refresh credential is
durable; the ID token stays in process memory and is zeroized by the core.

On Unix, refresh state and project context live below the per-user DS config
root (`DS_CONFIG_HOME` as an explicit absolute override, then absolute XDG
config, then an absolute `HOME` plus `.config`). Directories are owner-only,
files are mode 0600, reads
refuse symlinks/hard links/oversize data, writes are atomic and fsynced, and
OS advisory locks serialize refresh rotation and context changes across
processes. A crash-left plaintext stage has one destination-derived name and
is removed only while the next process holds that destination's exact lease;
unsafe stage links are refused untouched. Lock waits are bounded. The
Linux-first slice explicitly refuses
native state on Windows until a DPAPI plus `LockFileEx` adapter is shipped; it
does not claim that ordinary plaintext files are protected there.

`auth project list` refreshes all Active, Archived, and Testing buckets and
emits a bounded projection (`--limit`, default 100, maximum 1000). `auth
project use` always fetches the complete fresh directory, requires one exact
visible `ds_project`, then saves a context fenced by deployment lane, stable
credential audience, Firebase UID, and canonical email. A project ID alone is
never authority. Harmless profile provenance/public-key rotation does not
invalidate the context; changing the stable audience, account, or lane does.

Every auth command declares `local_auth_state`: even status/list can rotate a
durable refresh credential, while login/logout/project use can also clear or
replace context.
Confirmed Firebase permanent revocation or identity mismatch clears both the
exact credential and its context. Transient, unreadable, and conflict failures
do not silently delete the context. Logout is authority `none`, so cleanup remains callable even
when the credential is stale or absent.
