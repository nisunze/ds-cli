#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMP="$(mktemp -d)"
trap 'rm -rf -- "$TEMP"' EXIT INT TERM

# The installer resolves its bundle root as the parent of its own scripts
# directory and reads `receipt.json` from there, so testing receipt
# propagation used to mean writing into the repository root. `.gitignore`
# covered only `/target`, so a run killed between writing that file and its
# trap left an untracked *and* unignored `receipt.json` for the next
# `git add -A` to commit. Copy the bundle into the temporary directory and
# mutate that instead: this test now writes nothing outside "$TEMP".
BUNDLE="$TEMP/bundle"
mkdir -p "$BUNDLE"
cp -R -- "$ROOT/skills" "$BUNDLE/skills"
cp -R -- "$ROOT/scripts" "$BUNDLE/scripts"
INSTALLER="$BUNDLE/scripts/install-skills.sh"

export HOME="$TEMP/home"
export CODEX_HOME="$HOME/.codex"
mkdir -p "$HOME"

# Run the installer against one scratch target in place of all three real
# ones, so a refusal case cannot disturb the installation the later
# assertions read.
scratch_install() {
	local target="$1" mode="$2"
	CODEX_SKILLS_DIR="$target" CLAUDE_SKILLS_DIR="$target" COPILOT_SKILLS_DIR="$target" \
		"$INSTALLER" "$mode"
}

"$INSTALLER" install >/dev/null
for target in "$HOME/.codex/skills" "$HOME/.claude/skills" "$HOME/.copilot/skills"; do
	[[ -f "$target/ds/SKILL.md" ]]
	[[ -f "$target/ds-solar-portfolio/SKILL.md" ]]
	[[ -f "$target/ds-solar-workflow/SKILL.md" ]]
	[[ "$(cat "$target/ds/.ds-cli-skills-owner")" == 'nisunze/ds-cli' ]]
	[[ "$(cat "$target/ds-solar-portfolio/.ds-cli-skills-owner")" == 'nisunze/ds-cli' ]]
	[[ "$(cat "$target/ds-solar-workflow/.ds-cli-skills-owner")" == 'nisunze/ds-cli' ]]
	[[ "$(head -n 1 "$target/.ds-cli-skills-owned")" == 'ds-cli-skills-install/v1' ]]
	[[ ! -e "$HOME/.agents/skills/ds/SKILL.md" ]]
done

# A packaged bundle propagates its provenance receipt; a source checkout does
# not invent one. Reinstalling from source removes a stale packaged receipt.
[[ ! -e "$HOME/.codex/skills/.ds-cli-skills-receipt.json" ]]
printf '{"contract":"ds-cli-skills-bundle/v3"}\n' > "$BUNDLE/receipt.json"
"$INSTALLER" install >/dev/null
cmp "$BUNDLE/receipt.json" "$HOME/.codex/skills/.ds-cli-skills-receipt.json"
rm -f -- "$BUNDLE/receipt.json"
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

# The uninstall ownership check guards the one `rm -rf` in the installer whose
# argument is a name read from a file on disk. It is reached only after a
# directory named in the inventory has stopped being ours, which no earlier
# case produces — so produce it here, and prove the refusal happens before any
# deletion, not after some of them.
unowned="$TEMP/unowned/skills"
mkdir -p "$unowned"
scratch_install "$unowned" install >/dev/null
printf 'someone-else\n' > "$unowned/ds/.ds-cli-skills-owner"
if scratch_install "$unowned" uninstall >/dev/null 2>&1; then
	echo "ERROR: uninstall removed a skill this installer no longer owns" >&2
	exit 1
fi
[[ -f "$unowned/ds/SKILL.md" ]]
[[ -f "$unowned/ds-solar-portfolio/SKILL.md" ]]
[[ -f "$unowned/.ds-cli-skills-owned" ]]

# An owner marker that is a symlink is not a claim of ownership: it can be
# pointed at any file whose first line happens to read `nisunze/ds-cli`.
printf '%s\n' 'nisunze/ds-cli' > "$TEMP/unowned/borrowed-marker"
ln -s "$TEMP/unowned/borrowed-marker" "$unowned/ds/.ds-cli-skills-owner.link"
mv -- "$unowned/ds/.ds-cli-skills-owner.link" "$unowned/ds/.ds-cli-skills-owner"
if scratch_install "$unowned" install >/dev/null 2>&1; then
	echo "ERROR: a symlinked owner marker was accepted as proof of ownership" >&2
	exit 1
fi
[[ -L "$unowned/ds/.ds-cli-skills-owner" ]]

# A first line that is not this installer's contract means the file belongs to
# something else. Both actions must refuse rather than read a list of
# directories to delete out of it.
foreign="$TEMP/foreign/skills"
mkdir -p "$foreign"
scratch_install "$foreign" install >/dev/null
printf '%s\n%s\n' 'someone-elses-inventory/v9' 'ds' > "$foreign/.ds-cli-skills-owned"
if scratch_install "$foreign" install >/dev/null 2>&1; then
	echo "ERROR: install accepted an inventory it does not own" >&2
	exit 1
fi
if scratch_install "$foreign" uninstall >/dev/null 2>&1; then
	echo "ERROR: uninstall accepted an inventory it does not own" >&2
	exit 1
fi
[[ -f "$foreign/ds/SKILL.md" ]]

# Every name in the inventory is joined onto the target path, so the name
# pattern is a path-traversal guard: without it `rm -rf -- "$target/../canary"`
# deletes a directory outside the one the operator named. The canary is
# deliberately shaped like an owned skill, so the ownership check would wave it
# through and the name pattern is the only thing left standing.
traversal="$TEMP/traversal"
mkdir -p "$traversal/skills" "$traversal/canary"
printf 'do not delete me\n' > "$traversal/canary/SKILL.md"
printf '%s\n' 'nisunze/ds-cli' > "$traversal/canary/.ds-cli-skills-owner"
scratch_install "$traversal/skills" install >/dev/null
printf '%s\n%s\n' 'ds-cli-skills-install/v1' '../canary' \
	> "$traversal/skills/.ds-cli-skills-owned"
if scratch_install "$traversal/skills" uninstall >/dev/null 2>&1; then
	echo "ERROR: uninstall accepted a path-traversing skill name" >&2
	exit 1
fi
[[ -f "$traversal/canary/SKILL.md" ]]
if scratch_install "$traversal/skills" install >/dev/null 2>&1; then
	echo "ERROR: install accepted a path-traversing skill name" >&2
	exit 1
fi
[[ -f "$traversal/canary/SKILL.md" ]]

# A symlinked destination is refused before anything is written, because
# writing through it lands outside the target directory entirely. The link
# points at a directory shaped like an owned skill, so ownership alone would
# let the installer write through it; only the `! -L` test stops that.
linked="$TEMP/linked/skills"
mkdir -p "$linked" "$TEMP/linked/elsewhere"
printf 'not ours\n' > "$TEMP/linked/elsewhere/SKILL.md"
printf '%s\n' 'nisunze/ds-cli' > "$TEMP/linked/elsewhere/.ds-cli-skills-owner"
ln -s "$TEMP/linked/elsewhere" "$linked/ds"
if scratch_install "$linked" install >/dev/null 2>&1; then
	echo "ERROR: a symlinked skill destination was replaced" >&2
	exit 1
fi
[[ -L "$linked/ds" ]]
[[ "$(cat "$TEMP/linked/elsewhere/SKILL.md")" == 'not ours' ]]

# The rollback path cannot be reached by any input: every refusal fires in
# preflight, before the first rename. A bug in it loses the installation the
# operator already had, so inject a failure into exactly one command — the
# inventory commit, the last step of the transaction — and prove the previous
# installation comes back. The witness file is the specific assertion: a
# fresh copy of the bundle would not carry it, so its presence proves the
# backup was restored rather than the install half-succeeding.
rollback="$TEMP/rollback/skills"
mkdir -p "$rollback" "$TEMP/bin"
scratch_install "$rollback" install >/dev/null
printf 'kept\n' > "$rollback/ds/witness"
real_mv="$(command -v mv)"
cat > "$TEMP/bin/mv" <<EOF
#!/usr/bin/env bash
# Fail only \`mv -f -- <inventory>.tmp.XXXXXXXX <inventory>\`; pass everything
# else through, including the rollback's own restoring moves.
for arg in "\$@"; do
	case "\$arg" in
	*.ds-cli-skills-owned.tmp.*) exit 1 ;;
	esac
done
exec "$real_mv" "\$@"
EOF
chmod +x "$TEMP/bin/mv"
saved_path="$PATH"
PATH="$TEMP/bin:$PATH"
if scratch_install "$rollback" install >/dev/null 2>&1; then
	PATH="$saved_path"
	echo "ERROR: an install whose inventory commit failed reported success" >&2
	exit 1
fi
PATH="$saved_path"
[[ -f "$rollback/ds/SKILL.md" ]]
[[ "$(cat "$rollback/ds/witness")" == kept ]]
[[ -f "$rollback/ds-solar-portfolio/SKILL.md" ]]
[[ "$(head -n 1 "$rollback/.ds-cli-skills-owned")" == 'ds-cli-skills-install/v1' ]]
[[ -z "$(find "$rollback" -maxdepth 1 \
	\( -name '.ds-cli-skills-stage.*' -o -name '.ds-cli-skills-backup.*' \) -print -quit)" ]]
# And the rolled-back target still installs cleanly afterwards.
scratch_install "$rollback" install >/dev/null
[[ ! -e "$rollback/ds/witness" ]]

"$INSTALLER" uninstall >/dev/null
for target in "$HOME/.codex/skills" "$HOME/.claude/skills" "$HOME/.copilot/skills"; do
	[[ ! -e "$target/ds" ]]
	[[ ! -e "$target/ds-solar-portfolio" ]]
	[[ ! -e "$target/ds-solar-workflow" ]]
	[[ ! -e "$target/.ds-cli-skills-owned" ]]
done
[[ -f "$HOME/.codex/skills/unrelated/SKILL.md" ]]

echo "install-skills contract passed"
