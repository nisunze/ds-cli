# `ds desktop` — instances, and which one a command is for

Tier-4 reference. Each command's `--help` is its contract; this is the shape
they share. Sibling: [`ds desktop status`](desktop.status.md),
[`ds desktop project`](desktop.project.md). Owner of every rule below:
`ds-command-kernel/src/desktop_instance.rs`
(`docs/contracts/ds-desktop-instance.md`).

## An instance, not an install

A machine can run several DS GridDesign runtimes at once: Stable beside Canary,
two windows of one build, a developer's local build beside an installed one. So
the unit of pairing is one **live instance**, identified by 32 lowercase hex
characters that the instance mints when it starts.

Each running instance publishes its own descriptor:

```
<app data>/cli-bridge.d/<instance_id>.json   one file per live instance
<app data>/cli-bridge.json                   the legacy file, for an older ds
```

…where `<app data>` is that install profile's Tauri identifier directory —
`$XDG_DATA_HOME/<identifier>/` on Linux, `%APPDATA%\<identifier>\` on Windows,
`~/Library/Application Support/<identifier>/` on macOS — for the four profiles
`stable`, `canary`, `dev` and `dev-canary`.

`ds` reads **both**: an instance that publishes its own file and refreshes the
legacy one is de-duplicated to a single instance, by identity and by endpoint,
before anything else happens. Presenting one instance twice is a defect the
kernel refuses outright, deliberately, so the de-duplication has to be right
here rather than tolerated there.

A descriptor that names no instance id — written by a desktop that predates
them — is given one **derived** from what it does say: its profile, its
loopback origin and its pid. That identity is stable while the descriptor is,
so an older desktop can be listed and targeted; it changes when the instance
restarts, because a new port and a new pid are a new instance. `ds desktop
list` reports which kind an id is (`minted` or `derived`), and a derived id is
only valid until that instance restarts.

## Liveness is an authenticated handshake, never a connection

A descriptor is a candidate only after the bridge answers its own
`GET /v1/session` with the pairing secret inside that file, 200, a session
revision, within 150 ms, and without a redirect. A TCP connection proves only
that *something* took the port; a stale file whose port an unrelated local
listener now holds must not out-rank a live desktop, and must never receive
more than that one bounded probe.

The token itself is only ever sent to the exact numeric loopback origin the
descriptor names: scheme `http`, host literally `127.0.0.1`, an explicit
non-default port, no userinfo, no query, no fragment. `http://127.0.0.1@evil.example/`
parses as *userinfo* — that is the case the rule exists for, and it is refused
before a byte of the secret leaves this process.

## Naming a host: one flag, both of them

```
--target desktop                     this machine's native client (the default)
--target desktop:<instance_id>       one named live instance
--target server                      the running `ds server serve`
DS_TARGET=…                          the same value, for a whole session
```

`--target` is one flag with one grammar, because CLI/MCP → Desktop or Server is
no difference at all: one command id, the same arguments, the same answer,
whichever host runs it. `DS_TARGET` is a **default** for the flag, never an
override of it — a command that named a host has named it.

`--desktop-descriptor <path>` (and `DS_DESKTOP_DESCRIPTOR`) is a different
question and stays: it names one descriptor **file**, is used verbatim, and is
what the desktop's own `cl` terminal pins so a shell it opened keeps talking to
the window that opened it. Naming both a file and an instance is allowed, and
they must agree — the instance the file reaches is asked who it is, and a
descriptor that belongs to another instance is refused rather than used.

## How an instance is chosen

The kernel decides; `ds` performs. In order:

1. **Compatible** means the same lane, the same account and the same credential
   audience as the caller. A development-lane instance, or one signed in as
   somebody else, is not a candidate for this caller's work — and is not
   disclosed to it either.
2. **An explicit target wins and never falls through.** Not among the live
   instances → `desktop_target_not_live`, *even when exactly one other instance
   would have been the automatic answer*. Live but incompatible →
   `desktop_target_mismatch`, with the reason. A target that is not an instance
   id is refused before any candidate is read.
3. **Automatic routing only on exactly one match.** None → `desktop_not_paired`.
   More than one → `desktop_ambiguous`, listing the choices. Focus, recency,
   list order and the last-active window never decide: the answer is a function
   of the *set* of live instances, and every list is ordered by instance id.
4. **A project narrows; it never switches.** For project work, only instances
   that have the selected project open — as the session's project or in one of
   their windows — are eligible.

When `ds` has no identity of its own on this machine (no native profile is
configured), it adopts the identity of the live instances instead. One signed-in
account routes as usual; two different accounts signed in at once is an
ambiguity only the caller can settle, and it refuses the same way.

## A saved CLI selection never moves a live map

Before 2026-09-12, a map command whose CLI project differed from the window's
switched the window and then ran. It no longer does. The selected project is a
condition on *where the work may go*; when no eligible instance has it open the
answer is:

```
desktop_project_not_open
  → open it in the app, or switch one explicitly with
    ds desktop project switch --target desktop:<instance_id> --project <project>
```

That switch is now the only way a `ds` command moves a window's project, and it
proves it landed: the same instance, the same principal, and that project open
when it answered. A scripted `ds map …` that relied on the implicit switch needs
the explicit switch first, or a window already on that project. The cost is
honest and it is the point — two windows on two projects stay two projects.

## `ds desktop list`

```
$ ds desktop list
2 live instances
  1f0c…  canary  canary   arjgpydw_survey_test     ready  ← yours
  8a41…  stable  stable   no project               signed_out
```

Each row is the kernel's own safe projection — instance id, install profile,
lane, open project, build, start time, window count — plus whether this client
identified the instance by a minted or a derived id, and whether it is in a
state that can serve work (`ready`, `signed_out`, `contract_mismatch`).
`compatible` names the ids this account may use, when `ds` knows which account
is running it.

Never a pairing token, never a bridge address, never an account uid or email:
those fields do not exist on the projection, so no command can decide to
include them. `unusable` names any descriptor file that exists and cannot be
used, with the reason — an operator whose app is running and whose `ds` says
"not paired" needs to be told which file is wrong.

Nothing running is an answer, not a failure. The same is true of
`ds desktop status`, which describes one instance and asks the caller to name
one when several are live.

## Refusals

| Code | When | Remedy |
|---|---|---|
| `desktop_not_paired` | no compatible live instance | start DS GridDesign and sign in |
| `desktop_ambiguous` | more than one could serve it | `ds desktop list`, then `--target desktop:<instance_id>` |
| `desktop_target_not_live` | the named instance is not live | `ds desktop list`, then name a live one |
| `desktop_target_mismatch` | live, but another lane, account or audience | name one that can serve it |
| `desktop_project_not_open` | no eligible instance holds the project | open it, or switch explicitly |
| `desktop_contract_mismatch` | a live instance published a session this build does not speak | update DS GridDesign and `ds` together |
| `descriptor_unusable` | a descriptor file exists and cannot be used | restart DS GridDesign to republish it |
| `desktop_unreachable` | a named descriptor's port answers nothing | it may have exited; restart and retry |
| `context_generation_stale` | the window switched project or account mid-operation | retry against the view as it is now |
| `unknown_target` | `--target` is not one of the three hosts | pass `desktop`, `desktop:<instance_id>` or `server` |
| `target_host_unsupported` | `--target server` for an operation only the desktop performs | run it with `--target desktop` |

`desktop_not_paired` names nothing on purpose: an instance signed in as another
account is another person's session on this machine, and its existence is not
this caller's to learn. `ds desktop list` shows the operator their own machine
and answers that question directly.

## Compatibility

The descriptor stays **version 1** and every field added to it is optional, in
both directions: an older `ds` reading a newer file sees the four fields it
knows, and a newer `ds` reading an older file derives an identity rather than
refusing. A version bump would have made every older `ds` refuse every command
until both sides were upgraded together, and there is no window for that.

One consequence worth knowing: the descriptor's shape is the kernel's, and it
is closed. A new field must land in `ds_command_kernel::desktop_instance` — and
in a `ds` built against it — before a desktop publishes it.
