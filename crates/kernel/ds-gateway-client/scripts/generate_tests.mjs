import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import test from 'node:test';
import { artifacts, main, operations, rust } from './generate.mjs';

const fixture = () => ({
  openapi: '3.0.3',
  info: { title: 'Test gateway', version: '1' },
  paths: {
    '/projects/{project}': {
      get: {
        operationId: 'readProject',
        parameters: [{ name: 'project', in: 'path', required: true, schema: { type: 'string' } }],
        responses: { 200: { description: 'Project' } },
      },
    },
  },
});

test('duplicate operation identity refuses rather than changing generated dispatch', () => {
  const spec = fixture();
  spec.paths['/other'] = { get: { operationId: 'readProject' } };
  assert.throws(() => operations(spec), /Duplicate operationId/);
});

test('missing or mismatched path authority refuses generation', () => {
  const spec = fixture();
  spec.paths['/projects/{project}'].get.parameters[0].name = 'other_project';
  assert.throws(() => operations(spec), /Path parameter declaration mismatch/);
  spec.paths['/projects/{project}'].get.parameters[0].name = 'project';
  spec.paths['/projects/{project}'].get.parameters[0].required = false;
  assert.throws(() => operations(spec), /Path parameter declaration mismatch/);
});

test('external references cannot make generation depend on an outside schema', () => {
  const spec = fixture();
  spec.paths['/projects/{project}'].get.responses[200].$ref = 'https://example.invalid/schema';
  assert.throws(() => operations(spec), /External reference refused/);
});

test('a response without schema remains explicitly unspecified', () => {
  const output = rust(fixture(), 'fixture-source-digest');
  assert.match(output, /has_response_schema: false/);
  assert.doesNotMatch(output, /struct ReadProjectResponse/);
});

test('conversion preserves security and gateway extension without fabricating a schema', async () => {
  const swagger = {
    swagger: '2.0', info: { title: 'Test', version: '1' },
    securityDefinitions: { user: { type: 'apiKey', in: 'header', name: 'Authorization' } },
    security: [{ user: [] }],
    paths: { '/projects': { get: {
      operationId: 'listProjects', 'x-google-backend': { address: 'https://example.invalid' },
      responses: { 200: { description: 'Projects' } },
    } } },
  };
  const outputs = await artifacts(Buffer.from(JSON.stringify(swagger)));
  const openapi = JSON.parse(outputs.get('spec/gateway-openapi.json'));
  assert.equal(openapi.openapi, '3.0.3');
  assert.deepEqual(openapi.security, [{ user: [] }]);
  assert.equal(openapi.components.securitySchemes.user.name, 'Authorization');
  assert.equal(openapi.paths['/projects'].get['x-google-backend'].address, 'https://example.invalid');
  assert.equal(openapi.paths['/projects'].get.responses[200].content, undefined);
});

test('copied source regenerates byte-identically and covers every declared route', async () => {
  const source = await fs.readFile(new URL('../spec/gateway-swagger.yaml', import.meta.url));
  const before = Buffer.from(source);
  const one = await artifacts(source);
  const two = await artifacts(source);
  assert.deepEqual(one, two);
  assert.deepEqual(source, before);
  const provenance = JSON.parse(one.get('spec/provenance.json'));
  assert.equal(provenance.paths, 114);
  assert.equal(provenance.operations, 126);
  assert.equal(provenance.cors_operations, 112);
  for (const [name, contents] of one) {
    assert.equal(await fs.readFile(new URL(`../${name}`, import.meta.url), 'utf8'), contents, name);
  }
});

test('unknown generation options and check-plus-copy refuse before reading/writing', async () => {
  await assert.rejects(main(['--mystery']), /Usage/);
  await assert.rejects(main(['--check', '--copy-from', 'unused']), /Usage/);
});
