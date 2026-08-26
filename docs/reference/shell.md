# `ds shell` — reference

Tier-4 reference. `ds shell <command> --help` is the contract.

## The problem it solves

DS GridDesign installs `ds` beside itself — `%LOCALAPPDATA%\DS GridDesign\`
or `…\DS GridDesign Canary\` on Windows, `/usr/bin` from the Linux package.
The Linux location is on every shell's PATH already. The Windows one is on
nobody's: until something registers it, `ds` runs from the app's own
command-line launcher (`cl`) and from nowhere else, and the chatbot bootstrap
that starts with `ds doctor` cannot start.

`ds shell` is that something, and it is the way to find out what a terminal
will resolve `ds` to before trusting a script to it.

## Two questions, kept apart

| Question | Command field | What answers it |
|---|---|---|
| Does **this shell** resolve `ds` to this executable? | `reachable`, `resolves_to` | the PATH of the process that ran the command |
| Will a **new shell** resolve `ds` to this executable? | `registration.new_shells_see` | Windows: `HKCU\Environment\Path` · Unix: a `~/.local/bin/ds` link, or a system directory |

They differ more often than one would think. A terminal opened before the
install keeps the PATH it started with, so `reachable` is false while
`new_shells_see` is true — and the remedy is "open a new terminal", not
"register again". `ds doctor` folds the two into one word:

| `doctor.shell.status` | Meaning |
|---|---|
| `reachable` | this shell finds it |
| `registered` | only a new shell will |
| `unreachable` | neither; run `ds shell register` |

## What `register` writes, and what it never does

Exactly one entry: the directory holding the executable that ran the command.

- **Windows** — appended to the user's `HKCU\Environment\Path` through the
  Win32 registry API, written back as `REG_EXPAND_SZ` so `%VAR%` entries keep
  expanding, then `WM_SETTINGCHANGE` is broadcast so a terminal opened from
  the Start menu sees it without signing out. The machine PATH is never
  touched — the installer is a per-user install and so is this.
- **Unix** — a symlink `~/.local/bin/ds` to the executable. When the
  executable already lives in `/usr/bin`, `/usr/local/bin` or `/bin`, there is
  nothing to register and the command says so. Whether `~/.local/bin` is on
  PATH is reported, never edited: rc files belong to the operator.

It appends rather than prepends. A user who deliberately put a different `ds`
first keeps it; `ds shell status` lists every other `ds` on the PATH under
`others` so the choice is visible rather than silently overridden.

`unregister` removes that one entry and leaves every other entry's order and
spelling exactly as it found them. A `~/.local/bin/ds` that points somewhere
else is not ours to remove (`link_foreign` on register, untouched on
unregister).

## Who runs it

- The desktop installer's post-install hook runs `ds shell register`; its
  pre-uninstall hook runs `ds shell unregister`. An upgrade therefore leaves
  exactly one entry, whatever order the old uninstall and the new install run
  in.
- The `cl` shortcut in the desktop opens a terminal with the install directory
  *prepended* to that terminal's PATH and `DS_DESKTOP_DESCRIPTOR` set, so it
  works before and regardless of registration, and always runs the build that
  opened it.
- A person with a source build: `ds shell register` once, from the built
  binary.

Two profiles (Stable and Canary) both registered means both directories on
the user PATH, first installed first. `others` makes that visible; `cl` from
either app always gets its own build.

## Refusals

| Code | Class | Meaning |
|---|---|---|
| `executable_unresolved` | failed | the running executable cannot be located on disk |
| `registration_unreadable` | failed | the user's registration cannot be read |
| `registration_unwritable` | failed | the user's registration cannot be written |
| `link_foreign` | conflict | `~/.local/bin/ds` exists and is not a link to this executable |

## Related

- `ds desktop status` — pairing, which `cl` pins with `DS_DESKTOP_DESCRIPTOR`
- `ds-web/desktop-installer-hooks.nsh` — the installer's register/unregister calls
- `ds-web/src-tauri/src/command_line.rs` — the `cl` launcher
