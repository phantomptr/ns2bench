// End-to-end UI test: the real engine with a synthetic video source, driven in
// headless Chromium. No console or input device needed.
//   cargo build -p ns2bench (in native/), then:
//   npm i --no-save --no-package-lock playwright@1.65.0 && npx playwright install chromium
//   node --test tests/web/
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const exe = join(root, 'native/target/debug', process.platform === 'win32' ? 'ns2bench.exe' : 'ns2bench');
const PORT = 7650 + (process.pid % 200);
const BASE = `http://127.0.0.1:${PORT}`;
const UI_KEY = 'testuikey1234';
let engine, home, browser;

async function waitFor(fn, ms = 20000) {
  const end = Date.now() + ms;
  for (;;) {
    try { const v = await fn(); if (v) return v; } catch { /* not yet */ }
    if (Date.now() > end) throw new Error('timed out');
    await new Promise((r) => setTimeout(r, 200));
  }
}

before(async () => {
  assert.ok(existsSync(exe), `build the engine first: ${exe}`);
  home = mkdtempSync(join(tmpdir(), 'ns2bench-ui-'));
  const cfg = join(home, 'config.json');
  writeFileSync(cfg, JSON.stringify({
    server: { host: '127.0.0.1', port: PORT },
    input: { backend: 'none' }, audio: { backend: 'none' },
    video: { backend: 'ffmpeg', width: 640, height: 360, fps: 15, quality: 70,
      ffmpeg_input: ['-re', '-f', 'lavfi', '-i', 'testsrc2=size=640x360:rate=15'] },
  }));
  engine = spawn(exe, ['serve'], { env: { ...process.env, HOME: home, USERPROFILE: home, NS2BENCH_CONFIG: cfg, NS2BENCH_UI_KEY: UI_KEY }, stdio: 'ignore' });
  await waitFor(async () => (await fetch(`${BASE}/api/status`)).ok);
  browser = await chromium.launch();
});

after(async () => {
  await browser?.close();
  engine?.kill();
  if (home) rmSync(home, { recursive: true, force: true });
});

async function page(opts = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...opts });
  const p = await ctx.newPage();
  const errors = [];
  p.on('pageerror', (e) => errors.push(String(e)));
  p.on('console', (m) => { if (m.type() === 'error' && !/WebSocket|Failed to load resource/.test(m.text())) errors.push(m.text()); });
  // the link `ns2bench serve` prints: this browser is the person at the bench
  await p.goto(BASE + (opts.path || '/') + (opts.noKey ? '' : `${(opts.path || '').includes('?') ? '&' : '?'}ui=${UI_KEY}`));
  // saved choices from an earlier page are applied with one reload: wait until that is over
  await p.waitForFunction(() => window.__ns2benchPrefs === 'ready', null, { timeout: 15000 });
  return { p, errors, ctx };
}

test('page loads without errors and video goes live', async () => {
  const { p, errors, ctx } = await page();
  await p.waitForFunction(() => document.querySelector('#pill-video')?.classList.contains('ok'), null, { timeout: 20000 });
  assert.match(await p.textContent('#pill-video'), /fps/);
  assert.match(await p.textContent('#pill-input'), /not set up/);
  assert.deepEqual(errors, []);
  await ctx.close();
});

test('every settings tab opens; status pills open the right tab', async () => {
  const { p, errors, ctx } = await page();
  await p.click('#btn-settings');
  for (const tab of ['devices', 'network', 'integrations', 'install', 'appearance', 'updates', 'help', 'about']) {
    await p.click(`[data-tab="${tab}"]`);
    assert.equal(await p.isVisible(`[data-pane="${tab}"]`), true, tab);
  }
  assert.match(await p.textContent('#about-version'), /^v\d+\.\d+\.\d+$/);
  await p.click('#settings-close');
  await p.click('#pill-input');
  await p.waitForSelector('[data-pane="devices"]:not([hidden])');
  assert.deepEqual(errors, []);
  await ctx.close();
});

test('toolbar: notifications list, update and reconnect buttons, Thank you link', async () => {
  const { p, errors, ctx } = await page();
  // the engine's start-up notifications (if any) are listed; opening the bell marks them read
  await p.click('#btn-bell');
  assert.equal(await p.isVisible('#inbox'), true);
  await p.keyboard.press('Escape');
  assert.equal(await p.isVisible('#inbox'), false);
  assert.equal(await p.isVisible('#bell-count'), false);
  await p.click('#btn-update');
  await p.waitForSelector('[data-pane="updates"]:not([hidden])');
  await p.click('#settings-close');
  await p.click('#btn-refresh');
  await p.waitForFunction(() => document.querySelector('#pill-video')?.textContent.includes('fps'), null, { timeout: 15000 });
  assert.equal(await p.getAttribute('#btn-thanks', 'href'), 'https://ns2bench.phantomptr.com/#thanks');
  assert.deepEqual(errors, []);
  await ctx.close();
});

test('themes apply and the toolbar toggle remembers light/dark choices', async () => {
  const { p, ctx } = await page({ colorScheme: 'dark' });
  await p.click('#btn-settings');
  await p.click('[data-tab="appearance"]');
  for (const t of ['light', 'peach', 'dark', 'oled']) {
    await p.click(`[data-theme-pick="${t}"]`);
    assert.equal(await p.getAttribute('html', 'data-theme'), t);
    assert.equal(await p.getAttribute(`[data-theme-pick="${t}"]`, 'aria-checked'), 'true');
  }
  await p.click('#settings-close');
  await p.click('#btn-theme');                                        // oled -> last light (peach)
  assert.equal(await p.getAttribute('html', 'data-theme'), 'peach');
  await p.click('#btn-theme');                                        // back to oled
  assert.equal(await p.getAttribute('html', 'data-theme'), 'oled');
  await ctx.close();
});

test('text size and accessibility settings apply and persist', async () => {
  const { p, ctx } = await page();
  await p.click('#btn-settings');
  await p.click('[data-tab="appearance"]');
  await p.click('[data-scale="1.25"]');
  assert.equal(await p.evaluate(() => document.documentElement.style.zoom), '1.25');
  await p.click('[data-motion="none"]');
  assert.equal(await p.getAttribute('html', 'data-motion'), 'none');
  for (const [box, attr, on] of [['#a11y-transparency', 'data-transparency', 'reduced'], ['#a11y-contrast', 'data-contrast', 'high'], ['#a11y-dyslexia', 'data-dyslexia', 'true']]) {
    await p.click(`label:has(${box})`);
    assert.equal(await p.getAttribute('html', attr), on, box);
  }
  await p.reload();
  assert.equal(await p.evaluate(() => document.documentElement.style.zoom), '1.25');
  assert.equal(await p.getAttribute('html', 'data-contrast'), 'high');
  await p.click('#btn-settings'); await p.click('[data-tab="appearance"]');
  await p.click('[data-scale="1"]'); await p.click('[data-motion="auto"]');
  assert.equal(await p.evaluate(() => document.documentElement.style.zoom), '');
  assert.equal(await p.getAttribute('html', 'data-motion'), null);
  await ctx.close();
});

test('languages translate the interface and switch back', async () => {
  const { p, ctx } = await page();
  await p.click('#btn-settings');
  await p.click('[data-tab="appearance"]');
  await p.selectOption('#lang-select', 'de');
  await p.waitForFunction(() => document.documentElement.lang === 'de');
  assert.equal((await p.textContent('#btn-settings')).trim(), 'Einstellungen');
  await p.selectOption('#lang-select', 'ja');
  await p.waitForFunction(() => document.documentElement.lang === 'ja');
  assert.equal((await p.textContent('#settings-title')).trim(), '設定');
  await p.selectOption('#lang-select', 'en');
  await p.waitForFunction(() => document.documentElement.lang === 'en');
  // the choice is saved by the engine before the next test opens a page
  for (let i = 0; (await (await fetch(`${BASE}/api/prefs`)).json())['ns2bench.lang'] !== 'en'; i++) {
    assert.ok(i < 50, 'English was not saved');
    await new Promise((r) => setTimeout(r, 100));
  }
  assert.equal((await p.textContent('#btn-settings')).trim(), 'Settings');
  await ctx.close();
});

test('driving choices are limited to what the input device can do', async () => {
  const { p, ctx } = await page();
  await p.waitForFunction(() => document.querySelector('#pill-input')?.textContent.includes('Bridge'));
  // No Linux board: Pro Controller is unavailable; Controller + keyboard is the default.
  assert.equal(await p.getAttribute('#seg-usb [data-v=standard]', 'aria-checked'), 'true');
  assert.equal(await p.isDisabled('#seg-usb [data-v=pro]'), true);
  await p.click('#seg-kb [data-v=usb]');
  assert.equal(await p.getAttribute('#seg-kb [data-v=usb]', 'aria-checked'), 'true');
  await p.click('#seg-kb [data-v=pad]');
  assert.equal(await p.getAttribute('#seg-kb [data-v=pad]', 'aria-checked'), 'true');
  await ctx.close();
});

test('the fullscreen toolbar has every control', async () => {
  const { p, ctx } = await page();
  const acts = await p.$$eval('#fs-bar [data-fs]', (bs) => bs.map((b) => b.dataset.fs));
  assert.deepEqual(acts, ['pad', 'keys', 'sound', 'shot', 'clip', 'exit']);
  await ctx.close();
});

test('security: headers, cross-site and DNS-rebinding requests', async () => {
  const r = await fetch(`${BASE}/`);
  assert.equal(r.headers.get('x-frame-options'), 'DENY');
  assert.match(r.headers.get('content-security-policy'), /frame-ancestors 'none'/);
  const press = (headers) => fetch(`${BASE}/api/press`, { method: 'POST', headers: { 'content-type': 'application/json', ...headers }, body: '{"buttons":["A"]}' });
  assert.equal((await press({ origin: 'https://evil.example' })).status, 403);
  // Node's fetch sets Host from the URL; a raw request shows the rebinding check.
  const { request } = await import('node:http');
  const status = await new Promise((resolve, reject) => {
    const req = request({ host: '127.0.0.1', port: PORT, path: '/api/status', agent: false,
      headers: { host: `rebind.example:${PORT}`, connection: 'close' } }, (res) => { res.resume(); resolve(res.statusCode); });
    req.on('error', reject);
    req.end();
  });
  assert.equal(status, 403);
  // Scripts can't claim to be the person at the controls.
  const c = await fetch(`${BASE}/api/control`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"manual_only":false}' });
  assert.equal(c.status, 403);
});

test('choices are saved by the engine and come back in a fresh browser', async () => {
  const a = await page({ colorScheme: 'dark' });
  await a.p.click('#btn-settings');
  await a.p.click('[data-tab="appearance"]');
  await a.p.click('[data-theme-pick="peach"]');
  for (let i = 0; ; i++) {
    if ((await (await fetch(`${BASE}/api/prefs`)).json())['ns2bench.theme'] === 'peach') break;
    assert.ok(i < 50, 'the choice was not saved');
    await new Promise((r) => setTimeout(r, 100));
  }
  await a.ctx.close();
  // a different browser profile: nothing stored locally, the saved choice is applied
  const b = await page({ colorScheme: 'dark' });
  await b.p.waitForFunction(() => document.documentElement.dataset.theme === 'peach', null, { timeout: 10000 });
  await b.ctx.close();
  // scripts can't rewrite the page's preferences
  const r = await fetch(`${BASE}/api/prefs`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"ns2bench.theme":"\\"dark\\""}' });
  assert.equal(r.status, 403);
});

test('a page opened without the link from `ns2bench serve` can watch but is not the person at the bench', async () => {
  const { p, ctx } = await page({ noKey: true });
  await p.waitForFunction(() => document.querySelector('#pill-video')?.textContent.includes('fps'), null, { timeout: 15000 });
  // Block agents on is allowed for anyone; turning it off needs the key
  const on = await p.evaluate(async () => (await fetch('/api/control', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"manual_only":true}' })).status);
  assert.equal(on, 200);
  const off = await p.evaluate(async () => (await fetch('/api/control', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"manual_only":false}' })).status);
  assert.equal(off, 403);
  await ctx.close();
  const keyed = await page();
  const ok = await keyed.p.evaluate(async (k) => (await fetch('/api/control', { method: 'POST', headers: { 'content-type': 'application/json', 'X-NS2Bench-UI': k }, body: '{"manual_only":false}' })).status, UI_KEY);
  assert.equal(ok, 200);
  await keyed.ctx.close();
});

test('guidance: shortcuts, action search, confirm before installing, What’s new after an update', async () => {
  const { p, errors, ctx } = await page();
  await p.waitForFunction(() => document.querySelector('#pill-video')?.textContent.includes('fps'), null, { timeout: 15000 });
  assert.equal(await p.isVisible('#no-picture'), false);
  await p.keyboard.press('?');
  assert.equal(await p.isVisible('#shortcuts'), true);
  await p.keyboard.press('Escape');
  await p.keyboard.press('Control+k');
  await p.keyboard.type('agent');
  // the first hit is selected (its text is in whatever language an earlier test left)
  assert.equal(await p.locator('#palette-list li[aria-selected="true"]').count(), 1);
  await p.keyboard.press('Enter');
  await p.waitForSelector('[data-pane="integrations"]:not([hidden])');
  assert.match(await p.textContent('#mcp-url'), /\/mcp$/);
  // nothing is installed without saying what happens first
  await p.click('[data-tab="install"]');
  await p.fill('#board-host', 'board.example');
  await p.evaluate(() => { document.querySelector('#board-install').disabled = false; });
  await p.click('#board-install');
  await p.waitForSelector('#confirm[open]');
  await p.click('#confirm-cancel');
  await p.fill('#board-host', '');
  await p.click('#settings-close');
  // after an update (not a first install) the changes are shown once
  await p.evaluate(() => localStorage.setItem('ns2bench.lastSeenVersion', '0.0.1'));
  await p.reload();
  await p.waitForSelector('#whatsnew[open]', { timeout: 10000 });
  assert.ok((await p.textContent('#whatsnew-body')).length > 50);
  assert.deepEqual(errors, []);
  await ctx.close();
});
