# Component policy

Load this only after `ds workstation status` identifies a component concern.
The live command descriptor and receipt outrank this reference.

## LibreOffice

Purpose: document/report editing and governed headless conversion. The native
Windows lifecycle is proven and prefers the platform package manager. A vendor fallback is
allowed only after a verified package-manager transport failure: resolve the
current installer from The Document Foundation's official Metalink response,
select only a URL listed there, verify the published SHA-256, and execute only
after the match. Never pin a version, mirror, or old incident in the skill.

Installation requires explicit confirmation and a user present for UAC. Verify
package registration, executable/version, harmless headless conversion, and
task-owned cleanup. Never remove a pre-existing installation. LibreOffice has
no separate MCP.

## Git Bash

Git Bash is a Windows component. Detect it first; an existing suitable copy is
the result, not an excuse to exercise installation. On macOS/Linux use the
native shell and platform package manager. Default-profile changes are a
separate, target-specific intent; see `windows-shells.md`.

## Rwanda reference components

The acquisition command reads only the fixed official NISR Village Boundary
2022 Open Data service, in bounded low-bandwidth-tolerant pages. It never asks
the service to simplify or reduce coordinate precision. It commits GeoJSON only with a
DS-governed receipt naming source, version, license, installation time,
ownership, and SHA-256. Discovery never downloads. A pre-existing or
non-task-owned component is never removed by a repeatability test.

## Chromium

Chrome, Edge, or Chromium prints the bundled voltage-drop A4 report in one
local export operation. `ds workstation configure --component chromium
--target reporter --yes` selects an existing executable, proves its version and
a real task-owned headless HTML-to-PDF conversion, and stores the exact path in
DS-owned `chromium/browser-selection.json`. It uses a private temporary browser
profile, cleans it, and claims no ownership of the pre-existing browser.

An explicit `DS_VD_CHROME` or `CHROME` overrides the stored selection. When no
selection exists, Windows prefers Edge in Program Files; Linux may use an
already-present Playwright headless shell. Invalid explicit overrides fail
instead of silently choosing another browser. The local renderer uses
`--no-sandbox` only for bounded local HTML on Linux where AppArmor may disable
the browser's user namespace. `install --component chromium` is unsupported;
DS never downloads or installs a browser. The report command reuses the saved
selection automatically and does not open a browser window.
