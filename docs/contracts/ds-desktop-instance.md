# Desktop and Server instances — which live runtime is this operation for?

**Status:** the kernel owner is implemented and proven on
`fable/desktop-instances`, 2026-09-12; the ds-web shell, the browser and the CLI
that consume it are the rest of the slice, and nothing below is a claim that the
installed Desktop already behaves this way. Parent:
[project and instance isolation](../../../ds-web/docs/project-and-instance-isolation-contract.md)
§"Desktop instances and CLI targeting" and acceptance items 1 and 4. Sibling:
[the execution context](ds-execution-context.md), whose vocabulary and bounds
this reuses. Consumer: [`project_context`](#7-what-project_context-decides-now).

## 1. The question this settles

Two DS GridDesign windows are open on two projects, a Server is running, and a
CLI or MCP caller issues a command. **Which runtime does it go to, and what
happens to the reply if the operator switches project while it is in flight?**

Answered once in `src/desktop_instance.rs` (`ds.desktop-instance/v1`, WASM
`desktopInstance`):

| Question | Op |
|---|---|
| May this published descriptor be used at all, and who is it? | `admit_descriptor` |
| Which live instance is this operation for? | `select` |
| What may a client show about the instances it found? | `list` |
| A view's project or account changed — by how much? | `advance_generation` |
| May a reply captured before that still be applied? | `admit_result` |
| What is this request called, so its completion names its instance? | `mint_request_id` / `read_request_id` |

Pure: every list is bounded, no clock, no filesystem and no network enter it,
and the same request always has the same answer. Hosts perform; nothing here
reads a file, sends a token or moves a window.

**One registry, both hosts.** A Desktop instance *is* a Server instance with a
webview (the owner's boundary ruling of 2026-09-12, quoted in
[ds-execution-context](ds-execution-context.md) §2b: "a Desktop instance IS a
Server instance with a webview; the desktop descriptor and the Server
`connection.json` converge into one instance registry"). So this owner's
`Descriptor` and `Candidate` are one shape for both: the Server's
`connection.json` fields (address, owner, lane, token) and the Desktop's
`cli-bridge.json` fields (version, url, token, pid) are the same record with
`instance_id`, `build`, `started_at_ms`, `profile` and `windows` added. One
enumeration answers `ds desktop list` and `ds server status`; one
`--target desktop:<instance_id>` / `--target server[:<instance_id>]` names either.

## 2. The descriptor, and why it stays version 1

```
Descriptor { version: 1, url, token, pid,          // the four fields v1 always had
             instance_id?, lane?, build?,          // added, all optional
             started_at_ms?, profile? }
```

`version` stays **1** and every field added is optional, in both directions:

- an **older `ds` reading a newer file** sees the four fields it knows and
  ignores the rest (its reader has no `deny_unknown_fields`);
- a **newer `ds` reading an older file** derives an identity rather than
  refusing (§3), so no operator whose app and CLI upgrade apart loses pairing.

A version bump would have made every older `ds` report `descriptor speaks
version 2` on every command until both sides were upgraded together. There is no
compatibility window for that, so there is no version bump.

`admit_descriptor` also answers the rules that used to live in
`ds-cli-desktop::discover::read`, moved here as decisions with cases: the
pairing token is only ever sent to **the exact numeric loopback origin** —
scheme `http`, host literally `127.0.0.1`, an explicit non-default non-zero
port, no userinfo, no query, no fragment, path `""` or `"/"` — normalised once,
here, to `origin().ascii_serialization()`. `http://127.0.0.1@evil.example/`
parses as userinfo and would send the secret to a remote host; that is the case
the rule exists for, and it is in the corpus with nine siblings.

**The token cannot reach an answer.** `Descriptor` has no `Serialize` and a
`Debug` that prints `<redacted>`; `Admission`, `InstanceSummary` and every
refusal detail are separate types with no token field. The corpus asserts the
outcome rather than the intent: no recorded answer contains any token, uid or
audience its own request handed in.

## 3. Instance identity

`instance_id` is 32 lowercase hex characters, **minted by the instance at
start**. A descriptor that carries none gets one **derived** here, from
`sha256(schema ‖ "derived" ‖ profile ‖ origin ‖ pid)`:

- it is stable while that descriptor is, so a client can enumerate an old
  desktop, print it and target it within one enumeration;
- it **changes on restart** — a new ephemeral port and a new pid derive a new
  instance — so a dead descriptor's id can never be answered by the process that
  replaced it, and `--target desktop:<old id>` correctly becomes
  `desktop_target_not_live`;
- it is per profile, so Stable and Canary are never one instance;
- two spellings of one origin (`…:41234` and `…:41234/`) derive one identity,
  because the *normalised* origin is the input.

`Identity` on the answer says which it was: `minted` or `derived`.

A **window** is an identity inside an instance: `{label, project, generation}`.
The kernel routes to an *instance*; a caller may pin a window and then the route
carries it, and the shell dispatches only there. Which window serves an
unpinned operation is the shell's own rule (its owner window), not a decision
made here — see §8.1.

## 4. Selection

```
select(candidates: SET, target: Option<Target>, requirement: Requirement) -> Route | Refusal
```

`Candidate` is the admitted descriptor plus the authenticated handshake:
`{instance_id, profile, lane, uid, audience_sha256, project, build,
started_at_ms, session_revision, windows}`. `Requirement` is the caller's own
identity plus what the operation is about: `{lane, uid, audience_sha256,
project?, project_independent}`.

The rules, in order:

1. **Compatible** means same lane, same uid, same credential audience. A
   development-lane instance and an instance signed in as another account are
   not candidates for this caller's work — and are not disclosed to it either.
2. **An explicit target wins and never falls through.** Not among the live
   candidates → `desktop_target_not_live`, *even when exactly one other
   instance would have been the automatic answer*. Live but incompatible →
   `desktop_target_mismatch` with `reason` ∈ {`lane`, `account`, `audience`}. A
   named window that is not open → `reason: window`. A target that is not an
   instance id → `reason: malformed`, refused before any candidate is read.
3. **Automatic routing only on exactly one match.** None → `desktop_not_paired`.
   More than one → `desktop_ambiguous`, listing the choices. **Order never
   decides**: `select` is a function of a set, every list this owner emits is
   sorted by instance id, and the corpus replays each multi-candidate case in
   every ordering and requires identical bytes.
4. **A project narrows; it never switches.** When the operation is about a
   project and is not project-independent, only instances that have it open —
   as the session's project or in one of their windows — are eligible. None →
   `desktop_project_not_open`, naming every compatible instance that *does* hold
   it, with the explicit switch as the remedy. Several → `desktop_ambiguous`.

`Route { instance_id, window }` names the selected candidate by its own
identity, so the answer is the same under every permutation and the host looks
the descriptor back up by id.

## 5. The context generation

A view's generation is a single integer, ≥ 1, advanced by **`project_switch`,
`account_change` and `sign_out` and by nothing else**. A handler captures it at
entry and asks `admit_result(captured, current)` before writing anything:
equal → apply; older → `context_generation_stale`; newer → a hard fault,
because a result cannot be captured after the view it is for.

It is deliberately **not** `session_revision`. That counter is process-wide and
is bumped by every session publish including a map `moveend`, and every webview
window publishes into it — so it moves for reasons that are not context changes,
and with two windows open it races. A generation that moves when a map animates
would refuse honest work and admit stale work. Both numbers are reported;
only this one fences.

Generations are bounded at `Number.MAX_SAFE_INTEGER`, because the browser
compares them as doubles and a value the kernel decided must be the value
JavaScript reads.

## 6. The request id

`cli-<instance_id>-<n>` — 32 hex, then a decimal sequence from 1 with no
padding. The old grammar was `cli-<pid>-<n>`, which named a process and not an
instance; `cli-4711-1` is now `request_id_malformed`, as is a case-shifted id
and a padded sequence (one request, one spelling, so a completion cannot be
replayed under a second text). This is what lets the shell bind a completion to
the window that was dispatched rather than to any window that calls back.

## 7. What `project_context` decides now

`project_context` is a consumer: `desktop_instance::select` chooses the
instance, `Context::of(&Candidate)` builds the fence from that same decision,
and `route` decides only what happens inside it.

- `route` takes a `Switch`. Every ordinary paired operation passes
  `Switch::Never`, so a caller whose saved project differs from the instance's
  is refused `desktop_project_not_open`. Only
  `ds desktop project switch --target desktop:<id> --project <p>` passes
  `Switch::Explicit` and gets `Route::SwitchDesktop`.
- `IdentityMismatch` became `Refusal`, which publishes its own typed code:
  `auth_context_mismatch`, or the same `desktop_project_not_open` this owner
  raises, so one remedy answers the same situation whichever owner reached it.
- `switch_completed` now also requires the re-observation to come back from the
  instance the switch was sent to.

## 8. Refusals, and the remedy each carries

| Code | When | Detail | Remedy |
|---|---|---|---|
| `descriptor_unusable` | a published descriptor cannot be used | `reason` | restart the app, or name a descriptor path |
| `desktop_contract_mismatch` | a live instance's handshake is not the contract | `field` | update DS GridDesign |
| `desktop_not_paired` | no compatible live instance | — (nothing is named) | start DS GridDesign and sign in |
| `desktop_ambiguous` | more than one could serve it | `candidates` | name one with `--target desktop:<instance_id>` |
| `desktop_target_not_live` | the named instance is not live | `target` | `ds desktop list`, then name a live one |
| `desktop_target_mismatch` | live, but cannot serve this | `target`, `reason` | name one that can |
| `desktop_project_not_open` | no eligible instance holds the project | `project`, `instances` | open it, or `ds desktop project switch --target desktop:<id> --project <p>` |
| `context_generation_stale` | the view changed while that was in flight | `captured`, `current` | retry against the view as it is now |
| `request_id_malformed` | not `cli-<instance_id>-<n>` | `request_id` | — |

Safe metadata is exactly `InstanceSummary { instance_id, profile, lane, project,
build, started_at_ms, windows }`. Not a token, not a url, not a uid, not an
email — those fields do not exist on the type, so no host can decide to include
them. The one place an address is answered is `admit_descriptor`'s normalised
`origin`, to the host that already holds the file and is about to send the token
there; no refusal carries one, and the corpus asserts that too.

## 9. Acceptance mapping

Contract bullets, §"Desktop instances and CLI targeting":

| Bullet | Where |
|---|---|
| Unique instance identity; explicit identity per window | §3; `instance_id` minted or derived, `Window{label, project, generation}` |
| A second launch may not silently reuse another window and change its project | the kernel half: identity is per process (§3) and a project difference refuses rather than switches (§4.4, §7). The shell half — per-instance descriptor files, ownership before write, removal on exit — is ds-web's, and **`tauri-plugin-single-instance` must not be added**: a redirect is the prohibited behaviour, not the fix for it. |
| Enumerate with safe metadata; explicit target; automatic only on exactly one; ambiguity refuses before an effect; focus/recency/order never decides | §4, `list`; the permutation replay is the proof of the last clause |
| An explicitly targeted dead or mismatched instance refuses and never falls through; PID/port reuse cannot impersonate | §4.2, and §3 — a restart derives a different identity, and a minted id is a new random value per process |
| A project/account switch advances that view's generation; late responses cannot mutate the new view | §5 |

Acceptance items:

- **Item 1** (two instances on different projects, explicit routing, equal
  names, same-project windows): cases `select-the-instance-that-has-the-project`,
  `select-a-named-instance-among-several`,
  `select-equal-display-names-are-still-two-projects` (nothing here has ever
  seen a display name — only ids), `select-the-same-project-in-two-instances`,
  `select-a-named-window`. Kernel-level only: the end-to-end proof with two real
  loopback bridges is the slice's proof phase.
- **Item 4** (stale/ambiguous descriptors, instance restart, port reuse,
  explicit missing targets, zero misrouted effects): the descriptor cases,
  `descriptor-derived-identity-follows-the-port` / `-the-process`,
  `select-a-named-dead-instance-never-falls-through`, and the corpus test
  `no_recorded_selection_routes_anywhere_but_the_instance_that_was_named`.

## 10. What hosts keep

Hosts perform I/O on these answers and decide nothing. **ds-web's shell** mints
an `instance_id`, writes one descriptor per instance, refreshes the legacy
per-profile file only when no other live instance owns it, removes its own on
exit, publishes `instance_id`/`build`/`windows` from `/v1/session`, mints
request ids through `mint_request_id` and binds completions to the dispatched
window. **ds-web's browser** publishes the session from the owner window only,
advances the generation from `switchProject` / `deactivateProject` /
`clearAuthenticatedSessionRuntime` through `advance_generation`, wraps every
bridge handler in a capture-then-`admit_result`, and raises the typed
`desktop_project_not_open` where it raises an untyped `project_mismatch` today.
**ds-cli** enumerates descriptors, admits each through `admit_descriptor`,
de-duplicates by instance id *before* asking (presenting one instance twice is a
hard fault, deliberately), probes each with the authenticated handshake, and
relays these codes verbatim. **The kernel holds no token and opens no socket**:
liveness is the host's authenticated probe, and this owner is only told the
result.

## 11. Decisions to challenge

1. **Automatic `SwitchDesktop` is retired.** Before this slice, any map command
   whose CLI project differed from the window's switched the window
   (`project_context::map_uses_cli_target_and_user_operations_leave_ui_alone`).
   Under the owner's ruling of 2026-09-12 — a saved CLI selection must never
   flip a live map — it now refuses, and the operator switches explicitly. The
   cost is honest: a scripted `ds map …` that relied on the implicit switch now
   needs `ds desktop project switch --target desktop:<id>` first, or a window
   already on that project.
2. **A derived identity names a live descriptor, not a durable runtime.** For a
   desktop old enough to publish no instance id, `--target desktop:<id>` is
   valid only until that instance restarts. The alternative — deriving from the
   profile alone — would be stable across restarts and would therefore let a
   successor answer to its predecessor's name, which is the impersonation the
   contract forbids. Chosen: stability loses to non-impersonation.
3. **`desktop_not_paired` names nothing.** An instance signed in as another
   account is not listed, not counted and not hinted at, so `select` cannot be
   used to learn who else uses this machine. The cost is a thinner error when an
   operator is simply signed into the wrong account; `ds desktop list` shows
   their own instances and answers that question directly.
4. **The kernel routes to an instance, not to a window, unless one is named.**
   Two windows of one instance on the same project are not an ambiguity here —
   the contract explicitly wants same-project windows to work — so the shell's
   owner window serves an unpinned operation. If a later slice needs
   window-level automatic routing, it is a rule added here, not in the shell.
5. **`admit_descriptor` answers the normalised `origin`.** Every other answer is
   address-free, but the whole point of admitting a descriptor is to tell the
   host the one address the token may go to. The host already holds the file, so
   nothing is disclosed that it did not have.
6. **A candidate's lane is open text; the requirement's is closed.** A
   development build publishes `local` and must still enumerate honestly, so
   `Candidate.lane` admits any bounded value and compatibility filters it, while
   `Requirement.lane` is `stable | canary` and anything else is a host bug. This
   replaces `desktop_operation_unsupported` for the selection path.
7. **`bounded()` is repeated here rather than shared.** The predicate is four
   lines and `execution_context`'s copy is private to that module; the *bounds*
   (`MAX_UID_CHARS`, `MAX_PROJECT_CHARS`) are imported, so the two owners can
   disagree about a value but never about a number. Promoting one text predicate
   to a shared helper is a tidy-up for a later slice, not a rule.

Proof: `tests/fixtures/desktop-instance-cases.json`, 78 recorded cases —
byte-identical to ds-web's `src/tests/fixtures/desktop-instance/cases.json` and
replayed there through the vendored WASM — plus 18 unit tests in
`src/desktop_instance.rs` and 6 in `src/project_context.rs`.
