# adapters/pls-cadd/interim — working drivers awaiting integration

Written 2026-09-23 during the Nyamagabe/Nyaruguru delivery so native PLS-CADD work is not
blocked by driver defects. Each script reuses the characterized drivers **verbatim** (loaded
from their AST) and applies small patches that assert the original code still exists, so a
changed driver makes them refuse instead of drifting. Codex: integrate the patches into
`../pls-backup-restore-qualify.ps1` / `../pls-backup-restore-lib.psm1` with contract tests
(queue items C3a/C3c/C3d/C3e/C3f), then delete this folder.

| script | what |
|---|---|
| `pls-interim-loader.ps1` | dot-source; loads driver + lib functions, applies the patches below |
| `pls-restore-open-interim.ps1` | one fresh native Restore + open; leaves PLS open for report driving |
| `pls-close-interim.ps1` | exit without saving + post-close Protected tree check |

| patch | defect | status |
|---|---|---|
| C3a | `Close-PlsWithoutSaving` throws "Expected one visible PLS-CADD main window, found 0" when the frame is destroyed before the process exits | Production driver now derives modals from the same frame enumeration; Windows contract and native retest pending. Interim close entrypoint calls that body. |
| C3g | `Test-PlsRestoredTree` normalized only the restored side, so CRLF source members (DS shell `.brk`) false-mismatched | Production verifier now extracts the digest-checked source payload and normalizes both text sides; Windows contract and native retest pending. The interim verifier remains an independent control. |
| C3c | final `Restore Backup of <leaf>` Yes was a posted BM_CLICK that leaves the modal stacked | fixed upstream 88c0885; **native-verified 2026-09-23** (IDYES opened the Iter3 restore) |
| C3d | pre-open `Full` byte check while the Yes dialog is up | fixed upstream 88c0885 (presence check pre-open, `Full` after open) |
| C3e | inventory assumed the `.xyz` parent was the only source root; real backups span several | Production inventory and qualifier now accept an explicit `-SourceRoot`, validate every native path under it, and allow implied intermediate directories; Windows contract and native retest pending. |
| C3f | the post-open `Full` check hashed the `.xyz` that the open project locks ("being used by another process", seen on the Gisagara control) | Production two-restore qualifier now verifies Full and Protected after each native close; Windows contract and native retest pending. The interim open-only helper still uses post-close Protected verification. |

For C3e, the full check now derives only the intermediate folders required by recorded
member paths. Extra files and unrelated directories still fail.

## Proven

2026-09-23, approved Gisagara rev 3 control (`Gisagararev3.bak`, sha256 `10555298…71bde`,
147 records from 3 source roots): Restore 145 files / 0 skipped → IDYES open → six native
reports → exit without saving → post-close Protected 63/63 files, digest equal. The native
Structure Usage reproduces the approved PDF 411/411 rows (one 0.01 m station rounding).
Evidence: Drive `Nyamagabe Nyaruguru Project/Working/pls-native-20260923/gisagara-control-20260923/`.

```powershell
.\pls-restore-open-interim.ps1 -CandidateBackupPath C:\PLSN\gis-20260923\in\Gisagararev3.bak `
  -ExpectedCandidateBackupSha256 1055529834e8dcd5d592e2d5f28d46b4b8f26c7c6d34ad417d5a3b5138e71bde `
  -RestoreDirectory C:\PLSN\gis-20260923\r1 -EvidenceDirectory C:\PLSN\gis-20260923\ev-r1b `
  -SourceRoot 'D:\Jessy\submissions' -Execute
# reports: ..\pls-report-any.ps1 -ProcessId <pid> -MainWindowHandle <hwnd> -CommandId 40014 ...
.\pls-close-interim.ps1 -ProcessId <pid> -EvidenceDirectory C:\PLSN\gis-20260923\ev-r1b -RestoreDirectory C:\PLSN\gis-20260923\r1
```

The Restore directory picker takes keyboard focus for a few seconds (Ctrl+L navigation);
do not type during that step.
