// The right panel follows the console this page shows: its card (who, where the controller is,
// wake / sleep), the controller's connection chain (computer → board → console) with what to do
// when a link is down, the mode the console needs, picture-and-sound shortcuts, and its network.
// Engine text goes in as text, never as HTML.
import { apiFetch } from './client.js';
import { t } from './i18n.js';
import { toast } from './ui.js';
import { controllerIsOn, editConsole, inputBlock, viewed } from './consoles.js';

const $ = (s) => document.querySelector(s);
const el = (tag, props = {}) => Object.assign(document.createElement(tag), props);
const COLOR = /^#[0-9a-f]{6}$/i;
const MODE_NAME = { standard: 'Controller + keyboard', hori: 'Controller + keyboard', pro: 'Pro Controller', pro1: 'Pro Controller' };
let last = null;

const ago = (t0) => {
  if (!t0) return t('never');
  const s = Math.max(0, Date.now() / 1000 - t0);
  return s < 90 ? `${Math.round(s)} s` : s < 5400 ? `${Math.round(s / 60)} min` : `${Math.round(s / 3600)} h`;
};
const button = (text, onclick, cls = '') => {
  const b = el('button', { type: 'button', textContent: text, className: cls });
  b.onclick = onclick;
  return b;
};
const failed = (e) => toast(String(e.message || e), 'error');
// Status arrives every second: rebuild a part only when what it shows changed, or focus on its
// buttons is lost each second and a click that straddles a rebuild goes nowhere.
function keyed(parent, key, nodes) {
  key = JSON.stringify(key);
  if (parent.dataset.key === key) return;
  parent.dataset.key = key;
  parent.replaceChildren(...nodes());
}
async function post(path, body) {
  const r = await apiFetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  if (!r.ok) throw new Error(await r.text());
  return r.json().catch(() => ({}));
}

// ------------------------------------------------------------------ the console card
function consoleCard(s, v) {
  const card = $('#panel-console');
  card.hidden = !v;
  if (!v) return;
  const consoles = s.consoles || [];
  // (this console's own choice: a console set to controller + keyboard doesn't say "Pro Controller")
  const mode = v.controller === 'keyboard' ? 'standard' : (s.models || []).find((m) => m.id === v.model)?.mode;
  card.style.setProperty('--tab-color', COLOR.test(v.color) ? v.color : '#888888');
  $('#pc-name').textContent = v.name;
  $('#pc-sum').textContent = [v.model_name, mode ? t(MODE_NAME[mode]) : ''].filter(Boolean).join(' · ');
  const chips = [];
  const chip = (text, cls = '') => chips.push([text, cls]);
  const on = consoles.find((c) => c.id === s.controller_on);
  if (v.controller_here) chip(`🎮 ${t('Controller here')}`, 'ok');
  else if (consoles.length > 1) chip(on ? t('Controller on %1').replace('%1', () => on.name) : t('Controller: which console?'), 'warn');
  const live = v.video_status?.age_s != null && v.video_status.age_s < 3;
  chip(live ? `● ${t('live')}` : t('no picture'), live ? 'ok' : '');
  if (v.network_ip) chip(v.network_ip);
  const here = consoles.length > 1 && !v.controller_here;
  keyed($('#pc-chips'), [v.id, chips, here, document.documentElement.lang], () => [
    ...chips.map(([text, cls]) => el('span', { className: `chip ${cls}`, textContent: text })),
    ...(here ? [button(t('The controller is plugged in here'), () => controllerIsOn(v.id).catch(failed), 'chip-btn')] : []),
  ]);
  // wake and sleep reach the console through the controller (or its power command): only the
  // console the controller is plugged into
  const blocked = inputBlock();
  // Sleep is sent over HDMI-CEC only: without it set up, the button can't work
  const cec = s.power?.cec === true;
  $('#pc-wake').disabled = !!blocked || powerBusy;
  $('#pc-sleep').disabled = !!blocked || !cec || powerBusy;
  $('#pc-sleep').title = cec ? t('Put the console to sleep (over HDMI-CEC)') : t('Sleep needs HDMI-CEC: set it up in Settings → Devices → Power and wake');
  const why = blocked ? t('Wake and sleep go to the console the controller is plugged into.')
    : !cec ? t('Sleep needs HDMI-CEC (Settings → Devices → Power and wake). Wake works through the controller.') : '';
  $('#pc-note').hidden = !why;
  $('#pc-note').textContent = why;
}
$('#pc-edit').onclick = () => { const v = viewed(); if (v) editConsole(v.id); };
let powerBusy = false;                    // (a status arriving mid-request must not re-enable the buttons)
async function power(action, btn) {
  if (powerBusy) return;
  powerBusy = true;
  btn.disabled = true;
  try {
    const r = await post('/api/power', { action });
    if (r.ok === false) throw new Error(r.detail || t('It didn’t work.'));
    toast(action === 'wake' ? t('Wake sent.') : t('Sleep sent.'), 'ok');
  } catch (e) { failed(e); } finally {
    powerBusy = false;
    if (last) consoleCard(last.s, last.v); else btn.disabled = false;
  }
}
$('#pc-wake').onclick = (e) => power('wake', e.currentTarget);
$('#pc-sleep').onclick = (e) => power('sleep', e.currentTarget);

// ------------------------------------------------------------------ the controller card
function chain(s) {
  const inp = s.input || {};
  const consoles = s.consoles || [];
  const target = consoles.find((c) => c.id === s.controller_on) || (consoles.length === 1 ? consoles[0] : null);
  const name = target?.name || t('the console');
  const steps = [];                 // [class, text]
  const step = (text, state) => (steps.length && steps.push(['arrow', '→']), steps.push([state, `${text} ${state === 'ok' ? '✓' : state === 'bad' ? '✗' : state === 'warn' ? '…' : ''}`.trim()]));
  let why = '', fixes = () => [], fixKey = '';
  step(t('Computer'), 'ok');
  if (inp.backend === 'none' || !inp.backend) {
    step(t('No input device'), '');
    why = t('View only: no input device is set up. Pick one to press buttons.');
    fixKey = 'devices';
    fixes = () => [button(t('Choose input device'), () => document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'devices' })))];
  } else if (!inp.connected) {
    step(t('Board'), 'bad');
    step(name, '');
    why = t('The computer can’t reach the board. Check its network cable, and that it has power (a console’s USB port powers it).');
    if (inp.detail) why += ` (${inp.detail})`;
  } else {
    step(`${t('Board')} ${inp.rtt_ms != null ? `${Math.round(inp.rtt_ms)} ms` : ''}`.trim(), 'ok');
    const usb = inp.usb || '';
    if (usb.startsWith('configured')) step(name, 'ok');
    else if (usb === 'suspended') {
      step(name, 'warn');
      why = t('%1 is asleep. A button press wakes it.').replace('%1', () => name);
    } else if (usb === 'not attached') {
      step(name, 'bad');
      why = t('The board has power but %1 hasn’t connected to it. Wake the console, reseat the cable, or try another USB port.').replace('%1', () => name);
    } else if (inp.usb == null) step(name, '');
    else {
      step(name, 'warn');
      why = t('%1 is connecting to the board…').replace('%1', () => name);
    }
  }
  // Several consoles and nobody knows which one the cable is in: keys are paused. Say it here,
  // where the person looks when keys do nothing, with the answer one click away.
  if (inp.connected && consoles.length > 1 && !s.controller_on) {
    steps.splice(-1, 1, ['warn', `${t('Which console?')} …`]);
    why = s.controller_detecting
      ? t('Keys are paused while NS2Bench finds which console the controller is plugged into (up to a minute). Or pick it:')
      : t('Keys are paused: which console is the controller plugged into?');
    fixKey = consoles.map((c) => [c.id, c.name]);
    fixes = () => consoles.map((c) => button(c.name, () => controllerIsOn(c.id).catch((e) => { $('#chain-why').textContent = String(e.message || e); })));
  }
  keyed($('#chain'), steps, () => steps.map(([cls, text]) => el('li', cls === 'arrow' ? { className: cls, textContent: text, ariaHidden: 'true' } : { className: cls, textContent: text })));
  $('#chain-sum').textContent = inp.backend && inp.backend !== 'none' ? (inp.backend === 'hidd' ? 'NS2Bench Bridge' : inp.backend) : '';
  $('#chain-callout').hidden = !why;
  $('#chain-why').textContent = why;
  keyed($('#chain-actions'), [fixKey, document.documentElement.lang], fixes);
  modePick(s, target);
}
// The mode the controller's console needs (its model), and a one-click fix when the board is in
// the other one. The engine switches by itself when the controller moves between consoles.
function modePick(s, target) {
  const inp = s.input || {};
  const p = $('#mode-pick');
  // the console's own choice for its type
  const want = !target ? null : target.controller === 'keyboard' ? 'standard' : target.model === 'switch1' ? 'pro1' : 'pro';
  if (inp.backend !== 'hidd' || !inp.connected || !want || !target || $('#usb-mode').dataset.busy) { p.hidden = true; return; }
  const now = inp.usb_mode === 'hori' ? 'standard' : inp.usb_mode || 'standard';
  const fits = now === want;
  p.hidden = false;
  p.classList.toggle('warn', !fits);
  // (pro vs pro1: both are "Pro Controller", so name whose)
  const wantName = now.startsWith('pro') && want.startsWith('pro') ? `${t(MODE_NAME[want])} (${target.model_name || target.model})` : t(MODE_NAME[want]);
  keyed(p, [fits, want, now, target.name, target.model_name || target.model, document.documentElement.lang], () => (fits
    ? [t('Picked for %1: %2 needs this one.').replace('%1', () => target.name).replace('%2', () => target.model_name || target.model)]
    : [`${t('%1 needs “%2”.').replace('%1', () => target.name).replace('%2', () => wantName)} `,
      button(t('Use %1').replace('%1', () => wantName), () => $(`#seg-usb [data-v="${want}"]`)?.click(), 'link')]));
}
$('#btn-letgo').onclick = () => post('/api/release', {}).then(() => toast(t('Let go of everything.'), 'ok')).catch(failed);
$('#btn-check').onclick = () => {
  document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'help' }));
  setTimeout(() => $('#help-doctor')?.click(), 300);
};

// ------------------------------------------------------------------ picture and sound
// the same actions as the buttons along the top (one place does the work)
$('#p-shot').onclick = () => $('#btn-shot').click();
$('#p-clip').onclick = () => $('#btn-clip').click();
$('#p-sound').onclick = () => $('#btn-sound').click();
function mirrorSound() {
  const on = $('#btn-sound').getAttribute('aria-pressed') === 'true';
  $('#p-sound').setAttribute('aria-pressed', String(on));
  $('#p-sound').textContent = on ? `🔊 ${t('Sound on')}` : `🔇 ${t('Sound off')}`;
}
new MutationObserver(mirrorSound).observe($('#btn-sound'), { attributes: true, attributeFilter: ['aria-pressed'] });
mirrorSound();
function videoSummary(v) {
  const st = v?.video_status;
  const live = st?.age_s != null && st.age_s < 3;
  const h = v?.video?.height;
  $('#v-sum').textContent = live ? [st.fps ? `${Math.round(st.fps)} fps` : '', h ? `${h}p` : ''].filter(Boolean).join(' · ') : '';
}

// ------------------------------------------------------------------ network
let net = null, netAt = 0, netBusy = false;
async function loadNetwork() {
  if (netBusy) return;
  netBusy = true;
  try {
    const r = await apiFetch('/api/network');
    net = r.ok ? await r.json() : null;
    netAt = Date.now();
  } catch { net = null; } finally { netBusy = false; }
  if (last) network(last.v);
}
function network(v) {
  const card = $('#panel-network');
  card.hidden = !v;
  if (!v) return;
  const rows = [];
  const row = (k, val, cls = '') => rows.push(el('dt', { textContent: k }), el('dd', { textContent: val, className: cls }));
  const state = $('#pn-state'), note = $('#pn-note');
  $('#pn-addr').hidden = !!v.network_ip;
  note.hidden = true;
  if (!v.network_ip) {
    state.textContent = ''; state.className = 'pc-sum';
    note.hidden = false;
    note.textContent = t('No network address set for %1. Add it to see whether it is protected.').replace('%1', () => v.name);
    $('#pn-rows').replaceChildren();
    return;
  }
  row(t('Console'), v.network_ip);
  const d = net?.daemon, shared = net?.shared;
  const w = shared?.watch?.consoles?.find((c) => c.console === v.network_ip);
  const states = {
    protected: [t('Protected'), 'ok'], warning: [t('Not using NS2Bench'), 'bad'], waiting: [t('Waiting'), ''],
    away: [t('Not on the network'), ''], 'helper-down': [t('Helper not running'), 'bad'], unknown: [t('Unknown'), ''],
  };
  const running = !!d?.topology && shared?.watch?.helper_running !== false;
  const [label, cls] = !net ? [t('Unknown'), ''] : !running ? states['helper-down'] : w ? (states[w.state] || [w.state, '']) : [t('Not watched'), 'warn'];
  state.textContent = `● ${label}`;
  state.className = `pc-sum ${cls}`;
  if (net) {
    // only what the helper reports: without it running, NS2Bench can't tell
    if (running) row(t('Internet'), d.forwarding_off === true ? t('Blocked: no route out') : t('Check: this computer may pass traffic'), d.forwarding_off === true ? 'ok' : 'bad');
    if (w) {
      row(t('Nintendo names'), t('%1 refused · %2 lookups').replace('%1', () => w.refused || 0).replace('%2', () => w.lookups || 0));
      row(t('Connection test'), w.last_ctest ? `${t('answered')} · ${ago(w.last_ctest)}` : t('never'), w.last_ctest ? 'ok' : '');
    } else if (running) {
      note.hidden = false;
      note.textContent = t('This address isn’t on the shared network list. Add it in Network settings to protect and watch it.');
    }
    const helper = !running ? t('not running') : `${d.topology === 'shared' ? t('shared mode') : d.topology}${d.interface ? ` · ${d.interface}` : ''}`;
    row(t('Helper'), helper, running ? '' : 'bad');
  }
  $('#pn-rows').replaceChildren(...rows);
}
$('#pn-check').onclick = () => loadNetwork();
$('#pn-addr').onclick = () => { const v = viewed(); if (v) editConsole(v.id); };
$('#pn-settings').onclick = () => document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'network' }));

/** From each status (after the console strip has taken it). */
let lastStatus = null;
export function updatePanel(s) {
  lastStatus = s;
  const v = viewed();
  last = { s, v };
  consoleCard(s, v);
  chain(s);
  videoSummary(v);
  if (Date.now() - netAt > 30000) loadNetwork();
  else network(v);
}
// another language: the parts built from translated text are built again
document.addEventListener('ns2bench:language', () => { if (lastStatus) updatePanel(lastStatus); mirrorSound(); });
