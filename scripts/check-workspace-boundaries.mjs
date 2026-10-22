#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const forbidden = new Map([
  ['cli', new Set(['interactions', 'web'])],
  ['kernel', new Set(['cli', 'interactions', 'web'])],
  ['interactions', new Set(['cli', 'kernel', 'web'])],
  ['shared', new Set(['kernel', 'cli', 'interactions', 'web'])],
  ['web', new Set(['cli'])],
]);

function within(directory, candidate) {
  const relative = path.relative(directory, candidate);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}

function groupFor(manifestPath, workspaceRoot, webRoot) {
  const directory = path.dirname(manifestPath);
  for (const group of ['kernel', 'cli', 'interactions', 'shared']) {
    if (within(path.join(workspaceRoot, 'crates', group), directory)) return group;
  }
  if (webRoot && within(webRoot, directory)) return 'web';
  return within(workspaceRoot, directory) ? 'unclassified' : 'external';
}

function packageSummary(pkg) {
  return { id: pkg.id, name: pkg.name, group: pkg.group, manifestPath: pkg.manifestPath };
}

/**
 * Check Cargo metadata without reading files or running Cargo. Inputs may include
 * both the consolidated workspace and ds-web's metadata; package IDs join their
 * graphs. Paths must be physical, absolute paths (the CLI canonicalizes them).
 * Shared and external packages remain in the graph, never traversal exemptions.
 */
export function checkWorkspaceBoundaries(metadataInputs, options = {}) {
  const inputs = Array.isArray(metadataInputs) ? metadataInputs : [metadataInputs];
  const violations = [];
  const workspaceRoot = options.workspaceRoot ?? inputs[0]?.workspace_root;
  const webRoot = options.webRoot ?? (workspaceRoot && path.join(path.dirname(workspaceRoot), 'ds-web'));
  const invalid = (message) => violations.push({ code: 'metadata_invalid', message });
  if (typeof workspaceRoot !== 'string' || !path.isAbsolute(workspaceRoot)) {
    invalid('An absolute consolidated workspace root is required.');
    return { ok: false, violations };
  }
  if (webRoot != null && (typeof webRoot !== 'string' || !path.isAbsolute(webRoot))) {
    invalid('The ds-web root must be an absolute path.');
    return { ok: false, violations };
  }
  if (inputs.length === 0) invalid('At least one complete Cargo metadata document is required.');

  const packages = new Map();
  const declarations = [];
  const nodes = [];
  for (const [index, metadata] of inputs.entries()) {
    if (!Array.isArray(metadata?.packages) || !Array.isArray(metadata?.resolve?.nodes)) {
      invalid(`Metadata document ${index + 1} must include packages and resolve.nodes; --no-deps is not accepted.`);
      continue;
    }
    for (const pkg of metadata.packages) {
      if (typeof pkg?.id !== 'string' || typeof pkg.name !== 'string'
        || typeof pkg.manifest_path !== 'string' || !path.isAbsolute(pkg.manifest_path)
        || !Array.isArray(pkg.dependencies)) {
        invalid(`Metadata document ${index + 1} contains an incomplete package.`);
        continue;
      }
      const manifestPath = path.normalize(pkg.manifest_path);
      const previous = packages.get(pkg.id);
      if (previous && (previous.manifestPath !== manifestPath || previous.name !== pkg.name)) {
        invalid(`Package ID ${pkg.id} has conflicting manifest paths or names.`);
        continue;
      }
      packages.set(pkg.id, {
        id: pkg.id,
        name: pkg.name,
        manifestPath,
        group: groupFor(manifestPath, workspaceRoot, webRoot),
      });
      declarations.push({ id: pkg.id, dependencies: pkg.dependencies });
    }
    nodes.push(...metadata.resolve.nodes);
  }

  const byManifest = new Map();
  const edges = new Map();
  for (const pkg of packages.values()) {
    const matches = byManifest.get(pkg.manifestPath) ?? [];
    matches.push(pkg.id);
    byManifest.set(pkg.manifestPath, matches);
    edges.set(pkg.id, new Set());
    if (pkg.group === 'unclassified') {
      violations.push({
        code: 'unclassified_workspace_package',
        message: `${pkg.name} must live under crates/kernel, crates/cli, crates/interactions, or crates/shared.`,
        package: packageSummary(pkg),
      });
    }
  }
  const addEdge = (from, to) => {
    if (!packages.has(from) || !packages.has(to)) {
      invalid(`Resolved dependency ${from} -> ${to} refers to a package absent from metadata.`);
      return;
    }
    edges.get(from).add(to);
  };
  for (const node of nodes) {
    if (typeof node?.id !== 'string' || !packages.has(node.id)) {
      invalid('A resolve node has no matching package.');
      continue;
    }
    if (Array.isArray(node.deps)) {
      for (const dep of node.deps) addEdge(node.id, dep.pkg);
    } else if (Array.isArray(node.dependencies)) {
      for (const id of node.dependencies) addEdge(node.id, id);
    } else {
      invalid(`Resolve node ${node.id} has no dependency list.`);
    }
  }
  // Resolve nodes reflect activated features. Read declarations too so an
  // optional, renamed, dev, build, or target-specific local edge cannot hide.
  for (const { id, dependencies } of declarations) {
    for (const dep of dependencies) {
      if (dep.path == null) continue;
      if (typeof dep.path !== 'string' || !path.isAbsolute(dep.path)) {
        invalid(`Local dependency of ${id} has no absolute path.`);
        continue;
      }
      const manifestPath = path.join(dep.path, 'Cargo.toml');
      const targets = byManifest.get(manifestPath);
      if (!targets) {
        violations.push({
          code: 'unresolved_local_dependency',
          message: `${packages.get(id).name} declares local dependency ${dep.name ?? '(unnamed)'} absent from metadata; use --all-features.`,
          from: packageSummary(packages.get(id)),
          manifestPath,
        });
        continue;
      }
      for (const target of targets) addEdge(id, target);
    }
  }

  const ordered = [...packages.values()].sort((a, b) => a.manifestPath.localeCompare(b.manifestPath) || a.id.localeCompare(b.id));
  for (const origin of ordered) {
    const targets = forbidden.get(origin.group);
    if (!targets) continue;
    const previous = new Map([[origin.id, null]]);
    const pending = [origin.id];
    for (let cursor = 0; cursor < pending.length; cursor += 1) {
      const current = pending[cursor];
      for (const next of [...edges.get(current)].sort()) {
        if (previous.has(next)) continue;
        previous.set(next, current);
        pending.push(next);
        const destination = packages.get(next);
        if (!targets.has(destination.group)) continue;
        const chain = [];
        for (let step = next; step !== null; step = previous.get(step)) {
          chain.push(packageSummary(packages.get(step)));
        }
        chain.reverse();
        violations.push({
          code: 'forbidden_dependency',
          message: `${origin.group} -> ${destination.group} is forbidden: ${chain.map((pkg) => pkg.name).join(' -> ')}`,
          from: packageSummary(origin),
          to: packageSummary(destination),
          path: chain,
        });
      }
    }
  }
  return { ok: violations.length === 0, violations };
}

export function cargoMetadataArguments(manifestPath) {
  return ['metadata', '--format-version', '1', '--locked', '--offline', '--all-features', '--manifest-path', manifestPath];
}

/** Load missing optional local owners, retaining one metadata graph per workspace. */
export function collectWorkspaceMetadata(manifestPaths, loadMetadata) {
  const pending = [...new Set(manifestPaths)].map((manifestPath) => ({ manifestPath, initial: true }));
  const queued = new Set(manifestPaths);
  const attempted = new Set();
  const workspaces = new Map();
  const known = new Set();
  for (let cursor = 0; cursor < pending.length; cursor += 1) {
    const { manifestPath, initial } = pending[cursor];
    if (attempted.has(manifestPath) || (!initial && known.has(manifestPath))) continue;
    attempted.add(manifestPath);
    // A failed owner read propagates. Never retry, omit the edge, or switch to
    // a partial graph when --locked/--offline admission refuses.
    const metadata = loadMetadata(manifestPath);
    if (typeof metadata?.workspace_root !== 'string' || !path.isAbsolute(metadata.workspace_root)
      || !Array.isArray(metadata.packages) || !Array.isArray(metadata.resolve?.nodes)) {
      throw new Error(`Incomplete Cargo metadata from ${manifestPath}`);
    }
    const previous = workspaces.get(metadata.workspace_root);
    if (previous) {
      // Preserve all declarations and resolved edges if two feature selections
      // describe the same workspace; the checker joins them by package ID.
      previous.packages = [...previous.packages, ...metadata.packages];
      previous.resolve = { ...previous.resolve, nodes: [...previous.resolve.nodes, ...metadata.resolve.nodes] };
    } else {
      workspaces.set(metadata.workspace_root, { ...metadata, packages: [...metadata.packages] });
    }
    for (const document of workspaces.values()) {
      for (const pkg of document.packages) {
        if (typeof pkg.manifest_path === 'string') known.add(path.normalize(pkg.manifest_path));
      }
    }
    for (const document of workspaces.values()) {
      for (const pkg of document.packages) {
        for (const dep of pkg.dependencies ?? []) {
          if (typeof dep.path !== 'string' || !path.isAbsolute(dep.path)) continue;
          const ownerManifest = path.join(dep.path, 'Cargo.toml');
          if (known.has(ownerManifest) || queued.has(ownerManifest)) continue;
          queued.add(ownerManifest);
          pending.push({ manifestPath: ownerManifest, initial: false });
        }
      }
    }
  }
  return [...workspaces.values()];
}

function readMetadata(manifestPath) {
  const result = spawnSync('cargo', cargoMetadataArguments(manifestPath), {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) throw new Error(`Could not run cargo metadata: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`cargo metadata failed for ${manifestPath}:\n${result.stderr.trim()}`);
  const metadata = JSON.parse(result.stdout);
  metadata.workspace_root = realpathSync(metadata.workspace_root);
  for (const pkg of metadata.packages) {
    pkg.manifest_path = realpathSync(pkg.manifest_path);
    for (const dep of pkg.dependencies) {
      if (dep.path != null) dep.path = realpathSync(dep.path);
    }
  }
  return metadata;
}

function main(args) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const options = {
    '--workspace-manifest': path.join(root, 'Cargo.toml'),
    '--web-manifest': path.resolve(root, '../ds-web/src-tauri/Cargo.toml'),
    '--web-root': path.resolve(root, '../ds-web'),
  };
  let componentOnly = false;
  let explicitWeb = false;
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === '--component-only') {
      if (componentOnly) throw new Error('Duplicate --component-only');
      componentOnly = true;
      continue;
    }
    if (arg === '--help') {
      console.log('Usage: node scripts/check-workspace-boundaries.mjs [--component-only] [--workspace-manifest Cargo.toml] [--web-manifest ds-web/src-tauri/Cargo.toml] [--web-root ds-web]\nDefault: complete workspace and Web. --component-only checks the consolidated component graph without requiring an unrelated Web checkout; it does not qualify the application.');
      return 0;
    }
    if (!Object.hasOwn(options, arg) || !args[index + 1] || args[index + 1].startsWith('--')) {
      throw new Error(`Unknown or incomplete argument: ${arg}`);
    }
    options[arg] = path.resolve(args[++index]);
    if (arg.startsWith('--web-')) explicitWeb = true;
  }
  if (componentOnly && explicitWeb) throw new Error('--component-only cannot ignore an explicit Web input');
  const metadata = collectWorkspaceMetadata([
    options['--workspace-manifest'], ...(componentOnly ? [] : [options['--web-manifest']]),
  ], readMetadata);
  const result = checkWorkspaceBoundaries(metadata, {
    workspaceRoot: metadata[0].workspace_root,
    webRoot: componentOnly ? options['--web-root'] : realpathSync(options['--web-root']),
  });
  if (!result.ok) {
    for (const violation of result.violations) console.error(`${violation.code}: ${violation.message}`);
    return 1;
  }
  console.log(componentOnly
    ? 'Component dependency boundaries passed (including optional local owners). Full application qualification is still required.'
    : 'Workspace dependency boundaries passed (including optional local dependencies and ds-web).');
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    console.error(`workspace_boundaries_failed: ${error.message}`);
    process.exitCode = 1;
  }
}
