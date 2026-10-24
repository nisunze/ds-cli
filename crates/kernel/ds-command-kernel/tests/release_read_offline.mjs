// Run against a generated, paired Kernel artifact:
// node release_read_offline.mjs /absolute/ds_command_kernel.js /absolute/ds_command_kernel_bg.wasm
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const [entry, wasm] = process.argv.slice(2);
assert(entry && wasm, 'name the actual generated JS and WASM pair');
const kernel = await import(pathToFileURL(entry));
await kernel.default({ module_or_path: readFileSync(wasm) });

const url = 'https://gateway.example.test/api/v1/app_releases?component=core&channel=stable&runtime=native';
const originalFetch = globalThis.fetch;
let requests = [];
let storeCalls = 0;
const store = { transaction: async () => { storeCalls++; throw Error('isolated fixture has no store'); } };
globalThis.fetch = async request => {
 assert.equal(request.method, 'GET');
 assert.equal(request.headers.get('Accept'), 'application/json');
 assert.equal(request.headers.get('Authorization'), null);
 requests.push(request.url);
 const response = new Response(JSON.stringify({ release: { version: 'fixture' } }), {
  headers: { 'Content-Type': 'application/json' },
 });
 // A real fetch response carries its URL; Node's synthetic Response does not.
 Object.defineProperty(response, 'url', { value: request.url });
 return response;
};
const facts = { now_ms: 1, has_session: true, offline_mode: false, link_up: true, principal: 'fixture-user' };
const host = overrides => ({ facts: () => ({ ...facts, ...overrides }), refused: () => assert.fail('quiet read reported a refusal') });

try {
 for (const overrides of [{ offline_mode: true }, { link_up: false }, { offline_mode: true, link_up: false }]) {
  requests = []; storeCalls = 0;
  assert.equal(await kernel.releaseRefresh(host(overrides), store, url), undefined);
  assert.equal(await kernel.installationManagerRefresh(host(overrides), store, url, 'stable'), undefined);
  assert.deepEqual(requests, []);
  assert.equal(storeCalls, 0, 'offline metadata acquisition must not touch retained rows');
 }
 for (const unavailable of [
  { facts: () => { throw Error('host facts unavailable'); } },
  { facts: () => ({ ...facts, link_up: 'unreadable' }) },
 ]) {
  requests = []; storeCalls = 0;
  assert.equal(await kernel.releaseRefresh(unavailable, store, url), undefined);
  assert.equal(await kernel.installationManagerRefresh(unavailable, store, url, 'canary'), undefined);
  assert.deepEqual(requests, []);
  assert.equal(storeCalls, 0);
 }
 requests = []; storeCalls = 0;
 assert.equal(await kernel.installationManagerRefresh(host({}), store, url, 'other'), undefined);
 assert.deepEqual(requests, [], 'invalid lanes refuse before acquisition');
 assert.equal(storeCalls, 0);

 // Explicit online Refresh stays a force read; no implicit cache or stamp call.
 requests = [];
 for (let i = 0; i < 2; i++) {
  assert.deepEqual(await kernel.releaseRefresh(host({}), store, url), { release: { version: 'fixture' } });
 }
 assert.deepEqual(requests, [url, url]);
 requests = [];
 await kernel.installationManagerRefresh(host({}), store, url, 'canary');
 assert.deepEqual(requests, [url]);
 console.log('release acquisition: offline/unavailable facts/invalid lane = zero requests; explicit online Refresh = one anonymous GET');
} finally {
 globalThis.fetch = originalFetch;
}
