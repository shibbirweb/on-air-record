#!/usr/bin/env node
// End to end: the real service on seeded recordings, driven through real headless Chrome.
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
// Settings, all optional: E2E_BINARY (the service, default the debug build), E2E_STATIC_DIR (the built UI),
// E2E_PORT (8199), E2E_ARTIFACTS (where screenshots go), CHROME_PATH.

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const BINARY =
  process.env.E2E_BINARY ??
  join(ROOT, 'backend', 'target', 'debug', `on-air-record${process.platform === 'win32' ? '.exe' : ''}`);
const STATIC_DIR = process.env.E2E_STATIC_DIR ?? join(ROOT, 'frontend', 'dist');
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
  let DatabaseSync;
  try {
    ({ DatabaseSync } = await import('node:sqlite'));
  } catch {
    throw new Error(`node:sqlite is not available in Node ${process.version}; use Node 22.13 or newer.`);
  }
  mkdirSync(ARTIFACTS, { recursive: true });
  const dataDir = mkdtempSync(join(tmpdir(), 'oar-e2e-'));

  // Once to let the service create its database and run its migrations, then stopped to seed it.
  await withService(dataDir, async () => {});
  const plan = seed(DatabaseSync, join(dataDir, 'on-air-record.sqlite'));

  try {
    await withService(dataDir, async () => {
      await request('POST', '/api/auth/open');
      await checkApi(plan);
      await withChrome(async (page) => {
        await checkTimeline(page, plan);
        await checkSettings(page);
      });
    });
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
function seed(DatabaseSync, path) {
  const db = new DatabaseSync(path);
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
  // CI has no microphone, and a developer's should stay out of it.
  db.prepare('INSERT OR REPLACE INTO settings (key, value, updated_at_ms) VALUES (?, ?, ?)').run(
    'auto_start',
    'false',
    Date.now(),
  );
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

async function request(method, path, body) {
  const reply = await fetch(`${BASE}${path}`, {
    method,
    // The guard refuses a state change whose Origin is not the page's own, as a browser would send.
    headers: { Origin: BASE, 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!reply.ok) {
    throw new Error(`${method} ${path} answered ${reply.status}: ${await reply.text()}`);
  }
  return reply.headers.get('content-type')?.includes('json') ? reply.json() : null;
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
      `--remote-debugging-port=${debugPort}`,
      `--user-data-dir=${profile}`,
      'about:blank',
    ],
    { env: ENV, stdio: 'ignore' },
  );
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
    rmSync(profile, { recursive: true, force: true });
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

async function checkTimeline(page, plan) {
  await page.open('/');
  check('the control room has the previous and next sound buttons', Boolean(
    (await page.run(`Boolean(${labelled('Previous sound')}) && Boolean(${labelled('Next sound')})`)),
  ));

  // An hour at a time covers the whole recording; stop following live so the view holds still.
  await page.click(button('1h'));
  await sleep(800);
  await page.click(buttonContaining('Following live'));
  await sleep(2000);
  await page.shot('timeline');

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
  const stored = (await request('GET', '/api/settings')).soundSensitivity;
  check('choosing High and saving stores it', stored === 'high', stored);

  await page.open('/settings');
  check('the page shows High after a reload', (await shown()) === 'High', String(await shown()));
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
