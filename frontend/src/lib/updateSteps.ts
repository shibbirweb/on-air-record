/**
 * How to install an update, for the way this copy was installed.
 *
 * Notify only: the service never updates itself, so this is what an admin is told to run. Each plan is
 * built from what the service reports about itself (how it was started, its folder, its platform and
 * build target), so the commands can be copied as they are.
 */

import type { ReleaseInfo, UpdateStatus } from '@/api/types';

const REPOSITORY = 'shibbirweb/on-air-record';
const INSTALL_SH = `https://raw.githubusercontent.com/${REPOSITORY}/master/scripts/install.sh`;
const INSTALL_PS1 = `https://raw.githubusercontent.com/${REPOSITORY}/master/scripts/install.ps1`;

/** The targets each release is built for. Anything else has no ready made download. */
export const RELEASED_TARGETS = [
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'x86_64-unknown-linux-gnu',
  'x86_64-pc-windows-msvc',
] as const;

export type UpdateStep = {
  text: string;
  /** Something to run, shown with a copy button. */
  command?: string;
};

export type UpdatePlan = {
  steps: UpdateStep[];
  /** The archive for this machine, when one is published. */
  download: { name: string; url: string } | null;
  note: string | null;
};

/** The release archive for a target, as the release workflow names it. */
export function archiveFor(tag: string, target: string): { name: string; url: string } | null {
  if (!(RELEASED_TARGETS as readonly string[]).includes(target)) {
    return null;
  }
  const name = `on-air-record-${tag}-${target}.${target.includes('windows') ? 'zip' : 'tar.gz'}`;
  return { name, url: `https://github.com/${REPOSITORY}/releases/download/${tag}/${name}` };
}

/** Quote a path for a POSIX shell: single quotes, with any single quote closed, escaped and reopened. */
export function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/** Quote a path for PowerShell: single quotes, with any single quote doubled. */
export function powershellQuote(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

export function updatePlan(status: UpdateStatus, release: ReleaseInfo): UpdatePlan {
  const { install } = status;
  const download = archiveFor(release.tag, install.target);
  const windows = install.os === 'windows';

  if (install.kind === 'installer' && install.dir) {
    const dir = install.dir;
    if (windows) {
      return {
        steps: [
          { text: 'Stop it:', command: `& ${powershellQuote(`${dir}\\stop.cmd`)}` },
          {
            text: 'Run the installer again in PowerShell. It downloads the new version, checks it, keeps your settings and recordings, and starts it again:',
            command: `& ([scriptblock]::Create((irm ${INSTALL_PS1}))) -Update -Dir ${powershellQuote(dir)}`,
          },
        ],
        download,
        note: null,
      };
    }
    return {
      steps: [
        { text: 'Stop it:', command: shellQuote(`${dir}/stop.sh`) },
        {
          text: 'Run the installer again. It downloads the new version, checks it, keeps your settings and recordings, and starts it again:',
          command: `curl -fsSL ${INSTALL_SH} | sh -s -- --update --dir ${shellQuote(dir)}`,
        },
      ],
      download,
      note: 'Run both as the account that owns the folder.',
    };
  }

  if (install.kind === 'systemd' && download) {
    const folder = download.name.replace(/\.tar\.gz$/, '');
    return {
      steps: [
        {
          text: 'On the host, download the new version, check it, install it and restart the service:',
          command: [
            'cd /tmp',
            `curl -fLO ${download.url}`,
            `curl -fLO ${download.url}.sha256`,
            `sha256sum -c ${download.name}.sha256`,
            `tar -xzf ${download.name}`,
            `sudo install -m 755 ${folder}/on-air-record /usr/local/bin/on-air-record`,
            'sudo systemctl restart on-air-record',
          ].join('\n'),
        },
      ],
      download,
      note: 'This is the layout from the setup guide. If the program lives somewhere other than /usr/local/bin, install it there instead.',
    };
  }

  return {
    steps: download
      ? [
          { text: `Download ${download.name} from the release page.` },
          { text: 'Stop the service, replace the program with the one in the download, and start it again.' },
        ]
      : [{ text: 'Get the new version from the release page.' }],
    download,
    note: download
      ? null
      : `There is no ready made download for this machine (${install.target}), so it needs building from source.`,
  };
}
