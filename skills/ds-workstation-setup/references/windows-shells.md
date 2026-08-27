# Windows shell targets

“Default Bash” is not one switch. Establish the requested target before any
future configuration command:

| Target | Evidence | Allowed future change |
|---|---|---|
| PATH Bash | resolved executable path | none unless separately requested |
| VS Code integrated terminal | `terminal.integrated.defaultProfile.windows` | merge this key only |
| Windows Terminal | `defaultProfile` and matching profile entry | merge only the selected default |
| DS subprocess execution | live `ds workstation status` field | none when DS uses direct execution |

An authorization for one row does not authorize another. Preserve comments,
formatting, profile definitions, unrelated settings, and Remote-SSH behavior.
Remote terminals continue to use the remote Linux/macOS native shell. A future
mutation receipt must contain before/after values and report an identical
second call as a no-op.
