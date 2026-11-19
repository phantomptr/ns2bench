// Several consoles on one bench (docs/design/multi-console.md): the console strip under the picture,
// which console this page shows, and where the shared controller is plugged in.
// Everything from the engine goes in as text, never as HTML.
import { apiFetch, authURL, currentConsole, setConsole } from './client.js';
import { t } from './i18n.js';

const $ = (s) => document.querySelector(s);
// shownId: the console whose picture and sound this page is connected to now
let consoles = [], controllerOn = null, onSwitch = () => {}, shownId = null, detecting = false;
// "pictures" (live thumbnails) or "names" (small chips, no pictures): the strip can be tucked away
let stripStyle = (() => { try { return localStorage.getItem('ns2bench.strip') === 'names' ? 'names' : 'pictures'; } catch { return 'pictures'; } })();

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
    return t('The controller is plugged into %1. Switch to it, or move the cable and say so above the picture.').replace('%1', () => on ? on.name : controllerOn);
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
// last picture per console, so a rebuilt strip doesn't flash empty tiles
const lastPic = new Map();
const DRAG_TYPE = 'application/x-ns2bench-console';

function thumb(c) {
  const b = document.createElement('button');
  b.type = 'button';
  b.className = 'console-thumb';
  b.dataset.console = c.id;
  b.setAttribute('aria-pressed', String(c.id === viewed()?.id));
  b.style.setProperty('--tab-color', /^#[0-9a-f]{6}$/i.test(c.color) ? c.color : '#888888');
  const pic = Object.assign(document.createElement('div'), { className: 'pic' });
  const img = Object.assign(document.createElement('img'), { alt: '', decoding: 'async', draggable: false });
  img.dataset.console = c.id;
  if (lastPic.has(c.id)) img.src = lastPic.get(c.id);
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
  // picking a console pins it (it no longer follows the controller around); the shared controller
  // comes along when set to (Settings → Consoles), unless the board shows it's still elsewhere
  b.onclick = () => pick(c);
  // its settings, from right here
  const tile = Object.assign(document.createElement('span'), { className: 'console-tile' });
  const ed = Object.assign(document.createElement('button'), { type: 'button', className: 'edit', title: t('Edit %1').replace('%1', () => c.name),
    innerHTML: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 20h4L19 9l-4-4L4 16z"/><path d="m13.5 6.5 4 4"/></svg>' });
  ed.setAttribute('aria-label', t('Edit %1').replace('%1', () => c.name));
  ed.onclick = () => editConsole(c.id);
  tile.append(b, ed);
  // drag a console onto another to put it there
  tile.draggable = true;
  tile.dataset.console = c.id;
  tile.addEventListener('dragstart', (e) => { e.dataTransfer.setData(DRAG_TYPE, c.id); e.dataTransfer.setData('text/plain', c.name); tile.classList.add('dragging'); });
  tile.addEventListener('dragend', () => tile.classList.remove('dragging'));
  // (only a console tile can be dropped here: not selected text, a file or a link)
  tile.addEventListener('dragover', (e) => { if (![...e.dataTransfer.types].includes(DRAG_TYPE)) return; e.preventDefault(); tile.classList.add('drop'); });
  tile.addEventListener('dragleave', () => tile.classList.remove('drop'));
  tile.addEventListener('drop', (e) => {
    e.preventDefault(); tile.classList.remove('drop');
    const from = e.dataTransfer.getData(DRAG_TYPE);
    const all = consoles.map((x) => x.id);
    if (!from || from === c.id || !all.includes(from)) return;
    const after = all.indexOf(from) < all.indexOf(c.id);     // dragged rightwards: lands after the target
    const ids = all.filter((x) => x !== from);
    ids.splice(ids.indexOf(c.id) + (after ? 1 : 0), 0, from);
    consoles.sort((a, b2) => ids.indexOf(a.id) - ids.indexOf(b2.id)); render();
    post({ action: 'order', ids }).catch((err) => alertText(err));
  });
  return tile;
}

/** Open Settings → Consoles at this console (or at the add form): its name field, in view. */
export function editConsole(id) {
  const focus = () => {
    const f = id ? document.querySelector(`#consoles-list [data-console="${CSS.escape(id)}"] input`) : $('#console-add-name');
    if (f?.offsetParent) { f.scrollIntoView({ block: 'center' }); f.focus(); }
  };
  // after the pane has drawn its fresh list (focusing an older copy is lost when it's replaced)
  document.addEventListener('ns2bench:consoles-shown', focus, { once: true });
  document.dispatchEvent(new CustomEvent('ns2bench:settings', { detail: 'consoles' }));
}

function pick(c) {
  const moving = c.id !== viewed()?.id;
  setConsole(c.id); render(); follow();
  if (moving && consoles.length > 1) post({ action: 'follow', id: c.id }).catch((e) => alertText(e));
}

// The strip as small name chips: no pictures, still one click to switch.
function chip(c) {
  const b = Object.assign(document.createElement('button'), { type: 'button', className: 'console-chip' });
  b.dataset.console = c.id;
  b.setAttribute('aria-pressed', String(c.id === viewed()?.id));
  b.style.setProperty('--tab-color', /^#[0-9a-f]{6}$/i.test(c.color) ? c.color : '#888888');
  b.append(Object.assign(document.createElement('span'), { className: 'dot' }), document.createTextNode(c.name));
  if (c.controller_here) b.append(Object.assign(document.createElement('span'), { className: 'pad', textContent: ' 🎮', title: t('The controller is plugged into this console') }));
  b.onclick = () => pick(c);
  return b;
}

function styleToggle() {
  const names = stripStyle === 'names';
  const b = Object.assign(document.createElement('button'), { type: 'button', className: 'strip-toggle',
    textContent: names ? t('Show pictures') : t('Hide pictures') });
  b.setAttribute('aria-pressed', String(names));
  b.onclick = () => {
    stripStyle = names ? 'pictures' : 'names';
    try { localStorage.setItem('ns2bench.strip', stripStyle); } catch { /* fine */ }
    render();
  };
  return b;
}

function addTile() {
  const b = Object.assign(document.createElement('button'), { type: 'button', className: 'console-thumb add', textContent: `+ ${t('Add console')}` });
  b.onclick = () => editConsole(null);
  return b;
}

// Refresh the thumbnails' pictures every 2 s (only while they're on screen, the page is visible
// and the console has a picture: no requests for a console that sends none).
let refreshing = false;
function refreshPictures() {
  if (refreshing || document.hidden || $('#console-bar')?.hidden || stripStyle === 'names') return;
  refreshing = true;
  const imgs = [...document.querySelectorAll('.console-thumb img')].filter((i) => {
    const c = consoles.find((x) => x.id === i.dataset.console);
    return c?.video_status?.age_s != null && c.video_status.age_s < 3;
  });
  let left = imgs.length;
  if (!left) { refreshing = false; return; }
  for (const img of imgs) {
    const next = new Image();
    // one card that stopped answering must not freeze every tile: give up on it after 4 s
    let finished = false;
    const done = () => { if (finished) return; finished = true; next.onload = next.onerror = null; if (--left === 0) refreshing = false; };
    setTimeout(done, 4000);
    next.onload = () => { img.src = next.src; lastPic.set(img.dataset.console, next.src); done(); };
    next.onerror = done;
    next.src = authURL(`/api/screenshot?console=${encodeURIComponent(img.dataset.console)}&max_width=320&r=${Date.now()}`);
  }
}
setInterval(refreshPictures, 2000);

function render() {
  const bar = $('#console-bar');
  if (!bar) return;
  // shown from the first console on, so "+ Add console" is always in sight
  bar.hidden = consoles.length < 1;
  if (bar.hidden) return;
  const strip = $('#console-tabs');
  strip.classList.toggle('names', stripStyle === 'names');
  // keyboard focus survives the redraw (a tile, or its edit button)
  const f = document.activeElement?.closest?.('#console-tabs [data-console]') ? document.activeElement : null;
  const focusId = f?.closest('[data-console]')?.dataset.console, focusEdit = f?.classList.contains('edit');
  strip.replaceChildren(...consoles.map(stripStyle === 'names' ? chip : thumb), addTile(), styleToggle());
  if (focusId) {
    const tile = [...strip.children].find((x) => x.dataset.console === focusId || x.querySelector?.(`[data-console="${CSS.escape(focusId)}"]`));
    (focusEdit ? tile?.querySelector('.edit') : tile?.querySelector('.console-thumb, .console-chip') || tile)?.focus?.();
  }
  refreshPictures();
  const v = viewed();
  const note = $('#console-note'), act = $('#console-note-actions');
  act.replaceChildren();
  if (consoles.length > 1 && !controllerOn) {
    note.textContent = detecting
      ? t('Finding which console the controller is plugged into… Input is paused until then. Or pick it:')
      : t('Which console is the controller plugged into? Input is paused until you say.');
    note.className = 'console-note warn';
    for (const c of consoles) {
      const b = Object.assign(document.createElement('button'), { type: 'button', textContent: c.name });
      b.onclick = () => controllerIsOn(c.id).catch((e) => alertText(e));
      act.append(b);
    }
  } else if (v && !v.video?.device && !(v.video_status?.age_s != null && v.video_status.age_s < 3)) {
    // added without its card (it can come later): say so, with the way to set it
    note.textContent = t('%1: no capture card yet. Connect it, then pick it in its settings.').replace('%1', () => v.name);
    note.className = 'console-note';
    const set = Object.assign(document.createElement('button'), { type: 'button', textContent: t('Set up') });
    set.onclick = () => editConsole(v.id);
    act.append(set);
  } else if (v && v.id !== controllerOn) {
    const on = consoles.find((c) => c.id === controllerOn);
    note.textContent = t('Watching %1. The controller is plugged into %2, so buttons go there, not here.').replace('%1', () => v.name).replace('%2', () => on ? on.name : controllerOn);
    note.className = 'console-note';
    const move = Object.assign(document.createElement('button'), { type: 'button', textContent: t('I moved the controller to %1').replace('%1', () => v.name) });
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
  const before = JSON.stringify([consoles.map((c) => [c.id, c.name, c.color, c.controller_here, c.model_name, c.video?.device, c.video_status?.age_s != null && c.video_status.age_s < 3]), controllerOn]);
  consoles = status.consoles;
  controllerOn = status.controller_on ?? null;
  const wasDetecting = detecting;
  detecting = !!status.controller_detecting;
  if (wasDetecting !== detecting) render();
  // a console that was removed: show the one with the controller
  if (currentConsole() && !consoles.some((c) => c.id === currentConsole())) setConsole('');
  const after = JSON.stringify([consoles.map((c) => [c.id, c.name, c.color, c.controller_here, c.model_name, c.video?.device, c.video_status?.age_s != null && c.video_status.age_s < 3]), controllerOn]);
  if (before !== after) render();
  follow();
}

// The page shows another console than its picture and sound are connected to (picked, or the
// default followed the controller, or the shown one was removed): reconnect them.
function follow() {
  const v = viewed();
  if (!v) return;
  if (shownId === null) { shownId = v.id; return; }   // first status: already connected to it
  if (v.id !== shownId) { shownId = v.id; onSwitch(v); }
}

/** Settings changed this console's capture card: reconnect if it's the one on screen. */
export function cardChanged(id) {
  if (viewed()?.id === id) onSwitch(viewed());
}

/** `switched(console)` runs when the page shows another console (reconnect picture and sound). */
export function mountConsoles(switched) {
  onSwitch = switched;
  render();
}
// another language: the strip's own words are written again
document.addEventListener('ns2bench:language', render);
