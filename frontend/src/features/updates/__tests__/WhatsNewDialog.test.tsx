// @vitest-environment jsdom

/**
 * The what's new dialog: every release since the running one with its notes, then the steps to update
 * written for the way this copy was installed, with commands that can be copied as they are. The plans
 * themselves are built and tested in lib/updateSteps; these prove the dialog shows the right one for each
 * install kind and that nothing the service reports is lost on the way to the page.
 */

import '@/test/dom';

import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { InstallInfo, UpdateStatus } from '@/api/types';

import { WhatsNewDialog } from '../WhatsNewDialog';
import { release, updateStatus } from './fixtures';

const DATE_FORMAT = new Intl.DateTimeFormat(undefined, { year: 'numeric', month: 'short', day: 'numeric' });

function renderDialog(status: UpdateStatus, open = true) {
  const onOpenChange = vi.fn<(open: boolean) => void>();
  const user = userEvent.setup();
  render(<WhatsNewDialog status={status} open={open} onOpenChange={onOpenChange} />);
  return { user, onOpenChange };
}

function withInstall(install: InstallInfo, overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  return updateStatus({ install, ...overrides });
}

/** The text of every step under How to update, commands included. */
function steps(): string[] {
  const heading = screen.getByRole('heading', { name: 'How to update' });
  const section = heading.closest('section');
  if (!section) {
    throw new Error('no update steps');
  }
  return within(section)
    .getAllByRole('listitem')
    .map((item) => item.textContent ?? '');
}

describe('the what is new dialog', () => {
  it('shows nothing when no newer release exists', () => {
    renderDialog(updateStatus({ available: null, releases: [] }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('shows nothing while closed', () => {
    renderDialog(updateStatus(), false);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('names the newest release and the one running here', () => {
    renderDialog(updateStatus());
    expect(screen.getByRole('dialog', { name: "What's new in 0.9.0" })).toBeInTheDocument();
    expect(screen.getByText(/You have 0\.8\.1\. Here is what changed\./)).toBeInTheDocument();
  });

  it('lists every release since the running one, newest first, not just the newest', () => {
    const newest = release('0.9.0', { notes: '- Newest change' });
    const skipped = release('0.8.2', { notes: '- A fix you skipped' });
    renderDialog(updateStatus({ available: newest, releases: [newest, skipped] }));

    expect(screen.getByText(/2 releases have come out since, newest first\./)).toBeInTheDocument();
    const headings = screen.getAllByRole('heading', { level: 3 }).map((heading) => heading.textContent);
    expect(headings).toEqual(['0.9.0', '0.8.2', 'How to update']);
    expect(screen.getByText('Newest change')).toBeInTheDocument();
    expect(screen.getByText('A fix you skipped')).toBeInTheDocument();
  });

  it('marks a beta and dates each release', () => {
    const beta = release('0.9.0-beta.2', { prerelease: true });
    renderDialog(updateStatus({ available: beta, releases: [beta], channel: 'beta' }));
    expect(screen.getByText('Beta')).toBeInTheDocument();
    expect(screen.getByText(DATE_FORMAT.format(new Date(beta.publishedAtMs ?? 0)))).toBeInTheDocument();
  });

  it('leaves the date out when GitHub gave none, and the Beta mark off a stable release', () => {
    const undated = release('0.9.0', { publishedAtMs: null });
    renderDialog(updateStatus({ available: undated, releases: [undated] }));
    expect(screen.queryByText('Beta')).not.toBeInTheDocument();
    expect(screen.queryByText(DATE_FORMAT.format(new Date(Date.UTC(2026, 8, 20, 12))))).not.toBeInTheDocument();
  });

  it('links each release page in a new tab', () => {
    renderDialog(updateStatus());
    const link = screen.getByRole('link', { name: 'Release page' });
    expect(link).toHaveAttribute('href', 'https://github.com/shibbirweb/on-air-record/releases/tag/v0.9.0');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noreferrer noopener');
  });

  it('draws the release notes as data, so markup in them stays text', () => {
    const risky = release('0.9.0', { notes: '<img src="x" onerror="alert(1)">' });
    renderDialog(updateStatus({ available: risky, releases: [risky] }));
    expect(screen.getByRole('dialog').querySelector('img')).toBeNull();
    expect(screen.getByText('<img src="x" onerror="alert(1)">')).toBeInTheDocument();
  });

  it('warns that updating restarts the service and keeps everything', () => {
    renderDialog(updateStatus());
    expect(screen.getByText(/Updating restarts the service, so a few seconds are not recorded/)).toBeInTheDocument();
    expect(screen.getByText(/Settings, accounts and recordings are kept/)).toBeInTheDocument();
  });

  it('closes from its close button', async () => {
    const { user, onOpenChange } = renderDialog(updateStatus());
    await user.click(screen.getByRole('button', { name: 'Close' }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  describe('the steps for each way of installing', () => {
    it('tell an installer copy on Linux or macOS to stop it and rerun the installer in its folder', () => {
      renderDialog(updateStatus());
      const [stop, rerun] = steps();
      expect(stop).toContain("'/home/radio/on-air-record/stop.sh'");
      expect(rerun).toContain('install.sh | sh -s -- --update --dir');
      expect(rerun).toContain("'/home/radio/on-air-record'");
      expect(screen.getByText('Run both as the account that owns the folder.')).toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'on-air-record-v0.9.0-x86_64-unknown-linux-gnu.tar.gz' })).toHaveAttribute(
        'href',
        'https://github.com/shibbirweb/on-air-record/releases/download/v0.9.0/on-air-record-v0.9.0-x86_64-unknown-linux-gnu.tar.gz',
      );
    });

    it('tell an installer copy on Windows to run the PowerShell installer in its folder', () => {
      renderDialog(
        withInstall({
          kind: 'installer',
          dir: 'C:\\Users\\radio\\on-air-record',
          os: 'windows',
          target: 'x86_64-pc-windows-msvc',
        }),
      );
      const [stop, rerun] = steps();
      expect(stop).toContain("& 'C:\\Users\\radio\\on-air-record\\stop.cmd'");
      expect(rerun).toContain("install.ps1))) -Update -Dir 'C:\\Users\\radio\\on-air-record'");
      expect(screen.getByRole('link', { name: 'on-air-record-v0.9.0-x86_64-pc-windows-msvc.zip' })).toBeInTheDocument();
    });

    it('tell a Linux service to download, check, install and restart', () => {
      renderDialog(withInstall({ kind: 'systemd', dir: null, os: 'linux', target: 'x86_64-unknown-linux-gnu' }));
      const [only] = steps();
      expect(only).toContain('sha256sum -c on-air-record-v0.9.0-x86_64-unknown-linux-gnu.tar.gz.sha256');
      expect(only).toContain('sudo systemctl restart on-air-record');
      expect(screen.getByText(/If the program lives somewhere other than \/usr\/local\/bin/)).toBeInTheDocument();
    });

    it('tell a Docker copy to pull and recreate, with no download to offer', () => {
      renderDialog(withInstall({ kind: 'docker', dir: null, os: 'linux', target: 'x86_64-unknown-linux-gnu' }));
      const [only] = steps();
      expect(only).toContain('docker compose pull');
      expect(only).toContain('docker compose up -d');
      expect(screen.queryByText(/The download for this machine is/)).not.toBeInTheDocument();
      expect(screen.getByText(/docker pull ghcr\.io\/shibbirweb\/on-air-record:latest/)).toBeInTheDocument();
    });

    it('point a Docker copy on the beta channel at the beta image', () => {
      renderDialog(
        withInstall(
          { kind: 'docker', dir: null, os: 'linux', target: 'x86_64-unknown-linux-gnu' },
          { channel: 'beta' },
        ),
      );
      expect(screen.getByText(/docker pull ghcr\.io\/shibbirweb\/on-air-record:beta/)).toBeInTheDocument();
    });

    it('tell any other copy which download to get', () => {
      renderDialog(withInstall({ kind: 'manual', dir: null, os: 'macos', target: 'aarch64-apple-darwin' }));
      expect(steps()).toEqual([
        'Download on-air-record-v0.9.0-aarch64-apple-darwin.tar.gz from the release page.',
        'Stop the service, replace the program with the one in the download, and start it again.',
      ]);
    });

    it('say a machine with no ready made download needs building from source', () => {
      renderDialog(withInstall({ kind: 'manual', dir: null, os: 'linux', target: 'riscv64gc-unknown-linux-gnu' }));
      expect(steps()).toEqual(['Get the new version from the release page.']);
      expect(screen.getByText(/no ready made download for this machine \(riscv64gc-unknown-linux-gnu\)/)).toBeInTheDocument();
      expect(screen.queryByText(/The download for this machine is/)).not.toBeInTheDocument();
    });
  });

  describe('copying a command', () => {
    const secure = Object.getOwnPropertyDescriptor(window, 'isSecureContext');

    function setSecureContext(value: boolean) {
      Object.defineProperty(window, 'isSecureContext', { value, configurable: true });
    }

    beforeEach(() => {
      setSecureContext(true);
    });

    afterEach(() => {
      if (secure) {
        Object.defineProperty(window, 'isSecureContext', secure);
      } else {
        Reflect.deleteProperty(window, 'isSecureContext');
      }
    });

    it('puts the command on the clipboard exactly as shown', async () => {
      const { user } = renderDialog(updateStatus());
      const [stopCopy] = screen.getAllByRole('button', { name: 'Copy the command' });
      await user.click(stopCopy);
      await expect(navigator.clipboard.readText()).resolves.toBe("'/home/radio/on-air-record/stop.sh'");
    });

    it('offers a copy button for each command', () => {
      renderDialog(updateStatus());
      expect(screen.getAllByRole('button', { name: 'Copy the command' })).toHaveLength(2);
    });

    it('offers no copy button for steps with nothing to run', () => {
      renderDialog(withInstall({ kind: 'manual', dir: null, os: 'macos', target: 'aarch64-apple-darwin' }));
      expect(screen.queryByRole('button', { name: 'Copy the command' })).not.toBeInTheDocument();
    });

    it('leaves the command to select by hand on a plain http address', () => {
      setSecureContext(false);
      renderDialog(updateStatus());
      expect(screen.queryByRole('button', { name: 'Copy the command' })).not.toBeInTheDocument();
      expect(screen.getByText("'/home/radio/on-air-record/stop.sh'")).toBeInTheDocument();
    });
  });
});
