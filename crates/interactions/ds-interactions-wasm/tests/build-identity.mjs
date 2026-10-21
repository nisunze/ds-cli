import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';

// Run against locally generated web bindings without installing a package or
// importing any Kernel/Network unit: node tests/build-identity.mjs <js> <wasm>.
const [jsPath, wasmPath] = process.argv.slice(2);
assert.ok(jsPath && wasmPath, 'provide generated web JavaScript and WASM paths');
const module = await import(`data:text/javascript;base64,${readFileSync(jsPath).toString('base64')}`);
module.initSync({ module: readFileSync(wasmPath) });
const encoded = module.uiEngineBuildIdentity();
assert.equal(encoded, module.uiEngineBuildIdentity());
const identity = JSON.parse(encoded);
assert.deepEqual(Object.keys(identity), [
  'schema', 'component', 'protocol', 'ui_contract', 'source_sha',
  'cargo_lock_sha256', 'rustc', 'target', 'profile', 'config_sha256',
]);
assert.equal(identity.schema, 'ds.ui-engine-build/v1');
assert.equal(identity.component, 'interactions');
assert.equal(identity.protocol, 'ds.ui-wasm/v1');
assert.equal(identity.ui_contract, 1);
assert.equal(identity.target, 'wasm32-unknown-unknown');
assert.match(identity.cargo_lock_sha256, /^[0-9a-f]{64}$/);
assert.match(identity.config_sha256, /^[0-9a-f]{64}$/);
assert.match(identity.rustc, /^rustc /);
if (identity.profile === 'release') assert.match(identity.source_sha, /^[0-9a-f]{40}$/);
else assert.equal(identity.source_sha, 'development');
// A control executes existing algorithms through the same real WASM instance.
assert.equal(module.haversine_m(30, -2, 30, -2), 0);
const projected = JSON.parse(module.projectCoordinate(JSON.stringify({ lng: 30, lat: -2 })));
assert.equal(projected.projection.mode, 'nix_itrf2005');
assert.ok(projected.point.every(Number.isFinite));
const canonical = JSON.stringify(Object.fromEntries(Object.keys(identity).sort().map(key => [key, identity[key]])));
console.log(JSON.stringify({ identity, manifest_sha256: createHash('sha256').update(canonical).digest('hex') }));
