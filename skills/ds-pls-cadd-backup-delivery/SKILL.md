---
name: ds-pls-cadd-backup-delivery
description: Hand an independently prepared PLS-CADD backup to the operator for fresh native Restore/reopen acceptance before submission. Core backup construction and conversion commands are unavailable.
---

# Deliver a complete PLS-CADD backup

Use the `ds` skill first. Treat the source workspace, its closure receipt,
declared CRS and expected model counts as evidence. Never move or copy a live
workspace to make Backup succeed; native Restore is a lifecycle operation, not
an ordinary directory copy.

The backup is one item of the MV submission set; read
[MV submission deliverables](../ds-project-work/references/mv-submission-deliverables.md)
for the rest (staking stays XLSX, a dated folder holds only today's artifacts).

## Require a prepared candidate

The independent engineering owner must provide the candidate backup, its exact
SHA-256, source closure evidence, declared CRS, canonical validation, and
expected model counts. Core no longer exposes `pls`, `dsgrid`, or
`dsgrid-exchange` commands to construct or validate that candidate. Do not
substitute hand-edited native files or claim missing evidence was produced.

Design edits and native member replacement belong to the engineering owner.
A missing or unsuitable canonical library member remains an engineering gap.
The operator's native acceptance below cannot establish absent source evidence.

## Native acceptance is a separate gate

Restore the candidate with the required PLS-CADD version into a fresh empty
short path, close it, reopen the restored project, then run reference closure
and the count checks against the restored result. Do not call the backup
submittable until all of those pass.

The Core `pls desktop` controller is unavailable. Hand the exact fresh
Restore/reopen action to the PLS-CADD operator. Read
[references/windows-native-acceptance.md](references/windows-native-acceptance.md)
and use `ds-pls-cadd-native-dialogs` for the observed native transition.
Do not replace the missing controller with coordinate clicks, arbitrary
keystrokes, or an ad hoc Win32 driver.

Keep these verdicts separate in the final receipt:

1. reference-ready source workspace;
2. exact-byte backup framing and self-readback;
3. canonical model validation and expected counts;
4. native fresh Restore and reopen;
5. post-Restore closure and count parity;
6. engineering approval.

No lower verdict implies a higher one.

Stops at: the PLS-CADD operator performs fresh native Restore and reopen;
source and canonical evidence come from the independent engineering owner.
