# Windows shell targets

“Default Bash” is not one switch. Establish the requested target before the
one proven configuration command:

| Target | Evidence | Allowed future change |
|---|---|---|
| PATH Bash | resolved executable path | none unless separately requested |
| VS Code integrated terminal | default key plus existing suitable `Git Bash` profile | merge this key only |
| Windows Terminal | `defaultProfile` and matching profile entry | discovery/plan only |
| DS subprocess execution | live `ds workstation status` field | none when DS uses direct execution |

An authorization for one row does not authorize another. Preserve comments,
formatting, profile definitions, unrelated settings, and Remote-SSH behavior.
Remote terminals continue to use the remote Linux/macOS native shell. The VS
Code mutation receipt contains before/after values and reports an identical
second call as a no-op. It refuses a profile that does not name the discovered
Git for Windows `bash.exe` with `--login -i`.
