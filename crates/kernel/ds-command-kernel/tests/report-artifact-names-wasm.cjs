// Run after wasm-pack build --target nodejs --features wasm --out-dir <package>:
// node tests/report-artifact-names-wasm.cjs <package>/ds_command_kernel.js
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const kernel = require(path.resolve(process.argv[2]));
const cases = JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures/report-artifact-names.json')));
const command = request => JSON.parse(kernel.reportCommand(new TextEncoder().encode(JSON.stringify(request)))).result;
let checked = 0;
for (const [input, expected] of cases) {
  // A locator with no basename retains the existing artifact_name "file"
  // fallback; the shared filename corpus also tests the lower-level "report".
  if (!input || input.endsWith('/')) continue;
  const locator = `https://host/o/${encodeURIComponent(`reports/${input}`)}?signature=fixture`;
  assert.equal(command({ command: 'artifact_name', locator }).filename, expected, input);
  checked++;
}
const prefix = '280926-0916-report-gisagara-cjic-021d4e7aaba7bec355f7ceec5aececbc';
assert.deepEqual(command({ command: 'combined_entry_naming', prefix }), {
  label: '280926-0916-report-gisagara-cjic',
  folder: '280926-0916-report-gisagara-cjic',
  archive_name: '280926-0916-report-gisagara-cjic.zip',
});
console.log(`${checked} artifact names and the combined archive name passed through the compiled WASM binding`);
