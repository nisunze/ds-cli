#!/usr/bin/env bash
# Refresh vendor/ds-geo-lite/ from a ds-network checkout, and rewrite its
# MANIFEST.json.
#
# The kernel does NOT depend on ds-network (docs/contracts/
# ds-kernel-network-boundary.md). The portable geometry it needs is maintained
# in ds-network and compiled into the kernel as a digest-pinned SNAPSHOT —
# see docs/contracts/ds-portable-geometry.md. This script is the only
# sanctioned way to move bytes across that line; tests/geo_lite_drift.rs is
# what keeps the snapshot honest afterwards.
#
#   scripts/sync-geo-lite.sh [path-to-ds-network]   (default: ../ds-network)
#
# Whole-file entries are copied byte for byte. The one PARTIAL entry
# (geo/haversine.rs) is hand-derived from three ds-geo-ops files and is never
# overwritten: the script only re-reads its sources' digests, and tells you to
# re-derive it by hand when they moved.
set -euo pipefail

kernel_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
network_root="${1:-${kernel_root}/../ds-network}"
vendor_dir="${kernel_root}/vendor/ds-geo-lite"

[[ -d "${network_root}/crates/ds-geo" ]] ||
	{ echo "not a ds-network checkout: ${network_root}" >&2; exit 1; }
network_root="$(cd -- "${network_root}" && pwd)"

commit="$(git -C "${network_root}" rev-parse HEAD)"
dirty="$(git -C "${network_root}" status --porcelain --untracked-files=no | head -c 1)"
[[ -z "${dirty}" ]] ||
	{ echo "refusing to snapshot a dirty ds-network checkout: ${network_root}" >&2; exit 1; }

# vendored-path : ds-network source path(s), ':' separated
whole=(
	"geo/viewport.rs:crates/ds-geo/src/viewport.rs"
	"geo/spatial_predicate.rs:crates/ds-geo/src/spatial_predicate.rs"
	"geo/identity.rs:crates/ds-geo/src/identity.rs"
	"geo/comparison.rs:crates/ds-geo/src/comparison.rs"
	"geo/admission.rs:crates/ds-geo/src/admission.rs"
	# The admission tests read their BigQuery parity corpus beside them.
	"geo/admission_corpus.json:crates/ds-geo/src/admission_corpus.json"
	"canvas/lib.rs:crates/ds-canvas/src/lib.rs"
	"canvas/viewport.rs:crates/ds-canvas/src/viewport.rs"
	"canvas/selection.rs:crates/ds-canvas/src/selection.rs"
	"canvas/measured_path.rs:crates/ds-canvas/src/measured_path.rs"
	"canvas/free_layout.rs:crates/ds-canvas/src/free_layout.rs"
	"canvas/packed.rs:crates/ds-canvas/src/packed.rs"
	"canvas/layout_session.rs:crates/ds-canvas/src/layout_session.rs"
	# Not geometry: the processor catalogue `process_settings` is pure over.
	# ds-network publishes it, the kernel's tests read it, and an include_str!
	# reaching across the repo line is the same broken build as a path
	# dependency — so it is snapshotted here too.
	"network/processor_presets.json:crates/ds-network/src/network/processor_presets.json"
)
partial_vendored="geo/haversine.rs"
partial_sources=(
	"crates/ds-geo-ops/src/primitives/haversine.rs"
	"crates/ds-geo-ops/src/lib.rs"
	"crates/ds-geo-ops/src/types.rs"
)
partial_items=(
	"primitives/haversine.rs::distance  -> haversine_distance_m (Coord args flattened to f64)"
	"lib.rs::haversine_m                -> haversine_m (non-finite guard)"
	"types.rs::EARTH_RADIUS_M           -> EARTH_RADIUS_M"
)
partial_why="ds-geo-ops::haversine_m is three items spread over three files whose siblings pull the whole primitives/operations tree; the kernel calls one function, so the cut is at item level."

digest() { sha256sum "$1" | cut -d' ' -f1; }
json_array() { local first=1 x; for x in "$@"; do [[ ${first} -eq 1 ]] || printf ',\n'; first=0; printf '        "%s"' "${x}"; done; printf '\n'; }

mkdir -p "${vendor_dir}/geo" "${vendor_dir}/canvas" "${vendor_dir}/network"

changed=0
for pair in "${whole[@]}"; do
	vendored="${pair%%:*}"
	source="${pair#*:}"
	[[ -f "${network_root}/${source}" ]] ||
		{ echo "missing in ds-network: ${source}" >&2; exit 1; }
	if [[ ! -f "${vendor_dir}/${vendored}" ]] ||
		! cmp -s "${network_root}/${source}" "${vendor_dir}/${vendored}"; then
		cp -- "${network_root}/${source}" "${vendor_dir}/${vendored}"
		echo "updated ${vendored}"
		changed=$((changed + 1))
	fi
done

[[ -f "${vendor_dir}/${partial_vendored}" ]] ||
	{ echo "${partial_vendored} is hand-derived and missing; restore it from git" >&2; exit 1; }

{
	printf '{\n'
	printf '  "version": 1,\n'
	printf '  "generated_by": "scripts/sync-geo-lite.sh",\n'
	printf '  "contract": "docs/contracts/ds-portable-geometry.md",\n'
	printf '  "ds_network": {\n'
	printf '    "repo": "ds-network",\n'
	printf '    "commit": "%s"\n' "${commit}"
	printf '  },\n'
	printf '  "files": [\n'
	first=1
	for pair in "${whole[@]}"; do
		vendored="${pair%%:*}"
		source="${pair#*:}"
		[[ ${first} -eq 1 ]] || printf ',\n'
		first=0
		printf '    {\n'
		printf '      "vendored": "%s",\n' "${vendored}"
		printf '      "partial": false,\n'
		printf '      "sha256": "%s",\n' "$(digest "${vendor_dir}/${vendored}")"
		printf '      "source_commit": "%s",\n' "${commit}"
		printf '      "sources": [\n'
		printf '        { "path": "%s", "sha256": "%s" }\n' \
			"${source}" "$(digest "${network_root}/${source}")"
		printf '      ]\n'
		printf '    }'
	done
	printf ',\n'
	printf '    {\n'
	printf '      "vendored": "%s",\n' "${partial_vendored}"
	printf '      "partial": true,\n'
	printf '      "sha256": "%s",\n' "$(digest "${vendor_dir}/${partial_vendored}")"
	printf '      "source_commit": "%s",\n' "${commit}"
	printf '      "why": "%s",\n' "${partial_why}"
	printf '      "items": [\n'
	json_array "${partial_items[@]}"
	printf '      ],\n'
	printf '      "sources": [\n'
	pfirst=1
	for source in "${partial_sources[@]}"; do
		[[ -f "${network_root}/${source}" ]] ||
			{ echo "missing in ds-network: ${source}" >&2; exit 1; }
		[[ ${pfirst} -eq 1 ]] || printf ',\n'
		pfirst=0
		printf '        { "path": "%s", "sha256": "%s" }' \
			"${source}" "$(digest "${network_root}/${source}")"
	done
	printf '\n      ]\n'
	printf '    }\n'
	printf '  ]\n'
	printf '}\n'
} >"${vendor_dir}/MANIFEST.json"

echo "MANIFEST.json rewritten at ds-network ${commit} (${changed} file(s) updated)"
echo "run: cargo test --test geo_lite_drift"
