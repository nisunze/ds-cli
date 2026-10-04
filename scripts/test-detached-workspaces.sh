#!/usr/bin/env bash
# Run the tests of every kernel crate that is NOT in the kernel's workspace.
#
# `crates/ds-design-workspace` is a native host crate that legitimately depends
# on ds-network, and cargo resolves every workspace member's manifest before
# building any of them — so while it was a `members` entry the kernel could not
# build with ds-network absent (docs/contracts/ds-portable-geometry.md §4). It
# now carries its own `[workspace]`, which is correct and has one consequence
# worth a script: `cargo test --workspace` in the kernel no longer reaches it,
# and neither does ds-cli's (it sits outside that workspace's directory and
# declares its own). Without this, its tests run nowhere.
#
# `tests/geo_lite_drift.rs::every_detached_workspace_is_covered_by_the_gate_script`
# fails if a detached crate is added and not listed here.
#
#   scripts/test-detached-workspaces.sh [extra cargo args...]
set -euo pipefail

kernel_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

# Every detached crate, one manifest path per line.
manifests=(
	"crates/ds-design-workspace/Cargo.toml"
	"crates/ds-compute-runtime/Cargo.toml"
	"crates/ds-profile-runtime/Cargo.toml"
	"crates/ds-report-artifacts/Cargo.toml"
	# Server core crates moved here from ds-web on 2026-09-14: the server build
	# (ds-cli) must compile with zero ds-web source, so the crates it links live
	# in the kernel repository and the desktop consumes them from here.
	"crates/ds-client-core/Cargo.toml"
	"crates/ds-edge-authority/Cargo.toml"
	"crates/ds-layer-store/Cargo.toml"
	"crates/ds-project-data/Cargo.toml"
	"crates/ds-report-host/Cargo.toml"
	"crates/ds-assets-pdf-host/Cargo.toml"
	"crates/ds-sync-runtime/Cargo.toml"
	"crates/ds-tile-runtime/Cargo.toml"
)

# Crates whose manifests reach ../ds-network; the others build without it.
needs_network=(
	"crates/ds-design-workspace/Cargo.toml"
	"crates/ds-compute-runtime/Cargo.toml"
	"crates/ds-profile-runtime/Cargo.toml"
	"crates/ds-project-data/Cargo.toml"
)

for manifest in "${manifests[@]}"; do
	if [[ ! -d "${kernel_root}/../ds-network" && " ${needs_network[*]} " == *" ${manifest} "* ]]; then
		echo "SKIPPED: $manifest requires ../ds-network."
		continue
	fi
	echo "==> ${manifest}"
	cargo test --locked --manifest-path "${kernel_root}/${manifest}" "$@"
done
