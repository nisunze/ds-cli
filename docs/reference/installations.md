# `ds install` — reference

`ds install` is the product's own installation inventory: every registered copy
of DS GridDesign, desktop and server, with its licence state and the verbs that
govern it.

Until 2026-09-18 this inventory had no `ds` surface at all. It was reachable
only from Governance → Installations in a browser — the host least likely to be
running where an operator is asking which installations exist and whose licence
is blocked. `ds capabilities --search installation` answered with
`auth.device.*`, which is linked DEVICES: a different family entirely.

The domain is platform-global. Authority is `headless_user`: the machine must
hold a restored native user for the selected lane, but no project is selected
and none is sent. ds-brain gates every action behind `platform.admin` or
`app.admin`; that refusal is reported as `install_not_permitted`.

## What a row means

The shaping is not in this CLI. `ds-command-kernel::installation_inventory`
decides which rows are shown, how they are grouped and ordered, what the two
licence tokens mean, how old `last_seen` is, and what an operator should do
next. The browser calls the same projection. A terminal and the page therefore
cannot disagree about whether a machine is licensed.

Every human-facing value is a stable token, never prose:

| field | values |
|---|---|
| `identity.host_kind` | `desktop`, `server` |
| `last_seen.bucket` | `moments`, `minutes`, `hours`, `days`, `weeks`, `months`, `never`, `ahead` |
| `last_seen.staleness` | `current`, `stale`, `silent` |
| `last_seen.source` | `licence_refresh` |
| `state.governed` | `allowed`, `blocked` |
| `state.device`, `state.licence` | `active`, `blocked` |
| `state.lease` | `allowed`, `blocked`, `handshake_required`, `upgrade_required`, `clock_rollback` |
| `remedy` | `none`, `blocked_by_operator`, `upgrade_required`, `handshake_required`, `clock_rollback`, `never_handshaked`, `silent_consider_retiring`, `retired_recorded`, `retired_but_reporting` |

## What `last seen` actually means

ds-brain stamps `last_seen_at` inside the licence-refresh transaction and in no
other place. The projection therefore labels it `licence_refresh`: it means the
installation contacted the server, not that somebody opened a window.

Its worst case follows from the lease constants, not from a schedule invented
for this table. A running, online desktop refreshes at least every
`CHECK_IN_SECONDS` (24 h), so `current` means "within one check-in". Offline it
survives on a signed lease for `OFFLINE_LEASE_SECONDS` (7 days), which is what
`stale` covers. Past that it is `silent`, and a silent row is silent — nothing
concludes it was uninstalled.

A server host registers through exactly the same call, but only while a Sync
Center operation runs. A `ds server serve` host that never performs one never
stamps anything, and one that never syncs at all never registers. That is a
real gap in coverage, and the projection reports how long a row has been silent
rather than papering over it.

## Reads

```bash
ds install list --output json
ds install list --include-retired --search linux
ds install show --install <install-id> --output json
```

`list` groups by host kind and orders freshest first. `totals` reports `shown`,
`matched`, `received`, `retired_hidden`, `blocked`, `silent` and `truncated`;
`next_cursor` continues the read. `show` adds the observed users — authenticated
handshakes, not a count of sign-ins — and the installation's immutable history.
The revision it prints is what a governed change must be applied against.

## Governed transitions

```bash
ds install policy --install <id> --device allowed --licence blocked \
  --expected-revision 3 --reason "licence not paid" --yes
ds install retire --install <id> --expected-revision 4 \
  --reason "laptop wiped and returned" --yes
ds install retire --install <id> --restore --expected-revision 5 \
  --reason "recorded in error" --yes
```

Both tokens refuse work admission identically; they differ in reach. A blocked
LICENCE is carried into the signed offline lease, so the installation refuses
itself with no network. A blocked DEVICE takes effect on the next handshake.
Neither expires and neither is ever set automatically.

Every transition is fenced on the revision you read, requires a non-empty
reason of at most 1000 characters, and is appended to the installation's
`policy_events`. A revision that moved is `install_revision_conflict`: re-read
and reapply, never retry.

## Uninstallation

Nothing in the product observes an uninstallation today. The `.deb` ships no
maintainer script and the Windows uninstaller runs no custom step, so no
machine reports its own removal. `ds install retire` is therefore the
operator's explicit statement about a machine that was wiped, returned or
decommissioned.

It is a state transition, never a deletion. The row is retained, stamped with
who recorded the removal, when and why, and excluded from the default view;
`--include-retired` brings it back. That matters because this family exists to
investigate use, and a dropped row is destroyed evidence.

A retired installation that keeps checking in is NOT silently un-retired. It is
reported as `retired_but_reporting`, because either the removal did not happen
or the record was wrong, and both are things an operator needs to see.

Silence is never read as an uninstallation.
