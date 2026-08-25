#!/usr/bin/env bash
# Install this repository's skills into the current user's agent directories.
# Existing same-name skills are replaced only when this installer owns them.
set -euo pipefail

OWNER='nisunze/ds-cli'
LEGACY_OWNER='nisunze/ds-cli-skills'
OWNER_MARKER='.ds-cli-skills-owner'
INVENTORY='.ds-cli-skills-owned'
INVENTORY_CONTRACT='ds-cli-skills-install/v1'
RECEIPT='.ds-cli-skills-receipt.json'

usage() {
	printf 'usage: %s install|uninstall\n' "$(basename "$0")" >&2
}

mode="${1:-}"
[[ $# -eq 1 && ("$mode" == install || "$mode" == uninstall) ]] || {
	usage
	exit 2
}

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_root="$here/skills"
[[ -d "$source_root" ]] || {
	echo "ERROR: skills directory is missing: $source_root" >&2
	exit 1
}

codex_home="${CODEX_HOME:-$HOME/.codex}"
targets=(
	"${CODEX_SKILLS_DIR:-$codex_home/skills}"
	"${CLAUDE_SKILLS_DIR:-$HOME/.claude/skills}"
	"${COPILOT_SKILLS_DIR:-$HOME/.copilot/skills}"
)

declare -a skill_names=()
while IFS= read -r -d '' source_dir; do
	name="$(basename "$source_dir")"
	[[ "$name" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ && -f "$source_dir/SKILL.md" ]] || {
		echo "ERROR: invalid source skill directory: $source_dir" >&2
		exit 1
	}
	if find "$source_dir" -type l -print -quit | grep -q .; then
		echo "ERROR: source skill contains a symlink: $name" >&2
		exit 1
	fi
	skill_names+=("$name")
done < <(find "$source_root" -mindepth 1 -maxdepth 1 -type d -print0 | sort -z)
[[ ${#skill_names[@]} -gt 0 ]] || {
	echo "ERROR: no skills found under $source_root" >&2
	exit 1
}

is_owned() {
	local dir="$1" marker_owner
	[[ -f "$dir/$OWNER_MARKER" && ! -L "$dir/$OWNER_MARKER" ]] || return 1
	marker_owner="$(cat "$dir/$OWNER_MARKER")"
	[[ "$marker_owner" == "$OWNER" || "$marker_owner" == "$LEGACY_OWNER" ]]
}

read_inventory() {
	local target="$1" inventory="$target/$INVENTORY"
	OLD_SKILLS=()
	INVENTORY_PRESENT=0
	[[ -e "$inventory" ]] || return 0
	[[ -f "$inventory" && ! -L "$inventory" ]] || {
		echo "ERROR: install inventory is not a regular file: $inventory" >&2
		return 1
	}
	mapfile -t lines < "$inventory"
	[[ "${lines[0]:-}" == "$INVENTORY_CONTRACT" ]] || {
		echo "ERROR: refusing an inventory not owned by this installer: $inventory" >&2
		return 1
	}
	INVENTORY_PRESENT=1
	local name
	for name in "${lines[@]:1}"; do
		[[ "$name" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]] || {
			echo "ERROR: invalid owned skill name in $inventory: $name" >&2
			return 1
		}
		OLD_SKILLS+=("$name")
	done
}

preflight_target() {
	local target="$1" name destination
	read_inventory "$target"
	if [[ -e "$target/$RECEIPT" || -L "$target/$RECEIPT" ]]; then
		[[ "$INVENTORY_PRESENT" == 1 && -f "$target/$RECEIPT" && ! -L "$target/$RECEIPT" ]] || {
			echo "ERROR: refusing unowned or non-regular install receipt: $target/$RECEIPT" >&2
			return 1
		}
	fi
	for name in "${skill_names[@]}"; do
		destination="$target/$name"
		if [[ -e "$destination" || -L "$destination" ]]; then
			[[ -d "$destination" && ! -L "$destination" ]] || {
				echo "ERROR: skill destination is not a regular directory: $destination" >&2
				return 1
			}
			is_owned "$destination" || {
				echo "ERROR: refusing to replace unowned skill: $destination" >&2
				return 1
			}
		fi
	done
	for name in "${OLD_SKILLS[@]}"; do
		destination="$target/$name"
		if [[ -e "$destination" || -L "$destination" ]]; then
			[[ -d "$destination" && ! -L "$destination" ]] && is_owned "$destination" || {
				echo "ERROR: refusing to remove unowned skill from inventory: $destination" >&2
				return 1
			}
		fi
	done
}

install_target() {
	local target="$1" stage backup name destination inventory_tmp receipt_tmp
	mkdir -p "$target"
	preflight_target "$target"
	stage="$(mktemp -d "$target/.ds-cli-skills-stage.XXXXXXXX")"
	backup="$(mktemp -d "$target/.ds-cli-skills-backup.XXXXXXXX")"
	for name in "${skill_names[@]}"; do
		cp -R -- "$source_root/$name" "$stage/$name"
		printf '%s\n' "$OWNER" > "$stage/$name/$OWNER_MARKER"
	done
	if [[ -e "$here/receipt.json" || -L "$here/receipt.json" ]]; then
		[[ -f "$here/receipt.json" && ! -L "$here/receipt.json" ]] || {
			rm -rf -- "$stage" "$backup"
			echo "ERROR: bundle receipt is not a regular file: $here/receipt.json" >&2
			return 1
		}
		cp -- "$here/receipt.json" "$stage/$RECEIPT"
	fi
	[[ ! -e "$target/$INVENTORY" ]] || cp -- "$target/$INVENTORY" "$backup/$INVENTORY"
	[[ ! -e "$target/$RECEIPT" ]] || cp -- "$target/$RECEIPT" "$backup/$RECEIPT"

	# Move old owned directories aside first. Every rename stays on one
	# filesystem; a failed commit restores the previous installation.
	if ! {
		for name in "${OLD_SKILLS[@]}"; do
			destination="$target/$name"
			[[ ! -e "$destination" ]] || mv -- "$destination" "$backup/$name"
		done
		for name in "${skill_names[@]}"; do
			destination="$target/$name"
			if [[ -e "$destination" ]]; then
				mv -- "$destination" "$backup/$name"
			fi
			mv -- "$stage/$name" "$destination"
		done
		rm -f -- "$target/$RECEIPT"
		if [[ -f "$stage/$RECEIPT" ]]; then
			receipt_tmp="$(mktemp "$target/$RECEIPT.tmp.XXXXXXXX")"
			cp -- "$stage/$RECEIPT" "$receipt_tmp"
			mv -f -- "$receipt_tmp" "$target/$RECEIPT"
		fi
		inventory_tmp="$(mktemp "$target/$INVENTORY.tmp.XXXXXXXX")"
		printf '%s\n' "$INVENTORY_CONTRACT" "${skill_names[@]}" > "$inventory_tmp"
		mv -f -- "$inventory_tmp" "$target/$INVENTORY"
	}; then
		for name in "${skill_names[@]}"; do
			[[ ! -e "$target/$name" ]] || rm -rf -- "$target/$name"
		done
		for name in "${OLD_SKILLS[@]}" "${skill_names[@]}"; do
			[[ ! -e "$backup/$name" || -e "$target/$name" ]] || mv -- "$backup/$name" "$target/$name"
		done
		rm -f -- "$target/$INVENTORY" "$target/$RECEIPT"
		[[ ! -f "$backup/$INVENTORY" ]] || mv -- "$backup/$INVENTORY" "$target/$INVENTORY"
		[[ ! -f "$backup/$RECEIPT" ]] || mv -- "$backup/$RECEIPT" "$target/$RECEIPT"
		rm -rf -- "$stage" "$backup"
		echo "ERROR: skill installation rolled back for $target" >&2
		return 1
	fi
	rm -rf -- "$stage" "$backup"
	printf 'installed %s owned skill(s) -> %s\n' "${#skill_names[@]}" "$target"
}

uninstall_target() {
	local target="$1" name destination
	[[ -d "$target" ]] || return 0
	read_inventory "$target"
	[[ "$INVENTORY_PRESENT" == 1 ]] || {
		printf 'no owned skills -> %s\n' "$target"
		return 0
	}
	for name in "${OLD_SKILLS[@]}"; do
		destination="$target/$name"
		if [[ -e "$destination" || -L "$destination" ]]; then
			[[ -d "$destination" && ! -L "$destination" ]] && is_owned "$destination" || {
				echo "ERROR: refusing to remove unowned skill: $destination" >&2
				return 1
			}
		fi
	done
	for name in "${OLD_SKILLS[@]}"; do
		destination="$target/$name"
		[[ ! -e "$destination" ]] || rm -rf -- "$destination"
	done
	rm -f -- "$target/$INVENTORY" "$target/$RECEIPT"
	printf 'removed %s owned skill(s) from %s\n' "${#OLD_SKILLS[@]}" "$target"
}

declare -A seen=()
for target in "${targets[@]}"; do
	[[ -n "$target" ]] || { echo "ERROR: an agent skill directory is empty" >&2; exit 1; }
	[[ "$target" == /* ]] || { echo "ERROR: agent skill directory must be absolute: $target" >&2; exit 1; }
	[[ -z "${seen[$target]:-}" ]] || continue
	seen[$target]=1
	if [[ "$mode" == install ]]; then
		install_target "$target"
	else
		uninstall_target "$target"
	fi
done
