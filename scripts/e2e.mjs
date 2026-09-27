#!/usr/bin/env node
// End to end: the real service, driven through real headless Chrome, in two scenarios. One runs on
// seeded recordings, for the sounds, bookmarks, export, the day picker and settings. The other starts
// from a brand new recorder, for the first visit question, signing in and out, and what a listener sees.
//
// The unit tests prove the parts; this proves them joined up, for what jsdom cannot reach: buttons that
// talk to the server and seek, canvases that draw, and a settings card built on a component library that
// only responds to real pointer events. It seeds half an hour of recording with known sounds straight
// into the database (the trick CLAUDE.md describes for exercising the DVR), starts the service on it,
// and checks what a person would see and do.
//
// No dependencies, like the rest of scripts/: Chrome is driven over its DevTools protocol with the
// WebSocket built into Node, and the database is seeded with node:sqlite, so it needs Node 22.13 or newer.
//
//   node scripts/e2e.mjs            (after `make build`, or `npm run build` and `cargo build`)
//
// Every page state it reaches is also audited for accessibility with axe-core, in both themes. axe is
// read from frontend/node_modules as a plain script and injected into the page, so the script still
// imports nothing; `npm ci` in frontend, which the UI build needs anyway, is what provides it.
//
// Settings, all optional: E2E_BINARY (the service, default the debug build), E2E_STATIC_DIR (the built UI),
// E2E_PORT (8199), E2E_ARTIFACTS (where screenshots go), CHROME_PATH.

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const BINARY =
  process.env.E2E_BINARY ??
  join(ROOT, 'backend', 'target', 'debug', `on-air-record${process.platform === 'win32' ? '.exe' : ''}`);
const STATIC_DIR = process.env.E2E_STATIC_DIR ?? join(ROOT, 'frontend', 'dist');
const AXE = join(ROOT, 'frontend', 'node_modules', 'axe-core', 'axe.min.js');
const PORT = Number(process.env.E2E_PORT ?? 8199);
const ARTIFACTS = process.env.E2E_ARTIFACTS ?? join(tmpdir(), 'oar-e2e-artifacts');
const BASE = `http://127.0.0.1:${PORT}`;
const CHROME =
  process.env.CHROME_PATH ??
  [
    '/usr/bin/google-chrome',
    '/usr/bin/google-chrome-stable',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
  ].find((path) => existsSync(path));

// The page and the service both show and file times in local time; pin it so the expected labels are
// the same on every machine.
const ENV = { ...process.env, TZ: 'UTC' };

let axeSource = null;

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok });
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? `: ${detail}` : ''}`);
}
function clock(ms) {
  return new Date(ms).toISOString().slice(11, 19);
}

async function main() {
  for (const [what, path] of [
    ['the service binary', BINARY],
    ['the built UI', join(STATIC_DIR, 'index.html')],
  ]) {
    if (!existsSync(path)) {
      throw new Error(`${what} is missing at ${path}. Build it first: make build, or npm run build and cargo build.`);
    }
  }
  if (!CHROME) {
    throw new Error('No Chrome or Chromium found. Set CHROME_PATH.');
  }
  if (!existsSync(AXE)) {
    throw new Error(`axe-core is missing at ${AXE}. Install the UI's dependencies first: npm ci in frontend.`);
  }
  axeSource = readFileSync(AXE, 'utf8');
  let DatabaseSync;
  try {
    ({ DatabaseSync } = await import('node:sqlite'));
  } catch {
    throw new Error(`node:sqlite is not available in Node ${process.version}; use Node 22.13 or newer.`);
  }
  mkdirSync(ARTIFACTS, { recursive: true });

  console.log('-- recordings: sounds, bookmarks, export, the day picker and settings');
  await scenario(DatabaseSync, { seedRecordings: true }, async (plan) => {
    await request('POST', '/api/auth/open');
    await checkApi(plan);
    await withChrome(async (page) => {
      await checkTimeline(page, plan);
      await checkBookmarks(page);
      await checkExport(page);
      await checkDayPicker(page, plan);
      await checkSettings(page);
    });
  });

  console.log('-- a brand new recorder: the first visit, signing in, and a listener');
  await scenario(DatabaseSync, { seedRecordings: false }, async () => {
    await withChrome(async (page) => {
      await checkFirstRunAndSignIn(page);
    });
  });
}

/** A fresh data directory: created by the service itself, seeded while it is stopped, then served. */
async function scenario(DatabaseSync, { seedRecordings }, body) {
  const dataDir = mkdtempSync(join(tmpdir(), 'oar-e2e-'));
  try {
    // Once to let the service create its database and run its migrations, then stopped to seed it.
    await withService(dataDir, async () => {});
    const plan = seed(DatabaseSync, join(dataDir, 'on-air-record.sqlite'), seedRecordings);
    await withService(dataDir, () => body(plan));
  } finally {
    rmSync(dataDir, { recursive: true, force: true });
  }
}

// ---- seeding --------------------------------------------------------------------------------------

/**
 * Half an hour of 10 second segments over a background that wavers between 1 and 3, as real ones do,
 * with a clap, a click that must be ignored, speech with pauses, and a door. The sounds sit at a minute
 * and a half past a round ten minutes, where no gridline or hour tick can fall on their marks.
 */
function seed(DatabaseSync, path, withRecordings) {
  const db = new DatabaseSync(path);
  const setting = db.prepare('INSERT OR REPLACE INTO settings (key, value, updated_at_ms) VALUES (?, ?, ?)');
  // CI has no microphone, and a developer's should stay out of it.
  setting.run('auto_start', 'false', Date.now());
  // Three days, or the janitor prunes yesterday's recording within a minute of starting: the default
  // window is a day, and yesterday's sits just past it.
  setting.run('retention_hours', '72', Date.now());
  if (!withRecordings) {
    db.close();
    return null;
  }
  const tenMinutes = 600_000;
  const start = Math.floor((Date.now() - 45 * 60_000) / tenMinutes) * tenMinutes;
  const end = start + 30 * 60_000;
  const plan = {
    start,
    end,
    clap: start + 2 * 60_000 + 30_000,
    click: start + 7 * 60_000 + 30_000,
    speech: start + 12 * 60_000 + 30_000,
    door: start + 22 * 60_000 + 30_000,
  };

  let state = 7;
  const wavering = () => {
    state = (state * 1103515245 + 12345) & 0x7fffffff;
    return [1, 2, 2, 3][state % 4];
  };
  const levels = Array.from({ length: (end - start) / 100 }, wavering);
  const put = (at, ms, level) => {
    for (let slot = (at - start) / 100; slot < (at - start + ms) / 100; slot += 1) {
      levels[slot] = level;
    }
  };
  put(plan.clap, 3000, 60);
  put(plan.click, 200, 150);
  for (let burst = 0; burst < 4; burst += 1) {
    put(plan.speech + burst * 2000, 1200, 30);
  }
  put(plan.door, 1000, 120);

  db.prepare(
    'INSERT INTO sessions (device_id, device_name, sample_rate, channels, started_at_ms, ended_at_ms) VALUES (?, ?, 48000, 1, ?, ?)',
  ).run('e2e', 'seeded recording', start, end);
  const sessionId = db.prepare('SELECT last_insert_rowid() AS id').get().id;
  const insert = db.prepare(
    'INSERT INTO segments (session_id, sequence, path, started_at_ms, ended_at_ms, sample_rate, channels, byte_len, peaks, day) VALUES (?, ?, ?, ?, ?, 48000, 1, ?, ?, ?)',
  );
  for (let sequence = 0, at = start; at < end; sequence += 1, at += 10_000) {
    const day = new Date(at).toISOString().slice(0, 10);
    const slice = levels.slice((at - start) / 100, (at - start) / 100 + 100);
    insert.run(
      sessionId,
      sequence,
      `recordings/${day}/${sessionId}/${String(sequence).padStart(6, '0')}.pcm`,
      at,
      at + 10_000,
      10_000 * 96,
      Uint8Array.from(slice),
      day,
    );
  }
  // Ten flat minutes the day before, so the day picker has a second day to go to. Flat, so previous
  // sound from today's first still finds nothing earlier.
  const yesterday = start - 24 * 3_600_000;
  plan.yesterday = yesterday;
  for (let offset = 0; offset < 600_000; offset += 10_000) {
    const at = yesterday + offset;
    const day = new Date(at).toISOString().slice(0, 10);
    insert.run(
      sessionId,
      100_000 + offset / 10_000,
      `recordings/${day}/${sessionId}/y${String(offset / 10_000).padStart(5, '0')}.pcm`,
      at,
      at + 10_000,
      10_000 * 96,
      Uint8Array.from({ length: 100 }, () => 2),
      day,
    );
  }
  db.close();
  return plan;
}

// ---- the service ----------------------------------------------------------------------------------

async function withService(dataDir, body) {
  const log = [];
  const service = spawn(BINARY, [], {
    env: { ...ENV, OAR_PORT: String(PORT), OAR_DATA_DIR: dataDir, OAR_STATIC_DIR: STATIC_DIR },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  service.stdout.on('data', (chunk) => log.push(chunk.toString()));
  service.stderr.on('data', (chunk) => log.push(chunk.toString()));
  const exited = new Promise((done) => service.on('exit', done));
  try {
    let up = false;
    for (let attempt = 0; attempt < 120 && !up; attempt += 1) {
      await sleep(250);
      up = await fetch(`${BASE}/api/health`).then((reply) => reply.ok, () => false);
    }
    if (!up) {
      throw new Error(`the service never answered on ${BASE}:\n${log.join('')}`);
    }
    await body();
  } finally {
    service.kill('SIGTERM');
    await exited;
  }
}

async function request(method, path, body, cookie) {
  const reply = await fetch(`${BASE}${path}`, {
    method,
    // The guard refuses a state change whose Origin is not the page's own, as a browser would send.
    headers: {
      Origin: BASE,
      'Content-Type': 'application/json',
      ...(cookie ? { Cookie: cookie } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!reply.ok) {
    throw new Error(`${method} ${path} answered ${reply.status}: ${await reply.text()}`);
  }
  return reply.headers.get('content-type')?.includes('json') ? reply.json() : null;
}

/** Sign in over the API and return the session cookie, for setting up accounts a test then uses. */
async function sessionFor(email, password) {
  const reply = await fetch(`${BASE}/api/auth/login`, {
    method: 'POST',
    headers: { Origin: BASE, 'Content-Type': 'application/json' },
    body: JSON.stringify({ email, password }),
  });
  if (!reply.ok) {
    throw new Error(`signing in as ${email} answered ${reply.status}`);
  }
  const cookie = reply.headers.get('set-cookie')?.split(';')[0];
  if (!cookie) {
    throw new Error(`signing in as ${email} set no cookie`);
  }
  return cookie;
}

async function checkApi(plan) {
  const { sounds } = await request('GET', `/api/timeline/sounds?fromMs=${plan.start}&toMs=${plan.end}`);
  const starts = sounds.map((sound) => sound.startMs);
  check(
    'the service finds the clap, the speech and the door, and not the click',
    JSON.stringify(starts) === JSON.stringify([plan.clap, plan.speech, plan.door]),
    starts.map(clock).join(', '),
  );
}

// ---- Chrome ---------------------------------------------------------------------------------------

async function withChrome(body) {
  const profile = mkdtempSync(join(tmpdir(), 'oar-e2e-chrome-'));
  const debugPort = PORT + 1;
  const chrome = spawn(
    CHROME,
    [
      '--headless=new',
      '--no-sandbox',
      '--disable-gpu',
      '--hide-scrollbars',
      '--mute-audio',
      // Dates and numbers on the page are formatted for the browser's language; pin it so checks agree.
      '--lang=en-US',
      `--remote-debugging-port=${debugPort}`,
      `--user-data-dir=${profile}`,
      'about:blank',
    ],
    { env: ENV, stdio: 'ignore' },
  );
  const exited = new Promise((done) => chrome.on('exit', done));
  try {
    let target;
    for (let attempt = 0; attempt < 100 && !target; attempt += 1) {
      await sleep(150);
      target = await fetch(`http://127.0.0.1:${debugPort}/json/list`)
        .then((reply) => reply.json())
        .then((targets) => targets.find((item) => item.type === 'page'), () => undefined);
    }
    if (!target) {
      throw new Error('Chrome did not open a page to drive');
    }
    const page = await connect(target.webSocketDebuggerUrl);
    try {
      await body(page);
    } finally {
      page.close();
    }
  } finally {
    chrome.kill();
    // Chrome goes on writing its cache for a moment after being told to stop; deleting its profile
    // before it has exited fails with a folder that is not empty.
    await exited;
    rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
}

async function connect(url) {
  const socket = new WebSocket(url);
  await new Promise((done, fail) => {
    socket.addEventListener('open', done, { once: true });
    socket.addEventListener('error', fail, { once: true });
  });
  let nextId = 0;
  const pending = new Map();
  socket.addEventListener('message', (event) => {
    const message = JSON.parse(event.data);
    if (message.id && pending.has(message.id)) {
      pending.get(message.id)(message);
      pending.delete(message.id);
    }
  });
  const send = (method, params = {}) =>
    new Promise((done) => {
      nextId += 1;
      pending.set(nextId, done);
      socket.send(JSON.stringify({ id: nextId, method, params }));
    });
  const run = async (expression) => {
    const reply = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    return reply.result?.result?.value;
  };

  await send('Page.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });

  return {
    run,
    close: () => socket.close(),
    async open(path) {
      await send('Page.navigate', { url: `${BASE}${path}` });
      await sleep(3000);
    },
    async shot(name) {
      const reply = await send('Page.captureScreenshot', { format: 'png' });
      writeFileSync(join(ARTIFACTS, `${name}.png`), Buffer.from(reply.result.data, 'base64'));
    },
    /** Focus a field with a real click and type into it, as the keyboard would. */
    async type(selector, text) {
      const found = await this.click(selector);
      if (found) {
        await send('Input.insertText', { text });
      }
      return found;
    },
    /** Poll until an expression is truthy, or give up after `ms`. */
    async waitFor(expression, ms = 5000) {
      const until = Date.now() + ms;
      while (Date.now() < until) {
        if (await run(expression)) {
          return true;
        }
        await sleep(200);
      }
      return false;
    },
    async downloadsTo(folder) {
      await send('Page.setDownloadBehavior', { behavior: 'allow', downloadPath: folder });
    },
    /** A real mouse click at the element's centre: the settings dropdown ignores synthetic clicks. */
    async click(selector) {
      const point = await run(`(() => {
        const element = ${selector};
        if (!element) { return null; }
        element.scrollIntoView({ block: 'center' });
        const box = element.getBoundingClientRect();
        return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
      })()`);
      if (!point) {
        return false;
      }
      for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) {
        await send('Input.dispatchMouseEvent', { type, x: point.x, y: point.y, button: 'left', clickCount: 1 });
      }
      return true;
    },
  };
}

const button = (text) => `[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === ${JSON.stringify(text)})`;
const buttonContaining = (text) =>
  `[...document.querySelectorAll('button')].find((b) => b.textContent.includes(${JSON.stringify(text)}))`;
const labelled = (label) => `document.querySelector('[aria-label=${JSON.stringify(label)}]')`;
/** The field a visible label names, the way a person finds it. */
const field = (label) =>
  `(() => { const tag = [...document.querySelectorAll('label')].find((l) => l.textContent.trim() === ${JSON.stringify(label)}); return tag ? document.getElementById(tag.htmlFor) : null; })()`;
const hasText = (text) => `document.body.innerText.includes(${JSON.stringify(text)})`;

/** What the transport bar says: the cued or playing time, and any note beside the controls. */
async function transport(page) {
  const text = await page.run(
    `(${labelled('Next sound')}).closest('div').innerText.replace(/\\s+/g, ' ').trim()`,
  );
  return { text, time: text.match(/\d\d:\d\d:\d\d/)?.[0] ?? null };
}

async function press(page, label) {
  await page.click(labelled(label));
  await sleep(1500);
  return transport(page);
}

// ---- accessibility --------------------------------------------------------------------------------

// The WCAG 2.1 A and AA rules. Best practices are left out: they are advice, and a check that fails on
// advice gets ignored.
const AXE_TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'];

/**
 * Nodes axe is wrong about, each with the reason, as `{ rule, selector, why }`. Keep it narrow: a rule
 * and a selector for the one element, never a rule turned off for the whole page. Fix the component
 * instead wherever it can be. Empty, because everything axe has found so far was real.
 */
const AXE_EXCLUSIONS = [];

/**
 * Run axe on the page as it stands, once in each theme, and record a check for each. The theme is the
 * `dark` class useTheme puts on the root element, so it is flipped in place and put back: reloading to
 * change it would close the dialog or menu that is the state being audited.
 */
async function audit(page, name) {
  const loaded = await page.run(`typeof window.axe !== 'undefined'`);
  if (!loaded) {
    await page.run(`${axeSource}\n;true`);
  }
  const original = await page.run(`document.documentElement.classList.contains('dark') ? 'dark' : 'light'`);
  for (const theme of [original, original === 'dark' ? 'light' : 'dark']) {
    if (theme !== original) {
      await setTheme(page, theme);
    }
    const report = await page.run(`(async () => {
      const exclusions = ${JSON.stringify(AXE_EXCLUSIONS)};
      const excluded = (rule, target) => {
        const element = typeof target[0] === 'string' ? document.querySelector(target[0]) : null;
        return exclusions.some((entry) => entry.rule === rule && element && element.matches(entry.selector));
      };
      const { passes, violations } = await axe.run(document, { runOnly: { type: 'tag', values: ${JSON.stringify(AXE_TAGS)} } });
      const broken = violations
        .map((violation) => ({
          id: violation.id,
          impact: violation.impact,
          help: violation.help,
          url: violation.helpUrl,
          nodes: violation.nodes.filter((node) => !excluded(violation.id, node.target)).map((node) => ({
            target: node.target.flat().join(' '),
            summary: (node.failureSummary ?? '').split('\\n').slice(1).join(' ').replace(/\\s+/g, ' ').trim(),
          })),
        }))
        .filter((violation) => violation.nodes.length > 0);
      return { passes: passes.length, violations: broken };
    })()`);
    if (theme !== original) {
      await setTheme(page, original);
    }
    const label = `accessibility: ${name}, ${theme} theme`;
    if (!Array.isArray(report?.violations)) {
      check(label, false, 'axe did not run');
      continue;
    }
    const { violations } = report;
    check(label, violations.length === 0, `${report.passes} rules pass${violations.length === 0 ? '' : `, ${violations.length} broken`}`);
    // Enough to fix it from a CI log: the rule, where, and axe's own account of what is wrong.
    for (const violation of violations) {
      const count = violation.nodes.length;
      console.log(`       ${violation.id} (${violation.impact}): ${violation.help}, on ${count} element${count === 1 ? '' : 's'}, ${violation.url}`);
      for (const node of violation.nodes.slice(0, 5)) {
        console.log(`         ${node.target}`);
        console.log(`           ${node.summary}`);
      }
    }
  }
}

/**
 * Put a theme on the page the way useTheme does. Colour transitions are held off while it changes, so
 * axe measures the finished colours at once instead of waiting for a fade, and the page is not left
 * fading back afterwards.
 */
async function setTheme(page, theme) {
  await page.run(`(() => {
    const still = document.createElement('style');
    still.textContent = '*, *::before, *::after { transition: none !important; }';
    document.head.append(still);
    document.documentElement.classList.toggle('dark', ${JSON.stringify(theme === 'dark')});
    document.documentElement.style.colorScheme = ${JSON.stringify(theme)};
    void document.documentElement.offsetHeight;
    still.remove();
    return true;
  })()`);
}

async function checkTimeline(page, plan) {
  await page.open('/');
  check('the control room has the previous and next sound buttons', Boolean(
    (await page.run(`Boolean(${labelled('Previous sound')}) && Boolean(${labelled('Next sound')})`)),
  ));

  // An hour at a time covers the whole recording; stop following live so the view holds still.
  await page.click(button('1h'));
  await sleep(800);
  // Zooming holds the window's centre, and only the next range poll, every two seconds, pulls a followed
  // window back to the live edge; freezing the view before it comes left the clap and the speech off the
  // left of the hour. Following live again anchors it at once, so it is switched off, on, and off.
  await page.click(buttonContaining('Following live'));
  await sleep(300);
  await page.click(buttonContaining('Follow live'));
  await sleep(300);
  await page.click(buttonContaining('Following live'));
  await sleep(2000);
  await page.shot('timeline');
  await audit(page, 'the control room on seeded recordings');

  // The teal marks, read back from the canvases: the strip along the bottom of the timeline and of the
  // day overview. Teal is told apart by hue, since the overview's window box tints what lies under it.
  const marksOn = (label) => page.run(`(() => {
    const canvas = document.querySelector('canvas[aria-label=${JSON.stringify(label)}]');
    if (!canvas) { return null; }
    const context = canvas.getContext('2d');
    const ratio = window.devicePixelRatio || 1;
    const row = context.getImageData(0, canvas.height - Math.round(2 * ratio), canvas.width, 1).data;
    let clusters = 0;
    let inside = false;
    for (let x = 0; x < canvas.width; x += 1) {
      const [r, g, b] = [row[x * 4], row[x * 4 + 1], row[x * 4 + 2]];
      const teal = g - r > 40 && b - r > 40;
      if (teal && !inside) { clusters += 1; }
      inside = teal;
    }
    return clusters;
  })()`);
  const timeline = { clusters: await marksOn('Timeline') };
  const overview = { clusters: await marksOn('Day overview') };
  check('the timeline marks three sounds, and nothing for the click', timeline?.clusters === 3, `${timeline?.clusters} marks`);
  check('the day overview marks the same three', overview?.clusters === 3, `${overview?.clusters} marks`);

  // Browsing without playing: next goes from the left edge of the timeline, through every sound.
  const expected = [plan.clap, plan.speech, plan.door].map((start) => clock(start - 1000));
  for (const [index, name] of ['clap', 'speech', 'door'].entries()) {
    const state = await press(page, 'Next sound');
    check(`next sound cues the ${name} a second early`, state.time === expected[index], state.text);
    if (index === 0) {
      await page.shot('after-next');
    }
  }
  const pastEnd = await press(page, 'Next sound');
  check('next sound past the last one says there is no later sound', pastEnd.text.includes('No later sound'), pastEnd.text);

  // And back: straight after jumping to the door, previous goes to the speech rather than restarting it.
  const toSpeech = await press(page, 'Previous sound');
  check('previous sound goes back to the speech', toSpeech.time === expected[1], toSpeech.text);
  const toClap = await press(page, 'Previous sound');
  check('previous sound goes back to the clap', toClap.time === expected[0], toClap.text);
  const pastStart = await press(page, 'Previous sound');
  check('previous sound before the first says there is no earlier sound', pastStart.text.includes('No earlier sound'), pastStart.text);
}

async function checkSettings(page) {
  await page.open('/settings');
  const shown = () =>
    page.run(`document.querySelector('#sound-sensitivity')?.textContent.trim() ?? null`);
  check('the settings page has the sound detection choice, at Medium', (await shown()) === 'Medium', String(await shown()));

  await page.click(`document.querySelector('#sound-sensitivity')`);
  await sleep(500);
  await page.click(`[...document.querySelectorAll('[role="option"]')].find((o) => o.textContent.trim().startsWith('High'))`);
  await sleep(500);
  await page.click(buttonContaining('Save changes'));
  await sleep(1500);
  await page.shot('settings');
  await audit(page, 'the settings page');
  const stored = (await request('GET', '/api/settings')).soundSensitivity;
  check('choosing High and saving stores it', stored === 'high', stored);

  await page.open('/settings');
  check('the page shows High after a reload', (await shown()) === 'High', String(await shown()));
}

async function checkBookmarks(page) {
  // The previous checks leave the clap cued. A bookmark is added where the cue is.
  const cued = (await transport(page)).time;
  await page.click(labelled('Add a bookmark here'));
  await sleep(500);
  await page.type(`document.querySelector('input[placeholder="What happened here?"]')`, 'Clap');
  await page.click(button('Save'));
  await sleep(1500);
  const { bookmarks } = await request('GET', '/api/bookmarks');
  check('a bookmark is stored where the cue was, with its label', bookmarks.length === 1 && bookmarks[0].label === 'Clap' && clock(bookmarks[0].timestampMs) === cued, JSON.stringify(bookmarks));
  check('the bookmark count beside the timeline shows it', Boolean(await page.run(`Boolean(${button('1')})`)));

  await page.click(button('1'));
  await sleep(500);
  check('the bookmark list shows it', await page.run(hasText('Clap')));
  await page.shot('bookmarks');
  await audit(page, 'the bookmark list');
  await page.click(labelled('Remove Clap'));
  await sleep(1500);
  const after = await request('GET', '/api/bookmarks');
  check('removing it from the list deletes it', after.bookmarks.length === 0, JSON.stringify(after.bookmarks));
  await page.run(`document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))`);
  await sleep(300);
}

async function checkExport(page) {
  const folder = mkdtempSync(join(tmpdir(), 'oar-e2e-downloads-'));
  try {
    await page.downloadsTo(folder);
    await page.click(labelled('Export audio'));
    await sleep(1500);
    check('the export panel shows the length and the file size', (await page.run(hasText('Export as WAV'))) && (await page.run(hasText('File size'))));
    await page.shot('export');
    await audit(page, 'the export dialog');
    const href = await page.run(`document.querySelector('a[download][href*="/api/export"]')?.getAttribute('href') ?? null`);
    check('the export panel offers a download of the chosen range', typeof href === 'string', String(href));
    if (typeof href !== 'string') {
      return;
    }
    const query = href.slice(href.indexOf('?'));
    const plan = await request('GET', `/api/export/plan${query}`);
    await page.click(`document.querySelector('a[download][href*="/api/export"]')`);
    let saved = null;
    for (let attempt = 0; attempt < 50 && !saved; attempt += 1) {
      await sleep(200);
      const finished = readdirSync(folder).filter((name) => name.endsWith('.wav'));
      saved = finished[0] ?? null;
    }
    check('the download is saved as a WAV file', saved !== null, String(saved));
    if (saved !== null) {
      const size = statSync(join(folder, saved)).size;
      check('the saved file is exactly as long as the plan said', size === plan.totalBytes, `${size} bytes, plan ${plan.totalBytes}`);
    }
  } finally {
    // Chrome can still be tidying up after a download it reported finished, as with its profile below.
    rmSync(folder, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
}

async function checkDayPicker(page, plan) {
  await page.click(labelled('Choose a recorded day'));
  await sleep(800);
  await audit(page, 'the day picker');
  const yesterday = new Date(plan.yesterday);
  const today = new Date(plan.start);
  if (yesterday.getUTCMonth() !== today.getUTCMonth()) {
    await page.click(`[...document.querySelectorAll('button')].find((b) => /previous/i.test(b.getAttribute('aria-label') ?? ''))`);
    await sleep(500);
  }
  const dayNumber = String(yesterday.getUTCDate());
  const picked = await page.click(
    `[...document.querySelectorAll('[role="grid"] button')].find((b) => b.textContent.trim() === ${JSON.stringify(dayNumber)} && !b.disabled)`,
  );
  check('yesterday can be picked in the calendar', picked);
  await sleep(1500);
  await page.shot('day-picker');
  // The day overview is captioned with the day it shows.
  const caption = yesterday.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric', timeZone: 'UTC' });
  check('the timeline moves to the day picked', await page.run(hasText(caption)), caption);
}

async function checkFirstRunAndSignIn(page) {
  await page.open('/');
  check('a new recorder asks whether to protect it with a login', await page.run(hasText('Protect this recorder with a login?')));
  await page.shot('first-run');
  await audit(page, 'the first run question');

  await page.click(buttonContaining('Set up accounts'));
  await sleep(500);
  check('choosing accounts asks for the admin account', await page.run(hasText('Create the admin account')));
  await audit(page, 'the admin account form');
  await page.type(field('Email'), 'owner@example.com');
  await page.type(field('Password'), 'a long password');
  await page.type(field('Password again'), 'a long password');
  await page.click(buttonContaining('Create and sign in'));
  const signedIn = await page.waitFor(`Boolean(${labelled('Account: owner@example.com')})`);
  check('creating the admin signs them straight in', signedIn);
  const state = await request('GET', '/api/auth/state', undefined, await sessionFor('owner@example.com', 'a long password'));
  check('the recorder now asks for a login', state.mode === 'accounts', state.mode);

  await page.click(labelled('Account: owner@example.com'));
  await sleep(500);
  await page.click(buttonContaining('Sign out'));
  const loginShown = await page.waitFor(hasText('Sign in') + ` && Boolean(${field('Email')})`);
  check('signing out goes back to the sign in page', loginShown);
  await audit(page, 'the sign in page');

  await page.type(field('Email'), 'owner@example.com');
  await page.type(field('Password'), 'not the password');
  await page.click(button('Sign in'));
  await sleep(1500);
  check('a wrong password is refused with a message', Boolean(await page.run(`Boolean(document.querySelector('[role="alert"]'))`)));
  check('and nothing is opened', !(await page.run(`Boolean(${labelled('Account: owner@example.com')})`)));
  await audit(page, 'the sign in page with a wrong password');

  await page.open('/');
  await page.type(field('Email'), 'owner@example.com');
  await page.type(field('Password'), 'a long password');
  await page.click(button('Sign in'));
  check('the right password signs in', await page.waitFor(`Boolean(${labelled('Account: owner@example.com')})`));
  check('an admin sees the settings link', await page.run(`[...document.querySelectorAll('a')].some((a) => a.textContent.trim() === 'Settings')`));
  await page.click(labelled('Account: owner@example.com'));
  await sleep(500);
  await audit(page, 'the account menu');
  await page.click(`[...document.querySelectorAll('a')].find((a) => a.textContent.trim() === 'Account settings')`);
  const accountShown = await page.waitFor(`location.pathname === '/account' && ${hasText('Change password')}`);
  check('the account menu leads to the account page', accountShown, await page.run('location.pathname'));
  await audit(page, 'the account page');

  // A listener, added by the admin, signs in and is shown only what a listener may use.
  const admin = await sessionFor('owner@example.com', 'a long password');
  await request('POST', '/api/users', { email: 'kitchen@example.com', password: 'listen only', role: 'listener' }, admin);
  await page.click(labelled('Account: owner@example.com'));
  await sleep(500);
  await page.click(buttonContaining('Sign out'));
  await page.waitFor(`Boolean(${field('Email')})`);
  await page.type(field('Email'), 'kitchen@example.com');
  await page.type(field('Password'), 'listen only');
  await page.click(button('Sign in'));
  check('a listener signs in', await page.waitFor(`Boolean(${labelled('Account: kitchen@example.com')})`));
  await sleep(1000);
  await page.shot('listener');
  await audit(page, 'the control room as a listener');
  check('a listener sees no settings link', !(await page.run(`[...document.querySelectorAll('a')].some((a) => a.textContent.trim() === 'Settings')`)));
  check('a listener can listen and move through the recording', await page.run(`Boolean(${labelled('Next sound')}) && Boolean(${labelled('Previous sound')})`));
  check('a listener cannot add bookmarks', !(await page.run(`Boolean(${labelled('Add a bookmark here')})`)));
  await page.open('/settings');
  check('a listener who types the settings address is sent back', !(await page.run(`location.pathname.startsWith('/settings')`)), await page.run('location.pathname'));
}

main().then(
  () => {
    const failed = results.filter((result) => !result.ok);
    console.log(`\n${results.length - failed.length} of ${results.length} checks passed. Screenshots: ${ARTIFACTS}`);
    process.exit(failed.length === 0 ? 0 : 1);
  },
  (error) => {
    console.error(`\nThe end to end run could not finish: ${error.message}`);
    process.exit(1);
  },
);
