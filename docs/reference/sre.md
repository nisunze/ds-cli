# `ds sre` — reference

`ds sre` exposes two bounded, read-only projections from DS GridDesign's
Reliability surface. The paired application performs the request under its
current signed-in user and returns an outcome; `ds` receives no Firebase token,
Cloud Monitoring credential, BigQuery credential, or browser cache access.

This domain is platform-global. Its authority is `desktop_user`, not `project`:
the user must be signed in, but no project needs to be selected. The optional
`events --project` flag filters event metadata and does not switch or establish
project authority. The owner currently restricts Reliability access to platform
administrators; that refusal is reported as `sre_not_permitted`.

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
`--lane`, `--action`, `--project`, and `--source`. Each text filter is trimmed,
non-empty, and at most 200 characters.

The result has `generated_at`, `window_days`, `scan_limit`, `filters`,
`scanned`, `matching`, `returned`, and `events`. `more.matching` means matching
rows were omitted by `--limit`; `more.scan` means the newest-first source scan
reached `--scan-limit`, so a rarer match may exist outside the scanned window.
Narrow filters do not make that scan unbounded.

Every projected event string is capped at 128 Unicode characters, except
`error_message`, which is capped at 1,000. Each row's `truncated_fields` array
names every clipped field, and `error_message_truncated` remains a convenient
dedicated signal. These limits keep the maximum 250-row projection below the
desktop bridge's 8 MiB response ceiling even for worst-case escaped text.

## Refusals

| Code | Meaning |
|---|---|
| `desktop_not_paired` | no DS GridDesign session is running |
| `desktop_signed_out` | the app is running without a signed-in user; a project is not required |
| `sre_not_permitted` | the signed-in user lacks Reliability access |
| `desktop_operation_unsupported` | the app build does not yet own this SRE operation |
| `invalid_number` | a numeric flag is outside its declared bound |
| `invalid_text` | a text filter is empty, untrimmed, or too long |

The closed wire operations are `sre.overview` with `{}` and `sre.events` with
only `days`, `limit`, `scanLimit`, `service`, `outcome`, `category`, `lane`,
`action`, `project`, and `source`. Source parity tests hold those names and
bounds to the paired application.
