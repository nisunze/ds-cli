# `ds sre` — reference

`ds sre` exposes two bounded, read-only projections of platform reliability.
The read is performed by `ds-client-core::sre` as the restored native user; `ds`
receives no Firebase token, Cloud Monitoring credential, BigQuery credential or
browser cache access, and needs no running DS GridDesign.

This domain is platform-global. Its authority is `headless_user`, not
`project`: the machine must hold a restored native user for the selected lane,
but no project needs to be selected and none is sent. The optional `events
--project` flag filters event metadata and does not switch or establish project
authority. ds-brain restricts both routes to the `platform.admin` capability;
that refusal is reported as `auth_rejected`.

`--lane` selects the credential lane, `stable` or `canary`, exactly as it does
everywhere else in `ds`.

## Overview

```bash
ds sre overview --output json
```

The result has exactly these top-level fields:

- `generated_at`, `fleet`, `combined_reports`
- bounded `services`, `service_ops`, `stale`, `incidents`, `error_catalog`
- exact owner collection counts in `totals`
- per-collection truncation booleans in `more`

`incidents` is the owner's incident feed and is currently unpopulated. An empty
array is therefore not evidence that an external incident-management system has
no open incidents.

## Events

```bash
ds sre events --service ds-brain --category timeout --output json
ds sre events --days 7 --outcome all --limit 100 --scan-limit 2500 --output json
```

Defaults and bounds are part of the command contract:

| Flag | Default | Bound |
|---|---:|---:|
| `--days` | 3 | 1..365 |
| `--limit` | 50 | 1..250 |
| `--scan-limit` | 1000 | 1..5000 |
| `--outcome` | `failure` | `failure`, `success`, `all` |

Optional exact, case-insensitive filters are `--service`, `--category`,
`--event-lane`, `--action`, `--project`, and `--source`. Each text filter is
trimmed, non-empty, and at most 200 characters. `--event-lane` matches the lane
an event was *recorded* on, which is not the lane this machine signs in to:
stable credentials may read canary's errors.

The result has `window_days`, `scan_limit`, `filters`, `scanned`, `matching`,
`returned`, and `events`. `more.matching` means matching rows were omitted by
`--limit`; `more.scan` means the newest-first source scan reached
`--scan-limit`, so a rarer match may exist outside the scanned window. Narrow
filters do not make that scan unbounded.

Every projected event string is capped at 128 Unicode characters, except
`error_message`, which is capped at 1,000. Each row's `truncated_fields` array
names every clipped field, and `error_message_truncated` remains a convenient
dedicated signal.

The event window arrives as a stream closed by its own row-count summary. A
window that ends without that summary, reports its own scan as failed, or
delivers fewer rows than it counted is refused as `auth_response_unreadable` rather
than returned short — a partial window silently presented as a whole one is how
an operator concludes there were no errors.

## Refusals

Both commands authenticate as the native user, so both can end in any of the
native authentication states every headless domain shares — no packaged
catalogue, an unsafe or mismatched profile, and a signed-out machine. `ds sre
<command> --help` lists that set with a remedy for each; copying it here would
be a second list that drifts the next time one is added.

What belongs to this domain:

| Code | Meaning |
|---|---|
| `auth_rejected` | the signed-in account may not read platform reliability; a platform administrator grants it |
| `unreadable_response` | the authority answered with something other than the read it was asked for, or an event window that lost rows |
| `invalid_number` | a numeric flag falls outside the bound in its summary; the refusal carries the accepted range |
| `invalid_text` | a text filter is empty, untrimmed, or longer than 200 characters |

The two input codes belong to `events` alone — `overview` declares no flags
beyond `--lane`.

The declared routes are `domains.sre.overview` (`GET /api/v1/sre/overview`) and
`domains.sre.events` (`POST /api/v1/data`, action `query_table`, table
`sre_requests`). Both are in `ds-command-kernel/routing/operations.json`, and
the client profile pins both at schema v29.

Until 2026-09-18 both commands travelled through a paired DS GridDesign window,
which held the same signed-in user and made the same two requests. That put
platform health behind the one host least likely to be running when it matters:
on a server, `ds sre overview` refused with `desktop_not_paired`.
