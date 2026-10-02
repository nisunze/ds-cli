#!/usr/bin/env bash
# Refresh vendor/ds-geo-lite/ from a ds-network checkout, and rewrite its
# MANIFEST.json.
#
# The kernel does NOT depend on ds-network (docs/contracts/
# ds-kernel-network-boundary.md). Legacy geometry/canvas snapshots remain
# maintained in ds-network; canonical primitives live in crates/ds-geo-lite —
# see docs/contracts/ds-portable-geometry.md. This script is the only
# sanctioned way to move bytes across that line; tests/geo_lite_drift.rs is
# what keeps the snapshot honest afterwards.
#
#   scripts/sync-geo-lite.sh [path-to-ds-network]   (default: ../ds-network)
#
# Whole-file entries are copied byte for byte. Primitive geometry is now
# canonical in crates/ds-geo-lite and must never be copied by this script.
set -euo pipefail

kernel_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# Ownership handoff only: preserve remaining source pins without reading a
# potentially dirty Network checkout. Retired bytes must match their old pins.
if [[ "${1:-}" == "--retire-canonical" ]]; then
    python3 - "${kernel_root}" <<'PYTHON'
from pathlib import Path
import hashlib, json, sys
root = Path(sys.argv[1])
vendor = root / "vendor/ds-geo-lite"
manifest_path = vendor / "MANIFEST.json"
manifest = json.loads(manifest_path.read_text())
retired = {"geo/admission.rs", "geo/admission_corpus.json", "geo/spatial_predicate.rs", "geo/viewport.rs"}
for entry in manifest["files"]:
    if entry["vendored"] in retired:
        path = vendor / entry["vendored"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == entry["sha256"], path
        assert (root / "crates/ds-geo-lite/src" / path.name).is_file(), path
for name in retired:
    (vendor / name).unlink(missing_ok=True)
manifest["files"] = [e for e in manifest["files"] if e["vendored"] not in retired]
manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
print("Retired canonical geometry snapshots; remaining drift pins preserved")
PYTHON
    exit 0
fi

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
	"geo/identity.rs:crates/ds-geo/src/identity.rs"
	"geo/comparison.rs:crates/ds-geo/src/comparison.rs"
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
digest() { sha256sum "$1" | cut -d' ' -f1; }

mkdir -p "${vendor_dir}/geo" "${vendor_dir}/canvas" "${vendor_dir}/network"

# Admission, predicates and extents are now canonical toolkit modules.
# Remove their retired snapshots while retaining the drift fence for the rest.
for retired in geo/admission.rs geo/admission_corpus.json geo/spatial_predicate.rs geo/viewport.rs; do
    rm -f -- "${vendor_dir}/${retired}"
done

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
	printf '\n'
	printf '  ]\n'
	printf '}\n'
} >"${vendor_dir}/MANIFEST.json"

echo "MANIFEST.json rewritten at ds-network ${commit} (${changed} file(s) updated)"
echo "run: cargo test --test geo_lite_drift"
