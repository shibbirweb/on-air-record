#!/usr/bin/env node
/**
 * Read and change the one version number this project has.
 *
 * `backend/Cargo.toml` is the source of truth, because it is the only version anybody ever sees: it is
 * what `/api/health` reports, what `--version` prints, and what the UI footer shows. The frontend has to
 * carry the same number because it is compiled into that same binary, and its lockfile has to agree with
 * its manifest. That is four files holding one value, which is three opportunities to drift.
 *
 *     node scripts/version.mjs show          print the authoritative version
 *     node scripts/version.mjs check         verify all four agree, exit 1 if they do not
 *     node scripts/version.mjs set 0.2.0     move all four at once
 *
 * `check` reads files and nothing else, no cargo and no npm, so CI can run it in a couple of seconds.
 * Only `set` needs the package managers, to regenerate the lockfiles rather than hand editing them.
 */

import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));

const CARGO_TOML = join(ROOT, 'backend', 'Cargo.toml');
const CARGO_LOCK = join(ROOT, 'backend', 'Cargo.lock');
const PACKAGE_JSON = join(ROOT, 'frontend', 'package.json');
const PACKAGE_LOCK = join(ROOT, 'frontend', 'package-lock.json');

const CRATE = 'on-air-record';

const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

const read = (path) => readFileSync(path, 'utf8');

/**
 * The version from the `[package]` table.
 *
 * Scoped to that table on purpose. A bare search for `version` would find the first dependency instead,
 * which is both wrong and the kind of wrong that looks right.
 */
function cargoTomlVersion(text) {
  let inPackage = false;
  for (const line of text.split('\n')) {
    const trimmed = line.trim();
    if (trimmed.startsWith('[')) {
      inPackage = trimmed === '[package]';
      continue;
    }
    if (inPackage) {
      const match = /^version\s*=\s*"([^"]+)"/.exec(trimmed);
      if (match) {
        return match[1];
      }
    }
  }
  return null;
}

/** The version recorded for this crate's own entry in the lockfile. */
function cargoLockVersion(text) {
  for (const block of text.split('[[package]]')) {
    if (new RegExp(`^\\s*name\\s*=\\s*"${CRATE}"\\s*$`, 'm').test(block)) {
      const match = /^\s*version\s*=\s*"([^"]+)"\s*$/m.exec(block);
      if (match) {
        return match[1];
      }
    }
  }
  return null;
}

/** Every declared version, authoritative one first. */
function collect() {
  const lock = JSON.parse(read(PACKAGE_LOCK));
  return [
    ['backend/Cargo.toml', cargoTomlVersion(read(CARGO_TOML))],
    ['backend/Cargo.lock', cargoLockVersion(read(CARGO_LOCK))],
    ['frontend/package.json', JSON.parse(read(PACKAGE_JSON)).version ?? null],
    ['frontend/package-lock.json (top level)', lock.version ?? null],
    ['frontend/package-lock.json (packages[""])', lock.packages?.['']?.version ?? null],
  ];
}

function authoritative() {
  const version = cargoTomlVersion(read(CARGO_TOML));
  if (version === null) {
    console.error('could not find the version in the [package] table of backend/Cargo.toml');
    process.exit(1);
  }
  return version;
}

function commandShow() {
  console.log(authoritative());
  return 0;
}

function commandCheck() {
  const found = collect();
  const expected = found[0][1];

  const width = Math.max(...found.map(([label]) => label.length));
  for (const [label, value] of found) {
    const agrees = value === expected;
    console.log(`  ${label.padEnd(width)}  ${value ?? '(missing)'}${agrees ? '' : '   <- disagrees'}`);
  }

  const missing = found.filter(([, value]) => value === null).map(([label]) => label);
  if (missing.length > 0) {
    console.error(`\nno version found in: ${missing.join(', ')}`);
    return 1;
  }

  const wrong = found.slice(1).filter(([, value]) => value !== expected).map(([label]) => label);
  if (wrong.length > 0) {
    console.error(
      `\nbackend/Cargo.toml says ${expected}, but ${wrong.join(', ')} disagrees.\n` +
        `Cargo.toml is the source of truth. Run \`node scripts/version.mjs set ${expected}\` to bring\n` +
        'everything into line, then commit the result.',
    );
    return 1;
  }

  console.log(`\nall four agree on ${expected}`);
  return 0;
}

function run(command, args, cwd) {
  console.log(`  $ ${command} ${args.join(' ')}`);
  execFileSync(command, args, { cwd, stdio: 'inherit' });
}

function commandSet(version) {
  if (!SEMVER.test(version)) {
    console.error(`'${version}' is not a semver version, expected something like 0.2.0`);
    return 2;
  }

  const text = read(CARGO_TOML);
  const current = cargoTomlVersion(text);
  if (current === null) {
    console.error('could not find the version in the [package] table of backend/Cargo.toml');
    return 1;
  }

  // Replace only inside the [package] table, for the same reason the reader is scoped to it. The negated
  // class stops at the next table header, so this cannot reach a dependency.
  const packageTable = /(\[package\][^[]*?version\s*=\s*)"[^"]+"/;
  if (!packageTable.test(text)) {
    console.error('could not rewrite the version in backend/Cargo.toml');
    return 1;
  }
  writeFileSync(CARGO_TOML, text.replace(packageTable, `$1"${version}"`), 'utf8');
  console.log(`backend/Cargo.toml     ${current} -> ${version}`);

  const manifest = JSON.parse(read(PACKAGE_JSON));
  const previous = manifest.version;
  manifest.version = version;
  writeFileSync(PACKAGE_JSON, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
  console.log(`frontend/package.json  ${previous} -> ${version}`);

  // The lockfiles are regenerated by the tools that own them. Hand editing them works right up until it
  // silently does not.
  console.log('refreshing lockfiles');
  run('cargo', ['update', '--package', CRATE, '--offline'], join(ROOT, 'backend'));
  run('npm', ['install', '--package-lock-only', '--silent'], join(ROOT, 'frontend'));

  console.log();
  return commandCheck();
}

const [action, argument, ...rest] = process.argv.slice(2);

if (action === 'show' && argument === undefined) {
  process.exit(commandShow());
} else if (action === 'check' && argument === undefined) {
  process.exit(commandCheck());
} else if (action === 'set' && argument !== undefined && rest.length === 0) {
  process.exit(commandSet(argument));
} else {
  console.error(
    [
      'Usage:',
      '  node scripts/version.mjs show          print the authoritative version',
      '  node scripts/version.mjs check         verify all four recorded versions agree',
      '  node scripts/version.mjs set 0.2.0     move all four at once',
    ].join('\n'),
  );
  process.exit(2);
}
