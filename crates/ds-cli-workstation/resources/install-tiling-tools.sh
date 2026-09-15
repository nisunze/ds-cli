#!/usr/bin/env bash
# Install the shared Linux tiling toolchain when compatible system commands are
# absent. DS Server and Linux Desktop both consume these ordinary host tools.
set -euo pipefail

die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

TIP_VERSION=2.82.0
TIP_REV=4f2621186acfec33b63ddf636f665623c0fef2dd
TIP_SOURCE_SHA256=9ae71b9580c62245317c4da0ce0ecf04f68ec5290e9ae43d538c79007896d201
PM_VERSION=1.20.0
PM_BINARY_SHA256=ee103decdb74bb56cb4a59c3b022afa23c9a14f60fdb1680789d719f2f3199c6
PREFIX="${DS_TILING_INSTALL_PREFIX:-/usr/local}"
LOCK_TIMEOUT="${DS_GRIDDESIGN_APT_LOCK_TIMEOUT_SECONDS:-900}"
BUILD_JOBS="${DS_TILING_BUILD_JOBS:-2}"

[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] \
	|| die 'The shared tiling tools require Linux x86-64.'
[[ "$PREFIX" == /* && ! -L "$PREFIX" ]] \
	|| die 'DS_TILING_INSTALL_PREFIX must be an absolute non-symlink path.'
[[ "$LOCK_TIMEOUT" =~ ^[0-9]+$ && "$LOCK_TIMEOUT" -ge 1 && "$LOCK_TIMEOUT" -le 3600 ]] \
	|| die 'APT lock timeout must be 1 through 3600 seconds.'
[[ "$BUILD_JOBS" =~ ^[12]$ ]] || die 'DS_TILING_BUILD_JOBS must be 1 or 2.'

tip_ready=0
pm_ready=0
tip_path="$(command -v tippecanoe 2>/dev/null || true)"
pm_path="$(command -v pmtiles 2>/dev/null || true)"
[[ -z "$tip_path" ]] || [[ "$("$tip_path" --version 2>&1 || true)" != "tippecanoe v$TIP_VERSION" ]] \
	|| tip_ready=1
[[ -z "$pm_path" ]] || [[ "$("$pm_path" version 2>&1 || true)" != pmtiles\ "$PM_VERSION,"* ]] \
	|| pm_ready=1

if ((tip_ready && pm_ready)); then
	printf 'Using shared Tippecanoe %s and PMTiles %s from the host PATH.\n' "$TIP_VERSION" "$PM_VERSION"
	exit 0
fi

for tool in curl install sha256sum tar; do
	command -v "$tool" >/dev/null 2>&1 || die "$tool is required to install shared tiling tools."
done
if [[ $EUID -ne 0 ]]; then
	command -v sudo >/dev/null 2>&1 || die 'Administrator access through sudo or root is required.'
	sudo_noninteractive="${DS_GRIDDESIGN_SUDO_NONINTERACTIVE:-0}"
	[[ "$sudo_noninteractive" == 0 || "$sudo_noninteractive" == 1 ]] \
		|| die 'DS_GRIDDESIGN_SUDO_NONINTERACTIVE must be 0 or 1.'
	admin=(sudo)
	[[ "$sudo_noninteractive" == 0 ]] || admin+=(-n)
else
	admin=()
fi
run_admin() { "${admin[@]}" "$@"; }
refresh_admin() {
	((${#admin[@]} == 0)) || "${admin[@]}" -v
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if ((!tip_ready)); then
	command -v apt-get >/dev/null 2>&1 || die 'apt-get is required to install the Tippecanoe build prerequisites.'
	run_admin apt-get -o "DPkg::Lock::Timeout=$LOCK_TIMEOUT" install -y \
		build-essential libsqlite3-dev zlib1g-dev
	tip_archive="$tmp/tippecanoe.tar.gz"
	curl --proto '=https' --proto-redir '=https' --fail --location --retry 3 \
		--output "$tip_archive" "https://codeload.github.com/felt/tippecanoe/tar.gz/$TIP_REV"
	actual="$(sha256sum "$tip_archive")"
	[[ "${actual%% *}" == "$TIP_SOURCE_SHA256" ]] \
		|| die 'Downloaded Tippecanoe source does not match the pinned SHA-256.'
	mkdir -p "$tmp/tippecanoe"
	tar -xzf "$tip_archive" -C "$tmp/tippecanoe" --strip-components=1
	make -C "$tmp/tippecanoe" -j"$BUILD_JOBS" tippecanoe
	[[ "$("$tmp/tippecanoe/tippecanoe" --version 2>&1)" == "tippecanoe v$TIP_VERSION" ]] \
		|| die 'Built Tippecanoe does not match the required version.'
fi

if ((!pm_ready)); then
	pm_archive="$tmp/pmtiles.tar.gz"
	pm_binary="$tmp/pmtiles"
	curl --proto '=https' --proto-redir '=https' --fail --location --retry 3 \
		--output "$pm_archive" \
		"https://github.com/protomaps/go-pmtiles/releases/download/v$PM_VERSION/go-pmtiles_${PM_VERSION}_Linux_x86_64.tar.gz"
	tar -xOf "$pm_archive" pmtiles >"$pm_binary"
	actual="$(sha256sum "$pm_binary")"
	[[ "${actual%% *}" == "$PM_BINARY_SHA256" ]] \
		|| die 'Downloaded PMTiles executable does not match the pinned SHA-256.'
fi

# Source compilation can outlive sudo's timestamp. Acquire or refresh
# elevation only after every artifact is ready, then perform the short owned
# installation as one contiguous step.
refresh_admin
run_admin install -d -m 0755 "$PREFIX/bin" "$PREFIX/share/doc/tippecanoe" "$PREFIX/share/doc/pmtiles"
if ((!tip_ready)); then
	run_admin install -m 0755 "$tmp/tippecanoe/tippecanoe" "$PREFIX/bin/tippecanoe"
	run_admin install -m 0644 "$tmp/tippecanoe/LICENSE.md" "$PREFIX/share/doc/tippecanoe/LICENSE.md"
fi
if ((!pm_ready)); then
	run_admin install -m 0755 "$pm_binary" "$PREFIX/bin/pmtiles"
fi

final_tip_path="$tip_path"
final_pm_path="$pm_path"
((tip_ready)) || final_tip_path="$PREFIX/bin/tippecanoe"
((pm_ready)) || final_pm_path="$PREFIX/bin/pmtiles"
[[ "$("$final_tip_path" --version 2>&1)" == "tippecanoe v$TIP_VERSION" ]] \
	|| die 'The installed Tippecanoe executable failed verification.'
[[ "$("$final_pm_path" version 2>&1)" == pmtiles\ "$PM_VERSION,"* ]] \
	|| die 'The installed PMTiles executable failed verification.'
printf 'Shared Tippecanoe %s and PMTiles %s are ready. Newly installed tools are under %s/bin.\n' \
	"$TIP_VERSION" "$PM_VERSION" "$PREFIX"
