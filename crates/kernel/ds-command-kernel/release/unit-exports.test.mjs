// node --test release/unit-exports.test.mjs
// DS_KERNEL_UNIT_DTS=<built ds_command_kernel.d.ts> also checks a built unit.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { exportsOf, missingExports, readPublished } from './unit-exports.mjs';

const sample = `export class SessionHandle {
    private constructor();
    free(): void;
    static openSession(features_json: string): SessionHandle;
    getRows(layer_name: string, offset: number, limit: number): Uint8Array;
}
export function decodeTableFilterResult(chunks: Uint8Array[], result: TableFilterResult): TableFilterResult;
export interface InitOutput {
    readonly decodeLater: (a: number) => void;
}
export function notAnExport(): void;
`;

test('functions, classes and class members are the export set; init internals are not', () => {
	assert.deepEqual([...exportsOf(sample)].sort(), [
		'SessionHandle', 'SessionHandle.free', 'SessionHandle.getRows', 'SessionHandle.openSession', 'decodeTableFilterResult',
	]);
});

test('the published list names the row-window exchange the page calls', () => {
	const published = readPublished();
	assert.ok(published.releases.length >= 1);
	for (const name of ['SessionHandle.getRows', 'decodeTableFilterResult', 'decodeTableProfilesResult', 'gridEngineRequest']) {
		assert.ok(published.releases[0].exports.includes(name), name);
	}
});

test('a planted removal is named; a superset passes', () => {
	const published = { releases: [{ version: 'r', exports: [...exportsOf(sample)] }] };
	assert.deepEqual(missingExports(published, `${sample}export function newExport(): void;\n`), []);
	assert.deepEqual(missingExports(published, sample.replace(/^ {4}getRows.*\n/m, '')),
		[{ version: 'r', missing: ['SessionHandle.getRows'] }]);
});

test('a built Kernel UI unit keeps every published export', { skip: !process.env.DS_KERNEL_UNIT_DTS && 'set DS_KERNEL_UNIT_DTS to a built ds_command_kernel.d.ts' }, () => {
	const built = readFileSync(process.env.DS_KERNEL_UNIT_DTS, 'utf8');
	assert.deepEqual(missingExports(readPublished(), built), []);
	// Control: the same built unit without one retired export fails.
	assert.deepEqual(missingExports(readPublished(), built.replace(/^export function decodeTableProfilesResult\b.*\n/m, '')),
		[{ version: readPublished().releases[0].version, missing: ['decodeTableProfilesResult'] }]);
});
