// Several consoles on one bench (docs/design/multi-console.md): the console strip under the picture,
// which console this page shows, and where the shared controller is plugged in.
// Everything from the engine goes in as text, never as HTML.
import { apiFetch, authURL, currentConsole, setConsole } from './client.js';
import { t } from './i18n.js';

const $ = (s) => document.querySelector(s);
let consoles = [], controllerOn = null, onSwitch = () => {};

/** The console this page shows (its info from the last status), or null. */
export function viewed() {
  return consoles.find((c) => c.id === currentConsole()) || consoles.find((c) => c.controller_here) || consoles[0] || null;
}

/** Why input from this page can't go out now (several consoles), or ''. */
export function inputBlock() {
  if (consoles.length < 2) return '';
  if (!controllerOn) return t('Which console is the controller plugged into? Pick it above the picture.');
  const v = viewed();
  if (v && v.id !== controllerOn) {
    const on = consoles.find((c) => c.id === controllerOn);
    return `${t('The controller is plugged into')} ${on ? on.name : controllerOn}. ${t('Switch to it, or move the cable and say so above the picture.')}`;
  }
  return '';
}

async function post(body) {
  const r = await apiFetch('/api/consoles', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  if (!r.ok) throw new Error(await r.text());
  return r.json();
}

/** Say where the controller is plugged in now. */
export async function controllerIsOn(id) {
  await post({ action: 'controller', id });
}

// A small live picture of each console (a screenshot about once a second: light, unlike a video
// stream per tile), its name and colour, and 🎮 on the one the controller is plugged into.
function thumb(c) {
  const b = document.createElement('button');
  b.type = 'button';
  b.className = 'console-thumb';
  b.dataset.console = c.id;
  b.setAttribute('role', 'tab');
  b.setAttribute('aria-selected', String(c.id === viewed()?.id));
  b.style.setProperty('--tab-color', /^#[0-9a-f]{6}$/i.test(c.color) ? c.color : '#888888');
  const pic = Object.assign(document.createElement('div'), { className: 'pic' });
  const img = Object.assign(document.createElement('img'), { alt: '', decoding: 'async', draggable: false });
  img.dataset.console = c.id;
  pic.append(img);
  if (c.controller_here) {
    const pad = Object.assign(document.createElement('span'), { className: 'controller-badge', textContent: '🎮', title: t('The controller is plugged into this console') });
    pad.setAttribute('aria-label', t('The controller is plugged into this console'));
    pic.append(pad);
  }
  const cap = Object.assign(document.createElement('div'), { className: 'cap' });
  const live = c.video_status?.age_s != null && c.video_status.age_s < 3;
  cap.append(Object.assign(document.createElement('span'), { className: 'dot' }),
    Object.assign(document.createElement('span'), { className: 'name', textContent: c.name }),
    Object.assign(document.createElement('span'), { className: `state${live ? ' live' : ''}`, textContent: live ? t('live') : t('no picture') }));
  b.title = `${c.name} · ${c.model_name || ''}`;
  b.append(pic, cap);
  b.onclick = () => { if (c.id !== viewed()?.id) { setConsole(c.id); render(); onSwitch(c); } };
  return b;
}

function addTile() {
  const b = Object.assign(document.createElement('button'), { type: 'button', className: 'console-thumb add', textContent: `+ ${t('Add console')}` });
  b.onclick = () => document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'consoles' }));
  return b;
}

// Refresh the thumbnails' pictures (only while they're on screen and the page is visible).
let refreshing = false;
function refreshPictures() {
  if (refreshing || document.hidden || $('#console-bar')?.hidden) return;
  refreshing = true;
  const imgs = [...document.querySelectorAll('.console-thumb img')];
  let left = imgs.length;
  if (!left) { refreshing = false; return; }
  for (const img of imgs) {
    const next = new Image();
    const done = () => { if (--left === 0) refreshing = false; };
    next.onload = () => { img.src = next.src; done(); };
    next.onerror = done;
    next.src = authURL(`/api/screenshot?console=${encodeURIComponent(img.dataset.console)}&max_width=320&r=${Date.now()}`);
  }
}
setInterval(refreshPictures, 1000);

function render() {
  const bar = $('#console-bar');
  if (!bar) return;
  bar.hidden = consoles.length < 2;
  if (bar.hidden) return;
  $('#console-tabs').replaceChildren(...consoles.map(thumb), addTile());
  refreshPictures();
  const v = viewed();
  const note = $('#console-note'), act = $('#console-note-actions');
  act.replaceChildren();
  if (!controllerOn) {
    note.textContent = t('Which console is the controller plugged into? Input is paused until you say.');
    note.className = 'console-note warn';
    for (const c of consoles) {
      const b = Object.assign(document.createElement('button'), { type: 'button', textContent: c.name });
      b.onclick = () => controllerIsOn(c.id).catch((e) => alertText(e));
      act.append(b);
    }
  } else if (v && v.id !== controllerOn) {
    const on = consoles.find((c) => c.id === controllerOn);
    note.textContent = `${t('Watching')} ${v.name}. ${t('The controller is plugged into')} ${on ? on.name : controllerOn}, ${t('so buttons go there, not here.')}`;
    note.className = 'console-note';
    const move = Object.assign(document.createElement('button'), { type: 'button', textContent: `${t('I moved the controller to')} ${v.name}` });
    move.onclick = () => controllerIsOn(v.id).catch((e) => alertText(e));
    act.append(move);
  } else {
    note.textContent = '';
    note.className = 'console-note';
  }
  note.hidden = !note.textContent;
  document.querySelector('.console')?.classList.toggle('input-elsewhere', !!inputBlock());
}

function alertText(e) {
  const n = $('#console-note');
  n.hidden = false; n.className = 'console-note warn'; n.textContent = String(e.message || e);
}

/** From each status: the consoles, and where the controller is. */
export function updateConsoles(status) {
  if (!Array.isArray(status.consoles)) return;
  const before = JSON.stringify([consoles.map((c) => [c.id, c.name, c.color, c.controller_here, c.model_name, c.video_status?.age_s != null && c.video_status.age_s < 3]), controllerOn]);
  consoles = status.consoles;
  controllerOn = status.controller_on ?? null;
  // a console that was removed: show the one with the controller
  if (currentConsole() && !consoles.some((c) => c.id === currentConsole())) { setConsole(''); onSwitch(viewed()); }
  const after = JSON.stringify([consoles.map((c) => [c.id, c.name, c.color, c.controller_here, c.model_name, c.video_status?.age_s != null && c.video_status.age_s < 3]), controllerOn]);
  if (before !== after) render();
}

/** `switched(console)` runs when the page shows another console (reconnect picture and sound). */
export function mountConsoles(switched) {
  onSwitch = switched;
  render();
}
