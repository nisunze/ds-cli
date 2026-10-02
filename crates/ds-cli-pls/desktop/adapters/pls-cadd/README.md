# PLS-CADD / PLS-POLE 16.81 adapter

Third party: Power Line Systems PLS-CADD 16.81 x64, installed at
`C:\Program Files\PLS\pls_cadd\pls_cadd64.exe` with the executable digest
pinned in `pls-backup-restore-profile.psd1` and `pls-report-profile.psd1`, and
PLS-POLE 16.81. Windows PowerShell 5.1 runs every script; PowerShell 7 is not a
substitute.

The scripts operate the programs through Win32 messages and characterized
command and control ids, answer only dialogs `pls-dialog-catalog.psd1` names,
and return raw evidence. They take no engineering decision; see `../README.md`
for what an adapter may and may not do.

## Map

| Group | Files |
|---|---|
| `ds` entries (one per `ds pls desktop` verb) | `ds-desktop-*.ps1`, `ds-desktop-lib.ps1` |
| Restore, open, backup, close | `pls-backup-restore-*.ps*`, `interim/`, `pls-launch-project.ps1`, `pls-open-project.ps1`, `pls-known-restore-open.ps1`, `open-verified-restore.ps1`, `pls-navigate-open.ps1`, `pls-pick-folder.ps1`, `pls-exit-nosave.ps1`, `pls-restore-provenance.ps1` |
| Delivery chain | `pls-deliver-autosag.ps1`, `pls-section-table-autosag.ps1`, `pls-sheet-paging.ps1`, `pls-optimum-run.ps1`, `pls-load-file.ps1` |
| Reports and sheets | `pls-report-*.ps*`, `pls-save-report-file.ps1`, `pls-close-report-session.ps1`, `pls-handle-report-prompt.ps1`, `pls-save-sheets-pdf.ps1`, `pls-print-sheets-pdf.ps1` |
| Dialogs and windows | `pls-dialog-*.ps*`, `pls-window*.ps*`, `pls-windows.ps1`, `pls-control.ps1`, `pls-click*.ps1`, `pls-press.ps1`, `pls-combo.ps1`, `pls-context-menu.ps1`, `pls-type-path.ps1`, `pls-modal-children.ps1`, `pls-safe-dump.ps1`, `pls-state.ps1`, `pls-inspect.ps1`, `pls-printwindow.ps1`, `pls-capture-evidence.ps1`, `pls-close-window.ps1`, `pls-dismiss-startup.ps1`, `pls-repair-continue.ps1`, `pls-goto-structure.ps1` |
| Menus | `pls-command.ps1`, `pls-menu-dump.ps1`, `pls-menu-evidence.ps1`, `pls-cadd-16.81-menu-commands.tsv` |
| Sessions and settings | `pls-session-hold.ps1`, `pls-units-check.ps1`, `pls-native-check.ps1`, `pls-cable-edit.ps1`, `pls-ini-set.py` |
| PLS-POLE | `pls-pole-run.ps1`, `pls-pole-hold.ps1` |
| PLS file formats (no program started) | `pls-bak-replace-files.py`, `pls-don-sheet-patch.py` |
| Project-specific evidence | `pls-rutsiro-report-coverage*.psd1`, `pls-handle-rutsiro-open-prompts.ps1` |
| Contract tests | `tests/`, `pls-report-bundle-tests.ps1` |

Only the scripts reachable from a `ds-desktop-*.ps1` entry run under `ds`; the
rest are operator tools shipped so another install can use them by hand.
`ds pls desktop toolkit --out <new folder>` writes the whole set with a
manifest of every SHA-256.

## Known engineering residue

`pls-report-bundle.ps1` checks report coverage against the per-project bounds
in `pls-rutsiro-report-coverage*.psd1`. That is acceptance logic, not program
operation; its owner should be Rust. It stays here until moved.
