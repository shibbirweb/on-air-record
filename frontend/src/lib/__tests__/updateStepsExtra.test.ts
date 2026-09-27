/**
 * The update plan branches `updateSteps.test.ts` leaves out: the exported image names and release
 * targets, an installer install that does not know its folder, a manual Windows install, and a systemd
 * install whose target has a download but not a tarball.
 *
 * The image names and targets are asserted literally on purpose: they must match what the release
 * workflow publishes, and a silent change here would hand people commands that pull nothing.
 */

import { describe, expect, it } from 'vitest';

import type { InstallInfo, ReleaseInfo, UpdateStatus } from '@/api/types';

import { archiveFor, DOCKER_HUB_IMAGE, IMAGE, RELEASED_TARGETS, updatePlan } from '../updateSteps';

const RELEASE: ReleaseInfo = {
  version: '0.7.0',
  tag: 'v0.7.0',
  prerelease: false,
  publishedAtMs: 0,
  notes: '',
  url: '',
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
    releasesUrl: '',
  };
}

describe('published names', () => {
  it('name the images the release workflow pushes', () => {
    expect(IMAGE).toBe('ghcr.io/shibbirweb/on-air-record');
    expect(DOCKER_HUB_IMAGE).toBe('shibbirweb/on-air-record');
  });

  it('list the four targets every release is built for, each with an archive', () => {
    expect([...RELEASED_TARGETS]).toEqual([
      'aarch64-apple-darwin',
      'x86_64-apple-darwin',
      'x86_64-unknown-linux-gnu',
      'x86_64-pc-windows-msvc',
    ]);
    for (const target of RELEASED_TARGETS) {
      expect(archiveFor('v0.7.0', target)?.name).toMatch(new RegExp(`^on-air-record-v0\\.7\\.0-${target}\\.`));
    }
  });
});

describe('updatePlan, remaining installs', () => {
  it('treats an installer install without a known folder like a manual one', () => {
    const plan = updatePlan(
      status({ kind: 'installer', dir: null, os: 'linux', target: 'x86_64-unknown-linux-gnu' }),
      RELEASE,
    );
    expect(plan.steps.map((step) => step.text)).toEqual([
      'Download on-air-record-v0.7.0-x86_64-unknown-linux-gnu.tar.gz from the release page.',
      'Stop the service, replace the program with the one in the download, and start it again.',
    ]);
    expect(plan.steps.every((step) => step.command === undefined)).toBe(true);
    expect(plan.note).toBeNull();
  });

  it('offers the zip for a manual Windows install', () => {
    const plan = updatePlan(
      status({ kind: 'manual', dir: null, os: 'windows', target: 'x86_64-pc-windows-msvc' }),
      RELEASE,
    );
    expect(plan.download?.name).toBe('on-air-record-v0.7.0-x86_64-pc-windows-msvc.zip');
  });

  it('has no Windows installer note, unlike the POSIX one about the folder owner', () => {
    const plan = updatePlan(
      status({ kind: 'installer', dir: 'C:\\oar', os: 'windows', target: 'x86_64-pc-windows-msvc' }),
      RELEASE,
    );
    expect(plan.note).toBeNull();
    expect(plan.download?.name).toBe('on-air-record-v0.7.0-x86_64-pc-windows-msvc.zip');
  });

  it('downloads and checks the checksum from the same address for systemd', () => {
    const plan = updatePlan(
      status({ kind: 'systemd', dir: null, os: 'linux', target: 'x86_64-unknown-linux-gnu' }),
      RELEASE,
    );
    const lines = (plan.steps[0].command ?? '').split('\n');
    expect(lines[0]).toBe('cd /tmp');
    expect(lines[1]).toBe(`curl -fLO ${plan.download?.url}`);
    expect(lines[2]).toBe(`curl -fLO ${plan.download?.url}.sha256`);
    expect(plan.note).toMatch(/\/usr\/local\/bin/);
  });

  it('asks only for the new version from the release page when nothing fits, with no steps to run', () => {
    const plan = updatePlan(
      status({ kind: 'manual', dir: null, os: 'freebsd', target: 'x86_64-unknown-freebsd' }),
      RELEASE,
    );
    expect(plan.steps).toEqual([{ text: 'Get the new version from the release page.' }]);
    expect(plan.note).toMatch(/building from source/);
  });
});
