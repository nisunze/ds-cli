#!/usr/bin/env node
// Boot compatibility of the Kernel UI unit across shell releases.
//
// A Desktop or Web page checks, at start, that the selected Kernel UI unit
// exports every function, class and class method it calls, and refuses to boot
// otherwise. A new unit therefore keeps every export of each published unit
// (an exchange it no longer serves stays as a typed refusal), so a page built
// against an older unit still boots. Compatibility is the typed export set,
// never a version, build date or digest.
//
//   node release/unit-exports.mjs <built ds_command_kernel.d.ts> [published-exports.json]
//
// exits 1 and names each published export the built unit lacks.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

/** Top-level functions and classes, and `Class.method` for class members, of a wasm-bindgen declaration. */
export function exportsOf(dts) {
	const names = new Set();
	const declarations = dts.split(/^export interface InitOutput\b/m)[0];
	let owner = null;
	for (const line of declarations.split('\n')) {
		const fn = /^export function (\w+)/.exec(line);
		if (fn) names.add(fn[1]);
		const cls = /^export class (\w+)/.exec(line);
		if (cls) { owner = cls[1]; names.add(owner); continue; }
		if (owner && /^}/.test(line)) { owner = null; continue; }
		const member = owner && /^ {4}(?:static )?(?:get |set )?(\w+)\s*\(/.exec(line);
		if (member && member[1] !== 'constructor') names.add(`${owner}.${member[1]}`);
	}
	return names;
}

/** Every published export the built declaration lacks, per published release. */
export function missingExports(published, builtDts) {
	const built = exportsOf(builtDts);
	return published.releases.map((release) => ({
		version: release.version,
		missing: release.exports.filter((name) => !built.has(name)),
	})).filter((release) => release.missing.length);
}

export function readPublished(path = fileURLToPath(new URL('published-exports.json', import.meta.url))) {
	const published = JSON.parse(readFileSync(path, 'utf8'));
	if (published.schema !== 'ds.ui-unit-published-exports/v1' || published.component !== 'command-kernel') {
		throw new Error(`${path} is not the command-kernel published export list`);
	}
	return published;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
	const [built, list] = process.argv.slice(2);
	if (!built) { console.error('usage: node release/unit-exports.mjs <built ds_command_kernel.d.ts> [published-exports.json]'); process.exit(2); }
	const missing = missingExports(readPublished(list), readFileSync(built, 'utf8'));
	for (const { version, missing: names } of missing) {
		console.error(`ui_unit_exports_removed: published ${version} exports ${names.join(', ')}; keep each as a typed refusal so older pages boot`);
	}
	process.exit(missing.length ? 1 : 0);
}
