#!/usr/bin/env bash
# Refresh the pinned Network snapshots in their consuming owners:
# shared/ds-geo-lite owns geometry; Kernel owns processor presets.
# Whole files retain exact Network bytes. Canonical portable primitives are
# maintained in ds-geo-lite and are never copied by this script.
#
# crates/kernel/ds-command-kernel/scripts/sync-geo-lite.sh [path-to-ds-network]
# The default is the ds-network checkout beside this source workspace.
set -euo pipefail

kernel_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
workspace_root="$(cd -- "${kernel_root}/../../.." && pwd)"
geometry_root="${workspace_root}/crates/shared/ds-geo-lite"

# Preserve the historical retirement operation without consulting a possibly
# dirty Network checkout. Remaining pins are unchanged by retirement.
if [[ "${1:-}" == "--retire-canonical" ]]; then
    node - "${kernel_root}" "${geometry_root}" <<'NODE'
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const [kernelRoot, geometryRoot] = process.argv.slice(2);
const retired = new Set(['geo/admission.rs', 'geo/admission_corpus.json', 'geo/spatial_predicate.rs', 'geo/viewport.rs', 'geo/comparison.rs']);
for (const vendor of [path.join(kernelRoot, 'vendor/ds-geo-lite'), path.join(geometryRoot, 'vendor')]) {
    const manifestPath = path.join(vendor, 'MANIFEST.json');
    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
    for (const entry of manifest.files.filter(entry => retired.has(entry.vendored))) {
        const filename = path.join(vendor, entry.vendored);
        const digest = crypto.createHash('sha256').update(fs.readFileSync(filename)).digest('hex');
        if (digest !== entry.sha256) throw new Error(`retired snapshot changed: ${filename}`);
        if (!fs.statSync(path.join(geometryRoot, 'src', path.basename(filename))).isFile()) {
            throw new Error(`missing canonical replacement: ${filename}`);
        }
    }
    for (const filename of retired) fs.rmSync(path.join(vendor, filename), { force: true });
    manifest.files = manifest.files.filter(entry => !retired.has(entry.vendored));
    fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
}
console.log('Retired canonical geometry snapshots; remaining drift pins preserved');
NODE
    exit 0
fi

network_root="${1:-${workspace_root}/../ds-network}"
[[ -d "${network_root}/crates/ds-geo" ]] ||
    { echo "not a ds-network checkout: ${network_root}" >&2; exit 1; }
network_root="$(cd -- "${network_root}" && pwd)"
commit="$(git -C "${network_root}" rev-parse HEAD)"
dirty="$(git -C "${network_root}" status --porcelain --untracked-files=no | head -c 1)"
[[ -z "${dirty}" ]] ||
    { echo "refusing to snapshot a dirty ds-network checkout: ${network_root}" >&2; exit 1; }

node - "${kernel_root}" "${geometry_root}" "${network_root}" "${commit}" <<'NODE'
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const [kernelRoot, geometryRoot, networkRoot, commit] = process.argv.slice(2);
const owners = [
    { vendor: path.join(geometryRoot, 'vendor'), files: [
        ['geo/identity.rs', 'crates/ds-geo/src/identity.rs'],
    ] },
    { vendor: path.join(kernelRoot, 'vendor/ds-geo-lite'), files: [
        ['network/processor_presets.json', 'crates/ds-network-contracts/src/processor_presets.json'],
    ] },
];
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
// Admit every source before changing either owner's snapshot.
for (const owner of owners) {
    owner.inputs = owner.files.map(([vendored, source]) => ({
        vendored, source, bytes: fs.readFileSync(path.join(networkRoot, source)),
    }));
}
let changed = 0;
for (const { vendor, inputs } of owners) {
    const files = inputs.map(({ vendored, source, bytes }) => {
        const filename = path.join(vendor, vendored);
        if (!fs.existsSync(filename) || !fs.readFileSync(filename).equals(bytes)) {
            fs.mkdirSync(path.dirname(filename), { recursive: true });
            fs.writeFileSync(filename, bytes);
            console.log(`updated ${filename}`);
            changed += 1;
        }
        return {
            vendored, partial: false, sha256: digest(bytes), source_commit: commit,
            sources: [{ path: source, sha256: digest(bytes) }],
        };
    });
    const manifest = {
        version: 1,
        generated_by: 'crates/kernel/ds-command-kernel/scripts/sync-geo-lite.sh',
        contract: 'crates/kernel/ds-command-kernel/docs/contracts/ds-portable-geometry.md',
        ds_network: { repo: 'ds-network', commit },
        files,
    };
    fs.writeFileSync(path.join(vendor, 'MANIFEST.json'), JSON.stringify(manifest, null, 2) + '\n');
}
console.log(`Snapshot manifests rewritten at ds-network ${commit} (${changed} file(s) updated)`);
console.log('run: cargo test -p ds-command-kernel --test geo_lite_drift');
NODE
