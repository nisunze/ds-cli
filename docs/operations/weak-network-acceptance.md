# Weak-network acceptance — the live procedure

> "we use and prove resumable uploads on weak networks."

This is the live half of that proof. The automated half is
`crates/ds-cli-auth/tests/weak_network.rs`, which runs on every
`cargo test -p ds-cli-auth`. **This document is not run by CI and must not be
run casually**: every step here degrades a real network interface on a real
box, and one variant takes the box off the network on purpose.

Nothing below was executed when this file was written. It is a procedure, and
a run of it is only evidence if the receipts in §6 are kept.

## 0. What a live run adds

The harness already proves the protocol against faults a live link cannot be
asked to produce on cue — a duplicate acknowledgement, a commit shorter than
the bytes written, a socket cut at byte 4 194 303. It runs the same
`ds-cli-auth` transfer host and the same `ds_command_kernel::transfer` machine
the product runs, end to end in Rust, and it is the faster and stricter of the
two. Run it first; a live run on a red harness proves nothing.

What only a live run adds:

| Only live | Why the harness cannot |
|---|---|
| Real TLS to `storage.googleapis.com` | the harness speaks cleartext to loopback |
| Real kernel TCP under loss, reordering and RTT variance | netem shapes an interface, not an in-process socket |
| A real DS-minted session, with its real expiry | the harness mints nothing |
| ds-brain's `finalize` re-hash and the tiling registration after it | out of the transfer's scope |
| A link outage longer than a whole backoff step | the harness would have to sleep it |

## 1. Preconditions

- The Linux box that runs the stack (`ds-server`), with the participating
  checkouts side by side and on `run`. `ds-work/ENVIRONMENTS.md` decides
  whether that is this box; do not assume it.
- `iproute2` and `sudo`. Every `tc` command here needs root.
- A signed-in `ds` with a selected project: `ds project use …`. The transfer
  host refuses anything that is not a DS-minted `storage.googleapis.com`
  session, so there is no way to point this at a test origin.
- An artifact large enough to span several protocol chunks. The chunk is
  8 MiB (`transfer::DEFAULT_CHUNK_BYTES`), so **use at least 64 MiB** — eight
  chunks, which leaves room for an interruption to land in the middle of one:

  ```bash
  ls -l ./acceptance-64mib.geojson   # ≥ 67108864 bytes
  ```

  `ds_client_core::MAX_UPLOAD_BYTES` is 1 GiB, so project data of this size is
  within the route's bound. (The 64 MiB *server* ceiling in
  `ds-command-kernel/docs/contracts/ds-transport-authority.md` §10 is the
  compute-artifact route, a different one.)

## 2. Bring the dev stack up

From `ds-web`, per `ds-web/docs/run-launcher.md`:

```bash
cd ~/data-solutions/ds-web
./run-web.sh local --loopback --services all
```

For the desktop surface instead (XRDP, or register the menu entry from SSH):

```bash
./run-linux-desktop.sh
./run-linux-desktop.sh --cli project current --output json
```

Know what that stack is and is not. `ds map data upload` reaches the deployed
gateway and then a DS-minted GCS session: **the bytes leave the box.** The
local stack supplies project context and the map view to watch the result
land. That is why §3 has two recipes — degrading loopback does not degrade the
transfer.

## 3. Degrade the link

### 3a. The loopback leg — browser ↔ ds-brain ↔ reporter

```bash
sudo tc qdisc add dev lo root netem loss 20% delay 300ms 100ms
sudo tc qdisc change dev lo root netem loss 20% delay 300ms 100ms
sudo tc -s qdisc show dev lo
sudo tc qdisc del dev lo root
```

This degrades the local services only. Use it to see the UI behave under a
weak link; it changes nothing about the resumable transfer.

### 3b. The egress leg — the bytes that actually resume

Find the interface that carries the storage session:

```bash
EGRESS=$(ip route get 142.250.0.0 | sed -n 's/.* dev \([^ ]*\).*/\1/p' | head -1)
echo "$EGRESS"
```

Degrade **only TCP/443**, so the SSH session that is driving the run survives
(an SSH reply leaves with source port 22, not destination 443):

```bash
sudo tc qdisc add dev "$EGRESS" root handle 1: prio bands 3
sudo tc qdisc add dev "$EGRESS" parent 1:3 handle 30: netem loss 20% delay 300ms 100ms
sudo tc filter add dev "$EGRESS" protocol ip parent 1: prio 1 u32 \
    match ip protocol 6 0xff \
    match ip dport 443 0xffff \
    flowid 1:3
sudo tc -s qdisc show dev "$EGRESS"
```

The unfiltered form —
`sudo tc qdisc add dev "$EGRESS" root netem loss 20% delay 300ms 100ms` — is
the one the owner named and is exactly equivalent for the transfer, but it
degrades the SSH session too. **Run that form from a physical or XRDP console
only.**

Run the publication (§4) once at each of these, in order, resetting with
`sudo tc qdisc del dev "$EGRESS" root` between them:

| Step | Netem on band 1:3 | What it should produce |
|---|---|---|
| 1 | `loss 20% delay 300ms 100ms` | completes; some chunks retried |
| 2 | `loss 40% delay 800ms 400ms` | completes; resume is the normal case |
| 3 | `loss 40% delay 800ms 400ms rate 256kbit` | completes, slowly, with no wall-clock kill |
| 4 | step 1, plus the outage in §5 | completes; resumes across the outage |

Step 3 is the one that catches a blanket operation deadline: a transfer that is
making progress must not be killed for being slow (contract §4.2 rule 13).

## 4. One real publication through the native host

```bash
ds map data inspect --path ./acceptance-64mib.geojson --output json
time ds map data upload --path ./acceptance-64mib.geojson --yes --output json
```

`inspect` computes the SHA-256 locally, offline, before a byte moves. `upload`
is the native path under test: `ds-client-core` mints the session, and
`crates/ds-cli-auth/src/upload.rs` streams it while
`ds_command_kernel::transfer` decides every probe, resume point, retry and
verdict. No browser, no Node, no TypeScript is involved at any point.

Keep both JSON envelopes. Watch the transfer while it runs:

```bash
watch -n1 'ss -tin state established dst :443 | head -20'
```

## 5. Interrupt, and watch it resume

Take the link away for longer than one backoff step (the curve is
1 s, 2 s, 4 s, 8 s, 16 s, then the transfer stops), while the upload from §4 is
running. **Console only** — this drops your SSH session:

```bash
sudo ip link set "$EGRESS" down
sleep 20
sudo ip link set "$EGRESS" up
```

What must happen, and what each outcome means:

- The same `ds` invocation continues, and the object ends complete. The
  transfer probed the session, learned the committed prefix, and resumed from
  the server's number.
- A restart from zero, or a re-upload of bytes the session already held, is a
  **failure**: contract §4.2 rules 2, 6 and 7.
- An endless quiet retry is a **failure**: the bound is six stalled attempts.
- `unreachable` where a status was available is the §9 defect returning.

### The ceiling to record honestly

Killing the **process** (`Ctrl-C`, or `pkill -INT -f 'ds map data upload'`) and
re-running the command does **not** resume today. Resume works fully inside one
`upload_bytes` call; the durable `transfer::TransferState` has nowhere to go
across the call boundary, because `Transport::upload_bytes` returns only a
`TransportResponse` (stated in `upload.rs`'s own header). A cross-invocation
resume needs a `ds-client-core` change. Record the observed behaviour; do not
report it as a pass.

## 6. What the receipts prove

| Receipt | Where it comes from | What it proves |
|---|---|---|
| `ds map data inspect` SHA-256 and byte count | before the run | the artifact's identity, computed offline |
| `ds map data upload` envelope: transferred bytes, SHA-256, `registered`, `tile_status` | the run | the session holds that same artifact, and the publication continued past the transfer |
| `ds map data list` — one row for the file | after the run | committed once, not twice |
| `sudo tc -s qdisc show dev "$EGRESS"` — `sent`/`dropped`/`overlimits` counters, captured before and after | the box | the link really was degraded during that window. Without this the run proves nothing about a weak network |
| `time` on the upload, read against the curve (1+2+4+8+16 s) | the run | the waiting was the kernel's bounded backoff, not an unbounded loop |
| `dmesg -T \| tail` around the `ip link` window | the box | the outage happened when it was supposed to |
| `cargo test -p ds-cli-auth --test weak_network` output | this repo | the ten fault scenarios a live link cannot schedule |

Keep all of it with the campaign's evidence. A live run with no `tc -s`
counters is a slow upload, not an acceptance.

## 7. Clean up — always, and first on the next login

```bash
sudo tc qdisc del dev "$EGRESS" root 2>/dev/null
sudo tc qdisc del dev lo root  2>/dev/null
tc qdisc show dev "$EGRESS"; tc qdisc show dev lo
ip -br link show "$EGRESS"
```

A netem qdisc survives the shell that created it and nothing else on the box
will mention it. Leaving one in place makes every later measurement on this
machine wrong, including someone else's.
