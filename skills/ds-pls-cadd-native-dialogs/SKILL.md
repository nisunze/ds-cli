---
name: ds-pls-cadd-native-dialogs
description: Classify PLS-CADD 16.81 native dialogs during an authorized Restore, reopen, backup, or report run. Wait for transient/progress windows, act only through a characterized workflow driver, and retain evidence for unknown prompts.
metadata:
  ds-chapters: pls-cadd
---

# Handle PLS-CADD native dialogs

Use the `ds` skill to discover and run the DS half of the workflow. This
skill governs the Windows PLS-CADD 16.81 handoff only; the DS model and its
receipts remain on ds-server. Read the host's supported native controller
contract before launching a run. The active operation and its expected next
dialog determine what a prompt means. A title alone never authorizes a click.

For every visible window, record the PLS process/version, current operation
and stage, window title/class, complete visible body, control IDs and
visible/enabled state, and the relevant new PLS log tail. Distinguish a modal
(the main frame is disabled) from a modeless report or Error Log. Match the
whole observation to exactly one characterized transition:

- **Wait, with a deadline:** a progress box or a known transient state has no
  operator decision. Poll for the next expected state without sending input.
  After the stage deadline, capture the window and stop. A transient must
  never be treated as success merely because its text says “complete.”
- **Act:** a workflow step has an exact expected title, body, stage, process,
  and visible/enabled control. Let the supported native driver choose the
  named control. Close an informational prompt only after its exact body and
  resulting artifact have been checked; use its characterized Close/OK control
  and verify the modal disappears. Verify the resulting project/window and
  artifacts before advancing. A remembered button ID is evidence, not a
  portable command.
- **Stop:** unknown or ambiguous dialog, unexpected stage, changed body,
  multiple competing modals, missing expected control, repair/criteria error,
  timeout, or output/count mismatch. Preserve the process and evidence so the
  operator can classify it. Never blanket-dismiss modals, click a guessed
  coordinate, or retry a state-changing operation from an uncertain stage.

## Characterized 16.81 Restore transition

After the backup picker and Directory Mapping For Restore are accepted, PLS
may briefly display a modal titled `Restore Backup` with body
`Restore complete`. In the Nyamagabe diagnostic run this was an intermediate
state, followed by the actual decision dialog. Wait for the next state for a
bounded interval (15 seconds for this transition); send no button to the
intermediate window. If it persists, record its controls and log tail and
stop. This observation does not establish restored file count or project
open success.

The final dialog observed in that run was titled
`Restore Backup of candidate-native.bak` and said
`63 files restored, 0 files skipped` followed by
`Would you like to open project
'C:\Users\magese\Desktop\PLS-AW-DIAG-20260922\restore-1\nyamagabe.xyz'?`.
The 63 files and path identify that *one diagnostic backup*, not a default
for other projects. For a new run, derive the expected count, backup leaf and
fresh destination from its pinned candidate and manifest. At that exact final
dialog, check backup identity, expected project path, restored count and zero
skips, then choose **Yes immediately** (control ID 6) when the purpose is to
open the restored project. Do not hold this dialog while attempting byte
comparison of the restored folder: PLS-CADD rewrites text line endings and
embedded absolute FILENAME paths during Restore. Check content, references,
counts and any native warnings after opening; do not turn those later gates
into reasons to leave the Yes dialog stacked. No (ID 7) is only for an explicit
workflow that deliberately leaves the project unopened. A dialog reporting
skipped files or another project is a refusal.

The supported native controller is `ds pls desktop` on the Windows desktop:
`check`, `restore`, `qualify`, `deliver`, `autosag`, `reports` and
`sheets-pdf` run the characterized drivers embedded in `ds`, and
`ds pls desktop dialogs` lists every catalogued decision. An unknown dialog
stops those runs with `unknown_dialog`; PLS-CADD stays open for inspection.
If the controller is unavailable, preserve the dialog and hand the exact
observation to the operator; do not improvise a Win32 message driver. The
generic posted button click was observed to leave this modal stacked, so do
not repeat that ineffective action.

A dialog that initially looks blocking may be only a draw/transition state.
A dialog that disappears may also hide a failed restore. The evidence is the
state transition, native artifacts and log, not the visual impression of a
message.

Do not add a prompt to the catalogue (`pls-dialog-catalog.psd1`, embedded in
`ds` with a pinned digest) until its title, body, stage, controls and safe
outcome have been reproduced with a pinned PLS version. Windows
runs only the supported native controller on the transferred workspace; it
does not become the application-code authority.

Stops at: the native PLS-CADD operator's decision to complete or refuse the
characterized dialog; hand over the pinned workspace and observed controls.
