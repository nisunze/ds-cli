// The page Kernel's one door, `invoke(capability_id, request_json, host)`,
// on built units, against the installed CLI's own `--output json` envelopes.
//
//   node tests/invoke-unit-export.mjs <web Kernel package> <desktop Kernel package> [ds executable]
//
// Build both packages with wasm-pack (--target web) through the web-local and
// desktop-local targets of release/wasm-unit.json. The host here is what a
// page passes: its doors, never a decision. Each executor kind answers once:
// an in-page Kernel owner (native and through the gateway executor), the
// Network unit, Core through the Desktop's Core door (here the installed `ds`
// itself, so its refusal is the CLI's byte for byte), a pending row, a
// discarded row, an unknown id and a malformed request. Every envelope must
// have the CLI's members and, where the CLI runs the same capability, its
// status and error class/code. No request leaves this process.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

let outgoingAttempts = 0;
globalThis.fetch = () => { outgoingAttempts++; throw Error('network forbidden'); };
globalThis.XMLHttpRequest = class { constructor() { outgoingAttempts++; throw Error('network forbidden'); } };
globalThis.WebSocket = class { constructor() { outgoingAttempts++; throw Error('network forbidden'); } };
globalThis.location ??= { origin: 'http://127.0.0.1:9' };

const [webPkg, desktopPkg, dsArg] = process.argv.slice(2);
if (!webPkg || !desktopPkg) { console.error('usage: node tests/invoke-unit-export.mjs <web Kernel package> <desktop Kernel package> [ds executable]'); process.exit(2); }
const ds = dsArg ?? (fs.existsSync('/usr/bin/ds') ? '/usr/bin/ds' : null);

async function load(pkg) {
	const dir = path.resolve(pkg);
	// Each unit is its own module instance, as each host loads one.
	const kernel = await import(`${pathToFileURL(path.join(dir, 'ds_command_kernel.js')).href}?unit=${encodeURIComponent(dir)}`);
	kernel.initSync({ module: fs.readFileSync(path.join(dir, 'ds_command_kernel_bg.wasm')) });
	assert.equal(typeof kernel.invoke, 'function', `${pkg} exports invoke`);
	return kernel;
}
const web = await load(webPkg);
const desktop = await load(desktopPkg);

const SUCCESS = ['command', 'contract', 'data', 'status', 'v'];
const ERROR = ['command', 'contract', 'error', 'status', 'v'];
const keys = (value) => Object.keys(value).sort();
async function call(kernel, id, request, host) {
	const text = await kernel.invoke(id, typeof request === 'string' ? request : JSON.stringify(request), host);
	assert.equal(typeof text, 'string', `${id} answers envelope text`);
	return JSON.parse(text);
}
function ok(envelope, id) {
	assert.deepEqual(keys(envelope), SUCCESS, `${id}: success envelope members`);
	assert.equal(envelope.v, 1); assert.equal(envelope.status, 'ok'); assert.equal(envelope.command, id);
	return envelope;
}
function refused(envelope, code) {
	assert.deepEqual(keys(envelope), ERROR, `${code}: error envelope members`);
	assert.equal(envelope.v, 1); assert.equal(envelope.status, 'error');
	assert.equal(envelope.error.code, code, JSON.stringify(envelope));
	for (const field of ['class', 'code', 'message']) assert.equal(typeof envelope.error[field], 'string');
	assert.equal(typeof envelope.error.retryable, 'boolean');
	return envelope;
}
/** The installed CLI's envelope for the same command, when `ds` is here. */
function cli(args) {
	if (!ds) return null;
	try { return JSON.parse(execFileSync(ds, [...args, '--output', 'json'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })); }
	catch (error) { return JSON.parse(error.stdout); }
}
/** Same frame and refusal as the CLI: members, version, status, class, code, retryable. */
function sameAsCli(ours, theirs, label) {
	if (!theirs) return 'skipped (no ds)';
	assert.deepEqual(keys(ours), keys(theirs), `${label}: envelope members`);
	for (const field of ['v', 'status', 'command']) assert.equal(ours[field], theirs[field], `${label}: ${field}`);
	for (const field of ['class', 'code', 'retryable']) assert.equal(ours.error[field], theirs.error[field], `${label}: error.${field}`);
	return 'matches';
}

const report = [];
const row = (kind, id, envelope, extra = {}) => report.push({ kind, id, status: envelope.status, code: envelope.error?.code, ...extra });

// Kernel module row, native owner: the same function `styleCatalog` calls.
const snapshot = { project_id: 'p', styles: { 'master/poles': { type: 'line', paint: { 'line-width': 3 } } },
	style_editors: [{ style_ref: 'master/poles', style_target: 'design', layer_name: 'poles', available_fields: ['status'],
		field_domains: { status: { type: 'boolean' } }, label_model: { type: 'symbol', paint: {}, layout: { 'text-field': ['get', 'status'] } } }],
	label_editor: { field_part_types: ['literal', 'field'] }, style_properties: {} };
const listRequest = { operation: 'list', snapshot, limit: 10 };
const listed = ok(await call(web, 'style.list', listRequest), 'style.list');
assert.deepEqual(listed.data, JSON.parse(web.styleCatalog(new TextEncoder().encode(JSON.stringify(listRequest)))), 'invoke answers what the typed export answers');
row('kernel:style_catalog (native owner)', 'style.list', listed, { data_equals_typed_export: true });

// Kernel module row through the gateway executor: the request admitted with
// the CLI's refusals, then the real `assetsTree` owner with an offline page.
const missing = refused(await call(web, 'assets.tree', {}), 'missing_input');
row('kernel:assets (members)', 'assets.tree', missing, { cli: sameAsCli(missing, cli(['assets', 'tree']), 'missing_input') });
const unknownFlag = refused(await call(web, 'assets.tree', { project: 'p1', bogus: 1 }), 'unknown_flag');
row('kernel:assets (members)', 'assets.tree', unknownFlag, { cli: sameAsCli(unknownFlag, cli(['assets', 'tree', '--project', 'p1', '--bogus', '1']), 'unknown_flag') });
const facts = { now_ms: Date.now(), has_session: false, offline_mode: true, link_up: false, principal: '' };
const told = [];
const gateway = { facts: () => facts, refused: (refusal) => told.push(refusal) };
const offline = await call(web, 'assets.tree', { project: 'p1' }, { gateway });
assert.deepEqual(keys(offline), ERROR);
assert.equal(offline.command, 'assets.tree');
row('kernel:assets (gateway owner)', 'assets.tree', offline, { class: offline.error.class });
const noDoor = refused(await call(web, 'assets.tree', { project: 'p1' }), 'capability_door_unavailable');
row('kernel:assets (no gateway door)', 'assets.tree', noDoor);
const mismatch = refused(await call(web, 'pm.task.create', { action: 'task_read', ds_project: 'p1', task_id: 't1' }, { gateway }), 'capability_request_mismatch');
row('kernel:project_work (shared owner)', 'pm.task.create', mismatch);

// A door-less owner answering its record as JSON text: the record, not a string.
const store = await call(web, 'store.status', {});
assert.ok(store.status === 'ok' ? typeof store.data === 'object' : keys(store).join() === ERROR.join(), JSON.stringify(store));
row('kernel:store_status (door-less owner)', 'store.status', store, store.status === 'ok' ? { schema: store.data.schema } : { class: store.error.class });

// Network unit row: the page's unit door.
const asked = [];
const unitHost = { unit: async (capability, input) => { asked.push({ capability, input }); return { session: 's1', echo: input }; } };
const unit = ok(await call(web, 'grid.session', { open: { model: 'm' } }, unitHost), 'grid.session');
assert.deepEqual(asked, [{ capability: 'grid.session', input: { open: { model: 'm' } } }]);
row('network_unit', 'grid.session', unit);

// Core row through the Desktop's Core door, with the row's run spec.
const coreCalls = [];
const fakeCore = { core: async (command, project, args) => { coreCalls.push({ command, project, args });
	return { v: 1, command, contract: 3, status: 'ok', data: { catalog: [] }, later: { added: true } }; } };
const settings = await call(desktop, 'design.process.settings', { project: 'p1' }, fakeCore);
assert.deepEqual(settings, { v: 1, command: 'design.process.settings', contract: 3, status: 'ok', data: { catalog: [] }, later: { added: true } }, 'Core envelope verbatim, additive member kept');
assert.deepEqual(coreCalls, [{ command: 'design.process.settings', project: 'p1', args: { catalog: true } }], 'run switches added, project on the door');
row('core (fake Core door, run spec)', 'design.process.settings', settings, { additive_member_kept: true });
// The Core door is the installed CLI itself: `ds <path> [--project p] [--k v]`.
const realCore = ds && { core: async (command, project, args) => {
	const argv = [...command.split('.'), ...(project ? ['--project', project] : [])];
	for (const [name, value] of Object.entries(args)) argv.push(`--${name}`, ...(value === true ? [] : [String(value)]));
	return cli(argv);
} };
if (realCore) {
	const viaCore = await call(desktop, 'assets.tree', {}, realCore);
	assert.deepEqual(viaCore, cli(['assets', 'tree']), 'the Desktop answers Core\'s own refusal');
	row('core (installed ds as the Core door)', 'assets.tree', viaCore, { cli: 'identical' });
	const flagViaCore = await call(desktop, 'assets.tree', { project: 'p1', bogus: 1 }, realCore);
	assert.equal(flagViaCore.error.code, 'unknown_flag');
	row('core (installed ds as the Core door)', 'assets.tree', flagViaCore, { cli: 'identical' });
}

// Pending, discarded, unbound, unknown and malformed: refused before any door.
const doorless = { core: () => assert.fail('no door opens'), unit: () => assert.fail('no door opens'), gateway };
const pending = refused(await call(web, 'design.autoprocess.plan', {}, doorless), 'capability_pending');
assert.equal(pending.error.detail.reason, 'engineering_on_core');
row('pending', 'design.autoprocess.plan', pending, { reason: pending.error.detail.reason });
const discarded = refused(await call(web, 'account.connect', {}, doorless), 'capability_discarded');
row('discarded', 'account.connect', discarded, { reason: discarded.error.detail.reason });
const desktopDiscarded = refused(await call(desktop, 'account.connect', {}, doorless), 'capability_discarded');
row('discarded (desktop)', 'account.connect', desktopDiscarded, { reason: desktopDiscarded.error.detail.reason });
const unbound = refused(await call(web, 'messaging.send', {}, doorless), 'capability_invoke_unbound');
row('kernel (unbound owner)', 'messaging.send', unbound, { export: unbound.error.detail.export });
const unknown = refused(await call(web, 'pm.task.launch', {}, doorless), 'capability_unknown');
assert.equal(unknown.error.class, 'invalid_input');
row('unknown id', 'pm.task.launch', unknown);
for (const request of ['not json', '[1]']) {
	const malformed = refused(await call(web, 'assets.tree', request, doorless), 'invalid_request');
	assert.equal(malformed.error.class, 'invalid_input');
	row('malformed request', 'assets.tree', malformed, { request });
}
const noHost = refused(await call(web, 'pm.task.launch', '{}'), 'capability_unknown');
assert.equal(noHost.command, 'pm.task.launch');

assert.equal(outgoingAttempts, 0);
console.log(JSON.stringify({ web_platform: web.capabilityRoute('assets.tree').platform, desktop_platform: desktop.capabilityRoute('assets.tree').platform,
	ds: ds ? execFileSync(ds, ['--version'], { encoding: 'utf8' }).trim() : null, calls: report.length, outgoingAttempts }));
for (const line of report) console.log(JSON.stringify(line));
