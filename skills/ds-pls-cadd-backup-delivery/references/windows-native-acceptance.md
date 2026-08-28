# Windows native acceptance for PLS-CADD backups

Use this only after deterministic DS checks have produced a candidate and the
task requires native Restore/reopen. It consolidates verified PLS-CADD 16.81
behavior; it is not a generic Windows automation recipe.

## Safe ownership boundary

- `ds` owns workspace closure, exact-byte backup framing, canonical import,
  model validation and count evidence.
- PLS-CADD owns Restore, reopen, solver behavior and native acceptance.
- The engineer owns engineering approval.

Use the host's supported Windows controller for an explicitly requested native
acceptance step. If that controller is unavailable, hand the exact step to the
operator. Do not improvise screen coordinates, raw keyboard sequences, Win32
messages, or a generic application-control MCP tool.

An existing operator-supplied launcher or acceptance driver may be used only
when the user authorized it and its own contract is known. Do not copy such a
driver into this skill or relax PowerShell execution policy to run it.

## Before and after the native step

1. Confirm PLS-CADD is closed and the source has no `.lock` file.
2. Record a source-tree digest or file manifest and the candidate `.bak`
   SHA-256.
3. Restore into a fresh empty short path. Do not file-copy or move a workspace
   to simulate Restore; native projects retain absolute path identity.
4. Close and reopen the restored project with the required PLS-CADD version.
5. Record the restored tree and compare expected project/core members and
   model counts.
6. Run `ds pls reference-closure` on the restored workspace and require zero
   unresolved references.
7. State native acceptance separately from deterministic DS validation and
   engineering approval.

PLS-CADD 16.81 was observed using `File > Backup` command id `33347` and
`File > Restore Backup` id `33348`. Those ids are version-specific diagnostic
evidence, not a portable API. Never post a remembered id to an unverified app
version. A supported controller should identify the visible command/dialog and
inspect its state before acting.

PLS-CADD writes a detailed log under `%APPDATA%\PLS\temp\PLS-CADD.log`. Prefer
the new log tail and produced artifact over interpreting pixels. If a dialog
appears, read and record it before answering; do not blindly accept cascades or
Backup Options whose defaults determine included members.

## Acceptance evidence

A successful native gate records:

- PLS-CADD version;
- candidate path, size and SHA-256;
- fresh Restore destination;
- reopen success;
- expected core project members;
- alignment, structure, tension-section, support and terrain counts;
- post-Restore unresolved-reference count;
- every file changed by the native operation;
- any native warnings still requiring operator or engineer judgment.
