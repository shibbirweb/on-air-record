import { describe, expect, it } from 'vitest';

import type { InstallInfo, ReleaseInfo, UpdateStatus } from '@/api/types';

import { archiveFor, powershellQuote, shellQuote, updatePlan } from '../updateSteps';

const RELEASE: ReleaseInfo = {
  version: '0.7.0',
  tag: 'v0.7.0',
  prerelease: false,
  publishedAtMs: 0,
  notes: '',
  url: 'https://github.com/shibbirweb/on-air-record/releases/tag/v0.7.0',
};

function status(install: InstallInfo): UpdateStatus {
  return {
    currentVersion: '0.6.0',
    channel: 'stable',
    automatic: true,
    checkedAtMs: 0,
    error: null,
    available: RELEASE,
    releases: [RELEASE],
    install,
    releasesUrl: 'https://github.com/shibbirweb/on-air-record/releases',
  };
}

describe('archiveFor', () => {
  it('names the archive the release workflow publishes', () => {
    expect(archiveFor('v0.7.0', 'x86_64-unknown-linux-gnu')).toEqual({
      name: 'on-air-record-v0.7.0-x86_64-unknown-linux-gnu.tar.gz',
      url: 'https://github.com/shibbirweb/on-air-record/releases/download/v0.7.0/on-air-record-v0.7.0-x86_64-unknown-linux-gnu.tar.gz',
    });
    expect(archiveFor('v0.7.0', 'x86_64-pc-windows-msvc')?.name).toBe(
      'on-air-record-v0.7.0-x86_64-pc-windows-msvc.zip',
    );
  });

  it('has nothing for a machine no release is built for', () => {
    expect(archiveFor('v0.7.0', 'aarch64-unknown-linux-gnu')).toBeNull();
  });
});

describe('quoting', () => {
  it('survives spaces and quotes in a folder name', () => {
    expect(shellQuote("/home/me/Jo's recorder")).toBe(`'/home/me/Jo'\\''s recorder'`);
    expect(powershellQuote("C:\\Users\\Jo's\\on-air-record")).toBe(`'C:\\Users\\Jo''s\\on-air-record'`);
  });
});

describe('updatePlan', () => {
  it('stops then re-runs the installer for an installer folder on Linux or macOS', () => {
    const plan = updatePlan(
      status({
        kind: 'installer',
        dir: '/home/me/on-air-record',
        os: 'linux',
        target: 'x86_64-unknown-linux-gnu',
      }),
      RELEASE,
    );
    expect(plan.steps.map((step) => step.command)).toEqual([
      `'/home/me/on-air-record/stop.sh'`,
      `curl -fsSL https://raw.githubusercontent.com/shibbirweb/on-air-record/master/scripts/install.sh | sh -s -- --update --dir '/home/me/on-air-record'`,
    ]);
    expect(plan.note).toMatch(/owns the folder/);
  });

  it('uses PowerShell for an installer folder on Windows', () => {
    const plan = updatePlan(
      status({
        kind: 'installer',
        dir: 'C:\\on-air-record',
        os: 'windows',
        target: 'x86_64-pc-windows-msvc',
      }),
      RELEASE,
    );
    expect(plan.steps[0].command).toBe(`& 'C:\\on-air-record\\stop.cmd'`);
    expect(plan.steps[1].command).toBe(
      `& ([scriptblock]::Create((irm https://raw.githubusercontent.com/shibbirweb/on-air-record/master/scripts/install.ps1))) -Update -Dir 'C:\\on-air-record'`,
    );
  });

  it('downloads, checks, installs and restarts for a systemd service', () => {
    const plan = updatePlan(
      status({ kind: 'systemd', dir: null, os: 'linux', target: 'x86_64-unknown-linux-gnu' }),
      RELEASE,
    );
    const command = plan.steps[0].command ?? '';
    expect(command).toContain('sha256sum -c on-air-record-v0.7.0-x86_64-unknown-linux-gnu.tar.gz.sha256');
    expect(command).toContain(
      'sudo install -m 755 on-air-record-v0.7.0-x86_64-unknown-linux-gnu/on-air-record /usr/local/bin/on-air-record',
    );
    expect(command.split('\n').at(-1)).toBe('sudo systemctl restart on-air-record');
  });

  it('points anything else at the download for this machine', () => {
    const plan = updatePlan(
      status({ kind: 'manual', dir: null, os: 'macos', target: 'aarch64-apple-darwin' }),
      RELEASE,
    );
    expect(plan.download?.name).toBe('on-air-record-v0.7.0-aarch64-apple-darwin.tar.gz');
    expect(plan.note).toBeNull();
  });

  it('says so when no ready made download fits the machine', () => {
    for (const kind of ['manual', 'systemd'] as const) {
      const plan = updatePlan(
        status({ kind, dir: null, os: 'linux', target: 'aarch64-unknown-linux-gnu' }),
        RELEASE,
      );
      expect(plan.download).toBeNull();
      expect(plan.note).toMatch(/aarch64-unknown-linux-gnu/);
    }
  });
});
