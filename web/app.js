import { apiFetch as fetch, authURL } from './client.js';
import './prefs.js';
import './settings.js';
import { t } from './i18n.js';
import { fromLog } from './notify.js';
// NS2Bench browser client: video, Joy-Con, keyboard/mouse/gamepad driving, live activity.
import { BIT, DPAD, mountPad } from './pad.js';
import { startLive } from './live.js';
import { toast } from './ui.js';
import { inputBlock, mountConsoles, updateConsoles, viewed } from './consoles.js';
import { updatePanel } from './panel.js';

const $ = (s) => document.querySelector(s);
const KEYMAP = { // KeyboardEvent.code -> controller input (keyboard → controller mode)
  ArrowUp: 'UP', ArrowDown: 'DOWN', ArrowLeft: 'LEFT', ArrowRight: 'RIGHT',
  KeyI: 'X', KeyJ: 'Y', KeyK: 'B', KeyL: 'A', KeyQ: 'L', KeyE: 'R', Digit1: 'ZL', Digit3: 'ZR',
  Enter: 'PLUS', Backspace: 'MINUS', Backquote: 'HOME', KeyP: 'CAPTURE', KeyV: 'LSTICK', KeyN: 'RSTICK',
  KeyC: 'C', KeyZ: 'GL', KeyX: 'GR',      // reach the console in Pro Controller mode
};
const STICKKEYS = { KeyW: [0, 1, -1], KeyS: [0, 1, 1], KeyA: [0, 0, -1], KeyD: [0, 0, 1],
  KeyT: [1, 1, -1], KeyG: [1, 1, 1], KeyF: [1, 0, -1], KeyH: [1, 0, 1] };
const MODBITS = { ControlLeft: 1, ShiftLeft: 2, AltLeft: 4, MetaLeft: 8,
  ControlRight: 16, ShiftRight: 32, AltRight: 64, MetaRight: 128 };
// Standard Gamepad API layout -> Switch, by physical position (bottom face = B).
const PADMAP = ['B', 'A', 'Y', 'X', 'L', 'R', 'ZL', 'ZR', 'MINUS', 'PLUS', 'LSTICK', 'RSTICK',
  'UP', 'DOWN', 'LEFT', 'RIGHT', 'HOME', 'CAPTURE'];
const DEADZONE = 0.12;
const MOUSE_BATCH_MS = 8;
const AIM_GAIN = 0.035, AIM_DECAY = 0.82;      // mouse → right stick in controller mode

const store = {
  get(k, d) { try { const v = localStorage.getItem(`ns2bench.${k}`); return v === null ? d : JSON.parse(v); } catch { return d; } },
  set(k, v) { try { localStorage.setItem(`ns2bench.${k}`, JSON.stringify(v)); } catch { /* private mode */ } },
};

// keyboard shortcuts shown on the Joy-Con (hover tip or keycap badges)
const KEYCAP = { ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→', Enter: 'Enter', Backspace: 'Bksp',
  Backquote: '` ~' };
const capOf = (code) => KEYCAP[code] ?? code.replace(/^Key|^Digit/, '');
const HINTS = {};
for (const [code, name] of Object.entries(KEYMAP)) (HINTS[name] ??= []).push(capOf(code));
HINTS.left = ['W', 'A', 'S', 'D'];    // up, left, down, right
HINTS.right = ['T', 'F', 'G', 'H'];   // up, left, down, right
const pad = mountPad($('#joy-left'), $('#joy-right'), { hints: HINTS });

// press trail: the last few inputs, yours in cyan, agents' in purple
const TRAIL_NAME = { UP: '↑', DOWN: '↓', LEFT: '←', RIGHT: '→', MINUS: '−', PLUS: '+', LSTICK: 'L3', RSTICK: 'R3' };
pad.onMacro(async (name) => {
  if (name !== 'C') return;
  addEvent({ source: 'ui', action: 'shortcut C → open GameChat', t: Date.now() / 1000 });
  const r = await fetch('/api/navigate', { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ route: 'gamechat', source: 'ui' }) });
  if (!r.ok) addEvent({ source: 'error', action: await r.text(), t: Date.now() / 1000 });
});
pad.onPress((name, who) => {
  const trail = $('#trail');
  const chip = document.createElement('span');
  chip.className = `chip ${who}`;
  chip.innerHTML = '<b></b><small></small>';
  chip.firstChild.textContent = TRAIL_NAME[name] ?? name;
  chip.lastChild.textContent = who;
  trail.append(chip);
  while (trail.children.length > 12) trail.firstChild.remove();
  setTimeout(() => chip.classList.add('old'), 4000);
});
const held = new Set();                         // keyboard codes down
let kbMode = store.get('kbmode', 'pad');
let ws, lastSent = '', lastPads = 0, serverPad = [0, 8, 128, 128, 128, 128], serverAgent = [0, 0];
const aim = [0, 0];
const mouse = { dx: 0, dy: 0, wheel: 0, buttons: 0, dirty: false, last: 0 };

// ------------------------------------------------------------ layout: fit the console to the window
const stage = $('#stage'), consoleEl = $('#console');
function fit() {
  const showPad = !consoleEl.classList.contains('nopad');
  const ratio = showPad ? 2.1769 : 16 / 9;  // .console width / --h in joycon.css (Joy-Con, bezelled screen, gaps)
  const full = document.fullscreenElement === consoleEl;
  const desktop = matchMedia('(min-width: 1000px) and (min-height: 600px)').matches;
  const outerHeight = (el) => {
    const css = getComputedStyle(el);
    return el.getBoundingClientRect().height + parseFloat(css.marginTop) + parseFloat(css.marginBottom);
  };
  // Reserve the measured toolbar/trail and a useful log area, rather than
  // sizing video to the viewport and appending the logs below its bottom.
  const logsReserve = Math.max(stage.clientHeight * 0.32, $('.logs-head').offsetHeight + 110);
  const extras = outerHeight($('#trail')) + outerHeight($('#capture-hint')) + logsReserve + 12;
  // Text size zooms the page: the screen's own pixels are the window's divided by that zoom.
  const zoom = parseFloat(document.documentElement.style.zoom) || 1;
  const availH = full ? window.innerHeight / zoom : desktop ? stage.clientHeight - extras
    : Math.min(window.innerHeight * 0.55, window.innerHeight - stage.getBoundingClientRect().top - 78);
  const availW = full ? window.innerWidth / zoom : stage.clientWidth;
  const h = Math.max(1, Math.min(availH, availW / ratio));
  consoleEl.style.setProperty('--h', `${Math.floor(h)}px`);
}
const fitObserver = new ResizeObserver(fit);
fitObserver.observe(stage);
fitObserver.observe($('.logs-head'));
window.addEventListener('resize', fit);

const showKeys = $('#show-keys');
showKeys.checked = store.get('showkeys', false);
const applyKeys = () => { consoleEl.classList.toggle('show-keys', showKeys.checked); store.set('showkeys', showKeys.checked); };
showKeys.addEventListener('change', applyKeys);
applyKeys();

const showPad = $('#show-pad');
showPad.checked = store.get('showpad', true);
const applyPad = () => { consoleEl.classList.toggle('nopad', !showPad.checked); store.set('showpad', showPad.checked); fit(); };
showPad.addEventListener('change', applyPad);
applyPad();

// ------------------------------------------------------------ websocket
let bridgeStatus = null, bridgeSeen = 0, canDrive = true;
const inputWarning = $('#input-warning');
function inputUnavailable() {
  if (ws?.readyState !== WebSocket.OPEN) return 'NS2Bench connection lost. Reconnecting automatically.';
  if (!bridgeStatus || performance.now() - bridgeSeen > 3000) return 'Waiting for NS2Bench Bridge status.';
  if (!canDrive) return 'This page can watch but not drive. Use the NS2Bench app window, or open the link that ns2bench serve printed.';
  const elsewhere = inputBlock();
  if (elsewhere) return elsewhere;
  if (!bridgeStatus.connected) return bridgeStatus.backend === 'none'
    ? 'NS2Bench Bridge is not configured. Set up an input device to send controls.'
    : 'NS2Bench Bridge is offline or starting. Check device power and its connection. Reconnecting automatically.';
  if (bridgeStatus.usb !== 'configured') return 'NS2Bench Bridge is connected, but console USB is not ready. Wake the console and check its USB cable.';
  return '';
}
function warnInput(reason = inputUnavailable()) {
  const message = `${t(reason)} ${t('Input was not sent; try again when ready.')}`;
  if (inputWarning.textContent !== message) inputWarning.textContent = message;
  inputWarning.hidden = false;
}
const activeInput = (obj) => obj.t === 'state'
  ? obj.buttons || obj.dpad || obj.mods || obj.keynames?.length || obj.sticks?.some(v => v !== 128)
  : obj.t === 'mouse' && (obj.dx || obj.dy || obj.wheel || obj.buttons);
consoleEl.addEventListener('pointerdown', (e) => {
  if (e.target.closest('.joycon') && inputUnavailable()) {
    warnInput(); e.preventDefault(); e.stopImmediatePropagation();
  }
}, true);
// The engine went away (app closing, crash, sleep): say so at once instead of showing the last
// status as if it were current, and offer a way out.
let lostTimer = 0, lost = false;
function engineLost() {
  if (lost) return;
  lost = true;
  for (const p of [$('#pill-video'), $('#pill-input')]) { p.className = 'pill'; p.textContent = t('reconnecting…'); }
  clearTimeout(lostTimer);
  lostTimer = setTimeout(() => { $('#engine-lost').hidden = false; }, 2000);
}
function engineBack() {
  if (!lost) return;
  lost = false; clearTimeout(lostTimer); $('#engine-lost').hidden = true;
}
$('#engine-retry').addEventListener('click', () => $('#btn-refresh').click());
let wsFails = 0;
// While anything is held, repeat the state every second: the engine lets go of input if the page
// falls silent (a sleeping laptop, lost Wi-Fi), instead of holding a button for minutes.
const idleState = JSON.stringify({ t: 'state', buttons: 0, dpad: 0, sticks: [128, 128, 128, 128], mods: 0, keynames: [] });
setInterval(() => {
  if (ws?.readyState !== WebSocket.OPEN) return;
  if (lastSent && lastSent !== idleState) ws.send(lastSent);
  if (mouse.buttons) ws.send(JSON.stringify({ t: 'mouse', dx: 0, dy: 0, wheel: 0, buttons: mouse.buttons }));
}, 1000);
function connect() {
  ws = new WebSocket(authURL('/ws').replace(/^http/, 'ws'));
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.t === 'pad') { serverPad = m.pad; serverAgent = m.agent ?? [0, 0]; }
    else if (m.t === 'status') { engineBack(); showStatus(m.status); serverPad = m.status.input.pad; canDrive = m.drive !== false; }
    else if (m.t === 'event') addEvent(m.event);
    else if (m.t === 'log') { addLog(m.entry); fromLog(m.entry); }
    else if (m.t === 'error') addEvent({ source: 'error', action: m.error, t: Date.now() / 1000 });
  };
  // slower each time (at most every 15 s) while the engine is away or refuses the page
  ws.onclose = () => { bridgeStatus = null; releaseAll(); engineLost(); setTimeout(connect, Math.min(15000, 1000 * ++wsFails)); };
  ws.onopen = () => { wsFails = 0; lastSent = ''; if (logsLoaded) loadLogs(); };
}
const send = (obj) => {
  const reason = inputUnavailable();
  if (activeInput(obj) && reason) { warnInput(reason); return false; }
  if (ws?.readyState === WebSocket.OPEN) { ws.send(JSON.stringify(obj)); return true; }
  return false;
};

// ------------------------------------------------------------ compose the human state
const axis = (v) => Math.max(0, Math.min(255, Math.round(128 + Math.max(-1, Math.min(1, v)) * 127.5)));

function compose() {
  let buttons = 0, dpad = 0;
  const sticks = [[0, 0], [0, 0]];
  const press = (n) => { if (n in BIT) buttons |= 1 << BIT[n]; else if (n in DPAD) dpad |= DPAD[n]; };

  const screen = pad.state();
  screen.held.forEach(press);
  sticks[0][0] += screen.sticks[0]; sticks[0][1] += screen.sticks[1];
  sticks[1][0] += screen.sticks[2]; sticks[1][1] += screen.sticks[3];

  const keynames = [];
  let mods = 0;
  for (const code of held) {
    if (kbMode === 'pad') {
      if (KEYMAP[code]) press(KEYMAP[code]);
      if (STICKKEYS[code]) { const [s, a, v] = STICKKEYS[code]; sticks[s][a] += v; }
    } else if (MODBITS[code]) mods |= MODBITS[code];
    else keynames.push(code);
  }
  if (kbMode === 'pad') { sticks[1][0] += aim[0]; sticks[1][1] += aim[1]; }

  let pads = 0;
  for (const gp of navigator.getGamepads?.() ?? []) {
    if (!gp) continue;
    pads++;
    gp.buttons.forEach((b, i) => { if (b.pressed && PADMAP[i]) press(PADMAP[i]); });
    gp.axes.slice(0, 4).forEach((v, i) => { if (Math.abs(v) > DEADZONE) sticks[i >> 1][i & 1] += v; });
  }
  if (pads !== lastPads) {
    lastPads = pads;
    $('#pill-pad').textContent = pads ? `${pads} ${pads > 1 ? t('gamepads') : t('gamepad')}` : t('no gamepad');
    $('#pill-pad').className = pads ? 'pill ok' : 'pill';
  }
  return { t: 'state', buttons, dpad, sticks: sticks.flat().map(axis), mods, keynames: keynames.slice(0, 6) };
}

function tick(now) {
  const state = compose();
  const key = JSON.stringify(state);
  if (key !== lastSent) {
    if (!send(state) && activeInput(state)) { held.clear(); pad.release(); aim[0] = aim[1] = 0; }
    lastSent = key;
  }
  if (mouse.dirty && now - mouse.last >= MOUSE_BATCH_MS) {
    if (!send({ t: 'mouse', dx: Math.round(mouse.dx), dy: Math.round(mouse.dy), wheel: mouse.wheel, buttons: mouse.buttons })) {
      mouse.dx = mouse.dy = mouse.wheel = mouse.buttons = 0;
    }
    mouse.dx -= Math.round(mouse.dx); mouse.dy -= Math.round(mouse.dy); mouse.wheel = 0;
    mouse.dirty = false; mouse.last = now;
  }
  aim[0] *= AIM_DECAY; aim[1] *= AIM_DECAY;
  // show what the console receives (agent included) plus what this person holds right now
  const local = [state.buttons, 8, ...state.sticks];
  const merged = [serverPad[0] | state.buttons, serverPad[1], ...serverPad.slice(2)];
  if (state.sticks.some((v) => v !== 128)) merged.splice(2, 4, ...local.slice(2));
  // only when something changed: this runs every frame, the drawing rarely changes
  const shownKey = `${merged}|${serverAgent}`;
  if (shownKey !== lastShown) { lastShown = shownKey; pad.highlight(merged, serverAgent); }
  requestAnimationFrame(tick);
}

function releaseAll() {
  held.clear(); pad.release(); aim[0] = aim[1] = 0;
  Object.assign(mouse, { dx: 0, dy: 0, wheel: 0, buttons: 0, dirty: false });
  send({ t: 'state', buttons: 0, dpad: 0, sticks: [128, 128, 128, 128], mods: 0, keynames: [] });
  send({ t: 'mouse', dx: 0, dy: 0, wheel: 0, buttons: 0 });
  lastSent = '';
}

// ------------------------------------------------------------ keyboard and mouse on the screen
const screenEl = $('#screen');
screenEl.addEventListener('keydown', (e) => {
  if (e.code === 'Escape' && kbMode === 'pad') { screenEl.blur(); return; }
  // Keyboard & mouse mode sends every key, Esc and Tab included: this chord is the way out.
  if (e.code === 'KeyU' && e.ctrlKey && e.altKey) { e.preventDefault(); held.clear(); document.exitPointerLock?.(); screenEl.blur(); return; }
  e.preventDefault();
  if (inputUnavailable()) { warnInput(); return; }
  held.add(e.code);
});
screenEl.addEventListener('keyup', (e) => {
  e.preventDefault(); held.delete(e.code);
  // macOS sends no keyup for a key let go while Command is down: release them with Command.
  if (e.code === 'MetaLeft' || e.code === 'MetaRight') for (const c of [...held]) if (!/^(Shift|Control|Alt|Meta)/.test(c)) held.delete(c);
});
screenEl.addEventListener('blur', () => held.clear());
window.addEventListener('blur', releaseAll);
document.addEventListener('visibilitychange', () => { if (document.hidden) releaseAll(); });

const HID_BUTTON = { 0: 1, 2: 2, 1: 4, 3: 8, 4: 16 };   // MouseEvent.button -> HID bit
screenEl.addEventListener('mousedown', (e) => {
  if (!document.pointerLockElement) {
    screenEl.focus();
    if (kbMode === 'usb' || e.shiftKey) screenEl.requestPointerLock?.({ unadjustedMovement: true })
      ?.catch?.(() => screenEl.requestPointerLock());
    return;
  }
  if (kbMode === 'usb') { mouse.buttons |= HID_BUTTON[e.button] ?? 0; mouse.dirty = true; }
  e.preventDefault();
});
window.addEventListener('mouseup', (e) => {
  if (kbMode === 'usb' && document.pointerLockElement === screenEl) {
    mouse.buttons &= ~(HID_BUTTON[e.button] ?? 0); mouse.dirty = true;
  }
});
document.addEventListener('mousemove', (e) => {
  if (document.pointerLockElement !== screenEl) return;
  if (kbMode === 'usb') { mouse.dx += e.movementX; mouse.dy += e.movementY; mouse.dirty = true; }
  else { aim[0] = Math.max(-1, Math.min(1, aim[0] + e.movementX * AIM_GAIN));
    aim[1] = Math.max(-1, Math.min(1, aim[1] + e.movementY * AIM_GAIN)); }
});
screenEl.addEventListener('wheel', (e) => {
  if (document.pointerLockElement !== screenEl || kbMode !== 'usb') return;
  e.preventDefault();
  mouse.wheel += e.deltaY > 0 ? -1 : e.deltaY < 0 ? 1 : 0; mouse.dirty = true;
}, { passive: false });
screenEl.addEventListener('contextmenu', (e) => e.preventDefault());
document.addEventListener('pointerlockchange', () => {
  const locked = document.pointerLockElement === screenEl;
  screenEl.classList.toggle('locked', locked);
  if (!locked) { mouse.buttons = 0; mouse.dirty = true; aim[0] = aim[1] = 0; }
});

const hint = () => {
  $('#capture-hint').textContent = kbMode === 'pad'
    ? 'Click the screen to drive with the keyboard · Shift+click to aim with the mouse · Esc to release'
    : 'Click the screen to send keyboard & mouse to the console · Esc releases the mouse · Ctrl+Alt+U releases the keyboard';
};
document.querySelectorAll('input[name=kbmode]').forEach((r) => {
  r.checked = r.value === kbMode;
  r.addEventListener('change', () => {
    releaseAll(); kbMode = r.value; store.set('kbmode', kbMode); hint();
    if (document.pointerLockElement) document.exitPointerLock();
  });
});
hint();

// ------------------------------------------------------------ status, activity, tools
let crashOffered = false;
function showStatus(s) {
  updateConsoles(s);
  // the picture of the console this page shows (status pushes are about the controller's)
  const shown = viewed();
  if (shown?.video_status) s = { ...s, video: shown.video_status };
  $('#safe-mode').hidden = !s.safe_mode;
  if (s.last_run_unclean && !crashOffered) {
    crashOffered = true;
    toast(t('NS2Bench closed unexpectedly last time. A bug report helps fix it.'), 'warn',
      { label: t('Save bug report'), run: () => { document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'help' })); setTimeout(() => $('#help-bundle').click(), 300); } });
  }
  if (s.version && $('#brand-version').textContent !== `v${s.version}`) $('#brand-version').textContent = `v${s.version}`;
  const v = $('#pill-video'), i = $('#pill-input');
  const live = s.video.age_s !== null && s.video.age_s < 3;
  // Whole numbers only: changing widths made the toolbar jump between one and two rows.
  v.textContent = live ? `${t('video')} ${Math.round(s.video.fps)} fps` : `${t('video')} · ${s.video.error ?? t('waiting')}`;
  v.title = live ? `${t('The capture card is sending the console picture.')} ${t('Click for video settings.')}`
    : `${s.video.error ?? t('No picture yet.')} — ${t('Check the HDMI cable goes into the card’s input and the console is awake. Click for video settings.')}`;
  v.className = live ? 'pill ok' : 'pill bad';
  const inp = s.input;
  const wasReady = bridgeStatus?.connected && bridgeStatus?.usb === 'configured';
  bridgeStatus = inp; bridgeSeen = performance.now();
  if (wasReady && (!inp.connected || inp.usb !== 'configured')) releaseAll();
  if (!inputUnavailable()) inputWarning.hidden = true;
  const extra = inp.usb_mode === 'pro' ? ` · ${t('Pro Controller')}` : inp.devices?.includes('mouse') ? ` · ${t('keyboard + mouse')}` : '';
  const usb = inp.usb === 'configured' ? t('console ready') : t('console not ready');
  i.textContent = inp.connected ? `Bridge ${inp.rtt_ms == null ? '?' : Math.max(1, Math.round(inp.rtt_ms))} ms · ${usb}${extra}`
    : `Bridge · ${inp.backend === 'none' ? t('not set up') : t('offline, reconnecting')}`;
  i.title = inp.connected && inp.usb === 'configured' ? `${t('The input device is connected and the console sees it.')} ${t('Click for input settings.')}`
    : inp.backend === 'none' ? `${t('No input device is set up, so nothing can press buttons yet.')} ${t('Click to choose one.')}`
      : inp.connected ? `${t('Connected, but the console hasn’t accepted the USB device yet (it may be asleep).')} ${t('Click for input settings.')}`
        : `${inp.detail ?? t('Not connected.')} — ${t('Check the device is powered (the console must be awake) and its address. Click for input settings.')}`;
  i.className = inp.connected && inp.usb === 'configured' ? 'pill ok' : 'pill bad';
  $('#manual-only').checked = inp.manual_only;
  if (s.ocr) $('#log-screen').checked = s.ocr.screen_log;
  // No picture: say why inside the screen, with the way to fix it.
  $('#no-picture').hidden = live || !s.video.error && performance.now() < 6000;
  if (!live) $('#no-picture-why').textContent = s.video.error ? `${t('Capture says:')} ${s.video.error}` : t('Waiting for the first picture…');
  $('#step-video-why').textContent = live ? '' : (s.video.error ?? '');
  $('#step-input-why').textContent = inp.connected && inp.usb === 'configured' || inp.backend === 'none' ? '' : (inp.detail ?? (inp.connected ? t('console not ready') : ''));
  welcome(live, inp);
  const mode = inp.usb_mode ?? 'standard';
  consoleEl.classList.toggle('pro', mode === 'pro');
  const sel = $('#usb-mode');
  if (sel && !sel.dataset.busy) sel.value = mode;
  syncDriving();
  updatePanel(s);
}

// ------------------------------------------------------------ Driving card: two plain choices
// "The console sees" (USB device the input board presents) and "Your keyboard sends"
// (what your keys turn into). Options the connected device can't do are disabled.
function syncDriving() {
  const inp = bridgeStatus || {};
  const mode = $('#usb-mode').value || 'standard';
  const devices = inp.devices || [];
  const proPossible = inp.backend === 'hidd';                // only Linux boards can be a Pro Controller
  const hasPad = !inp.connected || devices.includes('pad') || mode === 'pro';
  const hasKbd = !inp.connected || devices.includes('kbd');
  for (const b of $('#seg-usb').children) {
    b.setAttribute('aria-checked', String(b.dataset.v === mode));
    b.disabled = !!$('#usb-mode').dataset.busy || (b.dataset.v === 'pro' && !proPossible && mode !== 'pro');
    b.title = b.dataset.v === 'pro' && !proPossible ? t('Needs a Linux board as the input device') : '';
  }
  $('#usb-help').textContent = mode === 'pro'
    ? t('A Switch 2 Pro Controller: every button, including C (GameChat) and the GL/GR back buttons. No keyboard or mouse.')
    : t('A wired controller, a keyboard and a mouse. Every button except C and the Pro Controller’s GL/GR back buttons.');
  // A device without a controller (CH9329) can only take keyboard and mouse.
  if (inp.connected && !hasPad && hasKbd && kbMode === 'pad') document.querySelector('input[name=kbmode][value=usb]').click();
  for (const b of $('#seg-kb').children) {
    b.setAttribute('aria-checked', String(b.dataset.v === kbMode));
    b.disabled = b.dataset.v === 'usb' ? (mode === 'pro' || !hasKbd) : !hasPad;
  }
  $('#kb-help').textContent = kbMode === 'pad'
    ? t('Keys press controller buttons (see the keyboard map). Shift+click aims with the mouse.')
    : t('The console gets your real keyboard and mouse, for text boxes and apps that use them.');
}
$('#seg-usb').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-v]');
  const sel = $('#usb-mode');
  if (!b || b.disabled || sel.value === b.dataset.v) return;
  sel.value = b.dataset.v;
  sel.dispatchEvent(new Event('change'));
  // Pro mode has no keyboard: fall back to sending controller buttons.
  if (b.dataset.v === 'pro' && kbMode === 'usb') document.querySelector('input[name=kbmode][value=pad]').click();
  syncDriving();
});
$('#seg-kb').addEventListener('click', (e) => {
  const b = e.target.closest('button[data-v]');
  if (!b || b.disabled) return;
  document.querySelector(`input[name=kbmode][value=${b.dataset.v}]`).click();
  syncDriving();
});
document.addEventListener('ns2bench:language', syncDriving);
syncDriving();

function addEvent(ev) {
  if (ev.action === 'mouse') return;               // too chatty for the log
  if (ev.source === 'error' && ev.toast !== false) toast(String(ev.action), 'warn');
  const li = document.createElement('li');
  const agent = ev.source !== 'ui';
  const time = new Date(ev.t * 1000).toLocaleTimeString();
  const what = [ev.action, ev.buttons?.join('+'), ev.key, ev.text && JSON.stringify(ev.text),
    ev.stick && `${ev.stick} ${ev.x},${ev.y}`].filter(Boolean).join(' ');
  li.innerHTML = '<span class="src"></span><span class="what"></span>';
  li.querySelector('.src').textContent = agent ? ev.source : 'you';
  li.querySelector('.src').classList.toggle('agent', agent);
  li.querySelector('.what').textContent = `${time} ${what}`;
  $('#log').prepend(li);
  while ($('#log').children.length > 100) $('#log').lastChild.remove();
  if (agent && ev.source !== 'error') $('#driver').textContent = `${t('Last agent input:')} ${ev.source} (${what})`;
}

$('#usb-mode').addEventListener('change', async (e) => {
  const sel = e.target;
  sel.dataset.busy = '1'; sel.disabled = true;
  $('#usb-note').textContent = t('Switching… the console sees the controller reconnect (about 10 s).');
  const idle = () => { delete sel.dataset.busy; sel.disabled = false; syncDriving(); };
  let refused = '';
  try {
    const r = await fetch('/api/control', { method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ usb_mode: sel.value }) });
    if (!r.ok) refused = await r.text();
  } catch (err) { refused = String(err.message || err); }
  // Not switched: say why and give the control back at once.
  if (refused) { $('#usb-note').textContent = refused; idle(); return; }
  syncDriving();
  setTimeout(() => { $('#usb-note').textContent = ''; idle(); }, 12000);
});
$('#manual-only').addEventListener('change', async (e) => {
  let refused = '';
  try {
    const r = await fetch('/api/control', { method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ manual_only: e.target.checked }) });
    if (!r.ok) refused = await r.text();
  } catch (err) { refused = String(err.message || err); }
  if (refused) addEvent({ source: 'error', action: refused, t: Date.now() / 1000 });
});

$('#btn-shot').addEventListener('click', async () => {
  const r = await fetch('/api/screenshot?format=png', { headers: shown() });
  if (!r.ok) return addEvent({ source: 'error', action: await r.text(), t: Date.now() / 1000 });
  const a = Object.assign(document.createElement('a'), {
    href: URL.createObjectURL(await r.blob()),
    download: `ns2bench-${new Date().toISOString().replace(/[:.]/g, '-')}.png` });
  a.click();
  toast(t('Screenshot saved to your Downloads folder.'), 'ok');
  // WebKit fails blob downloads revoked at once.
  setTimeout(() => URL.revokeObjectURL(a.href), 2000);
});

// ------------------------------------------------------------ console sound
let audioCtx = null, audioWs = null, audioNode = null, audioFails = 0, audioGen = 0, audioRetry = 0;
// The context is made once per "Sound on" (inside the click: WebKit keeps a context made later
// suspended); a dropped socket only reopens the socket.
async function soundOn() {
  const gen = ++audioGen;
  const ctx = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' });
  audioCtx = ctx;
  await ctx.resume().catch(() => {});
  await ctx.audioWorklet.addModule('/static/audio-worklet.js');
  if (gen !== audioGen) { ctx.close(); return; }          // turned off (or on again) meanwhile
  audioNode = new AudioWorkletNode(ctx, 'pcm-player', { outputChannelCount: [2] });
  audioNode.connect(ctx.destination);
  openAudio(gen);
}
function openAudio(gen) {
  if (gen !== audioGen || !audioCtx) return;
  const sock = new WebSocket(authURL('/audio').replace(/^http/, 'ws'));
  audioWs = sock;
  sock.binaryType = 'arraybuffer';
  sock.onmessage = (e) => { audioFails = 0; if (typeof e.data !== 'string') audioNode?.port.postMessage(e.data, [e.data]); };
  // Reconnect with backoff; after repeated failures give up and say so.
  sock.onclose = () => {
    if (gen !== audioGen || !audioCtx) return;
    if (++audioFails > 5) { soundOff(); setSoundButton(false); addEvent({ source: 'error', action: t('No sound from the capture card. Turn sound on again to retry.'), t: Date.now() / 1000 }); return; }
    clearTimeout(audioRetry);
    audioRetry = setTimeout(() => openAudio(gen), 1000 * audioFails);
  };
}
function soundOff() {
  audioGen++;
  clearTimeout(audioRetry);
  const ctx = audioCtx;
  audioCtx = null;
  // Detach first: a socket closed on purpose must not schedule a reconnect.
  if (audioWs) { audioWs.onclose = null; audioWs.close(); }
  audioWs = null;
  audioNode = null;
  ctx?.close();
}
function setSoundButton(on) {
  const btn = $('#btn-sound');
  btn.setAttribute('aria-pressed', String(on));
  btn.querySelector('span').textContent = on ? t('Sound on') : t('Sound off');
  btn.classList.toggle('done', on);
}
$('#btn-sound').addEventListener('click', async (e) => {
  const on = e.currentTarget.getAttribute('aria-pressed') !== 'true';
  setSoundButton(on);
  audioFails = 0;
  if (!on) return soundOff();
  const gen = audioGen + 1;
  try { await soundOn(); } catch (err) {
    if (gen !== audioGen) return;                        // an older attempt: the newer one stands
    soundOff(); setSoundButton(false);
    addEvent({ source: 'error', action: String(err.message || err), t: Date.now() / 1000 });
  }
});
$('#btn-clip').addEventListener('click', async (e) => {
  const btn = e.currentTarget, label = btn.querySelector('span');
  label.textContent = 'Saving…';
  let r;
  try {
    r = await fetch('/api/clip', { method: 'POST', headers: shown({ 'Content-Type': 'application/json' }),
      body: JSON.stringify({ label: 'manual' }) });
  } catch (err) {
    label.textContent = 'Save clip';
    addEvent({ source: 'error', action: String(err.message || err), t: Date.now() / 1000 });
    return;
  }
  if (r.ok) {
    const d = await r.json();
    label.textContent = `${t('Saved')} ${d.seconds} s`;
    toast(`${t('Clip saved:')} ${d.seconds} s. ${t('It is in your Downloads folder.')}`, 'ok');
    // Download through the engine (the desktop app saves it to Downloads).
    Object.assign(document.createElement('a'), { href: authURL(d.url), download: d.url.split('/').pop() }).click();
  } else {
    label.textContent = 'Save clip';
    addEvent({ source: 'error', action: await r.text(), t: Date.now() / 1000 });
  }
  setTimeout(() => { label.textContent = 'Save clip'; }, 2500);
});
// Keys keep driving in and out of fullscreen: focus goes back to the picture.
$('#btn-full').addEventListener('click', async () => {
  await consoleEl.requestFullscreen?.().catch(() => {});
  $('#screen').focus({ preventScroll: true });
});
document.addEventListener('fullscreenchange', () => { if (!document.fullscreenElement) $('#screen').focus({ preventScroll: true }); });
document.addEventListener('fullscreenchange', fit);
let mjpegFails = 0;
$('#video').addEventListener('load', () => { mjpegFails = 0; });
$('#video').addEventListener('error', () => setTimeout(() => {
  if (live.mode === 'mjpeg') $('#video').src = authURL(`/stream.mjpg?r=${Date.now()}`); }, Math.min(15000, 1000 * ++mjpegFails)));
$('#video').src = authURL('/stream.mjpg');
const live = startLive($('#video'), (m) => { $('#screen').dataset.live = m; });
// Another console picked in the console bar: let go of what this page held, then reconnect the
// picture, sound and status for that console.
// Headers naming the console on screen (also when the page follows the controller's console).
const shown = (h = {}) => (viewed()?.id ? { ...h, 'X-NS2Bench-Console': viewed().id } : h);
// another console on screen: its picture and sound, and its own video settings in the panel
mountConsoles(() => { releaseAll(); $('#btn-refresh').click(); loadVideo().catch((e) => { $('#v-note').textContent = String(e); }); });

// ------------------------------------------------------------ video settings
const vsel = { device: $('#v-device'), size: $('#v-size'), fps: $('#v-fps'), q: $('#v-quality') };
let vdevices = [];
const opt = (value, label, sel) => Object.assign(document.createElement('option'), { value, textContent: label, selected: sel });
function fillSizes(settings) {
  const dev = vdevices.find((d) => d.id === vsel.device.value) ?? vdevices[0];
  const formats = dev?.formats ?? [];
  const want = `${settings.width}x${settings.height}`;
  vsel.size.replaceChildren(...formats.map((f) => opt(`${f.width}x${f.height}`,
    `${f.width}×${f.height} (≤${Math.round(f.max_fps)} fps)`, `${f.width}x${f.height}` === want)));
  if (!formats.length) for (const [w, h] of [[settings.width, settings.height], [1920, 1080], [1280, 720]]) {
    const v = `${w}x${h}`;
    if (![...vsel.size.options].some((o) => o.value === v)) vsel.size.append(opt(v, `${w}×${h}`, v === want));
  }
  fillFps(settings.fps);
}
function fillFps(want) {
  const dev = vdevices.find((d) => d.id === vsel.device.value) ?? vdevices[0];
  const f = dev?.formats.find((x) => `${x.width}x${x.height}` === vsel.size.value);
  const max = Math.round(f?.max_fps ?? 60);
  const rates = [120, 60, 50, 30, 25, 20, 15, 10, 5].filter((r) => r <= max);
  vsel.fps.replaceChildren(...rates.map((r) => opt(r, `${r}`, r === Number(want))));
  if (![...vsel.fps.options].some((o) => o.selected)) vsel.fps.value = String(Math.min(max, 60));
}
async function loadVideo() {
  $('#v-note').textContent = t('Loading devices…');
  const d = await (await fetch('/api/video', { headers: shown() })).json();
  // every capture card, also one that didn't say which sizes it offers (it still works)
  vdevices = d.devices.filter((x) => x.kind === 'video' || (x.kind !== 'audio' && (x.formats?.length || x.raw)));
  const s = d.settings;
  // Windows and Linux: the card's sound is a separate device (macOS pairs it by itself).
  const sounds = d.devices.filter((x) => x.kind === 'audio');
  $('#v-audio-field').hidden = !sounds.length && !s.audio_device;
  $('#v-audio').replaceChildren(opt('', t('Match the capture card (automatic)'), !s.audio_device),
    ...sounds.map((x) => opt(x.id, x.name, x.id === s.audio_device)));
  if (s.audio_device && !sounds.some((x) => x.id === s.audio_device)) $('#v-audio').append(opt(s.audio_device, s.audio_device, true));
  vsel.device.replaceChildren(...vdevices.map((x) => opt(x.id ?? '', `${x.name}${x.in_use ? ' (in use)' : ''} · ${x.id ?? ''}`, x.id === s.device)));
  // this console's card while it's unplugged (or Apply would hand it whichever card is listed first)
  if (s.device && !vdevices.some((x) => x.id === s.device)) vsel.device.prepend(opt(s.device, `${s.device} (${t('not connected')})`, true));
  // no card yet: say so instead of showing another console's
  if (!s.device) vsel.device.prepend(opt('', t('Not set: use a free card when one is connected'), true));
  vsel.q.value = s.quality; $('#v-qval').textContent = s.quality;
  fillSizes(s);
  $('#v-note').textContent = '';
}
// Every change is saved for the console on screen, by itself (no Apply): it's that console's.
let saveTimer = 0;
async function saveVideo() {
  const who = viewed();
  const [width, height] = vsel.size.value.split('x').map(Number);
  const body = { device: vsel.device.value || null, width, height, fps: Number(vsel.fps.value), quality: Number(vsel.q.value) };
  if (!$('#v-audio-field').hidden) body.audio_device = $('#v-audio').value || null;
  $('#v-note').textContent = t('Saving…');
  const r = await fetch('/api/video', { method: 'POST', headers: shown({ 'Content-Type': 'application/json' }), body: JSON.stringify(body) });
  if (r.ok) {
    $('#v-note').textContent = who ? t('Saved for %1').replace('%1', who.name) : t('Saved');
    setTimeout(() => { if (live.mode === 'mjpeg') $('#video').src = authURL(`/stream.mjpg?r=${Date.now()}`); live.reconnect(); }, 300);
  } else {
    $('#v-note').textContent = await r.text();
    loadVideo().catch(() => {});              // show what is really saved
  }
}
const saveSoon = () => { clearTimeout(saveTimer); saveTimer = setTimeout(() => saveVideo().catch((e) => { $('#v-note').textContent = String(e); }), 400); };
vsel.device.addEventListener('change', () => { fillSizes({ width: 1920, height: 1080, fps: 60 }); saveSoon(); });
vsel.size.addEventListener('change', () => { fillFps(vsel.fps.value); saveSoon(); });
vsel.fps.addEventListener('change', saveSoon);
vsel.q.addEventListener('input', () => { $('#v-qval').textContent = vsel.q.value; });
vsel.q.addEventListener('change', saveSoon);
$('#v-audio').addEventListener('change', saveSoon);
loadVideo().catch((e) => { $('#v-note').textContent = String(e); });

// ------------------------------------------------------------ logs panel
const LEVELS = ['debug', 'info', 'warn', 'error'];
const rank = (l) => Math.max(0, LEVELS.indexOf(l));
const ICON = {
  debug: '<circle cx="12" cy="12" r="2.5" fill="currentColor"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5h.01"/>',
  warn: '<path d="M12 3 2 20h20z"/><path d="M12 10v4M12 17h.01"/>',
  error: '<circle cx="12" cy="12" r="9"/><path d="m9 9 6 6M15 9l-6 6"/>',
  hint: '<path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2.1h5c0-.9.4-1.6 1-2.1A6 6 0 0 0 12 3z"/>',
};
const icon = (name) => `<svg class="li" viewBox="0 0 24 24" aria-hidden="true">${ICON[name]}</svg>`;
const logState = { entries: [], counts: {}, levelCounts: { debug: 0, info: 0, warn: 0, error: 0 }, source: store.get('logsource', ''),
  level: LEVELS.includes(store.get('loglevel', 'info')) ? store.get('loglevel', 'info') : 'info', filter: '' };
const logview = $('#logview');
const fmtTime = (t) => new Date(t * 1000).toLocaleTimeString([], { hour12: false });
const sourceLabel = (source) => source === 'board' || source === 'hidd' ? 'bridge' : source;

function renderChips() {
  const box = $('#log-sources');
  const names = ['', ...Object.keys(logState.counts).sort()];
  box.replaceChildren(...names.map((n) => {
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'chip';
    b.setAttribute('aria-pressed', String(n === logState.source));
    b.innerHTML = `<span></span><span class="n"></span>`;
    b.firstChild.textContent = sourceLabel(n) || t('all');
    b.lastChild.textContent = n ? logState.counts[n] : logState.entries.length;
    b.addEventListener('click', () => { logState.source = n; store.set('logsource', n); renderChips(); renderLogs(); });
    return b;
  }));
  const lv = $('#log-levels');
  lv.replaceChildren(...['debug', 'info', 'warn', 'error'].map((l) => {
    const b = document.createElement('button');
    b.type = 'button'; b.className = `chip lvl-${l}`;
    b.title = { debug: t('Show everything, including API calls and other detail'), info: t('Hide detail; show normal activity and problems'),
      warn: t('Only warnings and errors'), error: t('Only errors') }[l];
    b.setAttribute('aria-pressed', String(l === logState.level));
    const n = LEVELS.slice(rank(l)).reduce((a, k) => a + logState.levelCounts[k], 0);
    b.innerHTML = `${icon(l)}<span></span><span class="n"></span>`;
    b.children[1].textContent = { debug: t('Everything'), info: t('Info'), warn: t('Problems'), error: t('Errors') }[l];
    b.children[2].textContent = n;
    b.addEventListener('click', () => { logState.level = l; store.set('loglevel', l); renderChips(); renderLogs(); });
    return b;
  }));
}
const visible = (e) => (!logState.source || e.source === logState.source) && rank(e.level) >= rank(logState.level)
  && (!logState.filter || e.text.toLowerCase().includes(logState.filter) || (e.hint || '').toLowerCase().includes(logState.filter));
function row(e) {
  const li = document.createElement('li');
  const level = LEVELS.includes(e.level) ? e.level : 'info';
  li.className = `lv-${level}`;
  li.innerHTML = `<span class="ts"></span><span class="lv">${icon(level)}</span><span class="sv"></span><span class="tx"></span>`;
  li.children[1].title = level;
  li.children[0].textContent = fmtTime(e.t);
  li.children[2].textContent = sourceLabel(e.source); li.children[2].dataset.s = e.source;
  li.children[3].textContent = e.text;
  if (e.hint) {
    const h = document.createElement('span');
    h.className = 'why';
    h.innerHTML = icon('hint');
    h.append(e.hint);
    li.children[3].append(h);
  }
  return li;
}
function renderLogs() {
  logview.replaceChildren(...logState.entries.filter(visible).slice(-1000).map(row));
  logview.scrollTop = logview.scrollHeight;
}
let chipTimer = 0;
function addLog(e) {
  logState.entries.push(e);
  logState.levelCounts[e.level in logState.levelCounts ? e.level : 'info']++;
  const fresh = !(e.source in logState.counts);
  logState.counts[e.source] = (logState.counts[e.source] ?? 0) + 1;
  if (logState.entries.length > 5000) {
    // dropped lines leave the chip counts too
    for (const old of logState.entries.splice(0, 1000)) {
      logState.counts[old.source]--;
      logState.levelCounts[old.level in logState.levelCounts ? old.level : 'info']--;
    }
  }
  if (fresh) renderChips(); else if (!chipTimer) chipTimer = setTimeout(() => { chipTimer = 0; renderChips(); }, 500);
  if ($('#log-pause').checked || !visible(e)) return;
  const stick = logview.scrollHeight - logview.scrollTop - logview.clientHeight < 40;
  logview.append(row(e));
  while (logview.children.length > 1000) logview.firstChild.remove();
  if (stick) logview.scrollTop = logview.scrollHeight;
}
async function copyText(btn, text) {
  try { await navigator.clipboard.writeText(text); } catch {
    const ta = Object.assign(document.createElement('textarea'), { value: text });
    document.body.append(ta); ta.select(); document.execCommand('copy'); ta.remove();
  }
  const label = btn.querySelector('span');
  const old = label?.textContent;
  btn.classList.add('done'); if (label) label.textContent = t('Copied');
  setTimeout(() => { btn.classList.remove('done'); if (label) label.textContent = old; }, 1400);
}
const logText = () => logState.entries.filter(visible)
  .map((e) => `${new Date(e.t * 1000).toISOString()} ${e.level.toUpperCase()} [${e.source}] ${e.text}${e.hint ? `\n    → ${e.hint}` : ''}`).join('\n');
$('#log-copy').addEventListener('click', (e) => copyText(e.currentTarget, logText()));
// Reconnect everything the page listens to (after sleep, a cable change or an engine restart).
$('#btn-refresh').addEventListener('click', (e) => {
  const b = e.currentTarget;
  b.classList.remove('spin'); void b.offsetWidth; b.classList.add('spin');
  if (live.mode === 'mjpeg') $('#video').src = authURL(`/stream.mjpg?r=${Date.now()}`);
  live.reconnect();
  ws?.close();                                   // reopens by itself and reloads status and missed logs
  if (audioCtx) { soundOff(); soundOn().catch(() => { soundOff(); setSoundButton(false); }); }
});
$('#btn-update').addEventListener('click', () => document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'updates' })));
$('#btn-help').addEventListener('click', () => document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'help' })));
// click a log line to copy it
logview.addEventListener('click', (e) => {
  const li = e.target.closest('li');
  if (!li || window.getSelection()?.toString()) return;
  navigator.clipboard?.writeText([...li.children].map((c) => c.textContent).join(' '));
  li.classList.add('copied'); setTimeout(() => li.classList.remove('copied'), 700);
});
$('#log-filter').addEventListener('input', (e) => { logState.filter = e.target.value.toLowerCase(); renderLogs(); });
$('#log-screen').addEventListener('change', (e) =>
  fetch('/api/control', { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ screen_log: e.target.checked }) }));
$('#log-pause').addEventListener('change', (e) => { if (!e.target.checked) renderLogs(); });
$('#log-download').addEventListener('click', () => {
  const text = logText();
  const a = Object.assign(document.createElement('a'), {
    href: URL.createObjectURL(new Blob([text], { type: 'text/plain' })),
    download: `ns2bench-logs-${new Date().toISOString().replace(/[:.]/g, '-')}.txt` });
  a.click(); setTimeout(() => URL.revokeObjectURL(a.href), 2000);
});
// History at start, and whatever was logged while the connection was down after a reconnect.
// Lines are kept in order and once each (the socket may deliver some of the same ones).
let logsLoaded = false;
let lastShown = '';

function loadLogs() {
  const last = logState.entries.reduce((m, e) => Math.max(m, e.seq ?? 0), 0);
  return fetch(`/api/logs?limit=2000&level=debug${last ? `&after=${last}` : ''}`).then((r) => r.json()).then((d) => {
    const have = new Set(logState.entries.map((e) => e.seq));
    const fresh = d.entries.filter((e) => !have.has(e.seq));
    fresh.forEach((e) => fromLog(e, false));     // earlier notifications fill the bell's list
    logState.entries.push(...fresh);
    logState.entries.sort((a, b) => (a.seq ?? 0) - (b.seq ?? 0));
    logState.counts = {};
    for (const k in logState.levelCounts) logState.levelCounts[k] = 0;
    for (const e of logState.entries) {
      logState.counts[e.source] = (logState.counts[e.source] ?? 0) + 1;
      logState.levelCounts[e.level in logState.levelCounts ? e.level : 'info']++;
    }
    logsLoaded = true;
    renderChips(); renderLogs();
  }).catch(() => {});
}
loadLogs();

fetch('/api/events').then((r) => r.json()).then((evs) => evs.forEach(addEvent));
connect();
requestAnimationFrame(tick);

// Status pills are shortcuts: Bridge opens its settings, video jumps to the Video panel.
const activate = (el, fn) => {
  el.addEventListener('click', fn);
  el.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); fn(); } });
};
activate($('#pill-input'), () => document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'devices' })));
activate($('#pill-video'), () => {
  const card = $('#video-card');
  card.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  card.classList.remove('flash'); void card.offsetWidth; card.classList.add('flash');
  $('#v-device').focus({ preventScroll: true });
});

// Getting-started card: ticks itself off and goes away once video and input work.
let drove = store.get('drove', false);
function welcome(videoOk, inp) {
  const inputOk = inp.backend === 'none' ? store.get('viewOnly', false) : inp.connected && inp.usb === 'configured';
  $('#step-video').classList.toggle('done', videoOk);
  $('#step-input').classList.toggle('done', !!inputOk);
  $('#step-drive').classList.toggle('done', drove);
  // Once everything has worked, stay hidden (a sleeping console must not bring it back).
  if (videoOk && inputOk && drove && !store.get('welcomeHidden', false)) store.set('welcomeHidden', true);
  $('#welcome').hidden = !welcomeAsked && store.get('welcomeHidden', false);
}
// Help → Show getting started brings the card back until it is closed again.
let welcomeAsked = false;
document.addEventListener('ns2bench:welcome', () => {
  welcomeAsked = true; $('#welcome').hidden = false;
  $('#welcome').scrollIntoView({ behavior: 'smooth', block: 'nearest' });
});
// Each step goes where it is done.
for (const b of document.querySelectorAll('#welcome .step')) {
  b.addEventListener('click', () => {
    const step = b.dataset.step;
    if (step === 'video') $('#pill-video').click();
    else if (step === 'input') document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'devices' }));
    else if (step === 'drive') $('#screen').focus();
    else document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'integrations' }));
  });
}
$('#welcome-hide').addEventListener('click', () => { welcomeAsked = false; store.set('welcomeHidden', true); $('#welcome').hidden = true; });
$('#welcome-devices').addEventListener('click', () => document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'devices' })));
$('#no-picture-video').addEventListener('click', (e) => { e.stopPropagation(); $('#pill-video').click(); });
$('#no-picture-wake').addEventListener('click', (e) => { e.stopPropagation(); document.dispatchEvent(new CustomEvent('ns2bench:wake')); });
$('#no-picture-check').addEventListener('click', (e) => {
  e.stopPropagation();
  document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'help' }));
  setTimeout(() => $('#help-doctor').click(), 300);
});
$('#screen').addEventListener('focus', () => { if (!drove) { drove = true; store.set('drove', true); } });

// ------------------------------------------------------------ shortcuts and the action search
// Outside the picture and text fields: ? this list, Ctrl/⌘K search, "," settings, F fullscreen,
// S screenshot, M sound. Inside the picture every key goes to the console.
function showKeyMap() {
  const keys = $('details.keys');
  keys.open = true;
  keys.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  keys.classList.remove('flash'); void keys.offsetWidth; keys.classList.add('flash');
}
$('#shortcuts-map').addEventListener('click', () => { $('#shortcuts').close(); showKeyMap(); });
for (const d of document.querySelectorAll('#shortcuts, #whatsnew')) d.querySelector('[data-close]')?.addEventListener('click', () => d.close());
const settingsTab = (tab) => () => document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: tab }));
const ACTIONS = [
  ['Screenshot', 'save picture image capture', () => $('#btn-shot').click()],
  ['Save clip', 'record video last 30 seconds', () => $('#btn-clip').click()],
  ['Fullscreen', 'full screen big', () => $('#btn-full').click()],
  ['Sound on or off', 'audio mute', () => $('#btn-sound').click()],
  ['Reconnect the picture, sound and status', 'refresh reload retry', () => $('#btn-refresh').click()],
  ['Switch between light and dark', 'theme dark light mode', () => $('#btn-theme').click()],
  ['Video settings', 'capture card device resolution fps', () => $('#pill-video').click()],
  ['Wake the console', 'power on sleep wake cec black screen', () => document.dispatchEvent(new CustomEvent('ns2bench:wake'))],
  ['Power and wake', 'power button switchbot cec sleep', () => { settingsTab('devices')(); setTimeout(() => $('#power-settings').scrollIntoView({ block: 'center' }), 300); }],
  ['Input device (Bridge)', 'devices bridge pico ch9329 serial input', settingsTab('devices')],
  ['Calibrate presses', 'calibrate press length timing', () => { settingsTab('devices')(); setTimeout(() => $('#calibrate').scrollIntoView({ block: 'center' }), 300); }],
  ['Console network', 'network dhcp dns offline', settingsTab('network')],
  ['Connect an agent', 'mcp claude codex opencode ai agent', settingsTab('integrations')],
  ['API', 'http api token curl', settingsTab('integrations')],
  ['Block agents (on or off)', 'manual only stop agent', () => $('#manual-only').click()],
  ['Install command line tool', 'cli terminal path pico board helper install', settingsTab('install')],
  ['Appearance', 'theme text size language accessibility font', settingsTab('appearance')],
  ['Notifications', 'alerts bell', settingsTab('notifications')],
  ['Check for updates', 'update version upgrade', settingsTab('updates')],
  ['What’s new', 'changelog release notes', () => { settingsTab('updates')(); setTimeout(() => $('#whatsnew-open').click(), 300); }],
  ['Run the check', 'doctor diagnose broken problem', () => { settingsTab('help')(); setTimeout(() => $('#help-doctor').click(), 300); }],
  ['Help', 'faq problems report bug', settingsTab('help')],
  ['Save bug report', 'bundle zip issue', () => { settingsTab('help')(); setTimeout(() => $('#help-bundle').click(), 300); }],
  ['Show getting started', 'welcome onboarding setup', () => document.dispatchEvent(new CustomEvent('ns2bench:welcome'))],
  ['Show the keyboard map', 'keys keyboard map controls', showKeyMap],
  ['Keyboard shortcuts', 'shortcuts hotkeys', () => $('#shortcuts').showModal()],
  ['About', 'version license support thanks', settingsTab('about')],
];
let paletteHits = [], paletteAt = 0;
function renderPalette() {
  const q = $('#palette-q').value.trim().toLowerCase();
  // Names that match come before matches on a related word.
  const rank = ([label, words]) => (!q ? 0 : `${t(label)} ${label}`.toLowerCase().includes(q) ? 0 : words.includes(q) ? 1 : 2);
  paletteHits = ACTIONS.map((a, i) => [a, rank(a), i]).filter(([, r]) => r < 2).sort((x, y) => x[1] - y[1] || x[2] - y[2]).map(([a]) => a).slice(0, 12);
  paletteAt = Math.min(paletteAt, Math.max(0, paletteHits.length - 1));
  $('#palette-list').replaceChildren(...paletteHits.map(([label], i) => {
    const li = Object.assign(document.createElement('li'), { textContent: t(label) });
    li.setAttribute('role', 'option'); li.setAttribute('aria-selected', String(i === paletteAt));
    li.addEventListener('click', () => runPalette(i));
    return li;
  }));
}
function runPalette(i) { const a = paletteHits[i]; if (!a) return; $('#palette').close(); a[2](); }
function openPalette() { $('#palette-q').value = ''; paletteAt = 0; renderPalette(); $('#palette').showModal(); $('#palette-q').focus(); }
$('#palette-q').addEventListener('input', () => { paletteAt = 0; renderPalette(); });
$('#palette-q').addEventListener('keydown', (e) => {
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); paletteAt = (paletteAt + (e.key === 'ArrowDown' ? 1 : -1) + paletteHits.length) % Math.max(1, paletteHits.length); renderPalette(); }
  else if (e.key === 'Enter') { e.preventDefault(); runPalette(paletteAt); }
});
$('#palette').addEventListener('click', (e) => { if (e.target === e.currentTarget) e.currentTarget.close(); });
document.addEventListener('keydown', (e) => {
  if (e.repeat || document.querySelector('dialog[open]') || e.target.closest?.('#screen, input, select, textarea, dialog')) return;
  if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === 'k') { e.preventDefault(); openPalette(); return; }
  if ((e.metaKey || e.ctrlKey) && e.key === ',') { e.preventDefault(); $('#btn-settings').click(); return; }
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  const act = { '?': () => $('#shortcuts').showModal(), ',': () => $('#btn-settings').click(), f: () => $('#btn-full').click(),
    s: () => $('#btn-shot').click(), m: () => $('#btn-sound').click() }[e.key.length === 1 ? e.key.toLowerCase() : e.key];
  if (act) { e.preventDefault(); act(); }
});

// ------------------------------------------------------------ fullscreen toolbar
// Everything you need without leaving fullscreen: status, Joy-Con and key labels,
// sound, screenshot, clip, exit. Buttons reuse the main toolbar's actions and give
// keyboard focus back to the console so driving continues.
const fsBar = $('#fs-bar');
let fsTimer = 0;
const fsShow = () => {
  // Mouse aiming (pointer lock) must not keep the toolbar over the game.
  if (document.fullscreenElement !== consoleEl || document.pointerLockElement) return;
  consoleEl.classList.add('fs-show');
  clearTimeout(fsTimer);
  fsTimer = setTimeout(() => consoleEl.classList.remove('fs-show'), 2500);
};
consoleEl.addEventListener('mousemove', fsShow);
document.addEventListener('fullscreenchange', () => { syncFs(); fsShow(); });
function syncFs() {
  for (const [id, src] of [['#fs-video', '#pill-video'], ['#fs-input', '#pill-input']]) {
    $(id).textContent = $(src).textContent;
    $(id).className = `fs-pill ${$(src).classList.contains('ok') ? 'ok' : 'bad'}`;
  }
  fsBar.querySelector('[data-fs=pad]').setAttribute('aria-pressed', String(showPad.checked));
  fsBar.querySelector('[data-fs=keys]').setAttribute('aria-pressed', String(showKeys.checked));
  fsBar.querySelector('[data-fs=sound]').setAttribute('aria-pressed', $('#btn-sound').getAttribute('aria-pressed') || 'false');
}
setInterval(() => { if (document.fullscreenElement === consoleEl) syncFs(); }, 1000);
fsBar.addEventListener('click', (e) => {
  const b = e.target.closest('[data-fs]');
  if (!b) return;
  const act = b.dataset.fs;
  if (act === 'pad') showPad.click();
  else if (act === 'keys') showKeys.click();
  else if (act === 'sound') $('#btn-sound').click();
  else if (act === 'shot') $('#btn-shot').click();
  else if (act === 'clip') $('#btn-clip').click();
  else if (act === 'exit') { document.exitFullscreen?.(); return; }
  setTimeout(syncFs, 50);
  $('#screen').focus({ preventScroll: true });
});
