# `ds desktop status` — reference

Tier-4 reference. `ds desktop status --help` is the contract.

## Why pairing rather than a second login

When DS GridDesign is running it already holds a signed-in Firebase session, a
selected project, and live map/design context. A second CLI login would mean a
second identity to manage, a second token to store, and a second thing that
can be signed in while the other is signed out.

So `ds` borrows the application's authority instead. The application publishes
a private descriptor; `ds` finds it, authenticates to a random-loopback bridge
with the short-lived pairing secret inside it, and asks the application to
perform named semantic operations using the session it already has.

Two invariants make this safe, and both belong to the application:

- the bridge **never returns the Firebase JWT or a refresh token**, so no
  credential can become a CLI argument, a log line, or an agent's context;
- the bridge accepts only a **closed set of named operations**, so possession
  of the descriptor buys the ability to ask for a known thing — never the
  ability to run arbitrary code inside the application.

Possession of the descriptor is therefore a *transport* proof. It says a
process on this machine may talk to the app. It does not say who is asking,
and it can never authorize a project write on its own.

## Descriptor discovery

**The unit of pairing is a live instance, not an install.** Since 2026-09-12
each running instance publishes its own descriptor under
`<app data>/cli-bridge.d/<instance_id>.json`, one file per instance, and the
first one also refreshes the legacy per-profile `cli-bridge.json` below so an
older `ds` keeps pairing. `ds desktop list` enumerates them and `--target
desktop:<instance_id>` names one; the whole rule is in
[`ds desktop`](desktop.md), and what follows is where the files live.

Each install profile is a distinct Tauri bundle identifier, and each holds its
instances' descriptors:

| Profile | Identifier |
|---|---|
| stable | `rw.datasolutions.desktop` |
| canary | `rw.datasolutions.desktop.canary` |
| dev | `rw.datasolutions.desktop.dev` |

The registry directory `cli-bridge.d/` and the legacy `cli-bridge.json` are in
that identifier's app-data directory:

| Platform | Location |
|---|---|
| Linux | `$XDG_DATA_HOME/<identifier>/` or `~/.local/share/<identifier>/` |
| Windows | `%APPDATA%\<identifier>\` |
| macOS | `~/Library/Application Support/<identifier>/` |

The source-backed XRDP launcher deliberately uses its own
`rw.datasolutions.desktop.local-dev` identity. It is **not** a fourth
auto-discovery profile: it is a developer harness and must be named with
`--desktop-descriptor <path>` when a test deliberately pairs to it. That keeps
an ordinary `ds` invocation from silently mixing source-run state with an
installed Stable, Canary, or dev desktop.

Automatic discovery applies one bounded, authenticated handshake to every
descriptor it finds, so dead files left by exited instances do not create false
ambiguity. **Real ambiguity is refused, never resolved by preference.** Two
live instances at once produce `desktop_ambiguous` naming both, and order,
focus and recency never decide:

```
$ ds desktop status --output json
{"…","error":{"code":"desktop_ambiguous","detail":{"instances":[
  {"instance_id":"1f0c…","profile":"canary","lane":"canary","project":"…"},
  {"instance_id":"8a41…","profile":"stable","lane":"stable","project":null}]}}}
```

Settle it with `--target desktop:<instance_id>`, from `ds desktop list`. Or
with `--desktop-descriptor <path>`, which names one descriptor file and is used
verbatim and never second-guessed.

`DS_DESKTOP_DESCRIPTOR` names the same thing for a whole session. The
desktop's `cl` command line sets it in every terminal it opens, so that
terminal stays pinned to the window that opened it however many profiles are
running. Precedence is fixed: the flag, then the variable, then automatic
discovery — the variable is a default for the flag, never an override of it.

A descriptor is rejected if it is oversized, unparseable, declares a version
this build does not speak, or **does not point at loopback**. The bridge is
loopback by construction; a descriptor naming anything else is not one to hand
a pairing secret to.

## Why this command is always available

It is tempting to make `status` report `unavailable` when no desktop is
running, so `doctor` says something about the environment. It would also be
circular: this is the command whose job is to report whether a desktop is
running. Gating it on a desktop running means the one call that could explain
the situation is the one call that refuses to.

So "not paired" is a **success**:

```json
{
  "paired": false,
  "signed_in": false,
  "project": null,
  "design_context": null,
  "reason": "no_session",
  "remedy": "start DS GridDesign, then run `ds desktop status`",
  "searched": ["stable", "canary", "dev", "dev-canary"],
  "unusable": []
}
```

When exactly one instance is live, `status` describes it and reports its
`instance` id and how that id was identified (`minted` by the instance, or
`derived` by the kernel from a descriptor that predates them). When several
are, it refuses rather than choosing — the same rule every other paired
command follows, and for the same reason.

Commands that genuinely need the session declare `Authority::DesktopUser` and
report unavailable through their own check. Those are what make `doctor`
informative, without making the diagnostic itself undiagnosable.

## What is never in the output

The pairing token. Any bearer credential. The Firebase JWT or refresh token.
`Descriptor` deliberately has no `Debug` derive, so the secret cannot be
formatted into a result by accident, and `cli.rs` asserts the absence.

The bridge publishes only the paired session view and the fixed typed CLI
operations. `status` reports only the pairing/session fields, never a browser
cache, workspace path, token, or generic application state. When a transformer
is open in the project design editor, the bounded context is explicit:

```json
{
  "project": "arjgpydw_survey_test",
  "design_context": {
    "mode": "edit",
    "transformer": "agasharu",
    "project": "arjgpydw_survey_test",
    "context_type": "edit",
    "editor_ready": true,
    "map_ready": true,
    "dirty": false,
    "staged": false,
    "persisted": false
  }
}
```

`design_context` is otherwise `null`. It contains no layers, geometry,
selection, undo history, or local cache state. Older paired applications that
publish only `mode` and `transformer` remain readable; update DS GridDesign to
receive the readiness and mutation-state fields.

## Refusals

`ds desktop status --help` is the live list, with the remedy for each. What is
worth saying here is what the seven codes are *about*, because they divide into
three different failures that look alike from outside:

| Code | Meaning |
|---|---|
| `desktop_ambiguous` | more than one profile is responsive; name one with `--desktop-descriptor` |
| `descriptor_unusable` | a descriptor exists but is unreadable, stale, or not loopback |
| `desktop_unreachable` | the descriptor names a port nothing answers on |
| `pairing_rejected` | the application refused the secret; the descriptor is stale |
| `desktop_refused` | the session answered and declined the status request |
| `desktop_unreadable` | the reply could not be read within its bound |
| `desktop_contract_mismatch` | the reply does not match this build's contract |

The first three are about *finding* a session, the next two about *reaching*
one, and the last two about *understanding* what came back. Branch on
`error.code`; the class and exit code arrive with the envelope and follow the
output contract's table.

"Not paired" appears nowhere above, because it is a success — see the previous
section.

## Related

- `ds-web/src-tauri/src/cli_bridge.rs` — the bridge, and the closed operation list
- `ds-web/src/lib/desktop/cli-bridge.ts` — the typed CLI operations themselves

## `ds desktop sync plan` — the sync gate's one decision, headless

Authority `none`: no pairing, no credential. Name the scope — `--project <id>`,
or `--personal` for the signed-in account's own bytes (user data, notes), which
go through the same gate — hand the kernel what an install
holds (`--local <file>`) and what the shared record holds
(`--remote <file>`), optionally the work grant in hand (`--grant <file>`),
`--now-ms` (server-observed time; defaults to this machine's clock), `--offline`,
`--trigger` (why the plan is asked for now — `startup`, `navigation`,
`local_change`, `publish_completed`, `download_completed`, `reconnect`,
`remote_push`, `manual`, `grant_opened`; never "because time passed") and
`--remote-read-at-ms` (when the held remote heads were last read; omit when
never), and read back the ordered actions a host performs — `open_grant`
(always first when an upload needs one), `upload`, `download`, `conflict` (a
head that moved past the base a result was computed on; surfaced, never
overwritten), `nothing`, `refused` (a malformed or duplicate row, refused on its
own so one bad row never poisons the queue) — plus `refresh_remote` (whether the
remote heads must be re-read for this trigger: never on a local change, never
after a publish whose receipt carries the head), `wake` (`event`, or the one
timed wake: a grant renewal before a pending upload) and a summary. There is no
poll: the record is read on navigation, reconnect, an explicit sync or a push
from the messaging stream. Contract: `ds-command-kernel/docs/contracts/ds-sync-engine.md`.

Inventory shapes: local rows are
`{engine, operation, variant?, sha256, size_bytes, produced_at_ms, base_revision?, readable?, engine_release}`;
remote heads are `{engine, operation, variant?, revision, sha256, published_at_ms}`;
the grant is `{grant_id, install_id, project, engines[{name, release}], expires_at_ms}`.
Refusal `sync_plan_invalid` names the file or the field the kernel refused.
