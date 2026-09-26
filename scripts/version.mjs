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
 *     node scripts/version.mjs bump          show what is unreleased and pick the next version
 *     node scripts/version.mjs pending       report whether a release is due, for CI to surface
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

// ------------------------------------------------------------------ releasing

/** Run a git command, or return null when git has nothing to say. */
function git(...args) {
  try {
    return execFileSync('git', args, { cwd: ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch {
    return null;
  }
}

function lastTag() {
  return git('describe', '--tags', '--abbrev=0', '--match', 'v*');
}

/**
 * Commit subjects since a tag, newest first. Everything when there is no tag yet.
 *
 * Merge commits are left out: with pull requests into develop every change arrives with one, and its
 * subject ("Merge pull request #4 from ...") says nothing about what kind of change it carried.
 */
function commitsSince(tag) {
  const range = tag ? `${tag}..HEAD` : 'HEAD';
  const out = git('log', range, '--no-merges', '--format=%s');
  return out ? out.split('\n').filter(Boolean) : [];
}

/**
 * The `[Unreleased]` section of the changelog, without its heading: what beta.yml uses as the release
 * notes, and what a stable release's notes are pasted from by hand.
 */
function commandNotes() {
  const text = read(join(ROOT, 'CHANGELOG.md'));
  const start = text.indexOf('## [Unreleased]');
  if (start === -1) {
    console.error('CHANGELOG.md has no [Unreleased] section to take release notes from');
    return 1;
  }
  const body = text.slice(text.indexOf('\n', start) + 1);
  const end = body.search(/^## \[/m);
  const notes = (end === -1 ? body : body.slice(0, end)).trim();
  if (notes === '') {
    console.error('the [Unreleased] section of CHANGELOG.md is empty');
    return 1;
  }
  console.log(notes);
  return 0;
}

/** The next OAR ticket, so the suggested commit line is ready to paste. */
function nextTicket() {
  const out = git('log', '--format=%s') ?? '';
  const used = [...out.matchAll(/\[OAR-(\d+)\]/g)].map((match) => Number(match[1]));
  return used.length > 0 ? Math.max(...used) + 1 : 1;
}

/** The beta number of a version like 0.4.0-beta.2, or null for a stable version. */
function betaNumber(version) {
  const match = /-beta\.(\d+)$/.exec(version);
  return match ? Number(match[1]) : null;
}

/**
 * Every version the next release could carry, by level.
 *
 * From a stable version, `beta` is the first beta of whichever release the commits suggest, so a beta
 * always previews a real next version rather than an arbitrary one. From a beta, `beta` is the next beta
 * of the same version and `release` is that version, finished. `suggested` is the stable level the
 * commits imply.
 */
function nextVersions(current, suggested = 'minor') {
  const core = current.split('-')[0];
  const [major, minor, patch] = core.split('.').map(Number);
  const stable = {
    patch: `${major}.${minor}.${patch + 1}`,
    minor: `${major}.${minor + 1}.0`,
    major: `${major + 1}.0.0`,
  };

  const beta = betaNumber(current);
  if (beta === null) {
    return { ...stable, beta: `${stable[suggested]}-beta.1` };
  }
  return { beta: `${core}-beta.${beta + 1}`, release: core };
}

/** The levels `bump` offers from a version: betas can only move to another beta or be finished. */
function levelsFrom(current) {
  return betaNumber(current) === null ? ['patch', 'minor', 'major', 'beta'] : ['beta', 'release'];
}

/**
 * What the commits since the last release imply.
 *
 * The commit convention carries this already: a `feat:` is a new feature and a `fix:` is a bug fix, so
 * the suggestion is read off the log rather than guessed at.
 */
function suggest(subjects) {
  const kind = (subject) => (subject.split(/[:(\[]/, 1)[0] ?? '').trim().toLowerCase();
  const kinds = subjects.map(kind);
  if (kinds.includes('feat')) {
    return 'minor';
  }
  if (kinds.includes('fix')) {
    return 'patch';
  }
  return 'patch';
}

function summarise(subjects) {
  const counts = new Map();
  for (const subject of subjects) {
    const kind = (subject.split(/[:(\[]/, 1)[0] ?? '?').trim().toLowerCase() || '?';
    counts.set(kind, (counts.get(kind) ?? 0) + 1);
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([kind, count]) => `${count} ${kind}`)
    .join(', ');
}

/**
 * Everything worth knowing about whether a release is due.
 *
 * `git describe` needs the tags, so anywhere this runs on a shallow clone has to fetch them first.
 */
function releaseState() {
  const current = authoritative();
  const tag = lastTag();
  return {
    current,
    tag,
    released: tag !== null && git('rev-parse', '--verify', `refs/tags/v${current}`) !== null,
    subjects: commitsSince(tag),
  };
}

/**
 * A read only answer to "is there anything to release?", for CI to put in its run summary. Always exits
 * zero: this is a note, not a rule, and a release being due is not a build failure.
 */
function commandPending() {
  const { current, tag, subjects } = releaseState();

  console.log(`Current version  ${current}`);
  console.log(`Last release     ${tag ?? 'none yet'}`);

  if (subjects.length === 0) {
    console.log('');
    console.log('Nothing has landed since that release. Nothing to do.');
    return 0;
  }

  const level = suggest(subjects);
  const next = nextVersions(current, level);
  console.log(`Unreleased       ${subjects.length} commits (${summarise(subjects)})`);
  console.log('');
  if (betaNumber(current) === null) {
    console.log(`A ${level} release would make this ${next[level]}, or ${next.beta} as a beta.`);
  } else {
    console.log(`The next beta would be ${next.beta}, and finishing it would make ${next.release}.`);
  }
  console.log('Run `node scripts/version.mjs bump` to cut it.');
  return 0;
}

async function commandBump(requested) {
  const { current, tag, released } = releaseState();

  console.log(`Current version  ${current}`);
  console.log(`Last release     ${tag ?? 'none yet'}`);

  if (!released && tag !== null) {
    console.log(
      `\n${current} is in the manifests but has never been released. Nothing to bump: create a release` +
        `\ntagged v${current} instead, or pass an explicit level to move past it.`,
    );
    if (!requested) {
      return 0;
    }
  }

  const subjects = commitsSince(tag);
  if (subjects.length === 0) {
    // Finishing a beta usually has nothing new in it: the beta held up, so it ships as it is. That is the
    // one step that makes sense with no new commits, so it is allowed and offered; anything else is not.
    if (betaNumber(current) !== null && (requested === undefined || requested === 'release')) {
      const target = nextVersions(current).release;
      console.log(`\nNothing has landed since ${tag}, so the one step is to finish it as ${target}.`);
      return finishBump(current, 'release', target);
    }
    console.log('\nNothing has landed since that release, so there is nothing to put in a new one.');
    return 0;
  }

  console.log(`\n${subjects.length} commits since ${tag ?? 'the beginning'} (${summarise(subjects)}):\n`);
  for (const subject of subjects.slice(0, 20)) {
    console.log(`  ${subject}`);
  }
  if (subjects.length > 20) {
    console.log(`  ... and ${subjects.length - 20} more`);
  }

  const stableLevel = suggest(subjects);
  const next = nextVersions(current, stableLevel);
  const onBeta = betaNumber(current) !== null;
  // On a beta the question is whether it needs another round or is ready; the commits cannot tell, so
  // another beta is the careful suggestion.
  const suggested = onBeta ? 'beta' : stableLevel;
  const levels = levelsFrom(current);
  const describe = {
    patch: 'bug fixes only',
    minor: 'new features, nothing broken',
    major: 'something that was working now behaves differently',
    beta: onBeta ? 'another beta of the same version' : 'a pre-release to test first, from develop',
    release: 'the beta is ready: publish it as stable, from master',
  };

  let level = requested;
  if (level && !levels.includes(level)) {
    console.error(`\n'${level}' is not one of: ${levels.join(', ')}`);
    return 2;
  }

  if (!level) {
    console.log('\nWhat kind of release is this?\n');
    levels.forEach((name, index) => {
      const mark = name === suggested ? '  <- suggested' : '';
      console.log(`  ${index + 1}) ${name.padEnd(7)} ${next[name].padEnd(14)} ${describe[name]}${mark}`);
    });
    const cancel = levels.length + 1;
    console.log(`  ${cancel}) cancel`);

    if (!process.stdin.isTTY) {
      console.error('\nNo terminal to ask at. Pass the level: version.mjs bump minor');
      return 2;
    }

    // Imported here rather than at the top: node:readline/promises needs Node 17, and only this one
    // branch prompts. `show`, `check` and `pending` have to keep working on whatever node is lying around.
    const { createInterface } = await import('node:readline/promises');
    const rl = createInterface({ input: process.stdin, output: process.stdout });
    const answer = (await rl.question(`\nWhich? [${levels.indexOf(suggested) + 1}] `)).trim();
    rl.close();

    if (answer === String(cancel) || answer.toLowerCase() === 'cancel') {
      console.log('Nothing changed.');
      return 0;
    }
    const chosen = answer === '' ? suggested : levels[Number(answer) - 1] ?? answer;
    if (!levels.includes(chosen)) {
      console.error(`'${answer}' is not one of the options.`);
      return 2;
    }
    level = chosen;
  }

  return finishBump(current, level, next[level]);
}

/** Move the manifests to `target` and say what finishing the release takes from here. */
function finishBump(current, level, target) {
  console.log(`\n${level}: ${current} -> ${target}\n`);

  const result = commandSet(target);
  if (result !== 0) {
    return result;
  }

  // A beta goes to develop and is published as a pre-release; a stable version to master as a normal
  // release. Both branches take changes only by pull request. The release workflow refuses the wrong
  // pre-release setting, so say which it is up front.
  const isBeta = betaNumber(target) !== null;
  const branch = isBeta ? 'develop' : 'master';
  console.log(`\nNothing is released yet. Commit this on a branch and merge it into ${branch} by pull request:\n`);
  console.log(`  git commit -am "chore:[OAR-${nextTicket()}] release ${target}"`);
  console.log('');
  console.log(
    `Once it is on ${branch}, on GitHub: Releases, Draft a new release, create the tag v${target} on` +
      ` ${branch},` +
      (isBeta ? ' tick "Set as a pre-release",' : ' leave "Set as a pre-release" unticked,') +
      ' Publish.',
  );
  console.log('The workflow refuses any other tag, builds all four platforms, attaches them, and then');
  console.log('installs the result on macOS, Linux and Windows to prove it works.');
  if (isBeta) {
    console.log('The Beta release workflow does all of this for you; see "A beta in two clicks".');
    console.log('Testers install it with the installer\'s --beta option; stable users are not offered it.');
  }
  return 0;
}

const [action, argument, ...rest] = process.argv.slice(2);

if (action === 'show' && argument === undefined) {
  process.exit(commandShow());
} else if (action === 'check' && argument === undefined) {
  process.exit(commandCheck());
} else if (action === 'set' && argument !== undefined && rest.length === 0) {
  process.exit(commandSet(argument));
} else if (action === 'bump' && rest.length === 0) {
  process.exit(await commandBump(argument));
} else if (action === 'pending' && argument === undefined) {
  process.exit(commandPending());
} else if (action === 'notes' && argument === undefined) {
  process.exit(commandNotes());
} else if (action === 'next-ticket' && argument === undefined) {
  console.log(nextTicket());
  process.exit(0);
} else {
  console.error(
    [
      'Usage:',
      '  node scripts/version.mjs show                 print the authoritative version',
      '  node scripts/version.mjs check                verify all four recorded versions agree',
      '  node scripts/version.mjs set 0.2.0            move all four to an exact version',
      '  node scripts/version.mjs bump                 show what is unreleased and choose the next version',
      '  node scripts/version.mjs bump minor           the same without the question',
      '  node scripts/version.mjs bump beta            start or continue a beta, like 0.4.0-beta.1',
      '  node scripts/version.mjs bump release         finish a beta: 0.4.0-beta.3 becomes 0.4.0',
      '  node scripts/version.mjs pending              report whether a release is due, never fails',
      '  node scripts/version.mjs notes                print the [Unreleased] changelog section',
      '  node scripts/version.mjs next-ticket          print the next free OAR ticket number',
    ].join('\n'),
  );
  process.exit(2);
}
