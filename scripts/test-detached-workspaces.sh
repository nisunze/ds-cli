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
	"crates/ds-report-artifacts/Cargo.toml"
)

for manifest in "${manifests[@]}"; do
	if [[ ! -d "${kernel_root}/../ds-network" && "$manifest" != "crates/ds-report-artifacts/Cargo.toml" ]]; then
		echo "SKIPPED: $manifest requires ../ds-network."
		continue
	fi
	echo "==> ${manifest}"
	cargo test --locked --manifest-path "${kernel_root}/${manifest}" "$@"
done
