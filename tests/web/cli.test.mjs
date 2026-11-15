// End-to-end CLI test: the real engine (synthetic video, no input device) and the
// `ns2bench` command line against it. Runs in the same CI job as ui.test.mjs.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync, existsSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const exe = join(root, 'native/target/debug', process.platform === 'win32' ? 'ns2bench.exe' : 'ns2bench');
const PORT = 7450 + (process.pid % 200);
const URL = `http://127.0.0.1:${PORT}`;
let engine, home;

const cli = (...args) => {
  const r = spawnSync(exe, args, { env: { ...process.env, HOME: home, USERPROFILE: home, NS2BENCH_URL: URL }, encoding: 'utf8', timeout: 30000 });
  return { code: r.status, out: r.stdout, err: r.stderr };
};

before(async () => {
  assert.ok(existsSync(exe), `build the engine first: ${exe}`);
  home = mkdtempSync(join(tmpdir(), 'ns2bench-cli-'));
  const cfg = join(home, 'config.json');
  writeFileSync(cfg, JSON.stringify({
    server: { host: '127.0.0.1', port: PORT }, input: { backend: 'none' }, audio: { backend: 'none' },
    video: { backend: 'ffmpeg', width: 640, height: 360, fps: 15, quality: 70,
      ffmpeg_input: ['-re', '-f', 'lavfi', '-i', 'testsrc2=size=640x360:rate=15'] },
  }));
  engine = spawn(exe, ['serve'], { env: { ...process.env, HOME: home, USERPROFILE: home, NS2BENCH_CONFIG: cfg }, stdio: 'ignore' });
  const end = Date.now() + 20000;
  for (;;) {
    try { if ((await fetch(`${URL}/api/status`)).ok) break; } catch { /* starting */ }
    if (Date.now() > end) throw new Error('engine did not start');
    await new Promise((r) => setTimeout(r, 200));
  }
  // wait for the first video frame
  for (let i = 0; i < 50; i++) {
    const s = await (await fetch(`${URL}/api/status`)).json();
    if (s.video.age_s !== null && s.video.age_s < 3) break;
    await new Promise((r) => setTimeout(r, 200));
  }
});

after(() => {
  engine?.kill();
  if (home) rmSync(home, { recursive: true, force: true });
});

test('status, version and help', () => {
  const s = cli('status');
  assert.equal(s.code, 0, s.err);
  assert.match(s.out, /NS2Bench \d+\.\d+\.\d+/);
  assert.match(s.out, /video\s+\d/);
  const j = JSON.parse(cli('--json', 'status').out);
  assert.equal(j.input.backend, 'none');
  assert.match(cli('--help').out, /wait-text/);
  assert.equal(cli('--version').code, 0);
});

test('screenshot writes an image', () => {
  const out = join(home, 'shot.jpg');
  const r = cli('screenshot', '-o', out, '--max-width', '320');
  assert.equal(r.code, 0, r.err);
  assert.ok(statSync(out).size > 1000);
});

test('logs, events, diagnostics, network, input and video read back', () => {
  assert.match(cli('logs', '-n', '5').out, /bench/);
  assert.equal(cli('--json', 'events').code, 0);
  assert.match(cli('diagnostics').out, /NS2Bench/);
  assert.match(cli('network').out, /blocking/);
  assert.match(cli('input').out, /"backend"/);
  assert.match(cli('video').out, /"settings"/);
});

test('driving with no input device fails clearly', () => {
  const r = cli('press', 'A');
  assert.notEqual(r.code, 0);
  assert.match(r.err, /NS2Bench:/);
});

test('agents can block themselves but only the UI can unblock', () => {
  assert.equal(cli('manual-only', 'on').code, 0);
  const blocked = cli('press', 'A');
  assert.match(blocked.err, /manual-only/);
  const unblock = cli('manual-only', 'off');
  assert.notEqual(unblock.code, 0);
  assert.match(unblock.err, /only the person at the NS2Bench page/);
});

test('bad arguments are rejected before contacting the engine', () => {
  assert.notEqual(cli('usb-mode', 'turbo').code, 0);
  assert.notEqual(cli('stick', 'middle', '0', '0').code, 0);
});

test('new agent tools: health, wait-screen, cropped screenshots, events, levels', async () => {
  const h = cli('health');
  assert.notEqual(h.code, 0);                       // no input device in the test engine
  assert.match(h.out, /NOT READY/);
  assert.match(h.out, /Settings → Devices/);
  const hj = JSON.parse(cli('--json', 'health').out);
  assert.equal(hj.video, true);
  // the test pattern moves all the time: that motion is the noise floor, not a "change"
  const w = JSON.parse(cli('--json', 'wait-screen', '--timeout-ms', '1500').out);
  assert.equal(w.until, 'change');
  assert.ok(w.waited_ms >= 1000, JSON.stringify(w));
  const st = JSON.parse(cli('--json', 'wait-screen', '--stable', '--timeout-ms', '1500').out);
  assert.equal(st.until, 'stable');
  const out = join(home, 'crop.jpg');
  assert.equal(cli('screenshot', '-o', out, '--region', '0,0,0.5,0.5').code, 0);
  assert.ok(statSync(out).size > 500);
  assert.notEqual(cli('screenshot', '-o', out, '--region', '0,0,2,1').code, 0);
  assert.equal(cli('events', '-n', '5').code, 0);
  assert.equal(cli('reports').code, 0);
  // Block agents is on (turned on by an earlier test): agents can't change the screen log
  assert.match(cli('screen-log', 'off').err, /manual-only/);
  const lv = cli('--json', 'logs', '--level', 'warn', '-n', '200');
  assert.equal(lv.code, 0);
  for (const line of lv.out.trim().split('\n').filter(Boolean)) assert.ok(['warn', 'error'].includes(JSON.parse(line).level), line);
  assert.notEqual(cli('logs', '--level', 'loud').code, 0);
});

test('agents cannot switch USB mode or video while blocked', async () => {
  // manual-only is already on from the earlier test (only the page can turn it off)
  const r = await fetch(`${URL}/api/control`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ usb_mode: 'pro' }) });
  assert.equal(r.status, 409);
  const v = await fetch(`${URL}/api/video`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ fps: 10 }) });
  assert.equal(v.status, 409);
});

test('the page still works while agents are blocked, and agents cannot redirect the Bridge token', async () => {
  const json = { 'Content-Type': 'application/json' };
  const page = { ...json, Origin: URL, 'X-NS2Bench-Source': 'ui' };
  // the page's own requests are the person at the bench, not an agent
  const log = await fetch(`${URL}/api/control`, { method: 'POST', headers: page, body: JSON.stringify({ screen_log: false }) });
  assert.equal(log.status, 200);
  // an agent can't turn Block agents off, and nothing else in that request is applied
  const off = await fetch(`${URL}/api/control`, { method: 'POST', headers: json, body: JSON.stringify({ manual_only: false, screen_log: true }) });
  assert.equal(off.status, 403);
  // the page can
  const on = await fetch(`${URL}/api/control`, { method: 'POST', headers: page, body: JSON.stringify({ manual_only: false }) });
  assert.equal(on.status, 200);
  assert.equal((await on.json()).input.manual_only, false);
  // an agent may not point the Bridge (and so its token) at another host
  writeFileSync(join(home, '.ns2bench', 'hidd.token'), 'secret-token\n');
  const move = await fetch(`${URL}/api/input`, { method: 'POST', headers: json,
    body: JSON.stringify({ backend: 'hidd', host: '127.0.0.1', port: 9, token_file: '~/.ns2bench/hidd.token' }) });
  assert.equal(move.status, 403);
});
