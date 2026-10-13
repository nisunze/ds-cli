# `ds` reliability reporting and proactive feedback

**Status:** specification, owner direction 2026-10-07 — **priority**. The
reporter (§3) is `crates/ds-client-core/src/reporter.rs`; `ds` wires it in
`ds-cli/crates/ds/src/reliability.rs`. Remaining slices and proofs are in
[`backlog/OPEN.md` block 25](../../backlog/OPEN.md#25-cli-reliability-reporting-and-proactive-feedback-priority).
Open owner questions are in §9.

## 1. The gap

The owner has to ask, again and again, for errors to be logged and feedback to
be filed. Today:

- **`ds` reports nothing to SRE.** The request-event store
  (`ds-sre/REQUEST_EVENT_STORE_CONTRACT.md`) receives `client_api` and
  `client_runtime` rows from ds-web, `forwarded_compute` rows from ds-brain and
  `cloud_run_system` memory kills. A `ds` command that fails, times out, panics
  or is refused on a Desktop, Server or headless host leaves no trace. The
  gateway operation exists (`domains.sre.client_request_event`,
  `POST /api/v1/sre/requests`), but ds-brain stamps every client event
  `service="ds-web"`, so a `ds` event would be mislabelled.
- **Feedback is a last resort.** The `ds` skill reaches feedback only "after
  ruling out a stop and trying other vocabulary", and `feedback.submit` says
  "use only after live capability discovery confirms the task is unsupported or
  materially broken". Friction, confusing help, surprising output, slow
  commands, workarounds and ideas — the most useful signals — are never filed
  unless the owner asks.

## 2. The rule

Owner ruling, 2026-10-07: coding agents never submit feedback during coding
sessions. They solve findings or put unfinished work in OPEN.md, OPEN-X.md or
SEEDING.md according to the owning backlog split, then remove proved fixes.
The agent feedback duties below apply only outside coding sessions.

Two channels, separate and linked:

| | **SRE event** | **Feedback report** |
| --- | --- | --- |
| Records | what happened: one terminal outcome of one `ds` invocation | what should change: expected behaviour, observation, acceptance |
| Written by | `ds` itself, automatically, every time | the agent or person using `ds`, without being asked — plus `ds` itself for crashes (§5.3) |
| Needs the user? | never | never; the user is told the report ids afterwards |
| Store | `ds-sre` request-event store | the shared feedback backlog |
| Read with | `ds sre events --source cli_command` | `ds feedback list` |

The two are joined by `request_id` / `sre_event_id`, so triage sees the
evidence behind a report and SRE sees which faults already have one.

## 3. SRE reporting from `ds`

### 3.1 What is recorded

At most **one event per invocation** (one CLI process, or one MCP tool call),
written when it ends:

| Result class | Example | Store `outcome` | Counts against availability |
| --- | --- | --- | --- |
| `failed` | transport error, 5xx, timeout, `retryable` error, internal error | `failure` | yes |
| `crashed` | panic, abort, unexpected exit | `failure` | yes |
| `refused` | a declared refusal: `invalid_input`, `confirmation_required`, `feedback_not_permitted`, signed-out | `success` | no — the contract worked |
| `ok` | — | not sent per call; see §3.4 | — |

Refusals are recorded because repeated refusals of one command are the best
signal of an unclear contract; they never count as platform failure.

### 3.2 Event shape — a new client source

`source = "cli_command"`, added to the store's source enum. ds-brain stamps the
trusted fields from that source (`service = "ds"`, `container_kind = "client"`,
`profile = "client"`, `user_email` from the JWT or `anonymous`) instead of the
hard-coded `ds-web`.

```jsonc
{
  "source":        "cli_command",
  "platform":      "cli" | "mcp",
  "host_kind":     "desktop" | "server" | "headless",
  "os":            "linux" | "windows" | "macos",
  "release":       "ds 0.9.12 (kernel 2997236)",
  "lane":          "stable" | "canary" | "local",
  "action":        "data.vector.buffer",            // command id
  "action_class":  "report" | "async" | "orchestration" | "infra",
  "contract":      2,                               // command contract version
  "result_class":  "failed" | "crashed" | "refused",
  "outcome":       "failure" | "success",
  "error_code":    "vector_engine_empty",           // the refusal or error code
  "error_class":   "invalid_input" | "transport" | "internal" | …,
  "retryable":     false,
  "latency_ms":    412,
  "status":        502,                             // gateway status when one exists
  "request_id":    "…",                             // the X-Request-Id ds sent; joins brain rows
  "user_action_id":"inv_…",                         // this invocation
  "ds_project":    "kigali_north",                  // only when --project was passed
  "flags":         ["--source", "--radius-m"],      // flag NAMES present, never values
  "agent":         "claude-code",                   // MCP clientInfo.name or --agent when known
  "error_message": "…"                              // sanitized, ≤ 2048
}
```

### 3.3 Privacy — what never leaves the machine

Argument values, file paths and names, file contents, GeoJSON, survey answers,
customer or parcel data, tokens, headers and environment values. The
`error_message` passes the same sanitizer ds-web uses, then drops any path-like
or value-like substring. A test feeds sentinel values through every argument of
a failing command and asserts none appears in the event.

### 3.4 Delivery

- **Never changes the result.** Exit code, stdout, stderr and the JSON envelope
  are byte-identical with reporting on or off.
- **Bounded and non-blocking.** The send has a 1.5 s budget at exit; MCP sends
  off the request path.
- **Offline outbox.** A failed send appends to a bounded outbox in the
  owner-only DS state directory on disk (never `/tmp` or `/dev/shm`): at most
  200 events / 256 KiB, oldest dropped first, drops counted. The next
  invocation with network flushes it.
- **Dedupe and rate limit.** Identical `(action, error_code)` within 30 s is
  one event with `occurrences`; at most 30 events per minute per host.
- **Successes as one daily rollup.** `ok` calls are counted locally per
  command and sent as one `cli_rollup` event per host per day, so availability
  has a denominator without a Firestore write per call (Firestore cost
  discipline).
- **No recursion.** The reporter never reports its own send failure.
- **Tests and CI** set `DS_SRE_REPORTING=off`; development builds report on
  lane `local`.
- **How the code reads this section.** A `ds` error envelope's class decides
  the result class: `internal`, `failed` and `unavailable` (transport errors,
  timeouts, missing owners) are `failed`; `invalid_input`, `unauthorized` and
  `conflict` (signed out, stale view) are `refused`; a gateway 5xx is `failed`
  whatever its class. The send carries the gateway key and no bearer, so the
  store attributes it to `anonymous`. The state is one document,
  `$XDG_STATE_HOME/ds/reliability.json` (else `~/.local/state/ds`), behind a
  create-new lock. A process an MCP adapter runs for one tool call
  (`DS_CLI_SURFACE=mcp`), a discovery command (`doctor`, `capabilities`,
  `version`, which promise no network), and an ordinary command whose declared
  authority is `none` only record. Local failures retain their journal, outbox
  and automatic feedback queue without sending or filing at exit. A later
  network-authorized invocation or the existing MCP adapter owns delivery;
  the adapter's own process sends every 60 s and at its exit. No-authority
  offline computation and missing-addon refusals make zero telemetry requests.

### 3.5 Visibility

- `ds sre events --source cli_command` and `--action <command-id>` read them.
- `ds doctor` shows reporting state, outbox size and the last flush.
- The existing SRE overview gains a `ds` client row: fault rate and the top
  failing and most-refused commands.

## 4. The invocation journal

`ds` keeps a small local journal of its last 50 invocations (command id, flag
names, result class, error code, latency, request id, time — the same privacy
rules as §3.3) in the DS state directory. It is what makes filing cheap:

```text
ds feedback draft --last [--n 3] --output json
```

returns a ready `feedback.submit` payload for the most recent failed or refused
invocation(s): title, component (the owning repository, from the command
registry), kind, severity guess, evidence lines (command id, error code, ds
release, request id) and context (`sre_event`, `host_kind`, `lane`). The agent
adds only what it expected and the acceptance condition, then submits.

## 5. Feedback without being asked

### 5.1 When an agent files — the duty

Outside coding sessions, anyone using `ds` — an agent through the CLI or MCP, or a person — files a
sighting **as soon as it happens, without waiting to be asked**, when any of
these occurs:

1. A command failed, crashed, or refused in a way its remedy did not resolve.
2. Finding the right command took more than one discovery attempt, or the
   words the user used did not find it.
3. Help, refusal text or skill text disagreed with what the command did.
4. Output was wrong, incomplete, surprising, or needed post-processing outside
   `ds` to be usable.
5. A step was slow enough to matter, or needed a workaround.
6. A capability was missing (the existing case).
7. An idea would have saved the user steps.

One sighting per distinct gap; the service deduplicates repeats into
occurrences, so filing twice is cheap and filing never is the failure.

### 5.2 How it is reported to the user

At the end of a product-use task outside coding, the agent lists the report ids it filed, one line each.
It does not ask permission first; feedback is the agent's own observation, not
an effect on the user's project. It never puts secrets or customer data in a
report (the existing `invalid_text` contract).

### 5.3 What `ds` files by itself

- A `crashed` invocation (panic, abort, `internal` error class) files one
  `bug` report automatically, `agent = "ds (automatic)"`, with the draft of §4
  and the SRE event id. Repeats merge into that report's occurrences.
- Nothing else is filed automatically: a refusal or a slow call needs a
  person's or agent's expectation to be a useful report.
- **How the code reads this section.** `reporter::record` queues the report in
  the same state document (`automatic`, at most 20, one per command and code;
  a repeat raises its `occurrences`). `reporter::file_automatic` submits each
  through `feedback.submit` as the signed-in user, holding the state lock only
  to read and to settle; signed out or offline, the report waits for a later
  invocation, and a body the backlog refuses is dropped. A report sent after
  repeats carries `happened N times since <day>`; the backlog merges a later
  identical title into the same report's occurrences. A development build
  crashes on purpose when `DS_CLI_TEST_PANIC` names the command id (§7.3); a
  release build has no such hook.

### 5.3a Lifecycle of an automatic report

The SRE event is evidence and never changes; the feedback report is the unit
of work. The two meet on the command id, the error code and `sre_event`.

1. **Open.** The first crash or `internal` error of one command and code files
   one report (§5.3). Later ones raise its occurrences while it is unsettled.
2. **Closed.** A coding session proves the fix and closes the report
   (`resolved`, with the evidence) through `ds feedback close`.
3. **Recurred.** A matching ending after the close is a new sighting: the
   backlog files a NEW report whose `supersedes` names the closed one, so a
   proven closure is never reopened and a regression is never hidden.
4. **SRE view.** Events stay in the store for their full retention. Open SRE
   work is a command and code whose newest event is newer than the close of
   its newest settled report, or which has no settled report; the `ds_client`
   overview row must present the others as addressed, with their counts kept as
   evidence. Not implemented yet: the overview reads events only and still
   lists a closed crash among `top_failing` until its events age out of the
   seven-day window.

### 5.4 Where the duty is taught

| Surface | Change |
| --- | --- |
| `ds` skill | A "Report as you go" section near the top, before "Find one command", with the §5.1 list and §5.2; the "When `ds` cannot" section points to it. The skill description names feedback as a primary duty. |
| `feedback.submit` contract | Replace "use only after live capability discovery confirms…" with: file friction, confusion, slowness and ideas as well as gaps; distinguish kinds; include a negative control for `bug`. |
| MCP initialize instructions | A `FEEDBACK_GUIDANCE` constant beside `REGRESSION_GUIDANCE`: file sightings unprompted, use `ds feedback draft --last`, list ids at the end. |
| Error envelopes | Every `failed`, `crashed` or repeated `refused` envelope carries `next.feedback`: the `ds feedback draft --last` command and the SRE event id. Implemented as the last `error.next` command, `ds feedback draft --last`, on `failed` and `crashed` endings (`reporter::next_feedback`). The SRE event id is in the journal the draft reads, not in the envelope: an id there would make every failed answer differ between runs, between reporting on and off (§3.4), and between an MCP tool call and the same command run directly. A repeated refusal names none yet: deciding "repeated" would read local state into the answer. |
| Narrow skills | Their "Stops at" lines keep pointing to `ds`; no per-skill copies of the duty. |

## 6. Ownership

| Repository | Owns |
| --- | --- |
| `ds-command-kernel` (`ds-client-core`) | Result classification, the event builder and sanitizer, outbox, journal, dedupe and rate limits, daily rollup, the `next.feedback` envelope field. |
| `ds-cli` | Wiring for CLI and MCP, `ds feedback draft`, `ds doctor` lines, the skill text, `FEEDBACK_GUIDANCE`, discovery prose and gates. |
| `ds-brain` | Accepting `source="cli_command"` and `cli_rollup`; stamping `service="ds"` from the source; read filters by source. |
| `ds-sre` | The store contract amendment (new sources and fields), the overview row, and any alert on `ds` fault rate. |
| `ds-web` | Nothing new; the SRE page shows the new rows through existing reads. |

## 7. Acceptance

Proven through installed `ds`, on this box, with these exact scenarios:

1. **Transport failure:** point `ds` at an unreachable gateway; the command
   fails exactly as before; the event sits in the outbox; the next online call
   flushes it; `ds sre events --source cli_command` shows it with the same
   `request_id`.
2. **Refusal:** a declared refusal is recorded `result_class = refused`,
   `outcome = success`, and does not move the availability SLI.
3. **Crash:** a test-only panic hook produces one `crashed` event and one
   automatic `bug` report; a second crash raises that report's occurrences to 2
   and files nothing new.
4. **Privacy:** sentinel values in every argument of a failing command appear
   in no event, journal line or draft.
5. **Invariance:** output and exit code are byte-identical with
   `DS_SRE_REPORTING` on and off, online and offline.
6. **Draft:** after a failure, `ds feedback draft --last` returns a payload that
   `ds feedback submit --yes` accepts after adding only expectation and
   acceptance.
7. **Unprompted filing outside coding:** a blind low-powered agent given a product-use task that hits a
   seeded friction (a command it must search for twice) files a feedback report
   without being asked and lists its id at the end.

## 8. Non-goals

- Not product analytics: no usage profiling beyond the daily per-command
  success count.
- Product-use feedback stays in the shared feedback backlog; coding findings
  stay in the owning source backlogs. SRE stays the one event store.
- No feedback filed on the user's behalf about their project data or people.

## 9. Open owner questions

1. **Opt-out.** Is reporting always on for installed releases, with
   `DS_SRE_REPORTING=off` only for tests — or may an operator turn it off?
2. **Signed-out hosts.** Report as `anonymous` (the ingest already allows it),
   or keep the event in the outbox until a user signs in?
3. **Automatic filing scope.** Crashes only (this spec), or also a command that
   fails the same way N times in a day?
4. **Read access.** CLI events readable by platform admins only, as the SRE
   page is today, or also by the user who produced them?
