#!/usr/bin/env node
/**
 * Turn the guides in docs/ into GitHub wiki pages.
 *
 * A GitHub wiki is a **separate git repository**, `<repo>.wiki.git`. Nothing in `docs/` reaches it by
 * being merged to the default branch, which is why the guides render in the repository and the wiki stays
 * empty. This script produces the wiki repository's contents so a workflow can commit them.
 *
 * Three things have to change on the way across:
 *
 * - **Page names.** A wiki page is named by its filename, so `USER_GUIDE.md` becomes `User-Guide.md` and
 *   is addressed as `/wiki/User-Guide`.
 * - **Links between documents.** `[the user guide](USER_GUIDE.md)` is a valid relative link inside `docs/`
 *   and a dead one in the wiki. Links to pages that cross over become wiki links; links to anything that
 *   stays behind, such as DEVELOPMENT.md or the systemd unit, become absolute URLs into the repository.
 * - **The leading heading.** The wiki prints the page name above the body, so a `# Title` in the source
 *   would appear twice.
 *
 * Images already use absolute raw URLs and need no rewriting, which is why they were written that way.
 *
 * Usage: node scripts/build-wiki.mjs <output directory>
 */

import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { posix } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));

const REPOSITORY = 'shibbirweb/on-air-record';
const BRANCH = 'master';
const BLOB = `https://github.com/${REPOSITORY}/blob/${BRANCH}`;

/** Source path in the repository, mapped to the wiki page name it becomes. */
const PAGES = {
  'docs/SETUP.md': 'Installation-and-Setup',
  'docs/USER_GUIDE.md': 'User-Guide',
};

/** How each page is introduced on the Home page and in the sidebar, in the order shown. */
const DESCRIPTIONS = [
  [
    'Installation-and-Setup',
    'Downloading and running it on macOS, Linux and Windows, choosing a port, and keeping it running as a service.',
  ],
  [
    'User-Guide',
    'How to use every part of the interface, with screenshots. Written for people who just want to listen.',
  ],
];

const GENERATED_NOTE =
  `<!-- Generated from docs/ in ${REPOSITORY}. Do not edit this page in the wiki: the next push to ` +
  `${BRANCH} will overwrite it. Edit the source file instead. -->`;

/** A markdown link or image target, captured as (target, anchor). */
const LINK = /\]\(([^)\s#]+)(#[^)\s]*)?\)/g;

const LEADING_COMMENT = /^\s*<!--[\s\S]*?-->\s*/;
const LEADING_H1 = /^#\s+.*?\n+/;

/** Point one link at wherever its destination ended up. */
function rewriteLink(sourcePath, target, anchor) {
  if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('//')) {
    return `](${target}${anchor})`;
  }

  const resolved = posix.normalize(posix.join(posix.dirname(sourcePath), target));

  if (Object.hasOwn(PAGES, resolved)) {
    return `](${PAGES[resolved]}${anchor})`;
  }

  // Everything else stays in the repository, so it needs an absolute URL to survive the move.
  return `](${BLOB}/${resolved}${anchor})`;
}

function convert(sourcePath, text) {
  let body = text.replace(LEADING_COMMENT, '');
  body = body.replace(LEADING_H1, '');
  body = body.replace(LINK, (_match, target, anchor) => rewriteLink(sourcePath, target, anchor ?? ''));

  return `${GENERATED_NOTE}\n\n${body.trim()}\n`;
}

function home() {
  const lines = [
    GENERATED_NOTE,
    '',
    '# On Air Record',
    '',
    'A cross platform audio broadcast and DVR service. It records a microphone on one machine',
    'continuously, streams it live to any browser on the network, and lets you scrub back to any',
    'moment in the retention window on a CCTV style timeline.',
    '',
    '## Guides',
    '',
  ];
  for (const [page, description] of DESCRIPTIONS) {
    lines.push(`- **[${page.replaceAll('-', ' ')}](${page})** ${description}`);
  }
  lines.push(
    '',
    '## Elsewhere',
    '',
    `- [Source code](https://github.com/${REPOSITORY})`,
    `- [Releases](https://github.com/${REPOSITORY}/releases), with binaries for macOS, Linux and Windows`,
    `- [Report a problem](https://github.com/${REPOSITORY}/issues)`,
    `- [Developer documentation](${BLOB}/docs/DEVELOPMENT.md), for working on the code itself`,
    '',
  );
  return lines.join('\n');
}

function sidebar() {
  const lines = [GENERATED_NOTE, '', '### On Air Record', '', '- [Home](Home)'];
  for (const [page] of DESCRIPTIONS) {
    lines.push(`- [${page.replaceAll('-', ' ')}](${page})`);
  }
  lines.push('', `[Source](${BLOB}) &middot; [Issues](https://github.com/${REPOSITORY}/issues)`, '');
  return lines.join('\n');
}

function footer() {
  return (
    `These pages are generated from \`docs/\` in [the repository](https://github.com/${REPOSITORY}). ` +
    'Corrections are welcome as a pull request against the source file, since an edit made here would ' +
    'be overwritten by the next release.\n'
  );
}

const outDir = process.argv[2];
if (outDir === undefined || process.argv.length > 3) {
  console.error('Usage: node scripts/build-wiki.mjs <output directory>');
  process.exit(2);
}

mkdirSync(outDir, { recursive: true });

const written = [];
for (const [sourcePath, page] of Object.entries(PAGES).sort()) {
  const full = join(ROOT, sourcePath);
  if (!existsSync(full)) {
    console.error(`missing source: ${sourcePath}`);
    process.exit(1);
  }
  writeFileSync(join(outDir, `${page}.md`), convert(sourcePath, readFileSync(full, 'utf8')), 'utf8');
  written.push(`${sourcePath} -> ${page}.md`);
}

for (const [name, content] of [['Home.md', home()], ['_Sidebar.md', sidebar()], ['_Footer.md', footer()]]) {
  writeFileSync(join(outDir, name), content, 'utf8');
  written.push(`generated ${name}`);
}

for (const line of written) {
  console.log(line);
}
