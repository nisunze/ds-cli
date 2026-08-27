# Component policy

Load this only after `ds workstation status` identifies a component concern.
The live command descriptor and receipt outrank this reference.

## LibreOffice

Purpose: document/report editing and, after separate proof, governed headless
conversion. Prefer the platform package manager. A Windows vendor fallback is
allowed only after a verified package-manager transport failure: resolve the
current installer from The Document Foundation's official Metalink response,
select only a URL listed there, verify the published SHA-256, and execute only
after the match. Never pin a version, mirror, or old incident in the skill.

This lifecycle is provisional. Do not install, uninstall, reinstall, convert a
document, or clean installer files until the dedicated Windows proof lands.

## QGIS

Purpose: GIS desktop work outside the DS map surface. Status, planning, and a
fixed version probe are safe. Installation requires an explicit request for
that run and a conventional trusted package source. Never infer install intent
from a print or map task.

## Git Bash

Git Bash is a Windows component. Detect it first; an existing suitable copy is
the result, not an excuse to exercise installation. On macOS/Linux use the
native shell and platform package manager. Default-profile changes are a
separate, target-specific intent; see `windows-shells.md`.

## Rwanda reference components

The component is installed only when a DS-governed receipt names its upstream
source, version, installation time, ownership, and SHA-256 for every file.
Discovery must not download. Acquisition requires explicit intent plus the
governed source/version contract. A pre-existing or non-task-owned component
must never be removed by a repeatability test.
