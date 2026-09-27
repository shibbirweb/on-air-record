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
/** The container image the release workflow publishes, and the same image on Docker Hub. */
export const IMAGE = `ghcr.io/${REPOSITORY}`;
export const DOCKER_HUB_IMAGE = 'shibbirweb/on-air-record';

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

/**
 * Quote a path for PowerShell: single quotes, with any single quote doubled.
 *
 * PowerShell's tokenizer treats the typographic single quotes (U+2018 to U+201B) exactly like the ASCII
 * one, so a folder named O'Brien with a typographic apostrophe would otherwise end the string early and
 * break the pasted command. A doubled quote stands for its second character, so each is doubled with
 * itself and the folder name comes through unchanged.
 */
export function powershellQuote(value: string): string {
  return `'${value.replace(/['\u2018\u2019\u201A\u201B]/g, (quote) => quote + quote)}'`;
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

  // No archive to offer: the program inside a container is replaced by pulling the image, never by hand,
  // or it reverts the next time the container is recreated. Nothing inside a container says which
  // registry it came from, so the note names both.
  if (install.kind === 'docker') {
    const tag = status.channel === 'beta' ? 'beta' : 'latest';
    return {
      steps: [
        {
          text: 'In the folder holding your compose.yaml, pull the new image and recreate the container. Recordings, settings and accounts live in the data volume and are kept:',
          command: ['docker compose pull', 'docker compose up -d'].join('\n'),
        },
      ],
      download: null,
      note: `Started with docker run instead? Pull the image you started it from, docker pull ${IMAGE}:${tag} or docker pull ${DOCKER_HUB_IMAGE}:${tag} from Docker Hub, then remove the container and run it again with the same options. If your image line names a version, change it to ${release.version} first.`,
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
