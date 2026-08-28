---
name: ds-pls-cadd-backup-delivery
description: Create a complete exact-byte PLS-CADD .bak from a closed reference-ready workspace, prove canonical contents and counts, and require fresh native Restore/reopen before submission. For backup recovery and portability acceptance, not terrain or alignment editing.
metadata:
  ds-chapters: grid-model, pls-cadd
---

# Deliver a complete PLS-CADD backup

Use the `ds` skill first. Treat the source workspace, its closure receipt,
declared CRS and expected model counts as evidence. Never move or copy a live
workspace to make Backup succeed; native Restore is a lifecycle operation, not
an ordinary directory copy.

## Create and prove the candidate

Discover each live contract before invoking it:

```text
ds capabilities pls.reference-closure --output json
ds capabilities pls.backup-create --output json
ds capabilities dsgrid-exchange.inspect --output json
ds capabilities dsgrid-exchange.plan --output json
ds capabilities dsgrid-exchange.convert --output json
ds capabilities dsgrid.inspect --output json
ds capabilities dsgrid.validate --output json
```

Require `pls.reference-closure` to report zero unresolved references and a
handover-ready workspace. Then create the backup outside the workspace:

```text
ds pls backup-create --workspace <closed-workspace> --out <new.bak> --yes --output json
```

The receipt must report every source file framed, exact member-byte
preservation, no path healing, a source snapshot digest and a backup SHA-256.
It must also report `native_restore_reopen_accepted: false`; this deterministic
step cannot award native acceptance.

Inspect the `.bak`, plan and convert it to a new `.dsgrid` using the declared
source CRS, validate that package, and inspect its table counts. Compare
alignments, structures, tension sections, supports and terrain points with the
delivery's expected counts. No-loss DS conversion proves canonical readability
and engineering projection, not PLS-CADD Restore.

## Native acceptance is a separate gate

Restore the candidate with the required PLS-CADD version into a fresh empty
short path, close it, reopen the restored project, then run reference closure
and the count checks against the restored result. Do not call the backup
submittable until all of those pass.

When native acceptance is requested or fails, read
[references/windows-native-acceptance.md](references/windows-native-acceptance.md).
If the host's supported Windows controller is unavailable, stop at the
validated candidate and give the operator the exact Restore/reopen action;
never replace the missing controller with coordinate clicks, arbitrary
keystrokes, or an ad hoc Win32 driver.

Keep these verdicts separate in the final receipt:

1. reference-ready source workspace;
2. exact-byte backup framing and self-readback;
3. canonical model validation and expected counts;
4. native fresh Restore and reopen;
5. post-Restore closure and count parity;
6. engineering approval.

No lower verdict implies a higher one.
