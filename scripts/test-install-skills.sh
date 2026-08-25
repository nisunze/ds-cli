#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALLER="$ROOT/scripts/install-skills.sh"
TEMP="$(mktemp -d)"
trap 'rm -rf -- "$TEMP"' EXIT INT TERM

export HOME="$TEMP/home"
export CODEX_HOME="$HOME/.codex"
mkdir -p "$HOME"

"$INSTALLER" install >/dev/null
for target in "$HOME/.codex/skills" "$HOME/.claude/skills" "$HOME/.copilot/skills"; do
	[[ -f "$target/ds/SKILL.md" ]]
	[[ "$(cat "$target/ds/.ds-cli-skills-owner")" == 'nisunze/ds-cli' ]]
	[[ "$(head -n 1 "$target/.ds-cli-skills-owned")" == 'ds-cli-skills-install/v1' ]]
	[[ ! -e "$HOME/.agents/skills/ds/SKILL.md" ]]
done

# A packaged bundle propagates its provenance receipt; a source checkout does
# not invent one. Reinstalling from source removes a stale packaged receipt.
[[ ! -e "$HOME/.codex/skills/.ds-cli-skills-receipt.json" ]]
printf '{"contract":"ds-cli-skills-bundle/v3"}\n' > "$ROOT/receipt.json"
trap 'rm -f -- "$ROOT/receipt.json"; rm -rf -- "$TEMP"' EXIT INT TERM
"$INSTALLER" install >/dev/null
cmp "$ROOT/receipt.json" "$HOME/.codex/skills/.ds-cli-skills-receipt.json"
rm -f -- "$ROOT/receipt.json"
"$INSTALLER" install >/dev/null
[[ ! -e "$HOME/.codex/skills/.ds-cli-skills-receipt.json" ]]

touch "$HOME/.codex/skills/ds/stale"
"$INSTALLER" install >/dev/null
[[ ! -e "$HOME/.codex/skills/ds/stale" ]]

printf '%s\n%s\n' 'ds-cli-skills-install/v1' 'retired-skill' \
	> "$HOME/.codex/skills/.ds-cli-skills-owned.tmp"
mv "$HOME/.codex/skills/.ds-cli-skills-owned.tmp" "$HOME/.codex/skills/.ds-cli-skills-owned"
mkdir "$HOME/.codex/skills/retired-skill"
# A copy owned by the retired repository is safe to migrate and is rewritten
# with the canonical ds-cli owner on the next install.
printf '%s\n' 'nisunze/ds-cli-skills' > "$HOME/.codex/skills/retired-skill/.ds-cli-skills-owner"
"$INSTALLER" install >/dev/null
[[ ! -e "$HOME/.codex/skills/retired-skill" ]]

mkdir "$HOME/.codex/skills/unrelated"
printf 'mine\n' > "$HOME/.codex/skills/unrelated/SKILL.md"
if "$INSTALLER" typo >/dev/null 2>&1; then
	echo "ERROR: unknown installer action was accepted" >&2
	exit 1
fi
[[ -f "$HOME/.codex/skills/ds/SKILL.md" ]]

collision="$TEMP/collision"
mkdir -p "$collision/ds"
printf 'mine\n' > "$collision/ds/SKILL.md"
if CODEX_SKILLS_DIR="$collision" CLAUDE_SKILLS_DIR="$collision" \
	"$INSTALLER" install >/dev/null 2>&1; then
	echo "ERROR: unowned skill collision was replaced" >&2
	exit 1
fi
[[ "$(cat "$collision/ds/SKILL.md")" == mine ]]

"$INSTALLER" uninstall >/dev/null
for target in "$HOME/.codex/skills" "$HOME/.claude/skills" "$HOME/.copilot/skills"; do
	[[ ! -e "$target/ds" ]]
	[[ ! -e "$target/.ds-cli-skills-owned" ]]
done
[[ -f "$HOME/.codex/skills/unrelated/SKILL.md" ]]

echo "install-skills contract passed"
