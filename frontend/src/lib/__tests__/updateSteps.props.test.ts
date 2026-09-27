/**
 * Properties of the update instructions: the quoting that goes into commands an admin pastes into a
 * shell, and the plan built from whatever the service reports about itself.
 *
 * The install folder is whatever the admin chose, so it can hold spaces, quotes, dollar signs, backticks
 * and characters from any language. The only honest test of the quoting is to read the quoted string back
 * with each shell's own rules and get the folder out unchanged, for folders nobody thought to write down.
 * PowerShell's rules include the typographic single quotes, which it treats exactly like the ASCII one; a
 * name such as O'Brien written with a typographic apostrophe is where that matters.
 */

import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import type { InstallInfo, ReleaseInfo, UpdateStatus } from '@/api/types';

import { archiveFor, powershellQuote, RELEASED_TARGETS, shellQuote, updatePlan } from '../updateSteps';

/**
 * Read one POSIX shell word made of single quoted runs and backslash escapes, as `sh` does, returning
 * `null` for anything else. A bare character outside quotes counts as a failure: the quoting is meant to
 * leave nothing for the shell to interpret.
 */
function readPosixWord(word: string): string | null {
  let result = '';
  let index = 0;
  while (index < word.length) {
    const character = word[index];
    if (character === "'") {
      const close = word.indexOf("'", index + 1);
      if (close === -1) {
        return null;
      }
      result += word.slice(index + 1, close);
      index = close + 1;
    } else if (character === '\\' && index + 1 < word.length) {
      result += word[index + 1];
      index += 2;
    } else {
      return null;
    }
  }
  return result;
}

/** Every character PowerShell's tokenizer treats as a single quote. */
const POWERSHELL_QUOTES = ["'", '\u2018', '\u2019', '\u201A', '\u201B'];

/**
 * Read one PowerShell single quoted string, as its tokenizer does: it opens on any single quote character,
 * a pair of them stands for the second, and a lone one closes it. `null` when anything follows the close.
 */
function readPowershellString(literal: string): string | null {
  const characters = [...literal];
  if (!POWERSHELL_QUOTES.includes(characters[0])) {
    return null;
  }
  let result = '';
  let index = 1;
  while (index < characters.length) {
    const character = characters[index];
    if (POWERSHELL_QUOTES.includes(character)) {
      if (index + 1 < characters.length && POWERSHELL_QUOTES.includes(characters[index + 1])) {
        result += characters[index + 1];
        index += 2;
        continue;
      }
      return index === characters.length - 1 ? result : null;
    }
    result += character;
    index += 1;
  }
  return null;
}

/** Folder names leaning on the characters shells care about, plus anything at all. */
const folder = fc.oneof(
  fc
    .array(
      fc.oneof(
        fc.constantFrom("'", '"', '\\', '/', ' ', '$', '`', '!', '*', '&', ';', '|', '(', ')', 'O', 'x'),
        fc.constantFrom(...POWERSHELL_QUOTES),
        fc.string({ unit: 'binary', maxLength: 3 }),
      ),
      { maxLength: 20 },
    )
    .map((parts) => parts.join('')),
  fc.string({ unit: 'binary', maxLength: 40 }),
);

describe('shellQuote', () => {
  it('reads back as exactly the folder it quoted, leaving nothing for sh to expand', () => {
    fc.assert(
      fc.property(folder, (value) => {
        expect(readPosixWord(shellQuote(value))).toBe(value);
      }),
    );
  });
});

describe('powershellQuote', () => {
  it('reads back as exactly the folder it quoted, typographic quotes included', () => {
    fc.assert(
      fc.property(folder, (value) => {
        expect(readPowershellString(powershellQuote(value))).toBe(value);
      }),
    );
  });
});

describe('archiveFor', () => {
  it('offers an archive exactly for the released targets, named after the tag and target', () => {
    fc.assert(
      fc.property(
        fc.stringMatching(/^v\d{1,2}\.\d{1,2}\.\d{1,2}(-beta\.\d)?$/),
        fc.oneof(fc.constantFrom(...RELEASED_TARGETS), fc.string({ maxLength: 30 })),
        (tag, target) => {
          const archive = archiveFor(tag, target);
          if (!(RELEASED_TARGETS as readonly string[]).includes(target)) {
            expect(archive).toBeNull();
            return;
          }
          const extension = target.includes('windows') ? 'zip' : 'tar.gz';
          expect(archive?.name).toBe(`on-air-record-${tag}-${target}.${extension}`);
          expect(archive?.url.endsWith(`/releases/download/${tag}/${archive?.name}`)).toBe(true);
        },
      ),
    );
  });
});

describe('updatePlan', () => {
  const install: fc.Arbitrary<InstallInfo> = fc.record({
    kind: fc.constantFrom('installer', 'systemd', 'docker', 'manual'),
    dir: fc.option(folder, { nil: null }),
    os: fc.constantFrom('linux', 'macos', 'windows', 'freebsd'),
    target: fc.oneof(fc.constantFrom(...RELEASED_TARGETS), fc.constantFrom('aarch64-unknown-linux-gnu', '')),
  });

  const release: fc.Arbitrary<ReleaseInfo> = fc
    .stringMatching(/^\d{1,2}\.\d{1,2}\.\d{1,2}(-beta\.\d)?$/)
    .map((version) => ({
      version,
      tag: `v${version}`,
      prerelease: version.includes('beta'),
      publishedAtMs: null,
      notes: '',
      url: `https://github.com/shibbirweb/on-air-record/releases/tag/v${version}`,
    }));

  const scenario = fc.record({
    install,
    release,
    channel: fc.constantFrom<UpdateStatus['channel']>('stable', 'beta'),
  });

  function statusFor(value: {
    install: InstallInfo;
    release: ReleaseInfo;
    channel: UpdateStatus['channel'];
  }): UpdateStatus {
    return {
      currentVersion: '0.0.1',
      channel: value.channel,
      automatic: true,
      checkedAtMs: null,
      error: null,
      available: value.release,
      releases: [value.release],
      install: value.install,
      releasesUrl: 'https://github.com/shibbirweb/on-air-record/releases',
    };
  }

  it('always gives at least one step, each with words, and a command only when there is one to run', () => {
    fc.assert(
      fc.property(scenario, (value) => {
        const plan = updatePlan(statusFor(value), value.release);
        expect(plan.steps.length).toBeGreaterThan(0);
        for (const step of plan.steps) {
          expect(step.text.trim().length).toBeGreaterThan(0);
          if (step.command !== undefined) {
            expect(step.command.trim().length).toBeGreaterThan(0);
          }
        }
        if (plan.note !== null) {
          expect(plan.note.trim().length).toBeGreaterThan(0);
        }
      }),
    );
  });

  it('offers the archive for this machine, except inside a container', () => {
    fc.assert(
      fc.property(scenario, (value) => {
        const plan = updatePlan(statusFor(value), value.release);
        const expected = value.install.kind === 'docker' ? null : archiveFor(value.release.tag, value.install.target);
        expect(plan.download).toEqual(expected);
      }),
    );
  });

  it('puts an installer folder into every command quoted for the shell that will run it', () => {
    fc.assert(
      fc.property(scenario, folder.filter((dir) => dir !== ''), (value, dir) => {
        const installer = { ...value, install: { ...value.install, kind: 'installer' as const, dir } };
        const plan = updatePlan(statusFor(installer), value.release);
        const windows = installer.install.os === 'windows';
        const commands = plan.steps.flatMap((step) => (step.command ? [step.command] : []));

        expect(commands).toHaveLength(2);
        if (windows) {
          expect(commands[0]).toBe(`& ${powershellQuote(`${dir}\\stop.cmd`)}`);
          expect(commands[1].endsWith(` -Dir ${powershellQuote(dir)}`)).toBe(true);
        } else {
          expect(commands[0]).toBe(shellQuote(`${dir}/stop.sh`));
          expect(commands[1].endsWith(` --dir ${shellQuote(dir)}`)).toBe(true);
        }
      }),
    );
  });

  it('tells a container to pull the tag for its channel and names the new version', () => {
    fc.assert(
      fc.property(scenario, (value) => {
        const docker = { ...value, install: { ...value.install, kind: 'docker' as const } };
        const plan = updatePlan(statusFor(docker), value.release);
        const tag = value.channel === 'beta' ? 'beta' : 'latest';
        expect(plan.note).toContain(`:${tag}`);
        expect(plan.note).toContain(value.release.version);
      }),
    );
  });
});
