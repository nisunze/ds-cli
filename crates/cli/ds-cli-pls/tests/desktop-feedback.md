# Desktop review verification

This slice addresses feedback `cedcfd10-076d-48c6-abaa-34b05cf3dd7f`
through the existing PLS-CADD desktop drivers. The command declarations in
`src/desktop/reports.rs` and `autosag.rs` remain the flag/output authority.
Report receipts retain their v1 fields; format and session metadata are additive.
Modified native drivers retain their original upstream digests in the bundle.

Run the Rust desktop tests with the workspace dependency and heavy-build
wrappers, and run `tests/desktop-feedback.ps1` with PowerShell. The latter is
an independent headless mock harness: it parses the actual scripts, extracts
their actual functions/entry bodies, and substitutes process enumeration,
native messages, report generation and Word. It does not launch PLS-CADD.
It verifies full-path identity, singleton process selection, PID reuse,
executable path, frame identity/responsiveness, session lifecycle, RTF verdict
preservation, default Word preflight, About notifications and bounded retries.

## Native acceptance boundary

Headless checks do not establish Windows PowerShell 5.1 or user32 behavior.
Acceptance on the pinned Windows PLS-CADD 16.81 build requires a working copy
of Nyamagabe M1 and separate fresh evidence directories:

1. Without Word registration, explicitly select native RTF reporting and
   verify six nonempty RTFs, their digests and verdict counts; the default
   PDF selection must still refuse `word_not_found` before launch.
2. With startup About present, verify the journal's `about_command` event
   identifies its visible enabled OK (id 1), the dialog closes and the real
   frame becomes ready before AutoSag begins. A nonclosing About must stop
   after three responsive attempts or the watcher deadline, preserving
   evidence. An unknown prompt must remain open with its control tree recorded.
3. For attachment, verify that the current frame exposes the exact absolute
   project path. A filename-only title or an original launch command line
   cannot distinguish a later switch to a same-named project copy. Attachment
   deliberately refuses this unproven identity. If 16.81 exposes only the
   filename, a characterized read-only native source of the current full path
   is required before attachment can be accepted; weakening the guard is unsafe.
4. Once identity is proven, reporting must leave the same PID/frame open,
   return RTF verdicts and omit PDF fields when selected. AutoSag must save,
   return its Section Usage gate and leave that same session open. Repeat with
   another project, two processes, a changed/reused PID, a modal and an
   unresponsive frame; attachment must refuse before sending a workflow command.

The read-only desktop readiness check does not launch PLS-CADD or dismiss
dialogs. Its Word blocker describes the default PDF-capable environment;
RTF-only execution checks Word only when PDF conversion is selected.
