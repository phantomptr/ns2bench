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
  await p.waitForFunction(() => document.querySelectorAll('#logview li').length > 0, null, { timeout: 10000 });
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
  await p.waitForFunction(() => /\/mcp$/.test(document.querySelector('#mcp-url').textContent), null, { timeout: 5000 });
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
  // saved by the engine before the reload reads it back
  for (let i = 0; (await (await fetch(`${BASE}/api/prefs`)).json())['ns2bench.lastSeenVersion'] !== '0.0.1'; i++) {
    assert.ok(i < 50, 'not saved');
    await new Promise((r) => setTimeout(r, 100));
  }
  await p.reload();
  await p.waitForSelector('#whatsnew[open]', { timeout: 10000 });
  assert.ok((await p.textContent('#whatsnew-body')).length > 50);
  assert.deepEqual(errors, []);
  await ctx.close();
});

test('shared home network: pick an adapter, add consoles, get the console settings', async (t) => {
  if (process.platform === 'win32') { t.skip('the network helper runs on macOS and Linux'); return; }
  const list = await (await fetch(`${BASE}/api/network/interfaces`)).json();
  const iface = list.interfaces[0];
  if (!iface) { t.skip('no adapter with a private address on this machine'); return; }
  // a console address on the adapter's network that isn't this computer
  const [a, b, c, d] = iface.address.split('.').map(Number);
  const consoleIp = `${a}.${b}.${c}.${d === 250 ? 251 : 250}`;
  const { p, errors, ctx } = await page();
  // An earlier test leaves "What's new" due on the next load; it opens a moment after the page
  // (later on a slow machine). Close it whenever it opens.
  await p.evaluate(() => {
    const d = document.querySelector('#whatsnew');
    new MutationObserver(() => { if (d.open) d.close(); }).observe(d, { attributes: true, attributeFilter: ['open'] });
    if (d.open) d.close();
  });
  await p.click('#btn-settings');
  await p.click('[data-tab="network"]');
  await p.selectOption('#network-topology', 'shared');
  await p.waitForSelector('#shared-setup:not([hidden])');
  // DHCP can't be turned on for a shared network
  assert.equal(await p.isDisabled('#network-dhcp'), true);
  await p.waitForFunction(() => document.querySelector('#shared-iface').options.length > 0);
  await p.selectOption('#shared-iface', iface.name);
  await p.fill('#shared-consoles', consoleIp);
  // Save waits for the three checks
  assert.equal(await p.isDisabled('#shared-save'), true);
  for (const box of await p.$$('.shared-check')) await box.check();
  assert.equal(await p.isDisabled('#shared-save'), false);
  await p.click('#shared-save');
  if (list.forwarding_off) {
    await p.waitForSelector('#shared-settings tr');
    const cells = await p.$$eval('#shared-settings tr td', (tds) => tds.map((td) => td.textContent));
    // console, IP, mask, gateway (itself), DNS (this computer)
    assert.deepEqual([cells[0], cells[1], cells[3], cells[4]], [consoleIp, consoleIp, consoleIp, iface.address]);
    const n = await (await fetch(`${BASE}/api/network`)).json();
    assert.equal(n.controls.topology, 'shared');
    assert.equal(n.controls.dhcp, false);
    assert.deepEqual(n.shared.consoles, [consoleIp]);
  } else {
    // a machine that forwards (Docker hosts often do) is refused, and says why
    await p.waitForFunction(() => /forwarding/i.test(document.body.textContent));
  }
  // a script without the page's key can't set it up
  const r = await fetch(`${BASE}/api/network/shared`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ interface: iface.name, consoles: [consoleIp] }) });
  assert.equal(r.status, 403);
  assert.deepEqual(errors, []);
  await ctx.close();
});

test('a message shown over Settings can be closed with its ×', async () => {
  const { p, errors, ctx } = await page();
  await p.evaluate(() => {
    const d = document.querySelector('#whatsnew');
    new MutationObserver(() => { if (d.open) d.close(); }).observe(d, { attributes: true, attributeFilter: ['open'] });
    if (d.open) d.close();
  });
  await p.click('#btn-settings');
  await p.evaluate(async () => (await import('/static/ui.js')).toast('test message', 'error'));
  await p.waitForSelector('.toast.error');
  await p.click('.toast.error .x');
  await p.waitForSelector('.toast.error', { state: 'detached', timeout: 3000 });
  // and once Settings is closed, a new one still shows and closes on the page
  await p.click('#settings-close');
  await p.evaluate(async () => (await import('/static/ui.js')).toast('second', 'error'));
  await p.click('.toast.error .x');
  await p.waitForSelector('.toast.error', { state: 'detached', timeout: 3000 });
  assert.deepEqual(errors, []);
  await ctx.close();
});

test('shared home network: a console that is not on the picked adapter’s network is flagged before saving', async (t) => {
  if (process.platform === 'win32') { t.skip('the network helper runs on macOS and Linux'); return; }
  const list = await (await fetch(`${BASE}/api/network/interfaces`)).json();
  if (!list.interfaces[0]) { t.skip('no adapter with a private address on this machine'); return; }
  const { p, errors, ctx } = await page();
  await p.evaluate(() => {
    const d = document.querySelector('#whatsnew');
    new MutationObserver(() => { if (d.open) d.close(); }).observe(d, { attributes: true, attributeFilter: ['open'] });
    if (d.open) d.close();
  });
  await p.click('#btn-settings');
  await p.click('[data-tab="network"]');
  await p.selectOption('#network-topology', 'shared');
  await p.waitForFunction(() => document.querySelector('#shared-iface').options.length > 0);
  await p.selectOption('#shared-iface', list.interfaces[0].name);
  await p.fill('#shared-consoles', '203.0.113.5');      // a documentation address: on no adapter
  for (const box of await p.$$('.shared-check')) await box.check();
  await p.waitForSelector('#shared-mismatch:not([hidden])');
  assert.match(await p.textContent('#shared-mismatch'), /203\.0\.113\.5/);
  assert.equal(await p.isDisabled('#shared-save'), true);
  assert.deepEqual(errors, []);
  await ctx.close();
});

test('several consoles: add one, switch the view, input only goes where the controller is', async () => {
  const { p, errors, ctx } = await page();
  await p.evaluate(() => {
    const d = document.querySelector('#whatsnew');
    new MutationObserver(() => { if (d.open) d.close(); }).observe(d, { attributes: true, attributeFilter: ['open'] });
    if (d.open) d.close();
  });
  // one console: its tile and "+ Add console" are already under the picture, and the tile opens the form
  await p.waitForSelector('#console-bar:not([hidden]) .console-thumb[data-console]');
  assert.equal(await p.locator('.console-thumb[data-console]').count(), 1);
  assert.equal(await p.isHidden('#console-note'), true);
  await p.click('.console-thumb.add');
  await p.waitForSelector('#consoles-list .console-card');
  await p.waitForFunction(() => document.activeElement?.id === 'console-add-name');
  // only a name is needed (the card, address and colour can come later); colours are dots to click
  await p.fill('#console-add-name', 'Switch 1');
  await p.click('#console-add-color >> xpath=.. >> .swatch[data-color="#1edc00"]');
  assert.equal(await p.inputValue('#console-add-color'), '#1edc00');
  await p.click('#console-add');
  await p.waitForFunction(() => document.querySelectorAll('#consoles-list .console-card').length === 2);
  let list0 = await (await fetch(`${BASE}/api/consoles`)).json();
  const added = list0.consoles[1];
  assert.equal(added.color, '#1edc00');
  assert.ok(!added.video?.device, 'no card yet, and not the first console\'s');
  // its card, later (as its settings would; the test source stands in for a card)
  const set = await p.evaluate(async (id) => (await (await import('/static/client.js')).apiFetch('/api/video', { method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-NS2Bench-Console': id }, body: JSON.stringify({ device: 'test-card-2' }) })).status, added.id);
  assert.equal(set, 200);
  await p.evaluate(async (id) => (await import('/static/client.js')).apiFetch('/api/consoles', { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action: 'update', id, model: 'switch1', network_ip: '192.168.86.252' }) }), added.id);
  await p.click('#settings-close');
  // each tile opens its own settings
  await p.waitForSelector('.console-tile >> nth=1 >> .edit');
  await p.click('.console-tile >> nth=1 >> .edit');
  await p.waitForFunction((id) => document.activeElement?.closest('[data-console]')?.dataset.console === id, added.id);
  await p.click('#settings-close');
  // two tabs, the controller on the first
  await p.waitForSelector('#console-bar:not([hidden]) .console-thumb[data-console] >> nth=1', { timeout: 10000 });
  // each console's small live picture fills in, and 🎮 marks the one with the controller
  await p.waitForFunction(() => [...document.querySelectorAll('.console-thumb img')].every((i) => i.naturalWidth > 0), null, { timeout: 15000 });
  assert.equal(await p.locator('.console-thumb[data-console] >> nth=0 >> .controller-badge').count(), 1);
  const list = await (await fetch(`${BASE}/api/consoles`)).json();
  assert.equal(list.consoles.length, 2);
  const [first, second] = list.consoles;
  assert.equal(list.controller_on, first.id);
  assert.equal(second.model, 'switch1');
  // watching the second: the drawn controller can't press there
  await p.click('.console-thumb[data-console] >> nth=1');
  await p.waitForSelector('.console.input-elsewhere');
  assert.match(await p.textContent('#console-note'), /Switch 1/);
  // an agent pressing on the console without the controller is refused, with why
  const r = await fetch(`${BASE}/api/press`, { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-NS2Bench-Console': second.id },
    body: JSON.stringify({ buttons: ['A'] }) });
  assert.equal(r.status, 409);
  assert.match(await r.text(), /plugged into/);
  // its own picture is there
  await waitFor(async () => (await fetch(`${BASE}/api/screenshot?console=${second.id}`)).ok);
  // the person moves the cable and says so
  await p.click('#console-note-actions button');
  await waitFor(async () => (await (await fetch(`${BASE}/api/consoles`)).json()).controller_on === second.id);
  await p.waitForFunction(() => !document.querySelector('.console').classList.contains('input-elsewhere'));
  // a script can't move the controller or add consoles: only the person at the page
  const w = await fetch(`${BASE}/api/consoles`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ action: 'controller', id: first.id }) });
  assert.equal(w.status, 403);
  // back to one console
  await p.click('.console-thumb[data-console] >> nth=0');
  await p.click('#btn-settings');
  await p.click('[data-tab="consoles"]');
  await p.waitForSelector('#consoles-list .console-card >> nth=1');
  await p.click('#consoles-list .console-card >> nth=1 >> button.danger');
  await p.waitForSelector('#confirm[open]');
  await p.click('#confirm-ok');
  await waitFor(async () => (await (await fetch(`${BASE}/api/consoles`)).json()).consoles.length === 1);
  await p.click('#settings-close');
  await p.waitForFunction(() => document.querySelectorAll('.console-thumb[data-console]').length === 1);
  assert.deepEqual(errors, []);
  await ctx.close();
});

test('top buttons: half span by default, full span from Appearance, remembered', async () => {
  const { p, errors, ctx } = await page();
  await p.evaluate(() => {
    const d = document.querySelector('#whatsnew');
    new MutationObserver(() => { if (d.open) d.close(); }).observe(d, { attributes: true, attributeFilter: ['open'] });
    if (d.open) d.close();
  });
  const reach = () => p.evaluate(() => document.querySelector('.joycon.left .shoulders').getBoundingClientRect().right - document.querySelector('.joycon.left').getBoundingClientRect().right);
  const half = await reach();
  assert.ok(half > 20, `L reaches in over the screen (${half}px)`);
  await p.click('#btn-settings');
  await p.click('[data-tab="appearance"]');
  await p.click('#shoulder-choices [data-shoulders="full"]');
  assert.equal(await p.evaluate(() => document.documentElement.dataset.shoulders), 'full');
  const full = await reach();
  assert.ok(full > half * 1.6, `full span reaches further (${full} vs ${half})`);
  // no top button covers the screen itself
  const overlap = await p.evaluate(() => {
    const screen = document.querySelector('#screen').getBoundingClientRect();
    return [...document.querySelectorAll('.joycon .pill')].some((b) => { const r = b.getBoundingClientRect(); return r.bottom > screen.top + 1 && r.right > screen.left && r.left < screen.right; });
  });
  assert.equal(overlap, false);
  await p.click('#shoulder-choices [data-shoulders="half"]');
  await p.click('#settings-close');
  assert.deepEqual(errors, []);
  await ctx.close();
});

test('right panel follows the shown console: its card, the controller chain, its network', async () => {
  const { p, errors, ctx } = await page();
  await p.evaluate(() => {
    const d = document.querySelector('#whatsnew');
    new MutationObserver(() => { if (d.open) d.close(); }).observe(d, { attributes: true, attributeFilter: ['open'] });
    if (d.open) d.close();
  });
  // the console card: its name, and the chain from this computer (no input device in tests)
  await p.waitForSelector('#panel-console:not([hidden])');
  const list = await (await fetch(`${BASE}/api/consoles`)).json();
  const c = list.consoles[0];
  assert.equal(await p.textContent('#pc-name'), c.name);
  await p.waitForSelector('#chain li >> nth=1');
  assert.match(await p.textContent('#chain'), /Computer/);
  assert.match(await p.textContent('#chain-why'), /no input device/i);
  // the network card asks for the address, then shows it once set
  await p.waitForSelector('#panel-network:not([hidden]) #pn-addr:not([hidden])');
  await p.evaluate(async (id) => (await import('/static/client.js')).apiFetch('/api/consoles', { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action: 'update', id, network_ip: '192.168.86.251' }) }), c.id);
  await p.waitForFunction(() => /192\.168\.86\.251/.test(document.querySelector('#pn-rows').textContent), null, { timeout: 15000 });
  assert.equal(await p.isHidden('#pn-addr'), true);
  // Edit opens this console's settings, at its name
  await p.click('#pc-edit');
  await p.waitForFunction((id) => document.activeElement?.closest('[data-console]')?.dataset.console === id, c.id);
  await p.click('#settings-close');
  // picture shortcuts mirror the top bar's sound button
  await p.click('#btn-sound');
  await p.waitForFunction(() => document.querySelector('#p-sound').getAttribute('aria-pressed') === 'true');
  await p.click('#p-sound');
  await p.waitForFunction(() => document.querySelector('#btn-sound').getAttribute('aria-pressed') === 'false');
  // put the address back as it was
  await p.evaluate(async (id) => (await import('/static/client.js')).apiFetch('/api/consoles', { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action: 'update', id, network_ip: '' }) }), c.id);
  assert.deepEqual(errors, []);
  await ctx.close();
});
