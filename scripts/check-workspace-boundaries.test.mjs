import assert from 'node:assert/strict';
import path from 'node:path';
import test from 'node:test';
import { cargoMetadataArguments, checkWorkspaceBoundaries, collectWorkspaceMetadata } from './check-workspace-boundaries.mjs';

const workspaceRoot = path.resolve('/workspace/ds-cli');
const webRoot = path.resolve('/workspace/ds-web');

function pkg(name, group, dependencies = []) {
  const directory = group === 'web'
    ? path.join(webRoot, 'src-tauri', name)
    : group === 'external'
      ? path.resolve('/workspace/ds-network/crates', name)
      : path.join(workspaceRoot, 'crates', group, name);
  return {
    id: `path+file://${directory}#${name}@0.1.0`,
    name,
    source: null,
    manifest_path: path.join(directory, 'Cargo.toml'),
    dependencies,
  };
}

function declaration(target, extra = {}) {
  return { name: target.name, path: path.dirname(target.manifest_path), optional: false, kind: null, ...extra };
}

function metadata(packages, resolved = []) {
  return {
    workspace_root: workspaceRoot,
    workspace_members: packages.map((entry) => entry.id),
    packages,
    resolve: {
      nodes: packages.map((entry) => ({
        id: entry.id,
        deps: resolved.filter(([from]) => from.id === entry.id).map(([, to]) => ({ name: to.name, pkg: to.id, dep_kinds: [{ kind: null, target: null }] })),
      })),
    },
  };
}

function check(input) {
  return checkWorkspaceBoundaries(input, { workspaceRoot, webRoot });
}

test('allows CLI to Kernel, independent Interactions, and UI consumers of both', () => {
  const shared = pkg('ds-geo-lite', 'shared');
  const kernel = pkg('ds-client-core', 'kernel', [declaration(shared)]);
  const cli = pkg('ds', 'cli', [declaration(kernel)]);
  const interactions = pkg('ds-interactions-wasm', 'interactions', [declaration(shared)]);
  const web = pkg('ds-web-desktop', 'web', [declaration(kernel), declaration(interactions)]);
  const result = check([
    metadata([shared, kernel, cli, interactions]),
    metadata([shared, kernel, interactions, web]),
  ]);
  assert.deepEqual(result, { ok: true, violations: [] });
});

const forbiddenPairs = [
  ['cli', 'interactions'],
  ['kernel', 'interactions'],
  ['interactions', 'kernel'],
  ['kernel', 'cli'],
  ['interactions', 'cli'],
  ['web', 'cli'],
  ['cli', 'web'],
  ['kernel', 'web'],
  ['interactions', 'web'],
  ['shared', 'web'],
];

for (const [from, to] of forbiddenPairs) {
  test(`negative control: rejects resolved ${from} -> ${to}`, () => {
    const origin = pkg('origin', from);
    const target = pkg('target', to);
    const result = check(metadata([origin, target], [[origin, target]]));
    assert.equal(result.ok, false);
    const failure = result.violations.find((entry) => entry.code === 'forbidden_dependency' && entry.from.group === from);
    assert.equal(failure.from.group, from);
    assert.equal(failure.to.group, to);
    assert.deepEqual(failure.path.map((entry) => entry.id), [origin.id, target.id]);
  });

  test(`rejects transitive ${from} -> shared -> external -> ${to}`, () => {
    const target = pkg('target', to);
    const external = pkg('external-bridge', 'external', [declaration(target)]);
    const shared = pkg('shared-bridge', 'shared', [declaration(external)]);
    const origin = pkg('origin', from, [declaration(shared)]);
    const result = check(metadata([origin, shared, external, target]));
    assert.equal(result.ok, false);
    const failure = result.violations.find((entry) => entry.code === 'forbidden_dependency' && entry.from.group === from);
    assert.equal(failure.from.group, from);
    assert.equal(failure.to.group, to);
    assert.deepEqual(failure.path.map((entry) => entry.name), ['origin', 'shared-bridge', 'external-bridge', 'target']);
  });
}

test('rejects optional renamed local dependency missing from activated resolve edges', () => {
  const target = pkg('ds-cli-exec', 'cli');
  const web = pkg('desktop', 'web', [declaration(target, { optional: true, rename: 'executor' })]);
  const result = check(metadata([web, target]));
  assert.equal(result.ok, false);
  assert.equal(result.violations[0].code, 'forbidden_dependency');
  assert.deepEqual(result.violations[0].path.map((entry) => entry.name), ['desktop', 'ds-cli-exec']);
});

test('rejects optional transitive dependency across two metadata documents', () => {
  const cli = pkg('ds-cli-exec', 'cli');
  const owner = pkg('ds-compute-runtime', 'kernel', [declaration(cli, { optional: true, target: 'cfg(windows)' })]);
  const web = pkg('desktop', 'web', [declaration(owner)]);
  const result = check([metadata([owner, cli]), metadata([web, owner])]);
  assert.equal(result.ok, false);
  assert.ok(result.violations.some((entry) => entry.from.group === 'web'
    && entry.to.group === 'cli' && entry.path.length === 3));
  assert.ok(result.violations.some((entry) => entry.from.group === 'kernel' && entry.to.group === 'cli'));
});

for (const kind of ['build', 'dev']) {
  test(`rejects forbidden ${kind} dependency without a resolve edge`, () => {
    const cli = pkg('adapter', 'cli');
    const kernel = pkg('owner', 'kernel', [declaration(cli, { kind })]);
    assert.equal(check(metadata([kernel, cli])).ok, false);
  });
}

test('package names do not determine ownership', () => {
  const kernel = pkg('ds-cli-looking-contract', 'kernel');
  const web = pkg('ds-cli-looking-desktop', 'web', [declaration(kernel)]);
  assert.equal(check(metadata([web, kernel])).ok, true);
  const cli = pkg('innocent-name', 'cli');
  web.dependencies = [declaration(cli)];
  assert.equal(check(metadata([web, cli])).ok, false);
});

test('a shared directory nested inside Kernel does not exempt the package', () => {
  const cli = pkg('adapter', 'cli');
  const owner = pkg('owner', 'kernel', [declaration(cli)]);
  owner.manifest_path = path.join(workspaceRoot, 'crates/kernel/owner/crates/shared/helper/Cargo.toml');
  const result = check(metadata([owner, cli]));
  assert.equal(result.ok, false);
  assert.equal(result.violations[0].from.group, 'kernel');
});

for (const group of ['kernel', 'cli', 'interactions']) {
  test(`neutral shared crates cannot depend on ${group}`, () => {
    const target = pkg('owner', group);
    const shared = pkg('geometry', 'shared', [declaration(target)]);
    const result = check(metadata([shared, target]));
    assert.equal(result.ok, false);
    assert.equal(result.violations[0].from.group, 'shared');
    assert.equal(result.violations[0].to.group, group);
  });
}

test('rejects an unclassified local package rather than treating it as shared', () => {
  const unknown = pkg('unmoved-owner', 'shared-ish');
  const result = check(metadata([unknown]));
  assert.equal(result.ok, false);
  assert.equal(result.violations[0].code, 'unclassified_workspace_package');
});

test('missing optional local metadata refuses instead of passing an incomplete graph', () => {
  const target = pkg('missing-adapter', 'cli');
  const owner = pkg('owner', 'kernel', [declaration(target, { optional: true })]);
  const result = check(metadata([owner]));
  assert.equal(result.ok, false);
  assert.equal(result.violations[0].code, 'unresolved_local_dependency');
});

test('rejects --no-deps metadata and dangling resolved package IDs', () => {
  const owner = pkg('owner', 'kernel');
  const noDeps = { ...metadata([owner]), resolve: null };
  assert.equal(check(noDeps).violations[0].code, 'metadata_invalid');
  const dangling = metadata([owner]);
  dangling.resolve.nodes[0].deps.push({ name: 'missing', pkg: 'missing' });
  assert.equal(check(dangling).violations[0].code, 'metadata_invalid');
});

test('rejects conflicting package identity across metadata documents', () => {
  const owner = pkg('owner', 'kernel');
  const changed = { ...owner, manifest_path: path.join(workspaceRoot, 'crates/cli/owner/Cargo.toml') };
  const result = check([metadata([owner]), metadata([changed])]);
  assert.equal(result.ok, false);
  assert.equal(result.violations[0].code, 'metadata_invalid');
});

test('cycles terminate while retaining the complete forbidden dependency path', () => {
  const cli = pkg('adapter', 'cli');
  const shared = pkg('shared', 'shared', [declaration(cli)]);
  const kernel = pkg('owner', 'kernel', [declaration(shared)]);
  shared.dependencies.push(declaration(kernel));
  const result = check(metadata([kernel, shared, cli]));
  assert.equal(result.ok, false);
  const ownerFailure = result.violations.find((entry) => entry.from.group === 'kernel');
  assert.deepEqual(ownerFailure.path.map((entry) => entry.name), ['owner', 'shared', 'adapter']);
});

test('Cargo metadata invocation stays locked, offline, and includes optional features', () => {
  assert.deepEqual(cargoMetadataArguments('/project/Cargo.toml'), [
    'metadata', '--format-version', '1', '--locked', '--offline', '--all-features', '--manifest-path', '/project/Cargo.toml',
  ]);
});

test('discovers external optional owners to a fixed point and skips known sibling manifests', () => {
  const runtime = pkg('solar-runtime', 'external');
  const portfolio = pkg('solar-portfolio', 'external');
  const solar = pkg('solar-contracts', 'external', [declaration(runtime, { optional: true }), declaration(portfolio, { optional: true })]);
  const owner = pkg('owner', 'kernel', [declaration(solar)]);
  const web = pkg('desktop', 'web', [declaration(owner)]);
  const rootManifest = path.join(workspaceRoot, 'Cargo.toml');
  const webManifest = path.join(webRoot, 'src-tauri/Cargo.toml');
  const documents = new Map([
    [rootManifest, metadata([owner, solar])],
    [webManifest, { ...metadata([web, owner, solar]), workspace_root: webRoot }],
    [runtime.manifest_path, { ...metadata([solar, runtime, portfolio]), workspace_root: path.resolve('/workspace/ds-network') }],
  ]);
  const calls = [];
  const complete = collectWorkspaceMetadata([rootManifest, webManifest], (manifest) => {
    calls.push(manifest);
    assert.ok(documents.has(manifest), `unexpected owner read: ${manifest}`);
    return documents.get(manifest);
  });
  assert.deepEqual(calls, [rootManifest, webManifest, runtime.manifest_path]);
  assert.equal(complete.length, 3);
  assert.equal(check(complete).ok, true);
});

test('a forbidden edge discovered through an external optional owner still fails', () => {
  const cli = pkg('adapter', 'cli');
  const runtime = pkg('runtime', 'external', [declaration(cli)]);
  const external = pkg('external-owner', 'external', [declaration(runtime, { optional: true })]);
  const owner = pkg('owner', 'kernel', [declaration(external)]);
  const rootManifest = path.join(workspaceRoot, 'Cargo.toml');
  const complete = collectWorkspaceMetadata([rootManifest], (manifest) => manifest === rootManifest
    ? metadata([owner, external, cli])
    : { ...metadata([external, runtime, cli]), workspace_root: path.resolve('/workspace/ds-network') });
  const result = check(complete);
  assert.equal(result.ok, false);
  assert.ok(result.violations.some((entry) => entry.code === 'forbidden_dependency'
    && entry.from.group === 'kernel' && entry.to.group === 'cli' && entry.path.length === 4));
});

test('an external Cargo refusal propagates without retry or a partial success', () => {
  const runtime = pkg('missing-runtime', 'external');
  const owner = pkg('owner', 'kernel', [declaration(runtime, { optional: true })]);
  const rootManifest = path.join(workspaceRoot, 'Cargo.toml');
  const calls = [];
  assert.throws(() => collectWorkspaceMetadata([rootManifest], (manifest) => {
    calls.push(manifest);
    if (manifest === rootManifest) return metadata([owner]);
    throw new Error('locked offline owner refused');
  }), /locked offline owner refused/);
  assert.deepEqual(calls, [rootManifest, runtime.manifest_path]);
});

test('deduplicates workspace graphs without accepting an unresolved local edge', () => {
  const missing = pkg('missing', 'external');
  const owner = pkg('owner', 'kernel', [declaration(missing, { optional: true })]);
  const rootManifest = path.join(workspaceRoot, 'Cargo.toml');
  const calls = [];
  const complete = collectWorkspaceMetadata([rootManifest, rootManifest], (manifest) => {
    calls.push(manifest);
    return metadata([owner]);
  });
  assert.deepEqual(calls, [rootManifest, missing.manifest_path]);
  assert.equal(complete.length, 1);
  assert.equal(check(complete).ok, false);
  assert.ok(check(complete).violations.every((entry) => entry.code === 'unresolved_local_dependency'));
});
