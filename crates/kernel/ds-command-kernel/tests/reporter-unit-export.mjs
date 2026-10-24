// One browser report export through the page Kernel's `reporter_unit`
// envelope, with the real Reporter and Network units, against a native
// Reporter run of the same request.
//
//   node tests/reporter-unit-export.mjs <Kernel web package> <Reporter nodejs package> <Network nodejs package> <native run>
//
// Build the Kernel with wasm-pack (--target web) through one page target of
// release/wasm-unit.json, and both units with `--target nodejs`. <native run>
// holds the request the native `reporter` task ran (request.json), its result
// (result.json) and its artifacts. Every absolute path the request names is
// held under that exact name. The host loop here is what a page does: it
// executes the effects the Kernel names and returns each reply to it; it
// reads no report value. The Kernel's files must be the native artifacts,
// byte for byte; the controls are an older Reporter unit, a missing Network
// answer and an older Network unit, each refused typed with no outgoing
// request.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';

let outgoingAttempts = 0;
globalThis.fetch = () => { outgoingAttempts++; throw Error('network forbidden'); };
globalThis.XMLHttpRequest = class { constructor() { outgoingAttempts++; throw Error('network forbidden'); } };
globalThis.WebSocket = class { constructor() { outgoingAttempts++; throw Error('network forbidden'); } };

const [kernelPkg, reporterPkg, networkPkg, run] = process.argv.slice(2).map((value) => path.resolve(value));
const require = createRequire(import.meta.url);
const kernel = await import(pathToFileURL(path.join(kernelPkg, 'ds_command_kernel.js')).href);
kernel.initSync({ module: fs.readFileSync(path.join(kernelPkg, 'ds_command_kernel_bg.wasm')) });
const reporter = require(path.join(reporterPkg, 'ds_reporter_wasm.js'));
const network = require(path.join(networkPkg, 'ds_network_wasm.js'));
const sha256 = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');

const requestBytes = fs.readFileSync(path.join(run, 'request.json'));
const request = JSON.parse(requestBytes);
const native = JSON.parse(fs.readFileSync(path.join(run, 'result.json'), 'utf8'));
const exportKind = Array.isArray(request.transformers) ? 'combined' : 'transformer';
const identity = reporter.reporter_engine_identity();

function namedInputs(value, names = new Set()) {
	if (typeof value === 'string' && value.startsWith('/') && fs.existsSync(value) && fs.statSync(value).isFile()) names.add(value);
	else if (Array.isArray(value)) value.forEach((item) => namedInputs(item, names));
	else if (value && typeof value === 'object') Object.values(value).forEach((item) => namedInputs(item, names));
	return names;
}
const names = [...namedInputs(request)].sort();

// The page's unit entries, by the entry the Kernel names.
const ENTRIES = { 'reporter.export.transformer': 'export_transformer_report_json', 'reporter.export.combined': 'export_combined_report_json' };
function callReporter(effect) {
	assert.equal(effect.unit, 'reporter');
	const entry = reporter[ENTRIES[effect.entry]];
	if (typeof entry !== 'function') throw kernel.reporterEngineUnavailable(`the selected Reporter unit has no ${effect.entry} entry`);
	try { return { reply: entry(effect.payload), refused: false }; }
	catch (value) { if (value instanceof Error) throw value; return { reply: String(value), refused: true }; }
}
const networkUnit = (ask) => { assert.equal(ask.unit, 'network'); assert.equal(ask.entry, 'grid.model'); return network.gridUnitInvoke(ask.payload); };

function open(identityText = identity) {
	const session = new kernel.ReporterExportSession(exportKind, requestBytes, identityText);
	for (const name of names) session.hold(name, fs.readFileSync(name));
	return session;
}
/** The page's loop: Reporter effect, Network asks, again, until completed. */
function drive(session, answer = networkUnit) {
	let rounds = 0, answers = 0;
	for (;;) {
		const { reply, refused } = callReporter(session.effect());
		const asks = session.reporterReply(reply, refused);
		if (!asks.length) return { rounds, answers };
		assert.ok(++rounds <= 8);
		for (const ask of asks) { session.networkReply(ask.request_digest, answer(ask)); answers++; }
	}
}
const code = (fn) => { try { fn(); } catch (error) { return String(error.message ?? error).split(':')[0]; } assert.fail('expected a typed refusal'); };

const session = open();
const { rounds, answers } = drive(session);
const summary = JSON.parse(session.summary());
const files = session.fileNames();
const expected = Object.fromEntries(native.artifacts.map((artifact) => [artifact.filename, artifact.sha256]));
assert.deepEqual([...files].sort(), Object.keys(expected).sort());
for (const name of files) assert.equal(sha256(session.file(name)), expected[name], `${name} differs from the native artifact`);
assert.equal(summary.schema, 'ds.reporter-browser-export/v1');
assert.equal(summary.status, native.status);
assert.deepEqual(summary.blockers, native.blockers);
console.log(JSON.stringify({ export: exportKind, transformer: native.transformer ?? native.prefix, status: summary.status, rounds, network_answers: answers,
	files: files.map((name) => ({ name, sha256: sha256(session.file(name)) })), outgoingAttempts }));
session.free();

// Controls.
const older = JSON.stringify({ ...JSON.parse(identity), capabilities: ['render_held_svg_pdf_v1'] });
assert.equal(code(() => open(older)), 'reporter_engine_update_required');
// A Network answer the page does not return keeps the export from running.
const missing = open();
const first = callReporter(missing.effect());
const asks = missing.reporterReply(first.reply, first.refused);
assert.ok(asks.length, 'the export names Network facts');
assert.equal(code(() => missing.effect()), 'reporter_request_invalid');
missing.free();
// An older Network unit answers the closed-set refusal for an operation it
// does not carry (what `ds_network_contracts::grid_unit::answer` encodes).
function olderNetwork() {
	const header = Buffer.from(JSON.stringify({ protocol: 'ds.network.grid-unit/v1', operation: { kind: 'model', method: 'reportEngineering' }, status: 'refused',
		refusal: { code: 'method_unavailable', message: 'Operation unavailable in the selected Network unit: reportEngineering. Update the Network unit to enable it.' } }));
	const out = Buffer.alloc(12 + header.length);
	out.write('DSG1', 0, 'latin1'); out.writeUInt32LE(header.length, 4); header.copy(out, 8); out.writeUInt32LE(0, 8 + header.length);
	return new Uint8Array(out);
}
const outdated = open();
assert.equal(code(() => drive(outdated, olderNetwork)), 'network_engine_update_required');
outdated.free();
// A tampered Network answer is not the artifact its receipt names.
const tampered = open();
assert.equal(code(() => drive(tampered, (ask) => { const reply = networkUnit(ask); reply[reply.length - 2] ^= 1; return reply; })), 'network_response_invalid');
tampered.free();
assert.equal(outgoingAttempts, 0);
console.log(JSON.stringify({ refusals: 4, outgoingAttempts }));
