# Local PowerShell driver checks

Run this gate after changing an embedded PLS-CADD PowerShell file or its
bundle declaration. It checks the exact file set and SHA256 pins in
`crates/ds-cli-pls/src/desktop/bundle.rs`, parses every `.ps1`, `.psm1` and
`.psd1`, and explicitly enables PSScriptAnalyzer's `PSUseCompatibleSyntax`
rule for Windows PowerShell 5.1. It also tests the portable window-row
classifier and both pure AST loaders, including malformed helpers and
PowerShell 7 syntax that the production runtime cannot parse.

Use PowerShell 7.2 or newer and PSScriptAnalyzer 1.24.0 as local developer
tools. The Microsoft VS Code PowerShell extension includes the analyzer;
pass its installed `PSScriptAnalyzer.psd1` explicitly. A separately installed
1.24.0 module works too. The gate neither downloads modules nor edits editor
settings, and a missing or different analyzer fails the check.

```powershell
pwsh -NoLogo -NoProfile -NonInteractive -File scripts/test-pls-powershell.ps1 -AnalyzerManifest /absolute/path/to/PSScriptAnalyzer/1.24.0/PSScriptAnalyzer.psd1
```

The JSON result records every checked file's digest and the passing cases.
Only the platform-independent `Split-PlsWindowRows` module and two isolated
parser function definitions execute. Desktop entrypoints, Win32 and Word
automation, project files and application processes are never invoked.

This proves parsing, supported syntax and the tested pure functions. It does
not prove Windows PowerShell 5.1 command/type availability, Win32 or COM
behavior, PLS-CADD Restore or native delivery. Actual `ds pls desktop` runs
keep the characterized Windows PowerShell 5.1 runtime and Linux
`windows_only` refusal. Native Windows acceptance remains a separate gate.

Microsoft documents [the syntax compatibility rule](https://learn.microsoft.com/en-us/powershell/utility-modules/psscriptanalyzer/rules/usecompatiblesyntax?view=ps-modules),
[the VS Code extension and bundled analyzer](https://code.visualstudio.com/docs/languages/powershell),
and [official binary archives](https://learn.microsoft.com/en-us/powershell/scripting/install/alternate-install-methods).
Verify a downloaded archive against its exact official release checksum and
retain its license. A developer host that boots on an uncharacterized Linux
distribution supplies local test evidence, not a Microsoft support claim.
